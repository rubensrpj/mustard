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
//! side.
//!
//! ## Wave 3 — self-attributed reads, no JOIN
//!
//! The per-spec / per-wave / per-agent roll-ups (and the cost half of
//! `economy_summary`) used to read the `spans` table in `mustard.db` and
//! recover spec/wave/agent through the legacy W4 attribution CTE (a `spans`
//! LEFT JOIN on the agent-launch event). Telemetry now lives in a dedicated
//! `telemetry.db` whose `run_usage` table is **self-attributed** — `spec` /
//! `wave_id` / `agent_id` are stamped at write time (Wave 2) and backfilled for
//! history — so the read is a plain `GROUP BY` via
//! [`crate::telemetry::reader`]. The savings (`savings_records`) and
//! context-frame (`context_cost_frames`) aggregations still read the passed
//! `mustard.db` connection: those tables are not telemetry.

use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::telemetry::{self, TelemetryStore};

use super::model::{
    AgentCost, ContextRoutingMetrics, EconomySummary, SavingsBreakdown, SavingsBySource,
    SavingsSource, SessionCost, SpecCost, WaveCost,
};
use super::multi_project::MultiProjectReader;
use super::scope::{AgentId, EconomyScope, SpecId, WaveId};

/// Open the dedicated telemetry store for the project a scope is rooted at.
///
/// Single-project scopes carry their own root; `AllProjects` is handled by the
/// fan-out before this is reached, so its first path is a safe bootstrap. The
/// `MUSTARD_TELEMETRY_DB_PATH` env override is honoured by
/// [`TelemetryStore::for_project`], which is what the reader tests rely on.
fn open_telemetry(scope: &EconomyScope) -> Result<TelemetryStore> {
    let project = scope
        .project_paths()
        .first()
        .map(|p| p.as_path().to_path_buf())
        .unwrap_or_default();
    TelemetryStore::for_project(project)
}

/// `(spec, wave)` filter for the telemetry `run_usage` reads, derived from the
/// scope. Wave scope yields both; Spec yields the spec only; Project/AllProjects
/// yield neither.
fn telemetry_filter(scope: &EconomyScope) -> (Option<String>, Option<String>) {
    (
        scope.spec_filter().map(|s| s.0.clone()),
        scope.wave_filter().map(|w| w.0.clone()),
    )
}

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
                // Per-project summaries are unfiltered (Project scope), so each
                // already carries its own measured `by_session` / freshness.
                acc.by_session.extend(s.by_session.clone());
                acc.last_updated_ms = acc.last_updated_ms.max(s.last_updated_ms);
            }
            // Re-sort and truncate top agents after the merge.
            acc.top_agents_by_cost
                .sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
            acc.top_agents_by_cost.truncate(3);
            // Re-sort + cap the merged sessions the same way the single-project
            // path does (top by USD descending).
            acc.by_session
                .sort_by(|a, b| b.usd.partial_cmp(&a.usd).unwrap_or(std::cmp::Ordering::Equal));
            acc.by_session.truncate(8);
            Ok(acc)
        }
        _ => {
            // Run count always comes from telemetry.db's self-attributed
            // `run_usage` (Wave 3): it's a count of dispatched runs, meaningful
            // at every scope. The estimated cost/token totals from the same
            // query are only used when the scope filters by spec or wave.
            let (spec_f, wave_f) = telemetry_filter(&scope);
            let tele = open_telemetry(&scope)?;
            let (est_cost, est_tokens, span_count) = telemetry::reader::scoped_totals(
                tele.conn(),
                spec_f.as_deref(),
                wave_f.as_deref(),
            )?;

            // For an unfiltered (project-wide / single-project) scope the
            // headline cost + token totals come from the MEASURED `usage_totals`
            // OTEL counters (Anthropic's billed `claude_code.cost.usage` /
            // `.token.usage`), which carry no spec/wave dimension. When the scope
            // DOES filter by spec or wave we fall back to the ESTIMATED
            // `run_usage` totals — `usage_totals` cannot be attributed to a spec.
            let unfiltered = spec_f.is_none() && wave_f.is_none();
            let (total_cost, total_tokens) = if unfiltered {
                let measured_cost_usd = telemetry::reader::cost_total(tele.conn())?;
                let measured_tokens = telemetry::reader::token_total(tele.conn())?;
                // `usage_totals` carries cost in USD and tokens as a float
                // counter; `EconomySummary` is micro-USD + integer tokens.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let cost_micros = (measured_cost_usd * 1_000_000.0).round() as i64;
                #[allow(clippy::cast_possible_truncation)]
                let tokens = measured_tokens.round() as i64;
                (cost_micros, tokens)
            } else {
                (est_cost, est_tokens)
            };

            // MEASURED cost-by-session + freshness — populated ONLY at the
            // unfiltered (project) scope, since `usage_totals` carries no
            // spec/wave dimension. At spec/wave scope these stay empty/None.
            // Top sessions by USD, capped so the UI list stays compact.
            let (by_session, last_updated_ms) = if unfiltered {
                let sessions = telemetry::reader::cost_by_session(tele.conn())?
                    .into_iter()
                    .filter(|(_, usd)| *usd > 0.0)
                    .take(8)
                    .map(|(session_id, usd)| {
                        // Same telemetry.db connection — never cross into mustard.db
                        // from a telemetry read. Both helpers are fail-open at the
                        // SQL layer, so a degraded row still surfaces the cost.
                        let last_at_ms =
                            telemetry::reader::session_last_at(tele.conn(), &session_id)
                                .unwrap_or(None);
                        let specs = telemetry::reader::specs_for_session(tele.conn(), &session_id)
                            .unwrap_or_default();
                        SessionCost {
                            session_id,
                            usd,
                            last_at_ms,
                            specs,
                        }
                    })
                    .collect();
                let fresh = telemetry::reader::freshness(tele.conn())?;
                (sessions, fresh)
            } else {
                (Vec::new(), None)
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
                by_session,
                last_updated_ms,
            })
        }
    }
}

/// Per-agent cost roll-up, ordered by cost descending.
///
/// Reads telemetry.db's self-attributed `run_usage` (Wave 3): each row carries
/// its own `agent_id` (stamped at write time, backfilled for history), so the
/// roll-up is a plain `GROUP BY agent_id`. Rows with a `NULL`/empty `agent_id`
/// are excluded — they have no agent to attribute to, matching the legacy
/// behaviour. A Wave scope additionally filters on `wave_id`.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn per_agent_costs(_conn: &Connection, scope: EconomyScope) -> Result<Vec<AgentCost>> {
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
            let (_, wave_f) = telemetry_filter(&scope);
            let tele = open_telemetry(&scope)?;
            let groups =
                telemetry::reader::runs_by_agent_scoped(tele.conn(), None, wave_f.as_deref())?;
            Ok(groups
                .into_iter()
                .map(|g| AgentCost {
                    agent_id: AgentId(g.key),
                    cost_usd_micros: g.cost_usd_micros,
                    tokens: g.tokens,
                    span_count: g.run_count,
                })
                .collect())
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
pub fn per_spec_costs(_conn: &Connection, scope: EconomyScope) -> Result<Vec<SpecCost>> {
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
            // Self-attributed `run_usage` (Wave 3): GROUP BY the native `spec`
            // column. A Wave scope additionally filters on `wave_id`.
            let (_, wave_f) = telemetry_filter(&scope);
            let tele = open_telemetry(&scope)?;
            let groups = telemetry::reader::runs_by_spec_scoped(tele.conn(), wave_f.as_deref())?;
            Ok(groups
                .into_iter()
                .map(|g| SpecCost {
                    spec_id: SpecId(g.key),
                    cost_usd_micros: g.cost_usd_micros,
                    tokens: g.tokens,
                    span_count: g.run_count,
                })
                .collect())
        }
    }
}

/// Per-wave cost roll-up. The wave id is the run row's own `wave_id` column
/// (self-attributed at write time), so runs dispatched into a child wave from a
/// parent-spec context are correctly bucketed.
///
/// Regression-tested by `parent_spec_child_wave_attribution` in
/// `tests/economy_attribution.rs` (AC-6, absorbed from the superseded
/// `metrics-writers-pipeline-key` spec).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn per_wave_costs(_conn: &Connection, scope: EconomyScope) -> Result<Vec<WaveCost>> {
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
            // Self-attributed `run_usage` (Wave 3): GROUP BY (spec, wave_id).
            // Each row carries the wave it was dispatched against, which is the
            // answer the parent-spec/child-wave regression case needs.
            let (_, wave_f) = telemetry_filter(&scope);
            let tele = open_telemetry(&scope)?;
            let groups = telemetry::reader::runs_by_wave_scoped(tele.conn(), wave_f.as_deref())?;
            Ok(groups
                .into_iter()
                .map(|g| WaveCost {
                    spec_id: SpecId(g.spec),
                    wave_id: WaveId(g.wave_id),
                    cost_usd_micros: g.cost_usd_micros,
                    tokens: g.tokens,
                    span_count: g.run_count,
                })
                .collect())
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

            // Cache-hit ratio comes from telemetry.db's `run_usage` (Wave 3).
            // `run_usage` has no native wave column on this read path either, so
            // a Wave-scoped caller still collapses to the spec roll-up — same
            // denominator behaviour as before, just sourced self-attributed.
            let (spec_f, _) = telemetry_filter(&scope);
            let tele = open_telemetry(&scope)?;
            let cache_hit_ratio_permille =
                telemetry::reader::cache_hit_ratio_permille_for_spec(tele.conn(), spec_f.as_deref())?;

            let permille = |num: i64, den: i64| -> i64 {
                if den <= 0 {
                    0
                } else {
                    ((num as f64) * 1000.0 / (den as f64)) as i64
                }
            };

            Ok(ContextRoutingMetrics {
                prefix_stable_ratio_permille: permille(prefix_sum, prompt_sum),
                cache_hit_ratio_permille,
                retry_overhead_ratio_permille: permille(retry_sum, prompt_sum),
                frame_count,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Shared scope-to-SQL helpers for the NON-telemetry tables that still live in
// `mustard.db` (`savings_records`, `context_cost_frames`). The span-based
// reads moved to `telemetry::reader` (self-attributed `run_usage`, Wave 3), so
// there is no longer a spans↔events JOIN to express here.
//
// Each helper returns `(where_clause, params)` where `params` is the exact
// list of bind values referenced by the SQL — no `?2 = ?2` tautologies, no
// `NULL IS NULL` placeholders. Callers feed the params into rusqlite via
// `params_from_iter`, so the helper's `Vec` length matches the SQL's `?N`
// count for every scope variant.
// ---------------------------------------------------------------------------

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
    use crate::economy::model::SavingsRecord;
    use crate::economy::scope::{AgentId, ProjectPath};
    use crate::economy::writer::record_savings;
    use crate::store::sqlite_store::SqliteEventStore;
    use crate::telemetry::TelemetryWriter;
    use crate::telemetry::model::{RunUsage, UsageMetric};
    use crate::telemetry::writer::upsert_usage_metric;
    use rusqlite::Connection;
    use serde_json::Map;
    use tempfile::tempdir;

    fn fresh_conn(dir: &std::path::Path) -> Connection {
        let _store = SqliteEventStore::new(dir.join("mustard.db")).unwrap();
        Connection::open(dir.join("mustard.db")).unwrap()
    }

    /// Seed one `run_usage` row into the telemetry.db the economy reader opens
    /// for a project (`{project}/.claude/.harness/telemetry.db`). Wave 3 moved
    /// every span-based aggregation onto the self-attributed `run_usage` table,
    /// so the reader no longer touches the legacy `spans` table.
    fn seed_run(dir: &std::path::Path, id: &str, spec: &str, cost: i64, tokens: i64) {
        let store = TelemetryStore::for_project(dir).unwrap();
        store
            .record_run(&RunUsage {
                trace_id: None,
                span_id: id.into(),
                parent_span_id: None,
                name: None,
                started_at: Some(0),
                ended_at: None,
                duration_ms: None,
                attributes: None,
                spec: Some(spec.into()),
                phase: None,
                model: Some("claude-3-5-sonnet".into()),
                input_tokens: Some(tokens),
                output_tokens: Some(0),
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
                cost_usd_micros: Some(cost),
                is_error: false,
                project_path: None,
                ts_iso: Some("2026-05-21T00:00:00Z".into()),
                session_id: None,
                wave_id: None,
                tool_use_id: None,
                agent_id: Some("explore".into()),
            })
            .unwrap();
    }

    /// Seed one MEASURED `usage_totals` counter row (Anthropic's billed OTEL
    /// metric) into the same telemetry.db. `cost.usage` is USD, `token.usage`
    /// is a token count — both float counters with no spec/wave dimension.
    fn seed_measured(dir: &std::path::Path, metric: &str, sum: f64) {
        seed_measured_at(dir, metric, sum, "sess-1", 0);
    }

    /// Same as [`seed_measured`] but with an explicit `session_id` + `updated_at`
    /// so the per-session enrichment test can populate distinct sessions.
    fn seed_measured_at(
        dir: &std::path::Path,
        metric: &str,
        sum: f64,
        session_id: &str,
        updated_at: i64,
    ) {
        let store = TelemetryStore::for_project(dir).unwrap();
        upsert_usage_metric(
            store.conn(),
            &UsageMetric {
                metric: metric.into(),
                model: Some("claude-3-5-sonnet".into()),
                session_id: Some(session_id.into()),
                sum,
                updated_at: Some(updated_at),
            },
        )
        .unwrap();
    }

    /// Seed one `run_usage` row carrying an explicit `session_id` so the
    /// per-session enrichment can pick up its specs.
    fn seed_run_for_session(
        dir: &std::path::Path,
        span_id: &str,
        spec: &str,
        session_id: &str,
    ) {
        let store = TelemetryStore::for_project(dir).unwrap();
        store
            .record_run(&RunUsage {
                trace_id: None,
                span_id: span_id.into(),
                parent_span_id: None,
                name: None,
                started_at: Some(0),
                ended_at: None,
                duration_ms: None,
                attributes: None,
                spec: Some(spec.into()),
                phase: None,
                model: Some("claude-3-5-sonnet".into()),
                input_tokens: Some(10),
                output_tokens: Some(0),
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
                cost_usd_micros: Some(100),
                is_error: false,
                project_path: None,
                ts_iso: Some("2026-05-22T00:00:00Z".into()),
                session_id: Some(session_id.into()),
                wave_id: None,
                tool_use_id: None,
                agent_id: Some("explore".into()),
            })
            .unwrap();
    }

    #[test]
    fn economy_summary_unfiltered_uses_measured_totals() {
        // Project scope (no spec/wave filter): headline cost + tokens come from
        // the MEASURED `usage_totals`, NOT the ESTIMATED `run_usage` sums.
        let dir = tempdir().unwrap();
        let conn = fresh_conn(dir.path());
        // Estimated run_usage: cost 3000 micros, 300 tokens — must be ignored.
        seed_run(dir.path(), "s1", "spec-A", 1000, 100);
        seed_run(dir.path(), "s2", "spec-A", 2000, 200);
        // Measured: $49.00 cost, 1234 tokens.
        seed_measured(dir.path(), "claude_code.cost.usage", 49.0);
        seed_measured(dir.path(), "claude_code.token.usage", 1234.0);
        record_savings(
            &conn,
            SavingsRecord {
                ts: "2026-05-21T00:00:00Z".into(),
                source: SavingsSource::RtkRewrite,
                tokens_saved: 500,
                model_target: None,
                project_path: ProjectPath::new("/tmp/p"),
                spec_id: None,
                wave_id: None,
                agent_id: None,
                extra: Map::new(),
            },
        )
        .unwrap();

        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let summary = economy_summary(&conn, scope).unwrap();
        // Measured cost: $49.00 -> 49_000_000 micro-USD (not the 3000 estimate).
        assert_eq!(summary.total_cost_usd_micros, 49_000_000);
        // Measured tokens: 1234 (not the 300 estimate).
        assert_eq!(summary.total_tokens, 1234);
        // Run count + savings still come from run_usage / savings_records.
        assert_eq!(summary.span_count, 2);
        assert_eq!(summary.total_tokens_saved, 500);
    }

    #[test]
    fn economy_summary_spec_scope_uses_estimated_run_usage() {
        // Spec scope: usage_totals has no spec dimension, so the totals stay on
        // the ESTIMATED run_usage path even when measured counters are present.
        let dir = tempdir().unwrap();
        let conn = fresh_conn(dir.path());
        seed_run(dir.path(), "s1", "spec-A", 1000, 100);
        seed_run(dir.path(), "s2", "spec-A", 2000, 200);
        // Measured present but must NOT leak into a spec-scoped summary.
        seed_measured(dir.path(), "claude_code.cost.usage", 49.0);
        seed_measured(dir.path(), "claude_code.token.usage", 1234.0);

        let scope = EconomyScope::Spec {
            project: ProjectPath::new(dir.path()),
            spec: SpecId::new("spec-A"),
        };
        let summary = economy_summary(&conn, scope).unwrap();
        // Estimated run_usage totals (3000 micros, 300 tokens), not measured.
        assert_eq!(summary.total_cost_usd_micros, 3000);
        assert_eq!(summary.total_tokens, 300);
        assert_eq!(summary.span_count, 2);
    }

    #[test]
    fn economy_summary_aggregates_spans_and_savings() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(dir.path());
        seed_run(dir.path(), "s1", "spec-A", 1000, 100);
        seed_run(dir.path(), "s2", "spec-A", 2000, 200);
        // Measured totals so the unfiltered project scope has a real headline.
        seed_measured(dir.path(), "claude_code.cost.usage", 0.003);
        seed_measured(dir.path(), "claude_code.token.usage", 300.0);
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
    fn by_session_populated_with_specs_and_last_at_at_project_scope() {
        // Project (unfiltered) scope must enrich each `by_session` row with the
        // per-session freshness (`usage_totals.updated_at`) and the distinct
        // specs the session worked on (`run_usage.spec`). Spec/wave scope keeps
        // `by_session` empty, so this only exercises the unfiltered path.
        let dir = tempdir().unwrap();
        let conn = fresh_conn(dir.path());
        // Session A — measured cost 12 USD at updated_at=2000; two specs.
        seed_measured_at(dir.path(), "claude_code.cost.usage", 12.0, "sess-A", 2000);
        seed_run_for_session(dir.path(), "r1", "spec-Alpha", "sess-A");
        seed_run_for_session(dir.path(), "r2", "spec-Beta", "sess-A");
        seed_run_for_session(dir.path(), "r3", "spec-Alpha", "sess-A");
        // Session B — measured cost 1 USD at updated_at=1000; one spec.
        seed_measured_at(dir.path(), "claude_code.cost.usage", 1.0, "sess-B", 1000);
        seed_run_for_session(dir.path(), "r4", "spec-Gamma", "sess-B");

        let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
        let summary = economy_summary(&conn, scope).unwrap();
        // Ordered by USD descending: sess-A (12) before sess-B (1).
        assert_eq!(summary.by_session.len(), 2);
        let a = &summary.by_session[0];
        assert_eq!(a.session_id, "sess-A");
        assert_eq!(a.last_at_ms, Some(2000));
        assert_eq!(a.specs, vec!["spec-Alpha".to_string(), "spec-Beta".into()]);
        let b = &summary.by_session[1];
        assert_eq!(b.session_id, "sess-B");
        assert_eq!(b.last_at_ms, Some(1000));
        assert_eq!(b.specs, vec!["spec-Gamma".to_string()]);
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
