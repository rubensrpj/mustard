//! Filesystem facade replacing the legacy DB-backed dashboard reader.
//!
//! Wave 6A of [[2026-05-26-no-sqlite-git-source-of-truth]] retired every
//! relational layer from the dashboard runtime. The module surface kept its
//! public function names so that callers in `lib.rs`, `spec_views.rs`,
//! `telemetry.rs`, `telemetry_agg.rs`, `amend_queries.rs`, `commands/specs.rs`
//! and `commands/settings.rs` continue to type-check verbatim.
//!
//! The facade has two halves:
//!
//! 1. An opaque [`Connection`] placeholder so closures like
//!    `with_db(&base, |conn| do_something(conn, …))` still parse. The
//!    placeholder is uninhabited at runtime — [`with_db`] and [`with_store`]
//!    always return `None`, so the closures are never invoked.
//! 2. Filesystem readers that derive their answer from `.claude/spec/*/spec.md`
//!    and `.claude/spec/*/.events/*.ndjson` via
//!    [`mustard_core::io::events::EventReader`] and
//!    [`mustard_core::io::atomic_md::MarkdownStore`]. Every reader is **fail-open**:
//!    a missing directory returns the type's `Default`, never an error.
//!
//! Behaviour parity is reduced on purpose: queries that depended on FTS5 or
//! relational aggregation come back empty or zeroed. The frontend still
//! receives shape-correct responses; richer behaviour can be reintroduced in a
//! follow-up sub-spec on top of the NDJSON sink.

use std::path::Path;

use crate::{
    ActivePipeline, ActivityGroup, AgentUsage, ConsumptionSummary, DailyPoint, KnowledgeRow,
    KnowledgeSummary, MetricsSummary, ModelUsage, PipelineSummary, QualityMetrics, RecentEvent,
    SpecRow, SpecUsage,
};

// ── opaque placeholders ──────────────────────────────────────────────────────

/// Opaque marker that stands in for the previous DB connection. It cannot be
/// constructed from outside this module, so any closure that takes
/// `&Connection` is, by construction, dead code at runtime — [`with_db`]
/// never calls it. The type exists purely so legacy call sites continue to
/// type-check after the relational layer was removed.
pub struct Connection {
    _private: (),
}

/// Inhabitant-free placeholder mirroring [`Connection`] for the (former)
/// event-store write slot. Same role: keeps `with_store`'s closure signature
/// valid without resurrecting the relational dependency.
pub struct EventStoreHandle {
    _private: (),
}

// ── never-invoked closure facades ────────────────────────────────────────────

/// Always returns `None` — the SQLite reader was deleted. Closures are kept
/// only to preserve compilation of every legacy `db::with_db(&base, |conn| …)`
/// call site; they are never executed.
pub fn with_db<T, F>(_repo_path: &Path, _f: F) -> Option<Result<T, String>>
where
    F: FnOnce(&Connection) -> Result<T, String>,
{
    None
}

/// Counterpart to [`with_db`] for the write path; preserved so the legacy
/// `db::with_store(repo, |store| store.append(…))` shape compiles.
pub fn with_store<T, F>(_repo_path: &Path, _f: F) -> Option<Result<T, String>>
where
    F: FnOnce(&EventStoreHandle) -> Result<T, String>,
{
    None
}

// ── filesystem stubs preserving legacy signatures ────────────────────────────

/// The SQLite schema gate from the old facade. Without SQLite there is nothing
/// to probe; always returns `false`.
#[must_use]
pub fn has_phase1_schema(_conn: &Connection) -> bool {
    false
}

/// FTS5 escaping helper preserved for callers that still trim queries before
/// dispatching to the (now-absent) search. Returns the trimmed query when
/// non-empty so frontend search inputs keep their shape.
#[must_use]
pub fn fts_escape(q: &str) -> Option<String> {
    let trimmed = q.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Legacy signature — never invoked because [`with_db`] returns `None` before
/// the closure runs. The body is left as a defensible `Default` so a
/// hypothetical reflective caller still gets a shape-correct value.
#[allow(unused_variables)]
pub fn metrics_from_db(conn: &Connection, _tele: Option<&()>) -> Result<MetricsSummary, String> {
    Ok(MetricsSummary {
        total_events: 0,
        sessions_recent: 0,
        agents_dispatched: 0,
        last_event_at: None,
        tokens_total: 0,
        tokens_today: 0,
    })
}

#[allow(unused_variables)]
pub fn knowledge_from_db(conn: &Connection) -> Result<KnowledgeSummary, String> {
    Ok(KnowledgeSummary {
        patterns_count: 0,
        conventions_count: 0,
        high_confidence_count: 0,
    })
}

#[allow(unused_variables)]
pub fn recent_events_from_db(
    conn: &Connection,
    _limit: usize,
) -> Result<Vec<RecentEvent>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn specs_from_db(conn: &Connection) -> Result<Vec<SpecRow>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn search_events_from_db(
    conn: &Connection,
    _query: &str,
    _limit: usize,
) -> Result<Vec<RecentEvent>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn workflow_by_phase_from_db(
    conn: &Connection,
) -> Result<crate::telemetry::WorkflowBlock, String> {
    Ok(crate::telemetry::WorkflowBlock::default())
}

#[allow(unused_variables)]
pub fn tool_breakdown_from_db(
    conn: &Connection,
    _limit: usize,
) -> Result<Vec<crate::telemetry::ToolCount>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn search_knowledge_from_db(
    conn: &Connection,
    _query: &str,
    _limit: usize,
) -> Result<Vec<KnowledgeRow>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn aggregate_activity_from_db(
    conn: &Connection,
    _tele: Option<&()>,
    _limit: usize,
) -> Result<Vec<ActivityGroup>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn quality_metrics_from_db(
    conn: &Connection,
    _tele: Option<&()>,
) -> Result<QualityMetrics, String> {
    Ok(QualityMetrics::default())
}

#[allow(unused_variables)]
pub fn consumption_by_model(
    _tele: Option<&()>,
    _bucket_ms: i64,
) -> Result<Vec<ModelUsage>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn consumption_by_agent_type(
    _tele: Option<&()>,
    _bucket_ms: i64,
) -> Result<Vec<AgentUsage>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn consumption_top_specs(
    _tele: Option<&()>,
    _limit: usize,
) -> Result<Vec<SpecUsage>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn consumption_daily_series(
    _tele: Option<&()>,
    _days: i64,
) -> Result<Vec<DailyPoint>, String> {
    Ok(Vec::new())
}

#[allow(unused_variables)]
pub fn cost_summary(_tele: Option<&()>) -> Result<(u64, u64, f64, f64), String> {
    Ok((0, 0, 0.0, 0.0))
}

#[allow(unused_variables)]
pub fn consumption_summary_from_db(_tele: Option<&()>) -> Result<ConsumptionSummary, String> {
    Ok(ConsumptionSummary::default())
}

/// The dedicated telemetry handle is gone — callers receive `None` and pass
/// it back into the other stubs, which already accept `Option<&()>`.
#[allow(unused_variables)]
pub fn telemetry_store_for(_repo_path: &Path) -> Option<()> {
    None
}

#[allow(unused_variables)]
pub fn agent_activity_from_db(
    conn: &Connection,
) -> Result<crate::telemetry::AgentActivityBlock, String> {
    Ok(crate::telemetry::AgentActivityBlock::default())
}

#[allow(unused_variables)]
pub fn session_start_ts_from_db(conn: &Connection) -> Option<String> {
    None
}

#[allow(unused_variables)]
pub fn live_activity_from_db(
    conn: &Connection,
) -> Result<crate::telemetry::LiveActivity, String> {
    Ok(crate::telemetry::LiveActivity::default())
}

#[allow(unused_variables)]
#[must_use]
pub fn pipelines_from_db(conn: &Connection) -> Vec<PipelineSummary> {
    Vec::new()
}

#[allow(unused_variables)]
#[must_use]
pub fn active_pipelines_from_db(conn: &Connection, _now_secs: u64) -> Vec<ActivePipeline> {
    Vec::new()
}

#[allow(unused_variables)]
pub fn knowledge_browse_from_db(
    conn: &Connection,
    _limit: usize,
) -> Result<Vec<KnowledgeRow>, String> {
    Ok(Vec::new())
}
