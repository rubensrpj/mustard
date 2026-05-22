//! Reader side of the telemetry domain.
//!
//! Free functions over a borrowed [`Connection`], each returning a typed
//! aggregate. Unlike the legacy `economy::reader`, no attribution CTE is
//! needed: `run_usage` carries native `spec` / `wave_id` / `agent_id`
//! columns (load-bearing, stamped at write time in Wave 2 and backfilled for
//! history in [`migrate`](super::migrate)), so every roll-up is a direct
//! `GROUP BY`. `usage_totals` is read for the OTEL-counter aggregates.
//!
//! The [`TelemetryReader`](super::TelemetryReader) trait (declared in
//! [`super`]) is implemented for
//! [`TelemetryStore`](super::store::TelemetryStore) at the bottom by delegating
//! to these functions.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// A cost roll-up keyed by a single grouping column (model / session / spec /
/// agent / phase). `key` is the empty string for rows whose grouping column was
/// `NULL`.
#[derive(Debug, Clone, PartialEq)]
pub struct CostGroup {
    /// The grouping value (model name, spec id, agent id, …).
    pub key: String,
    /// Summed cost in micro-USD.
    pub cost_usd_micros: i64,
    /// Summed input + output tokens.
    pub tokens: i64,
    /// Number of runs in the group.
    pub run_count: i64,
}

/// A cost roll-up keyed by `(spec, wave)`.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveCostGroup {
    /// Spec the wave belongs to (empty string when `NULL`).
    pub spec: String,
    /// Wave id (empty string when `NULL`).
    pub wave_id: String,
    /// Summed cost in micro-USD.
    pub cost_usd_micros: i64,
    /// Summed input + output tokens.
    pub tokens: i64,
    /// Number of runs in the group.
    pub run_count: i64,
}

/// One point in a daily cost/token series.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyPoint {
    /// `YYYY-MM-DD` day bucket (UTC), derived from `ts_iso`.
    pub day: String,
    /// Summed cost in micro-USD for the day.
    pub cost_usd_micros: i64,
    /// Summed input + output tokens for the day.
    pub tokens: i64,
}

/// One `run_usage` row projected for a trace view.
#[derive(Debug, Clone, PartialEq)]
pub struct RunTraceRow {
    /// Span identifier.
    pub span_id: String,
    /// Human-readable span name.
    pub name: Option<String>,
    /// Pipeline phase.
    pub phase: Option<String>,
    /// Model in use.
    pub model: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: Option<i64>,
    /// Priced cost in micro-USD.
    pub cost_usd_micros: Option<i64>,
}

// ---------------------------------------------------------------------------
// usage_totals aggregates (OTEL counters)
// ---------------------------------------------------------------------------

/// Total summed cost across every `claude_code.cost.usage` datapoint.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn cost_total(conn: &Connection) -> Result<f64> {
    let total: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(sum), 0) FROM usage_totals \
             WHERE metric = 'claude_code.cost.usage'",
            [],
            |r| r.get(0),
        )
        .map_err(Error::from)?;
    Ok(total)
}

/// Total summed tokens across every `claude_code.token.usage` datapoint.
///
/// This is the *measured* token total reported by Claude Code's OTEL counter,
/// the token-side companion to [`cost_total`]. Unlike `run_usage`-derived
/// counts it carries no spec/wave dimension (only model/session), so it is only
/// meaningful for project-wide / all-projects roll-ups.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn token_total(conn: &Connection) -> Result<f64> {
    metric_sum(conn, "claude_code.token.usage")
}

/// Cost summed per model, descending. Rows with a `NULL` model collapse to the
/// empty-string key.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn cost_by_model(conn: &Connection) -> Result<Vec<(String, f64)>> {
    grouped_sum(
        conn,
        "SELECT COALESCE(model, ''), COALESCE(SUM(sum), 0) FROM usage_totals \
         WHERE metric = 'claude_code.cost.usage' GROUP BY model ORDER BY 2 DESC",
    )
}

/// Cost summed per session, descending. Rows with a `NULL` session collapse to
/// the empty-string key.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn cost_by_session(conn: &Connection) -> Result<Vec<(String, f64)>> {
    grouped_sum(
        conn,
        "SELECT COALESCE(session_id, ''), COALESCE(SUM(sum), 0) FROM usage_totals \
         WHERE metric = 'claude_code.cost.usage' GROUP BY session_id ORDER BY 2 DESC",
    )
}

/// Lifetime session count — `SUM(sum)` over `claude_code.session.count`.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn session_count(conn: &Connection) -> Result<f64> {
    metric_sum(conn, "claude_code.session.count")
}

/// Lifetime active time in seconds — `SUM(sum)` over
/// `claude_code.active_time.total`.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn active_time(conn: &Connection) -> Result<f64> {
    metric_sum(conn, "claude_code.active_time.total")
}

/// Freshness signal — `MAX(updated_at)` across `usage_totals`, or `None` when
/// the table is empty.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn freshness(conn: &Connection) -> Result<Option<i64>> {
    let max: Option<i64> = conn
        .query_row("SELECT MAX(updated_at) FROM usage_totals", [], |r| r.get(0))
        .map_err(Error::from)?;
    Ok(max)
}

// ---------------------------------------------------------------------------
// run_usage aggregates (per-execution cost)
// ---------------------------------------------------------------------------

/// Cost roll-up grouped by spec, descending.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_by_spec(conn: &Connection) -> Result<Vec<CostGroup>> {
    cost_group_by(conn, "spec")
}

/// Cost roll-up grouped by agent, descending.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_by_agent(conn: &Connection) -> Result<Vec<CostGroup>> {
    cost_group_by(conn, "agent_id")
}

/// Cost roll-up grouped by model, descending.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_by_model(conn: &Connection) -> Result<Vec<CostGroup>> {
    cost_group_by(conn, "model")
}

/// Cost roll-up grouped by phase, descending.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_by_phase(conn: &Connection) -> Result<Vec<CostGroup>> {
    cost_group_by(conn, "phase")
}

/// Cost roll-up grouped by `(spec, wave_id)`, descending by cost.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_by_wave(conn: &Connection) -> Result<Vec<WaveCostGroup>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(spec, ''), COALESCE(wave_id, ''), \
                COALESCE(SUM(cost_usd_micros), 0), \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                COUNT(*) \
         FROM run_usage GROUP BY spec, wave_id ORDER BY 3 DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(WaveCostGroup {
            spec: r.get(0)?,
            wave_id: r.get(1)?,
            cost_usd_micros: r.get(2)?,
            tokens: r.get(3)?,
            run_count: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Daily cost/token series, derived from the `YYYY-MM-DD` prefix of `ts_iso`,
/// ascending by day. Runs with a `NULL` / malformed `ts_iso` are excluded.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn daily_series(conn: &Connection) -> Result<Vec<DailyPoint>> {
    let mut stmt = conn.prepare(
        "SELECT substr(ts_iso, 1, 10) AS day, \
                COALESCE(SUM(cost_usd_micros), 0), \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) \
         FROM run_usage WHERE ts_iso IS NOT NULL AND length(ts_iso) >= 10 \
         GROUP BY day ORDER BY day ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(DailyPoint {
            day: r.get(0)?,
            cost_usd_micros: r.get(1)?,
            tokens: r.get(2)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Cache-hit ratio in permille (0–1000): cache-read tokens over total input
/// (cache-read + non-cached input). Returns `0` when the denominator is zero.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn cache_hit_ratio_permille(conn: &Connection) -> Result<i64> {
    let (input_sum, cache_sum): (i64, i64) = conn
        .query_row(
            "SELECT COALESCE(SUM(input_tokens), 0), \
                    COALESCE(SUM(cache_read_input_tokens), 0) \
             FROM run_usage",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(Error::from)?;
    let den = input_sum + cache_sum;
    if den <= 0 {
        Ok(0)
    } else {
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        Ok(((cache_sum as f64) * 1000.0 / (den as f64)) as i64)
    }
}

/// Every `run_usage` row attributed to `spec`, ordered by start time.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn trace_by_spec(conn: &Connection, spec: &str) -> Result<Vec<RunTraceRow>> {
    let mut stmt = conn.prepare(
        "SELECT span_id, name, phase, model, duration_ms, cost_usd_micros \
         FROM run_usage WHERE spec = ?1 ORDER BY started_at",
    )?;
    let rows = stmt.query_map(rusqlite::params![spec], |r| {
        Ok(RunTraceRow {
            span_id: r.get(0)?,
            name: r.get(1)?,
            phase: r.get(2)?,
            model: r.get(3)?,
            duration_ms: r.get(4)?,
            cost_usd_micros: r.get(5)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

// ---------------------------------------------------------------------------
// Full-row / scoped run_usage projections (Wave 3 — consumers that need more
// than the cost roll-ups above). All additive: no existing signature changes.
// ---------------------------------------------------------------------------

/// A full `run_usage` row projected for the `runs_by_spec()` reader shape
/// (`mustard_core::store::sqlite_store::RunRow`). Carries every column the MCP
/// span summary and dashboard trace pivot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    /// Trace the run belongs to.
    pub trace_id: Option<String>,
    /// Span identifier (primary key).
    pub span_id: String,
    /// Parent span, when this is a child span.
    pub parent_span_id: Option<String>,
    /// Human-readable span name.
    pub name: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: Option<i64>,
    /// Spec the run is attributed to.
    pub spec: Option<String>,
    /// Pipeline phase the run occurred in.
    pub phase: Option<String>,
    /// Model in use during the run.
    pub model: Option<String>,
    /// Input token count.
    pub input_tokens: Option<i64>,
    /// Output token count.
    pub output_tokens: Option<i64>,
    /// Whether the run ended in an error.
    pub is_error: bool,
}

/// One `run_usage` row reduced to the columns the MCP `get_span_summary` tool
/// aggregates over: the model and the three numeric measures it sums.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRow {
    /// Model in use during the run (`None` collapses to `"unknown"` downstream).
    pub model: Option<String>,
    /// Input token count for the run.
    pub input_tokens: Option<i64>,
    /// Output token count for the run.
    pub output_tokens: Option<i64>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: Option<i64>,
}

/// Per-run rows feeding the MCP `get_span_summary` aggregation, optionally
/// filtered to a spec and/or phase and capped to `limit` rows. Replaces the
/// legacy `spans` scan that tool consumed: each row carries the model and the
/// three summed measures (`input_tokens`, `output_tokens`, `duration_ms`), so
/// the caller reproduces the exact totals + per-model breakdown — including the
/// `limit` truncation semantics — in process.
///
/// Ordered by `started_at` so the `limit` cap is deterministic.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_for_summary(
    conn: &Connection,
    spec: Option<&str>,
    phase: Option<&str>,
    limit: usize,
) -> Result<Vec<SummaryRow>> {
    let mut clauses: Vec<&str> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    if let Some(s) = spec {
        params.push(s.to_string());
        clauses.push("spec = ?");
    }
    if let Some(p) = phase {
        params.push(p.to_string());
        clauses.push("phase = ?");
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        let joined = clauses
            .iter()
            .enumerate()
            .map(|(i, c)| c.replace('?', &format!("?{}", i + 1)))
            .collect::<Vec<_>>()
            .join(" AND ");
        format!("WHERE {joined}")
    };
    let sql = format!(
        "SELECT model, input_tokens, output_tokens, duration_ms \
         FROM run_usage {where_sql} ORDER BY started_at LIMIT {}",
        limit as i64
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
        Ok(SummaryRow {
            model: r.get(0)?,
            input_tokens: r.get(1)?,
            output_tokens: r.get(2)?,
            duration_ms: r.get(3)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Every `run_usage` row attributed to `spec`, ordered by start time, projected
/// to the full [`RunRow`] shape. Replaces the legacy
/// `SELECT … FROM spans WHERE spec = ?1` read.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_full_by_spec(conn: &Connection, spec: &str) -> Result<Vec<RunRow>> {
    let mut stmt = conn.prepare(
        "SELECT trace_id, span_id, parent_span_id, name, duration_ms, \
                spec, phase, model, input_tokens, output_tokens, is_error \
         FROM run_usage WHERE spec = ?1 ORDER BY started_at",
    )?;
    let rows = stmt.query_map(rusqlite::params![spec], |r| {
        Ok(RunRow {
            trace_id: r.get(0)?,
            span_id: r.get(1)?,
            parent_span_id: r.get(2)?,
            name: r.get(3)?,
            duration_ms: r.get(4)?,
            spec: r.get(5)?,
            phase: r.get(6)?,
            model: r.get(7)?,
            input_tokens: r.get(8)?,
            output_tokens: r.get(9)?,
            is_error: r.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Aggregate cost/tokens/count over `run_usage`, optionally filtered to a spec
/// and/or wave. The W4 attribution CTE this replaces resolved spec/wave from
/// the joined `agent.start`; now both columns are self-attributed on the row,
/// so the filter is a plain `WHERE`.
///
/// Returns `(cost_usd_micros, input+output tokens, run_count)`.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn scoped_totals(
    conn: &Connection,
    spec: Option<&str>,
    wave: Option<&str>,
) -> Result<(i64, i64, i64)> {
    let (where_sql, params) = scope_where(spec, wave);
    let sql = format!(
        "SELECT COALESCE(SUM(cost_usd_micros), 0), \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                COUNT(*) FROM run_usage {where_sql}"
    );
    let row = conn
        .query_row(&sql, rusqlite::params_from_iter(params.iter()), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })
        .map_err(Error::from)?;
    Ok(row)
}

/// Per-agent cost roll-up over `run_usage`, optionally scoped to a wave (the
/// only filter the legacy `per_agent_costs` applied post-attribution). Excludes
/// rows with a `NULL`/empty `agent_id` — they have no agent to attribute to,
/// matching the legacy CTE's `attr_agent_id IS NOT NULL AND != ''` guard.
/// Ordered by cost descending.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_by_agent_scoped(
    conn: &Connection,
    spec: Option<&str>,
    wave: Option<&str>,
) -> Result<Vec<CostGroup>> {
    let (mut where_sql, params) = scope_where(spec, wave);
    where_sql = if where_sql.is_empty() {
        "WHERE agent_id IS NOT NULL AND agent_id != ''".to_string()
    } else {
        format!("{where_sql} AND agent_id IS NOT NULL AND agent_id != ''")
    };
    cost_group_scoped(conn, "agent_id", &where_sql, &params)
}

/// Per-spec cost roll-up over `run_usage`, optionally scoped to a wave. Spec is
/// `COALESCE`'d to the empty string (legacy parity). Ordered by cost descending.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_by_spec_scoped(conn: &Connection, wave: Option<&str>) -> Result<Vec<CostGroup>> {
    let (where_sql, params) = scope_where(None, wave);
    cost_group_scoped(conn, "spec", &where_sql, &params)
}

/// Per-`(spec, wave)` cost roll-up over `run_usage`, optionally scoped to a
/// wave. Ordered by cost descending.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn runs_by_wave_scoped(conn: &Connection, wave: Option<&str>) -> Result<Vec<WaveCostGroup>> {
    let (where_sql, params) = scope_where(None, wave);
    let sql = format!(
        "SELECT COALESCE(spec, ''), COALESCE(wave_id, ''), \
                COALESCE(SUM(cost_usd_micros), 0), \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                COUNT(*) FROM run_usage {where_sql} \
         GROUP BY spec, wave_id ORDER BY 3 DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
        Ok(WaveCostGroup {
            spec: r.get(0)?,
            wave_id: r.get(1)?,
            cost_usd_micros: r.get(2)?,
            tokens: r.get(3)?,
            run_count: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Cache-hit ratio in permille for an optional spec scope. Mirrors
/// [`cache_hit_ratio_permille`] but constrained to a spec (the legacy economy
/// reader collapsed Wave→Spec here, so no wave filter is exposed).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn cache_hit_ratio_permille_for_spec(conn: &Connection, spec: Option<&str>) -> Result<i64> {
    let (where_sql, params) = scope_where(spec, None);
    let sql = format!(
        "SELECT COALESCE(SUM(input_tokens), 0), \
                COALESCE(SUM(cache_read_input_tokens), 0) FROM run_usage {where_sql}"
    );
    let (input_sum, cache_sum): (i64, i64) = conn
        .query_row(&sql, rusqlite::params_from_iter(params.iter()), |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(Error::from)?;
    let den = input_sum + cache_sum;
    if den <= 0 {
        Ok(0)
    } else {
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        Ok(((cache_sum as f64) * 1000.0 / (den as f64)) as i64)
    }
}

// ---------------------------------------------------------------------------
// Consumption / quality projections (Wave 3 — dashboard `db.rs` consumers).
//
// These return neutral core structs/tuples; the dashboard maps them onto its
// own `ModelUsage` / `AgentUsage` / `SpecUsage` / `DailyPoint` shapes. Cost is
// micro-USD on the wire (the legacy `spans` path extracted REAL USD from
// `attributes`); the consumer divides by 1_000_000 when it wants USD.
// ---------------------------------------------------------------------------

/// A consumption roll-up keyed by one grouping column (model / agent / spec).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumptionGroup {
    /// Grouping value (model name, agent id, spec slug). Empty string for `NULL`.
    pub key: String,
    /// Number of runs in the group.
    pub calls: i64,
    /// Summed input tokens.
    pub input_tokens: i64,
    /// Summed output tokens.
    pub output_tokens: i64,
    /// Summed cost in micro-USD.
    pub cost_usd_micros: i64,
}

/// One point of a daily consumption series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyConsumption {
    /// `YYYY-MM-DD` UTC day, derived from `started_at` (ms epoch).
    pub date: String,
    /// Number of runs on the day.
    pub calls: i64,
    /// Summed input tokens.
    pub input_tokens: i64,
    /// Summed output tokens.
    pub output_tokens: i64,
    /// Summed cost in micro-USD.
    pub cost_usd_micros: i64,
}

/// Per-model consumption over `run_usage`, ordered by total tokens descending.
/// Replaces the legacy `consumption_by_model` `spans` scan.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn consumption_by_model(conn: &Connection) -> Result<Vec<ConsumptionGroup>> {
    consumption_group_by(
        conn,
        "COALESCE(model, 'unknown')",
        "GROUP BY model ORDER BY (COALESCE(SUM(input_tokens),0) + COALESCE(SUM(output_tokens),0)) DESC",
    )
}

/// Per-agent consumption over `run_usage`, ordered by total tokens descending.
/// Replaces the legacy `consumption_by_agent_type` scan (which derived the
/// agent from `attributes -> mustard.agent_type`; `run_usage.agent_id` is the
/// native, write-time-stamped equivalent).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn consumption_by_agent(conn: &Connection) -> Result<Vec<ConsumptionGroup>> {
    consumption_group_by(
        conn,
        "COALESCE(agent_id, 'unknown')",
        "GROUP BY agent_id ORDER BY (COALESCE(SUM(input_tokens),0) + COALESCE(SUM(output_tokens),0)) DESC",
    )
}

/// Top specs by total tokens over `run_usage` (excludes `NULL` specs), limited.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn consumption_top_specs(conn: &Connection, limit: usize) -> Result<Vec<ConsumptionGroup>> {
    let sql = format!(
        "SELECT COALESCE(spec, '') AS key, COUNT(*), \
                COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), \
                COALESCE(SUM(cost_usd_micros), 0) \
         FROM run_usage WHERE spec IS NOT NULL \
         GROUP BY spec \
         ORDER BY (COALESCE(SUM(input_tokens),0) + COALESCE(SUM(output_tokens),0)) DESC \
         LIMIT {}",
        limit as i64
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], consumption_row)?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Daily consumption series over `run_usage`, for runs whose `started_at`
/// (ms epoch) is `>= since_ms`. Ascending by day.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn consumption_daily_series(conn: &Connection, since_ms: i64) -> Result<Vec<DailyConsumption>> {
    let mut stmt = conn.prepare(
        "SELECT date(started_at/1000, 'unixepoch') AS d, COUNT(*), \
                COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), \
                COALESCE(SUM(cost_usd_micros), 0) \
         FROM run_usage WHERE started_at >= ?1 GROUP BY d ORDER BY d ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ms], |r| {
        Ok(DailyConsumption {
            date: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            calls: r.get(1)?,
            input_tokens: r.get(2)?,
            output_tokens: r.get(3)?,
            cost_usd_micros: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Token + cost totals (lifetime and "today", where today = runs with
/// `started_at >= midnight_ms`). Returns
/// `(tokens_total, tokens_today, cost_total_micros, cost_today_micros)`.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn cost_summary(conn: &Connection, midnight_ms: i64) -> Result<(i64, i64, i64, i64)> {
    let row = conn
        .query_row(
            "SELECT \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                COALESCE(SUM(CASE WHEN started_at >= ?1 \
                    THEN COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0) ELSE 0 END), 0), \
                COALESCE(SUM(cost_usd_micros), 0), \
                COALESCE(SUM(CASE WHEN started_at >= ?1 THEN cost_usd_micros ELSE 0 END), 0) \
             FROM run_usage",
            rusqlite::params![midnight_ms],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )
        .map_err(Error::from)?;
    Ok(row)
}

/// Lifetime + "today" token totals over `run_usage` (today = `started_at >=
/// midnight_ms`). Returns `(tokens_total, tokens_today)`. Backs the dashboard
/// `metrics_from_db` token counters.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn token_totals(conn: &Connection, midnight_ms: i64) -> Result<(i64, i64)> {
    let (total, today, _, _) = cost_summary(conn, midnight_ms)?;
    Ok((total, today))
}

/// Average run duration in milliseconds across all `run_usage` rows.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn avg_duration_ms(conn: &Connection) -> Result<f64> {
    let avg: Option<f64> = conn
        .query_row("SELECT AVG(duration_ms) FROM run_usage", [], |r| r.get(0))
        .map_err(Error::from)?;
    Ok(avg.unwrap_or(0.0))
}

/// Per-agent sample counts over `run_usage`, ordered by sample count
/// descending, limited. Backs the dashboard quality "by role" list (the legacy
/// path grouped `spans.actor_id`; `run_usage.agent_id` is the native field).
/// Returns `(role, samples)` with `NULL` agent collapsed to `unknown`.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn samples_by_agent(conn: &Connection, limit: usize) -> Result<Vec<(String, i64)>> {
    let sql = format!(
        "SELECT COALESCE(agent_id, 'unknown') AS role, COUNT(*) AS samples \
         FROM run_usage GROUP BY role ORDER BY samples DESC LIMIT {}",
        limit as i64
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Slowest runs by `duration_ms` over `run_usage`, limited. Returns
/// `(spec, wave_id, duration_ms)` per row, descending by duration.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn slowest_runs(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<(Option<String>, Option<String>, i64)>> {
    let sql = format!(
        "SELECT spec, wave_id, COALESCE(duration_ms, 0) FROM run_usage \
         ORDER BY duration_ms DESC LIMIT {}",
        limit as i64
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Average input/output tokens per phase over `run_usage`, ascending by phase.
/// Returns `(phase, input_avg, output_avg)`. `NULL` phase collapses to
/// `unknown`.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn tokens_by_phase(conn: &Connection) -> Result<Vec<(String, f64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(phase, 'unknown') AS phase, \
                COALESCE(AVG(input_tokens), 0.0), COALESCE(AVG(output_tokens), 0.0) \
         FROM run_usage GROUP BY phase ORDER BY phase",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?))
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Total input+output tokens grouped by spec over `run_usage`. Backs the
/// dashboard `aggregate_activity_from_db`, which sums per-spec tokens onto each
/// activity bucket. `NULL` spec is excluded (it cannot match an event's spec).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a query failure.
pub fn tokens_by_spec_map(conn: &Connection) -> Result<std::collections::HashMap<String, i64>> {
    let mut stmt = conn.prepare(
        "SELECT spec, COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0) \
         FROM run_usage WHERE spec IS NOT NULL GROUP BY spec",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut map = std::collections::HashMap::new();
    for row in rows.filter_map(std::result::Result::ok) {
        map.insert(row.0, row.1);
    }
    Ok(map)
}

/// Shared consumption group query: a trusted `key_expr` SELECT plus a trusted
/// `group_order` tail (`GROUP BY … ORDER BY …`). Both are crate-internal
/// literals — never user input.
fn consumption_group_by(
    conn: &Connection,
    key_expr: &str,
    group_order: &str,
) -> Result<Vec<ConsumptionGroup>> {
    let sql = format!(
        "SELECT {key_expr} AS key, COUNT(*), \
                COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0), \
                COALESCE(SUM(cost_usd_micros), 0) \
         FROM run_usage {group_order}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], consumption_row)?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Row mapper for the five-column consumption SELECT.
fn consumption_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ConsumptionGroup> {
    Ok(ConsumptionGroup {
        key: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
        calls: r.get(1)?,
        input_tokens: r.get(2)?,
        output_tokens: r.get(3)?,
        cost_usd_micros: r.get(4)?,
    })
}

/// Build a `WHERE` clause + positional binds for an optional `(spec, wave)`
/// filter on `run_usage`. Returns `("", [])` when both are `None`.
fn scope_where(spec: Option<&str>, wave: Option<&str>) -> (String, Vec<String>) {
    let mut clauses: Vec<&str> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    if let Some(s) = spec {
        params.push(s.to_string());
        clauses.push("spec = ?");
    }
    if let Some(w) = wave {
        params.push(w.to_string());
        clauses.push("wave_id = ?");
    }
    if clauses.is_empty() {
        (String::new(), params)
    } else {
        // Positional placeholders are rewritten to ?1.. by SQLite's binder via
        // params_from_iter ordering; use explicit ?N to be unambiguous.
        let where_sql = clauses
            .iter()
            .enumerate()
            .map(|(i, c)| c.replace('?', &format!("?{}", i + 1)))
            .collect::<Vec<_>>()
            .join(" AND ");
        (format!("WHERE {where_sql}"), params)
    }
}

/// Cost roll-up over `run_usage` grouped by a trusted column, with a caller-built
/// `WHERE` clause and binds. Ordered by cost descending.
fn cost_group_scoped(
    conn: &Connection,
    column: &str,
    where_sql: &str,
    params: &[String],
) -> Result<Vec<CostGroup>> {
    let sql = format!(
        "SELECT COALESCE({column}, '') AS key, \
                COALESCE(SUM(cost_usd_micros), 0), \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                COUNT(*) FROM run_usage {where_sql} GROUP BY {column} ORDER BY 2 DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
        Ok(CostGroup {
            key: r.get(0)?,
            cost_usd_micros: r.get(1)?,
            tokens: r.get(2)?,
            run_count: r.get(3)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

/// Run a `SELECT key, SUM` query and collect `(String, f64)` pairs.
fn grouped_sum(conn: &Connection, sql: &str) -> Result<Vec<(String, f64)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// `SUM(sum)` over `usage_totals` for a single metric name.
fn metric_sum(conn: &Connection, metric: &str) -> Result<f64> {
    let total: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(sum), 0) FROM usage_totals WHERE metric = ?1",
            rusqlite::params![metric],
            |r| r.get(0),
        )
        .map_err(Error::from)?;
    Ok(total)
}

/// Cost roll-up over `run_usage`, grouped by a single trusted column name.
///
/// `column` is a crate-internal literal (`spec`, `agent_id`, `model`, `phase`)
/// — never user input — so interpolating it into the SQL is safe.
fn cost_group_by(conn: &Connection, column: &str) -> Result<Vec<CostGroup>> {
    let sql = format!(
        "SELECT COALESCE({column}, '') AS key, \
                COALESCE(SUM(cost_usd_micros), 0), \
                COALESCE(SUM(COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)), 0), \
                COUNT(*) \
         FROM run_usage GROUP BY {column} ORDER BY 2 DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(CostGroup {
            key: r.get(0)?,
            cost_usd_micros: r.get(1)?,
            tokens: r.get(2)?,
            run_count: r.get(3)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}
