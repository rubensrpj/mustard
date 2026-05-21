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
//! The single-project SQL stays *minimal* — no joins beyond what is
//! strictly necessary, no projection beyond what the aggregate needs. The
//! intent is that hooks, dashboards, and tests all call the same six entry
//! points; UI-specific shaping happens on the consumer side.

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
            let per_project = reader.fan_out(projects, |c| {
                economy_summary(c, EconomyScope::Project(projects[0].clone()))
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
            let (where_sql, spec_param, wave_param) = scope_where(&scope);
            let span_sql = format!(
                "SELECT COALESCE(SUM(cost_usd_micros), 0), \
                        COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                        COUNT(*) \
                 FROM spans {where_sql}"
            );
            let (total_cost, total_tokens, span_count): (i64, i64, i64) = conn
                .query_row(
                    &span_sql,
                    params![spec_param.as_deref(), wave_param.as_deref()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(Error::from)?;

            let savings_sql = format!(
                "SELECT COALESCE(SUM(tokens_saved), 0) FROM savings_records \
                 {}",
                savings_where(&scope)
            );
            let total_saved: i64 = conn
                .query_row(
                    &savings_sql,
                    params![spec_param.as_deref(), wave_param.as_deref()],
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
/// Agents are derived from `context_cost_frames.agent_id` joined back to spans
/// by `(spec, wave, ts)`. In W1 we keep the join naive — spans and frames are
/// not yet linked by a hard FK; aggregation is at the agent_id level using
/// the same scope filter.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn per_agent_costs(conn: &Connection, scope: EconomyScope) -> Result<Vec<AgentCost>> {
    match scope {
        EconomyScope::AllProjects(ref projects) => {
            let reader = MultiProjectReader::new();
            let per_project = reader.fan_out(projects, |c| {
                per_agent_costs(c, EconomyScope::Project(projects[0].clone()))
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
            let (where_sql, spec_param, wave_param) = scope_where(&scope);
            // Aggregate over context_cost_frames as the agent attribution source;
            // join nothing — cost lives on spans and is derived per-frame using
            // the same scope filter (broad approximation; W2 tightens this).
            let frame_sql = format!(
                "SELECT agent_id, \
                        COUNT(*) AS frame_count \
                 FROM context_cost_frames {where_sql} \
                 GROUP BY agent_id",
                where_sql = where_sql.replace("spec = ?1", "spec_id = ?1")
                    .replace("wave_id = ?2", "wave_id = ?2"),
            );
            let mut stmt = conn.prepare(&frame_sql)?;
            let agent_rows = stmt
                .query_map(
                    params![spec_param.as_deref(), wave_param.as_deref()],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
                )?
                .filter_map(std::result::Result::ok)
                .collect::<Vec<_>>();

            // For each agent, sum spans cost where the wider scope filter
            // applies. Spans are not directly attributed to an agent in W1, so
            // we proportionally distribute the in-scope span total by frame
            // share (W2 instruments per-dispatch span ids and removes this).
            let span_sql = format!(
                "SELECT COALESCE(SUM(cost_usd_micros), 0), \
                        COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                        COUNT(*) \
                 FROM spans {where_sql}"
            );
            let (total_cost, total_tokens, total_span_count): (i64, i64, i64) = conn
                .query_row(
                    &span_sql,
                    params![spec_param.as_deref(), wave_param.as_deref()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(Error::from)?;
            let total_frames: i64 = agent_rows.iter().map(|(_, c)| *c).sum();
            let mut out: Vec<AgentCost> = agent_rows
                .into_iter()
                .map(|(agent_id, frame_count)| {
                    let share = if total_frames > 0 {
                        f64::from(i32::try_from(frame_count).unwrap_or(0))
                            / f64::from(i32::try_from(total_frames).unwrap_or(1))
                    } else {
                        0.0
                    };
                    AgentCost {
                        agent_id: AgentId(agent_id),
                        cost_usd_micros: ((total_cost as f64) * share) as i64,
                        tokens: ((total_tokens as f64) * share) as i64,
                        span_count: ((total_span_count as f64) * share) as i64,
                    }
                })
                .collect();
            out.sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
            Ok(out)
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
            let per_project = reader.fan_out(projects, |c| {
                per_spec_costs(c, EconomyScope::Project(projects[0].clone()))
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
            let (where_sql, spec_param, wave_param) = scope_where(&scope);
            let sql = format!(
                "SELECT COALESCE(spec, '') AS s, \
                        COALESCE(SUM(cost_usd_micros), 0), \
                        COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                        COUNT(*) \
                 FROM spans {where_sql} \
                 GROUP BY s \
                 ORDER BY 2 DESC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(
                    params![spec_param.as_deref(), wave_param.as_deref()],
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

/// Per-wave cost roll-up. Sourced from `context_cost_frames.wave_id`; spans
/// alone do not carry a wave id, so the join is by `spec_id` + frame's
/// wave bucket.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a database failure.
pub fn per_wave_costs(conn: &Connection, scope: EconomyScope) -> Result<Vec<WaveCost>> {
    match scope {
        EconomyScope::AllProjects(ref projects) => {
            let reader = MultiProjectReader::new();
            let per_project = reader.fan_out(projects, |c| {
                per_wave_costs(c, EconomyScope::Project(projects[0].clone()))
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
            let (frame_where, spec_param, wave_param) = scope_where_frames(&scope);
            let sql = format!(
                "SELECT COALESCE(spec_id, '') AS s, COALESCE(wave_id, '') AS w, \
                        COUNT(*) AS frame_count \
                 FROM context_cost_frames {frame_where} \
                 GROUP BY s, w \
                 ORDER BY frame_count DESC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(
                    params![spec_param.as_deref(), wave_param.as_deref()],
                    |r| {
                        Ok(WaveCost {
                            spec_id: SpecId(r.get(0)?),
                            wave_id: WaveId(r.get(1)?),
                            cost_usd_micros: 0, // populated below.
                            tokens: 0,
                            span_count: r.get(2)?,
                        })
                    },
                )?
                .filter_map(std::result::Result::ok)
                .collect::<Vec<_>>();
            // Per-wave cost is approximated from the share of frames in the
            // wave times the in-scope span total — same approximation
            // documented in `per_agent_costs`.
            let (span_where, spec_param2, wave_param2) = scope_where(&scope);
            let span_sql = format!(
                "SELECT COALESCE(SUM(cost_usd_micros), 0), \
                        COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) \
                 FROM spans {span_where}"
            );
            let (total_cost, total_tokens): (i64, i64) = conn
                .query_row(
                    &span_sql,
                    params![spec_param2.as_deref(), wave_param2.as_deref()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(Error::from)?;
            let total_frames: i64 = rows.iter().map(|r| r.span_count).sum();
            let mut out: Vec<WaveCost> = rows
                .into_iter()
                .map(|mut w| {
                    let share = if total_frames > 0 {
                        (w.span_count as f64) / (total_frames as f64)
                    } else {
                        0.0
                    };
                    w.cost_usd_micros = ((total_cost as f64) * share) as i64;
                    w.tokens = ((total_tokens as f64) * share) as i64;
                    w
                })
                .collect();
            out.sort_by(|a, b| b.cost_usd_micros.cmp(&a.cost_usd_micros));
            Ok(out)
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
            let per_project = reader.fan_out(projects, |c| {
                savings_breakdown(c, EconomyScope::Project(projects[0].clone()))
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
            let (_, spec_param, wave_param) = scope_where(&scope);
            let sql = format!(
                "SELECT source, COALESCE(SUM(tokens_saved), 0), COUNT(*) \
                 FROM savings_records {savings_where_sql} \
                 GROUP BY source"
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut total = 0i64;
            let mut per_source = Vec::new();
            let rows = stmt.query_map(
                params![spec_param.as_deref(), wave_param.as_deref()],
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
            let per_project = reader.fan_out(projects, |c| {
                context_routing_quality(c, EconomyScope::Project(projects[0].clone()))
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
            let (frame_where, spec_param, wave_param) = scope_where_frames(&scope);
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
                    params![spec_param.as_deref(), wave_param.as_deref()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(Error::from)?;

            let (span_where, sp2, wp2) = scope_where(&scope);
            let span_sql = format!(
                "SELECT \
                    COALESCE(SUM(input_tokens), 0), \
                    COALESCE(SUM(cache_read_input_tokens), 0) \
                 FROM spans {span_where}"
            );
            let (input_sum, cache_sum): (i64, i64) = conn
                .query_row(
                    &span_sql,
                    params![sp2.as_deref(), wp2.as_deref()],
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

// ---------------------------------------------------------------------------
// Shared scope-to-SQL helpers.
//
// Returns: (where-clause-with-numbered-binds, spec_param, wave_param).
// Binds are positional (`?1`, `?2`) so callers always pass the same params
// list shape regardless of the variant.
// ---------------------------------------------------------------------------

/// Builds the `WHERE` clause for the `spans` table.
fn scope_where(scope: &EconomyScope) -> (String, Option<String>, Option<String>) {
    match scope {
        EconomyScope::Project(_) | EconomyScope::AllProjects(_) => {
            ("WHERE (?1 IS NULL OR spec = ?1) AND (?2 IS NULL OR ?2 = ?2)".to_string(), None, None)
        }
        EconomyScope::Spec { spec, .. } => (
            "WHERE spec = ?1 AND (?2 IS NULL OR ?2 = ?2)".to_string(),
            Some(spec.0.clone()),
            None,
        ),
        EconomyScope::Wave { spec, wave, .. } => (
            // Spans do not carry a native wave column; the wave filter is
            // injected via context_cost_frames in callers that need it.
            "WHERE spec = ?1 AND (?2 IS NULL OR ?2 = ?2)".to_string(),
            Some(spec.0.clone()),
            Some(wave.0.clone()),
        ),
    }
}

/// Builds the `WHERE` clause for `context_cost_frames` (which has its own
/// `spec_id` + `wave_id` columns).
fn scope_where_frames(scope: &EconomyScope) -> (String, Option<String>, Option<String>) {
    match scope {
        EconomyScope::Project(_) | EconomyScope::AllProjects(_) => (
            "WHERE (?1 IS NULL OR spec_id = ?1) AND (?2 IS NULL OR wave_id = ?2)".to_string(),
            None,
            None,
        ),
        EconomyScope::Spec { spec, .. } => (
            "WHERE spec_id = ?1 AND (?2 IS NULL OR wave_id = ?2)".to_string(),
            Some(spec.0.clone()),
            None,
        ),
        EconomyScope::Wave { spec, wave, .. } => (
            "WHERE spec_id = ?1 AND wave_id = ?2".to_string(),
            Some(spec.0.clone()),
            Some(wave.0.clone()),
        ),
    }
}

/// Builds the `WHERE` clause for `savings_records`.
fn savings_where(scope: &EconomyScope) -> String {
    match scope {
        EconomyScope::Project(_) | EconomyScope::AllProjects(_) => {
            "WHERE (?1 IS NULL OR spec_id = ?1) AND (?2 IS NULL OR wave_id = ?2)".to_string()
        }
        EconomyScope::Spec { .. } => {
            "WHERE spec_id = ?1 AND (?2 IS NULL OR wave_id = ?2)".to_string()
        }
        EconomyScope::Wave { .. } => "WHERE spec_id = ?1 AND wave_id = ?2".to_string(),
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
