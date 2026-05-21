//! Reader side of the economy domain.
//!
//! Six query functions, each taking `(conn, scope)` and returning a typed
//! aggregate. The match-on-scope shape is the same in every function:
//!
//! ```ignore
//! match scope {
//!     EconomyScope::Project(_)
//!     | EconomyScope::Spec { .. }
//!     | EconomyScope::Wave { .. } => /* single-project SQL */,
//!     EconomyScope::AllProjects(projects) => /* MultiProjectReader fan-out */,
//! }
//! ```
//!
//! The single-project SQL stays *minimal* — no projection beyond what the
//! aggregate needs. The intent is that hooks, dashboards, and tests all call
//! the same six entry points; UI-specific shaping happens on the consumer
//! side. The per-agent / per-spec / per-wave roll-ups share the W4
//! attribution CTE ([`attribution_cte`]) so the spans↔`agent.start` join is
//! defined exactly once.

use rusqlite::{Connection, params};

use crate::error::{Error, Result};

use super::model::{
    AgentCost, ContextRoutingMetrics, EconomySummary, SavingsBreakdown, SavingsBySource,
    SavingsSource, SpecCost, WaveCost,
};
use super::multi_project::MultiProjectReader;
use super::scope::{AgentId, EconomyScope, SpecId, WaveId};

/// Top-level summary — total cost, total tokens, total savings, top 3 agents.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn economy_summary(conn: &Connection, scope: EconomyScope) -> Result<EconomySummary> {
    match scope {
        EconomyScope::AllProjects(ref projects) => {
            let reader = MultiProjectReader::new();
            let per_project = reader.fan_out(projects, |c, project| {
                economy_summary(c, EconomyScope::Project(project.clone()))
            });
            let mut acc = EconomySummary::default();
            for s in per_project.values() {
                acc.total_cost_usd_micros += s.total_cost_usd_micros;
                acc.total_tokens += s.total_tokens;
                acc.total_tokens_saved += s.total_tokens_saved;
                acc.span_count += s.span_count;
                acc.top_agents_by_cost.extend(s.top_agents_by_cost.clone());
            }
            // Re-sort and truncate top agents after the merge.
            acc.top_agents_by_cost
                .sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
            acc.top_agents_by_cost.truncate(3);
            Ok(acc)
        }
        _ => {
            // Spans aggregation: Wave scope cannot filter on the `spans` table
            // directly (no native `wave_id` column), so it routes through the
            // attribution CTE — same join `per_agent_costs` uses — and filters
            // on the resolved `attr_wave_id`. Project/Spec scopes stay on the
            // direct path; the CTE is one extra index walk we only pay when
            // the caller actually constrained to a wave.
            let (total_cost, total_tokens, span_count) = match &scope {
                EconomyScope::Wave { spec, wave, .. } => {
                    let sql = format!(
                        "{cte} \
                         SELECT COALESCE(SUM(cost_usd_micros), 0), \
                                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                                COUNT(*) \
                         FROM attributed_spans \
                         WHERE attr_spec_id = ?1 AND attr_wave_id = ?2",
                        cte = attribution_cte("WHERE 1=1"),
                    );
                    conn.query_row(
                        &sql,
                        params![spec.0.as_str(), wave.0.as_str()],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
                    )
                    .map_err(Error::from)?
                }
                _ => {
                    let (where_sql, spec_param) = spans_scope_where(&scope);
                    let span_sql = format!(
                        "SELECT COALESCE(SUM(cost_usd_micros), 0), \
                                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                                COUNT(*) \
                         FROM {unified} AS spans {where_sql}",
                        unified = unified_spans_subquery()
                    );
                    conn.query_row(
                        &span_sql,
                        rusqlite::params_from_iter(spec_param.iter()),
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
                    )
                    .map_err(Error::from)?
                }
            };

            // Savings aggregation always uses `savings_records` directly —
            // the table carries native `spec_id` and `wave_id` columns so
            // every scope variant has a real, non-tautological filter.
            let savings_sql = format!(
                "SELECT COALESCE(SUM(tokens_saved), 0) FROM savings_records {}",
                savings_where(&scope)
            );
            let total_saved: i64 = conn
                .query_row(
                    &savings_sql,
                    rusqlite::params_from_iter(savings_params(&scope).iter()),
                    |r| r.get(0),
                )
                .map_err(Error::from)?;

            let top = per_agent_costs(conn, scope)?
                .into_iter()
                .take(3)
                .collect::<Vec<_>>();

            Ok(EconomySummary {
                total_cost_usd_micros: total_cost,
                total_tokens,
                total_tokens_saved: total_saved,
                span_count,
                top_agents_by_cost: top,
            })
        }
    }
}

/// Per-agent cost roll-up, ordered by cost descending.
///
/// W4 attribution: each in-scope `spans` row is joined to the originating
/// `agent.start` event by [`ATTRIBUTION_CTE`] — primary key is the Anthropic
/// `tool_use_id` (when both sides expose it), with a temporal-window fallback
/// keyed on `session_id` + the most-recent `agent.start.ts <= spans.ts_iso`.
/// Spans that fail both legs are excluded from the roll-up (they have no
/// agent to attribute to).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn per_agent_costs(conn: &Connection, scope: EconomyScope) -> Result<Vec<AgentCost>> {
    match scope {
        EconomyScope::AllProjects(ref projects) => {
            let reader = MultiProjectReader::new();
            let per_project = reader.fan_out(projects, |c, project| {
                per_agent_costs(c, EconomyScope::Project(project.clone()))
            });
            // Merge by agent_id.
            let mut merged: std::collections::HashMap<String, AgentCost> =
                std::collections::HashMap::new();
            for rows in per_project.values() {
                for row in rows {
                    let entry = merged.entry(row.agent_id.0.clone()).or_insert(AgentCost {
                        agent_id: row.agent_id.clone(),
                        cost_usd_micros: 0,
                        tokens: 0,
                        span_count: 0,
                    });
                    entry.cost_usd_micros += row.cost_usd_micros;
                    entry.tokens += row.tokens;
                    entry.span_count += row.span_count;
                }
            }
            let mut out: Vec<AgentCost> = merged.into_values().collect();
            out.sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
            Ok(out)
        }
        _ => {
            // The W4 attribution flips ordering: span row's `spec` is no
            // longer authoritative (the joined agent.start is). So the CTE
            // walks every span; the scope filter runs post-attribution.
            let span_where = "WHERE 1=1";
            let (wave_filter, scope_params) = wave_filter_for(&scope);
            // CTE-based join: every span is attributed once, then we GROUP BY
            // agent_id in the outer select. The CTE keeps the read path
            // O(N spans × log N events) with the `idx_events_event` +
            // `idx_spans_tool_use_id` indices doing the heavy lifting.
            let sql = format!(
                "{cte} \
                 SELECT attr_agent_id, \
                        COALESCE(SUM(cost_usd_micros), 0) AS cost, \
                        COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) AS tokens, \
                        COUNT(*) AS span_count \
                 FROM attributed_spans \
                 WHERE attr_agent_id IS NOT NULL AND attr_agent_id != '' {wave_filter} \
                 GROUP BY attr_agent_id \
                 ORDER BY cost DESC",
                cte = attribution_cte(span_where),
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(
                    rusqlite::params_from_iter(scope_params.iter()),
                    |r| {
                        Ok(AgentCost {
                            agent_id: AgentId(r.get(0)?),
                            cost_usd_micros: r.get(1)?,
                            tokens: r.get(2)?,
                            span_count: r.get(3)?,
                        })
                    },
                )?
                .filter_map(std::result::Result::ok)
                .collect();
            Ok(rows)
        }
    }
}

/// Per-spec cost roll-up. Meaningful for [`EconomyScope::Project`] and
/// [`EconomyScope::AllProjects`]; returns a single-row result when the scope
/// is already constrained to a spec.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn per_spec_costs(conn: &Connection, scope: EconomyScope) -> Result<Vec<SpecCost>> {
    match scope {
        EconomyScope::AllProjects(ref projects) => {
            let reader = MultiProjectReader::new();
            let per_project = reader.fan_out(projects, |c, project| {
                per_spec_costs(c, EconomyScope::Project(project.clone()))
            });
            let mut merged: std::collections::HashMap<String, SpecCost> =
                std::collections::HashMap::new();
            for rows in per_project.values() {
                for row in rows {
                    let entry = merged.entry(row.spec_id.0.clone()).or_insert(SpecCost {
                        spec_id: row.spec_id.clone(),
                        cost_usd_micros: 0,
                        tokens: 0,
                        span_count: 0,
                    });
                    entry.cost_usd_micros += row.cost_usd_micros;
                    entry.tokens += row.tokens;
                    entry.span_count += row.span_count;
                }
            }
            let mut out: Vec<SpecCost> = merged.into_values().collect();
            out.sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
            Ok(out)
        }
        _ => {
            // W4 attribution: aggregate by the spec_id resolved through the
            // attribution CTE (favours the `agent.start` payload's spec over
            // the span's own column — they differ in parent-spec/child-wave
            // dispatches where the span carries the parent and the agent was
            // launched against the child).
            let span_where = "WHERE 1=1";
            let (wave_filter, scope_params) = wave_filter_for(&scope);
            let sql = format!(
                "{cte} \
                 SELECT COALESCE(attr_spec_id, '') AS spec, \
                        COALESCE(SUM(cost_usd_micros), 0) AS cost, \
                        COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) AS tokens, \
                        COUNT(*) AS span_count \
                 FROM attributed_spans \
                 WHERE 1=1 {wave_filter} \
                 GROUP BY spec \
                 ORDER BY cost DESC",
                cte = attribution_cte(span_where),
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(
                    rusqlite::params_from_iter(scope_params.iter()),
                    |r| {
                        Ok(SpecCost {
                            spec_id: SpecId(r.get(0)?),
                            cost_usd_micros: r.get(1)?,
                            tokens: r.get(2)?,
                            span_count: r.get(3)?,
                        })
                    },
                )?
                .filter_map(std::result::Result::ok)
                .collect();
            Ok(rows)
        }
    }
}

/// Per-wave cost roll-up. W4 attribution: the wave id comes from the joined
/// `agent.start` event payload, not from the span — so spans dispatched into a
/// child wave from a parent-spec context are correctly bucketed.
///
/// Regression-tested by `parent_spec_child_wave_attribution` in
/// `tests/economy_attribution.rs` (AC-6, absorbed from the superseded
/// `metrics-writers-pipeline-key` spec).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn per_wave_costs(conn: &Connection, scope: EconomyScope) -> Result<Vec<WaveCost>> {
    match scope {
        EconomyScope::AllProjects(ref projects) => {
            let reader = MultiProjectReader::new();
            let per_project = reader.fan_out(projects, |c, project| {
                per_wave_costs(c, EconomyScope::Project(project.clone()))
            });
            let mut merged: std::collections::HashMap<(String, String), WaveCost> =
                std::collections::HashMap::new();
            for rows in per_project.values() {
                for row in rows {
                    let key = (row.spec_id.0.clone(), row.wave_id.0.clone());
                    let entry = merged.entry(key).or_insert(WaveCost {
                        spec_id: row.spec_id.clone(),
                        wave_id: row.wave_id.clone(),
                        cost_usd_micros: 0,
                        tokens: 0,
                        span_count: 0,
                    });
                    entry.cost_usd_micros += row.cost_usd_micros;
                    entry.tokens += row.tokens;
                    entry.span_count += row.span_count;
                }
            }
            let mut out: Vec<WaveCost> = merged.into_values().collect();
            out.sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
            Ok(out)
        }
        _ => {
            // W4 attribution: aggregate by (spec_id, wave_id) resolved through
            // the attribution CTE. Spans that join cleanly to an `agent.start`
            // carry the wave it was dispatched against — which is the answer
            // the "parent-spec/child-wave" regression case needs.
            let span_where = "WHERE 1=1";
            let (wave_filter, scope_params) = wave_filter_for(&scope);
            let sql = format!(
                "{cte} \
                 SELECT COALESCE(attr_spec_id, '') AS spec, \
                        COALESCE(attr_wave_id, '') AS wave, \
                        COALESCE(SUM(cost_usd_micros), 0) AS cost, \
                        COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) AS tokens, \
                        COUNT(*) AS span_count \
                 FROM attributed_spans \
                 WHERE 1=1 {wave_filter} \
                 GROUP BY spec, wave \
                 ORDER BY cost DESC",
                cte = attribution_cte(span_where),
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(
                    rusqlite::params_from_iter(scope_params.iter()),
                    |r| {
                        Ok(WaveCost {
                            spec_id: SpecId(r.get(0)?),
                            wave_id: WaveId(r.get(1)?),
                            cost_usd_micros: r.get(2)?,
                            tokens: r.get(3)?,
                            span_count: r.get(4)?,
                        })
                    },
                )?
                .filter_map(std::result::Result::ok)
                .collect();
            Ok(rows)
        }
    }
}

/// Savings roll-up by [`SavingsSource`].
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn savings_breakdown(conn: &Connection, scope: EconomyScope) -> Result<SavingsBreakdown> {
    match scope {
        EconomyScope::AllProjects(ref projects) => {
            let reader = MultiProjectReader::new();
            let per_project = reader.fan_out(projects, |c, project| {
                savings_breakdown(c, EconomyScope::Project(project.clone()))
            });
            let mut total = 0i64;
            let mut per_source: std::collections::HashMap<SavingsSource, (i64, i64)> =
                std::collections::HashMap::new();
            for b in per_project.values() {
                total += b.total_tokens_saved;
                for row in &b.per_source {
                    let entry = per_source.entry(row.source).or_insert((0, 0));
                    entry.0 += row.tokens_saved;
                    entry.1 += row.event_count;
                }
            }
            let mut rows: Vec<SavingsBySource> = per_source
                .into_iter()
                .map(|(source, (tokens_saved, event_count))| SavingsBySource {
                    source,
                    tokens_saved,
                    event_count,
                })
                .collect();
            rows.sort_by(|a, b| b.tokens_saved.cmp(&a.tokens_saved));
            Ok(SavingsBreakdown {
                total_tokens_saved: total,
                per_source: rows,
            })
        }
        _ => {
            let savings_where_sql = savings_where(&scope);
            let bind_params = savings_params(&scope);
            let sql = format!(
                "SELECT source, COALESCE(SUM(tokens_saved), 0), COUNT(*) \
                 FROM savings_records {savings_where_sql} \
                 GROUP BY source"
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut total = 0i64;
            let mut per_source = Vec::new();
            let rows = stmt.query_map(
                rusqlite::params_from_iter(bind_params.iter()),
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
            )?;
            for row in rows.filter_map(std::result::Result::ok) {
                if let Some(source) = SavingsSource::from_str_opt(&row.0) {
                    total += row.1;
                    per_source.push(SavingsBySource {
                        source,
                        tokens_saved: row.1,
                        event_count: row.2,
                    });
                }
            }
            per_source.sort_by(|a, b| b.tokens_saved.cmp(&a.tokens_saved));
            Ok(SavingsBreakdown {
                total_tokens_saved: total,
                per_source,
            })
        }
    }
}

/// Context-routing quality metrics — cache hit ratio, prefix-stable ratio,
/// retry overhead, all expressed in permille (0–1000) for integer transport.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn context_routing_quality(
    conn: &Connection,
    scope: EconomyScope,
) -> Result<ContextRoutingMetrics> {
    match scope {
        EconomyScope::AllProjects(ref projects) => {
            let reader = MultiProjectReader::new();
            let per_project = reader.fan_out(projects, |c, project| {
                context_routing_quality(c, EconomyScope::Project(project.clone()))
            });
            let mut acc = ContextRoutingMetrics::default();
            let mut weight_total = 0i64;
            for m in per_project.values() {
                let w = m.frame_count.max(1);
                acc.prefix_stable_ratio_permille += m.prefix_stable_ratio_permille * w;
                acc.cache_hit_ratio_permille += m.cache_hit_ratio_permille * w;
                acc.retry_overhead_ratio_permille += m.retry_overhead_ratio_permille * w;
                acc.frame_count += m.frame_count;
                weight_total += w;
            }
            if weight_total > 0 {
                acc.prefix_stable_ratio_permille /= weight_total;
                acc.cache_hit_ratio_permille /= weight_total;
                acc.retry_overhead_ratio_permille /= weight_total;
            }
            Ok(acc)
        }
        _ => {
            let (frame_where, frame_params) = frames_scope_where(&scope);
            let sql = format!(
                "SELECT \
                    COALESCE(SUM(prompt_size_bytes), 0), \
                    COALESCE(SUM(prefix_stable_bytes), 0), \
                    COALESCE(SUM(retry_overhead_bytes), 0), \
                    COUNT(*) \
                 FROM context_cost_frames {frame_where}"
            );
            let (prompt_sum, prefix_sum, retry_sum, frame_count): (i64, i64, i64, i64) = conn
                .query_row(
                    &sql,
                    rusqlite::params_from_iter(frame_params.iter()),
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(Error::from)?;

            // Span ratios (cache hit, prefix-stable share): the `spans` table
            // has no native wave column, so a Wave-scoped caller falls back to
            // the spec roll-up. Today this widens the denominator on the wave
            // breakdown — a real per-wave ratio would route through the
            // attribution CTE the way `economy_summary` does. Tracked as W5
            // follow-up; flagged here so the next reviewer does not chase it.
            let (span_where, span_params) = spans_scope_where(&scope);
            let span_sql = format!(
                "SELECT \
                    COALESCE(SUM(input_tokens), 0), \
                    COALESCE(SUM(cache_read_input_tokens), 0) \
                 FROM {unified} AS spans {span_where}",
                unified = unified_spans_subquery()
            );
            let (input_sum, cache_sum): (i64, i64) = conn
                .query_row(
                    &span_sql,
                    rusqlite::params_from_iter(span_params.iter()),
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(Error::from)?;

            let permille = |num: i64, den: i64| -> i64 {
                if den <= 0 {
                    0
                } else {
                    ((num as f64) * 1000.0 / (den as f64)) as i64
                }
            };

            Ok(ContextRoutingMetrics {
                prefix_stable_ratio_permille: permille(prefix_sum, prompt_sum),
                cache_hit_ratio_permille: permille(cache_sum, input_sum + cache_sum),
                retry_overhead_ratio_permille: permille(retry_sum, prompt_sum),
                frame_count,
            })
        }
    }
}

/// Unified read-only source for cost frames — `spans` ∪ `api_cost_frames`.
///
/// The W3 transcript adapter routes through `record_api_cost` → `spans` today,
/// but a parallel adapter that lands frames directly in the `api_cost_frames`
/// projection table (added in migration v5) would otherwise be invisible to
/// the economy reader. This helper materialises a single union-all subquery
/// over both, projecting the exact column set the reader (and the W4
/// attribution CTE) read from. The non-spans side fills `trace_id`, `name`,
/// `started_at`, etc. with `NULL` since those legacy columns do not survive
/// into the API-cost projection.
///
/// Use as `FROM {} AS spans` or `FROM {} AS s` — the parenthesised subquery
/// supports either alias. Callers must NOT rename columns or assume an
/// ordering; `UNION ALL` preserves duplicates by design (the only intentional
/// duplication path is a fixture test that writes the same span_id to both
/// tables, which is acceptable — the economy reader sums, it does not de-dup).
fn unified_spans_subquery() -> &'static str {
    "(SELECT trace_id, span_id, parent_span_id, name, started_at, ended_at, \
             duration_ms, attributes, spec, phase, model, input_tokens, \
             output_tokens, is_error, cache_read_input_tokens, \
             cache_creation_input_tokens, cost_usd_micros, project_path, \
             ts_iso, session_id, wave_id, tool_use_id \
      FROM spans \
      UNION ALL \
      SELECT NULL AS trace_id, span_id, NULL AS parent_span_id, NULL AS name, \
             NULL AS started_at, NULL AS ended_at, NULL AS duration_ms, \
             NULL AS attributes, spec, phase, model, input_tokens, \
             output_tokens, is_error, cache_read_input_tokens, \
             cache_creation_input_tokens, cost_usd_micros, project_path, \
             ts_iso, session_id, wave_id, tool_use_id \
      FROM api_cost_frames)"
}

// ---------------------------------------------------------------------------
// W4 attribution CTE — single source of truth for the spans↔agent.start join.
//
// Build via [`attribution_cte`] with a `spans` WHERE clause (currently always
// `WHERE 1=1` — scope filtering moved to the outer query, see
// [`wave_filter_for`]). Three columns are projected for downstream GROUP BY:
//
// - `attr_agent_id` — `agent.start.payload.agent_id` ?? `subagentType` ?? `actor_id`
// - `attr_spec_id`  — `agent.start.payload.spec_id`  ?? `events.spec` ?? `spans.spec`
// - `attr_wave_id`  — `agent.start.payload.wave_id`  ?? `CAST(events.wave AS TEXT)` ?? `spans.wave_id`
//
// The final `spans.*` legs keep W1 backward compatibility: a span recorded
// without an `agent.start` (legacy ingest, fixture tests) still attributes
// via its own columns instead of bucketing into the empty-string sentinel.
//
// The two correlated subqueries walk the join legs in priority order — the
// primary one keys on `tool_use_id` (an Anthropic-issued block id present
// when the W3 adapter could harvest it), the fallback walks the temporal
// window (most-recent `agent.start.ts <= spans.ts_iso` in the same session).
// Both are bounded by indices: `idx_spans_tool_use_id` (v4) for the primary,
// `idx_events_event` for the fallback's event-kind filter.
// ---------------------------------------------------------------------------

/// Build the outer-query filter that constrains attributed spans to the scope.
///
/// The W4 attribution flips the W1 ordering: spans no longer carry the
/// authoritative spec/wave (that lives on the joined `agent.start`), so the
/// scope filter applies *after* the CTE resolves attribution rather than
/// before it on the raw `spans` rows.
///
/// Returns the WHERE fragment plus the matching positional params, so the
/// caller passes exactly as many binds as the SQL references — rusqlite
/// rejects extra binds with `"Wrong number of parameters"`.
///
/// - Project / AllProjects → no filter, no params.
/// - Spec → `AND attr_spec_id = ?1` + `[spec]`.
/// - Wave → `AND attr_spec_id = ?1 AND attr_wave_id = ?2` + `[spec, wave]`.
fn wave_filter_for(scope: &EconomyScope) -> (&'static str, Vec<String>) {
    match scope {
        EconomyScope::Wave { spec, wave, .. } => (
            "AND attr_spec_id = ?1 AND attr_wave_id = ?2",
            vec![spec.0.clone(), wave.0.clone()],
        ),
        EconomyScope::Spec { spec, .. } => {
            ("AND attr_spec_id = ?1", vec![spec.0.clone()])
        }
        _ => ("", vec![]),
    }
}

/// Build the `WITH attributed_spans AS (...)` CTE prefix for the W4 join.
///
/// `span_where` is the scope filter as built by [`scope_where`] — it gets
/// inlined into the spans-side of the CTE so the join only walks rows the
/// caller actually wants. The two attribution legs (primary tool_use_id,
/// fallback temporal window) live as correlated subqueries — `COALESCE`
/// short-circuits to the second when the first is `NULL`.
fn attribution_cte(span_where: &str) -> String {
    // The primary leg joins on `JSON_EXTRACT(events.payload, '$.tool_use_id')`,
    // gated by `s.tool_use_id IS NOT NULL` so the index range scan does not
    // touch every event row for spans that have no id to match on.
    //
    // The fallback leg picks the most-recent `agent.start` in the same session
    // whose `ts` is on-or-before `spans.ts_iso`. `events.ts` is an ISO-8601
    // TEXT column — string sort matches chronological sort for that format,
    // so `ORDER BY ev.ts DESC LIMIT 1` is the temporal window upper bound.
    // A future `agent.stop` upper bound is not required: a later `agent.start`
    // in the same session already moves the window forward (the DESC + LIMIT 1
    // selects the most-recent ancestor, which is the active agent).
    format!(
        "WITH attributed_spans AS (\
            SELECT s.cost_usd_micros, s.input_tokens, s.output_tokens, \
                   COALESCE( \
                     (SELECT COALESCE( \
                                JSON_EXTRACT(ev.payload, '$.agent_id'), \
                                JSON_EXTRACT(ev.payload, '$.subagentType'), \
                                ev.actor_id) \
                      FROM events ev \
                      WHERE ev.event = 'agent.start' \
                        AND s.tool_use_id IS NOT NULL \
                        AND JSON_EXTRACT(ev.payload, '$.tool_use_id') = s.tool_use_id \
                      LIMIT 1), \
                     (SELECT COALESCE( \
                                JSON_EXTRACT(ev.payload, '$.agent_id'), \
                                JSON_EXTRACT(ev.payload, '$.subagentType'), \
                                ev.actor_id) \
                      FROM events ev \
                      WHERE ev.event = 'agent.start' \
                        AND ev.session_id IS NOT NULL \
                        AND s.session_id IS NOT NULL \
                        AND ev.session_id = s.session_id \
                        AND ev.ts <= s.ts_iso \
                      ORDER BY ev.ts DESC LIMIT 1) \
                   ) AS attr_agent_id, \
                   COALESCE( \
                     (SELECT COALESCE(JSON_EXTRACT(ev.payload, '$.spec_id'), ev.spec) \
                      FROM events ev \
                      WHERE ev.event = 'agent.start' \
                        AND s.tool_use_id IS NOT NULL \
                        AND JSON_EXTRACT(ev.payload, '$.tool_use_id') = s.tool_use_id \
                      LIMIT 1), \
                     (SELECT COALESCE(JSON_EXTRACT(ev.payload, '$.spec_id'), ev.spec) \
                      FROM events ev \
                      WHERE ev.event = 'agent.start' \
                        AND ev.session_id IS NOT NULL \
                        AND s.session_id IS NOT NULL \
                        AND ev.session_id = s.session_id \
                        AND ev.ts <= s.ts_iso \
                      ORDER BY ev.ts DESC LIMIT 1), \
                     s.spec \
                   ) AS attr_spec_id, \
                   COALESCE( \
                     (SELECT COALESCE(JSON_EXTRACT(ev.payload, '$.wave_id'), \
                                      CAST(ev.wave AS TEXT)) \
                      FROM events ev \
                      WHERE ev.event = 'agent.start' \
                        AND s.tool_use_id IS NOT NULL \
                        AND JSON_EXTRACT(ev.payload, '$.tool_use_id') = s.tool_use_id \
                      LIMIT 1), \
                     (SELECT COALESCE(JSON_EXTRACT(ev.payload, '$.wave_id'), \
                                      CAST(ev.wave AS TEXT)) \
                      FROM events ev \
                      WHERE ev.event = 'agent.start' \
                        AND ev.session_id IS NOT NULL \
                        AND s.session_id IS NOT NULL \
                        AND ev.session_id = s.session_id \
                        AND ev.ts <= s.ts_iso \
                      ORDER BY ev.ts DESC LIMIT 1), \
                     s.wave_id \
                   ) AS attr_wave_id \
            FROM {unified} s {span_where} \
         )",
        unified = unified_spans_subquery()
    )
}

// ---------------------------------------------------------------------------
// Shared scope-to-SQL helpers.
//
// Each helper returns `(where_clause, params)` where `params` is the exact
// list of bind values referenced by the SQL — no `?2 = ?2` tautologies, no
// `NULL IS NULL` placeholders. Callers feed the params into rusqlite via
// `params_from_iter`, so the helper's `Vec` length matches the SQL's `?N`
// count for every scope variant. Wave-scoped callers on the `spans` table
// must route through [`attribution_cte`] separately — spans have no native
// wave column, so this helper deliberately collapses Wave→Spec for the
// `spans` SQL (the real wave filter lives in the CTE-driven query path).
// ---------------------------------------------------------------------------

/// Builds the `WHERE` clause + bind list for the `spans` table.
///
/// Wave scope collapses to Spec for this helper: the `spans` table has no
/// native `wave_id` column, so Wave-aware callers must use the attribution
/// CTE (see `economy_summary`). Returning a real filter here — instead of
/// the legacy `?2 = ?2` tautology — keeps the bug from compiling silently.
fn spans_scope_where(scope: &EconomyScope) -> (&'static str, Vec<String>) {
    match scope {
        EconomyScope::Project(_) | EconomyScope::AllProjects(_) => ("", Vec::new()),
        EconomyScope::Spec { spec, .. } | EconomyScope::Wave { spec, .. } => {
            ("WHERE spec = ?1", vec![spec.0.clone()])
        }
    }
}

/// Builds the `WHERE` clause + bind list for `context_cost_frames`
/// (which has native `spec_id` and `wave_id` columns).
fn frames_scope_where(scope: &EconomyScope) -> (&'static str, Vec<String>) {
    match scope {
        EconomyScope::Project(_) | EconomyScope::AllProjects(_) => ("", Vec::new()),
        EconomyScope::Spec { spec, .. } => ("WHERE spec_id = ?1", vec![spec.0.clone()]),
        EconomyScope::Wave { spec, wave, .. } => (
            "WHERE spec_id = ?1 AND wave_id = ?2",
            vec![spec.0.clone(), wave.0.clone()],
        ),
    }
}

/// Builds the `WHERE` clause for `savings_records`.
///
/// `savings_records` carries native `spec_id` + `wave_id` columns, so every
/// scope variant gets a real, non-tautological filter. Pair with
/// [`savings_params`] to bind the right number of positional params.
fn savings_where(scope: &EconomyScope) -> &'static str {
    match scope {
        EconomyScope::Project(_) | EconomyScope::AllProjects(_) => "",
        EconomyScope::Spec { .. } => "WHERE spec_id = ?1",
        EconomyScope::Wave { .. } => "WHERE spec_id = ?1 AND wave_id = ?2",
    }
}

/// Positional bind list for the `savings_records` SQL built by
/// [`savings_where`] — length matches the number of `?N` placeholders.
fn savings_params(scope: &EconomyScope) -> Vec<String> {
    match scope {
        EconomyScope::Project(_) | EconomyScope::AllProjects(_) => Vec::new(),
        EconomyScope::Spec { spec, .. } => vec![spec.0.clone()],
        EconomyScope::Wave { spec, wave, .. } => vec![spec.0.clone(), wave.0.clone()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::model::{SavingsRecord, SpanRecord};
    use crate::economy::scope::{AgentId, ProjectPath};
    use crate::economy::writer::{record_savings, record_span};
    use crate::store::sqlite_store::SqliteEventStore;
    use rusqlite::Connection;
    use serde_json::Map;
    use tempfile::tempdir;

    fn fresh_conn(dir: &std::path::Path) -> Connection {
        let _store = SqliteEventStore::new(dir.join("mustard.db")).unwrap();
        Connection::open(dir.join("mustard.db")).unwrap()
    }

    fn span(id: &str, spec: &str, cost: i64, tokens: i64) -> SpanRecord {
        SpanRecord {
            ts: "2026-05-21T00:00:00Z".into(),
            session_id: None,
            span_id: id.into(),
            model: Some("claude-3-5-sonnet".into()),
            spec: Some(spec.into()),
            phase: None,
            input_tokens: Some(tokens),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
            cost_usd_micros: Some(cost),
            is_error: false,
            extra: Map::new(),
        }
    }

    #[test]
    fn economy_summary_aggregates_spans_and_savings() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(dir.path());
        record_span(&conn, span("s1", "spec-A", 1000, 100)).unwrap();
        record_span(&conn, span("s2", "spec-A", 2000, 200)).unwrap();
        record_savings(
            &conn,
            SavingsRecord {
                ts: "2026-05-21T00:00:00Z".into(),
                source: SavingsSource::RtkRewrite,
                tokens_saved: 500,
                model_target: None,
                project_path: ProjectPath::new("/tmp/p"),
                spec_id: Some(SpecId::new("spec-A")),
                wave_id: None,
                agent_id: Some(AgentId::new("explore")),
                extra: Map::new(),
            },
        )
        .unwrap();

        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let summary = economy_summary(&conn, scope).unwrap();
        assert_eq!(summary.total_cost_usd_micros, 3000);
        assert_eq!(summary.total_tokens, 300);
        assert_eq!(summary.total_tokens_saved, 500);
        assert_eq!(summary.span_count, 2);
    }

    #[test]
    fn savings_breakdown_groups_by_source() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(dir.path());
        for src in [
            SavingsSource::RtkRewrite,
            SavingsSource::RtkRewrite,
            SavingsSource::BashGuardBlock,
        ] {
            record_savings(
                &conn,
                SavingsRecord {
                    ts: "2026-05-21T00:00:00Z".into(),
                    source: src,
                    tokens_saved: 100,
                    model_target: None,
                    project_path: ProjectPath::new("/tmp/p"),
                    spec_id: None,
                    wave_id: None,
                    agent_id: None,
                    extra: Map::new(),
                },
            )
            .unwrap();
        }
        let breakdown =
            savings_breakdown(&conn, EconomyScope::Project(ProjectPath::new(dir.path())))
                .unwrap();
        assert_eq!(breakdown.total_tokens_saved, 300);
        assert_eq!(breakdown.per_source.len(), 2);
        // First entry is the larger one (RtkRewrite, 200 saved).
        assert_eq!(breakdown.per_source[0].source, SavingsSource::RtkRewrite);
        assert_eq!(breakdown.per_source[0].tokens_saved, 200);
    }
}
