//! Writer side of the telemetry domain.
//!
//! Free functions over a borrowed [`Connection`] (the caller owns the open
//! [`TelemetryStore`](super::store::TelemetryStore)). Each opens a short
//! transaction and issues an INSERT / UPSERT. Failures bubble back as
//! [`Result`] — never panic; the call site decides whether to fail open.
//!
//! The [`TelemetryWriter`](super::TelemetryWriter) trait (declared in
//! [`super`]) is implemented for [`TelemetryStore`] at the bottom of this file
//! by delegating to these functions, so production code talks to the trait and
//! tests can swap in the in-memory fake.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};

use super::model::{RunAttribution, RunUsage, UsageMetric};

/// UPSERT one aggregated usage counter into `usage_totals`.
///
/// On a `(metric, model, session_id)` collision the `sum` is **accumulated**
/// (`sum = sum + excluded.sum`) and `updated_at` advances to the larger of the
/// two values — the freshest contributing datapoint wins. This mirrors the
/// legacy `claude_code_otel` upsert (`sum = sum + excluded.sum`) collapsed onto
/// the reduced key.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure.
pub fn upsert_usage_metric(conn: &Connection, rec: &UsageMetric) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_totals (metric, model, session_id, sum, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(metric, model, session_id) \
         DO UPDATE SET sum = sum + excluded.sum, \
                       updated_at = MAX(COALESCE(updated_at, 0), COALESCE(excluded.updated_at, 0))",
        params![rec.metric, rec.model, rec.session_id, rec.sum, rec.updated_at],
    )
    .map_err(Error::from)?;
    Ok(())
}

/// Persist a [`RunUsage`] into `run_usage`.
///
/// `span_id` is the primary key; `INSERT OR REPLACE` makes a re-record of the
/// same id idempotent (the W3-style adapters can re-ingest the same Anthropic
/// request without a constraint violation).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure.
pub fn record_run(conn: &Connection, rec: &RunUsage) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO run_usage \
            (trace_id, span_id, parent_span_id, name, started_at, ended_at, \
             duration_ms, attributes, spec, phase, model, input_tokens, \
             output_tokens, cache_read_input_tokens, cache_creation_input_tokens, \
             cost_usd_micros, is_error, project_path, ts_iso, session_id, \
             wave_id, tool_use_id, agent_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
                 ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
        params![
            rec.trace_id,
            rec.span_id,
            rec.parent_span_id,
            rec.name,
            rec.started_at,
            rec.ended_at,
            rec.duration_ms,
            rec.attributes,
            rec.spec,
            rec.phase,
            rec.model,
            rec.input_tokens,
            rec.output_tokens,
            rec.cache_read_input_tokens,
            rec.cache_creation_input_tokens,
            rec.cost_usd_micros,
            i64::from(rec.is_error),
            rec.project_path,
            rec.ts_iso,
            rec.session_id,
            rec.wave_id,
            rec.tool_use_id,
            rec.agent_id,
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// UPSERT one write-time attribution stamp into `run_attribution`.
///
/// Keyed on `(session_id, tool_use_id)`; a re-stamp updates the spec / wave /
/// agent and advances `updated_at`. Idempotent by primary key.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure.
pub fn upsert_attribution(conn: &Connection, rec: &RunAttribution) -> Result<()> {
    conn.execute(
        "INSERT INTO run_attribution \
            (session_id, tool_use_id, spec, wave_id, agent_id, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(session_id, tool_use_id) \
         DO UPDATE SET spec = excluded.spec, wave_id = excluded.wave_id, \
                       agent_id = excluded.agent_id, updated_at = excluded.updated_at",
        params![
            rec.session_id,
            rec.tool_use_id,
            rec.spec,
            rec.wave_id,
            rec.agent_id,
            rec.updated_at,
        ],
    )
    .map_err(Error::from)?;
    Ok(())
}

/// Look up the attribution stamp for a `(session_id, tool_use_id)` pair.
///
/// Returns `None` when no stamp exists — a missing row is not an error.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a genuine query failure.
pub fn lookup_attribution(
    conn: &Connection,
    session_id: &str,
    tool_use_id: &str,
) -> Result<Option<RunAttribution>> {
    let row = conn
        .query_row(
            "SELECT session_id, tool_use_id, spec, wave_id, agent_id, updated_at \
             FROM run_attribution WHERE session_id = ?1 AND tool_use_id = ?2",
            params![session_id, tool_use_id],
            |r| {
                Ok(RunAttribution {
                    session_id: r.get(0)?,
                    tool_use_id: r.get(1)?,
                    spec: r.get(2)?,
                    wave_id: r.get(3)?,
                    agent_id: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Error::from)?;
    Ok(row)
}

/// Session-only attribution fallback: the most-recent `run_attribution` row for
/// `session_id` whose `updated_at` is at or before `before_ts` (ms-epoch).
///
/// This mirrors the legacy read-time W4 attribution CTE, which — when a span
/// carried no `tool_use_id` — fell back to the most-recent `agent.start` for the
/// same session with `ts <= span.ts`. The primary
/// [`lookup_attribution`] keys on `(session_id, tool_use_id)`; spans that arrive
/// without a `tool_use_id` would otherwise be permanently unattributed, so this
/// recovers the session-scoped stamp.
///
/// When `before_ts` is `None` (e.g. the span's timestamp could not be parsed),
/// the time filter is dropped and the single most-recent stamp for the session
/// is returned — best-effort attribution beats none. A row with a `NULL`
/// `updated_at` never matches the bounded query (it has no orderable time), but
/// can still surface under the unbounded fallback.
///
/// Returns `None` when the session has no stamp — a missing row is not an error.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for a genuine query failure.
pub fn lookup_attribution_by_session(
    conn: &Connection,
    session_id: &str,
    before_ts: Option<i64>,
) -> Result<Option<RunAttribution>> {
    let map_row = |r: &rusqlite::Row<'_>| {
        Ok(RunAttribution {
            session_id: r.get(0)?,
            tool_use_id: r.get(1)?,
            spec: r.get(2)?,
            wave_id: r.get(3)?,
            agent_id: r.get(4)?,
            updated_at: r.get(5)?,
        })
    };
    let row = match before_ts {
        Some(ts) => conn
            .query_row(
                "SELECT session_id, tool_use_id, spec, wave_id, agent_id, updated_at \
                 FROM run_attribution \
                 WHERE session_id = ?1 AND updated_at IS NOT NULL AND updated_at <= ?2 \
                 ORDER BY updated_at DESC LIMIT 1",
                params![session_id, ts],
                map_row,
            )
            .optional()
            .map_err(Error::from)?,
        None => conn
            .query_row(
                "SELECT session_id, tool_use_id, spec, wave_id, agent_id, updated_at \
                 FROM run_attribution WHERE session_id = ?1 \
                 ORDER BY updated_at DESC LIMIT 1",
                params![session_id],
                map_row,
            )
            .optional()
            .map_err(Error::from)?,
    };
    Ok(row)
}

/// Milliseconds in one day — the unit for the [`prune_older_than_days`] wrapper.
const MS_PER_DAY: i64 = 86_400_000;

/// Outcome of a [`backfill_null_spec`] sweep — how many rows we visited and
/// where the match came from. The two `updated_*` counters split so the user
/// can see whether attribution recovered via the precise (session, `tool_use`)
/// key or fell back to the session-only match.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct SpecBackfillReport {
    /// Rows where `spec IS NULL AND session_id IS NOT NULL`.
    pub scanned: usize,
    /// Rows matched via `(session_id, tool_use_id)` lookup. High-precision.
    pub updated_primary: usize,
    /// Rows matched via session-only fallback (most-recent stamp at/before the
    /// row's `started_at`). Coarser but covers spans without a `tool_use_id`.
    pub updated_fallback: usize,
    /// Rows where neither lookup found a stamp — left as NULL.
    pub unmatched: usize,
}

/// Backfill `spec` (and the sibling `wave_id` / `agent_id`) on `run_usage`
/// rows that arrived without write-time attribution.
///
/// Rows can arrive `spec = NULL` when the collector's write-time stamping path
/// loses the race against the `run_attribution` upsert (the agent.start hook
/// writes the stamp on a separate connection, so a fast trace can land first).
/// This function fixes that retroactively by replaying the same lookup the
/// collector uses on the hot path.
///
/// Lookup strategy mirrors the live OTEL collector's `stamp_attribution`
/// (`apps/rt/src/run/otel/collector.rs::stamp_attribution`):
///
/// 1. **Primary** — `(session_id, tool_use_id)` when the row has a
///    `tool_use_id`. Exact match by both fields.
/// 2. **Session-only fallback** — picks the most-recent stamp for the
///    session at-or-before the row's `started_at`. Same temporal rule the
///    collector uses for spans that arrive without a `tool_use_id`.
///
/// Idempotent: only rows with `spec IS NULL AND session_id IS NOT NULL` are
/// candidates, so a second run finds nothing new. Single transaction; an
/// UPDATE failure rolls back the whole sweep.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] on a database failure.
pub fn backfill_null_spec(conn: &Connection) -> Result<SpecBackfillReport> {
    // ── Step 1: collect candidates ─────────────────────────────────────
    // SELECT before UPDATE so we control the iteration order and can do the
    // lookups outside the write transaction (lookups read the same DB and
    // would compete with an open write tx).
    type CandidateRow = (String, String, Option<String>, Option<i64>);
    let mut stmt = conn.prepare(
        "SELECT span_id, session_id, tool_use_id, started_at \
         FROM run_usage \
         WHERE spec IS NULL AND session_id IS NOT NULL",
    )?;
    let candidates: Vec<CandidateRow> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    drop(stmt);

    let scanned = candidates.len();
    if scanned == 0 {
        return Ok(SpecBackfillReport::default());
    }

    // ── Step 2: resolve attribution per candidate ──────────────────────
    // Two-tier mirrors `stamp_attribution`: primary by (session, tool_use_id)
    // when both are present; otherwise session-only with the row's started_at
    // as the upper time bound. A row that matches nothing stays NULL.
    let mut updated_primary = 0usize;
    let mut updated_fallback = 0usize;
    let mut unmatched = 0usize;
    let mut resolved: Vec<(String, RunAttribution)> = Vec::new();
    for (span_id, session_id, tool_use_id, started_at) in candidates {
        // Primary lookup: (session, tool_use_id).
        if let Some(tool) = tool_use_id.as_deref() {
            if let Some(attr) = lookup_attribution(conn, &session_id, tool)? {
                updated_primary += 1;
                resolved.push((span_id, attr));
                continue;
            }
        }
        // Session-only fallback: most-recent stamp <= started_at.
        if let Some(attr) = lookup_attribution_by_session(conn, &session_id, started_at)? {
            updated_fallback += 1;
            resolved.push((span_id, attr));
            continue;
        }
        unmatched += 1;
    }

    // ── Step 3: UPDATE inside a single transaction ─────────────────────
    if resolved.is_empty() {
        return Ok(SpecBackfillReport { scanned, updated_primary, updated_fallback, unmatched });
    }
    let tx = conn.unchecked_transaction()?;
    for (span_id, attr) in resolved {
        tx.execute(
            "UPDATE run_usage \
             SET spec = ?1, wave_id = ?2, agent_id = ?3 \
             WHERE span_id = ?4 AND spec IS NULL",
            params![attr.spec, attr.wave_id, attr.agent_id, span_id],
        )?;
    }
    tx.commit()?;

    Ok(SpecBackfillReport { scanned, updated_primary, updated_fallback, unmatched })
}

/// Outcome of a [`backfill_null_costs`] sweep — how many rows we looked at and
/// how many we wrote back. The caller (a `mustard-rt run` subcommand) prints
/// this as JSON so the user can confirm the operation did something.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct BackfillReport {
    /// Rows that matched the candidate filter — NULL cost AND non-zero tokens.
    pub scanned: usize,
    /// Subset of `scanned` for which we wrote a non-zero cost. Equal to
    /// `scanned` in the happy path; smaller only if a row computed to 0.
    pub updated: usize,
}

/// Backfill `cost_usd_micros` on legacy `run_usage` rows.
///
/// Designed for one-shot retroactive pricing after the cost formula changes
/// (e.g. the 2026-05-23 cache-aware pricing fix — `cache_read` rebilled at
/// 10% of base, `cache_creation` at 125%). Delegates the actual pricing to
/// [`crate::economy::estimator::compute_cost_micros`], so the SQL writer and
/// the transcript ingest path share a single source of truth.
///
/// ## Modes
///
/// - `force = false` (default): only touch rows with `cost_usd_micros IS
///   NULL OR cost_usd_micros = 0`. Idempotent — running twice updates nothing
///   new because the second pass finds no candidates. Use this after adding
///   a new pricing branch (a model id we didn't track before).
///
/// - `force = true`: SELECT every row with any non-zero token bucket and
///   recompute its cost. The UPDATE does NOT filter on the prior cost value,
///   so an existing wrong number is overwritten. Use this when the *formula*
///   changes and historical rows now compute to a different value.
///
/// In both modes, rows with all-zero tokens are skipped (nothing to price —
/// NULL/0 is honest).
///
/// All UPDATEs run inside one transaction so a sweep is all-or-nothing.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] on a database failure.
pub fn backfill_null_costs(conn: &Connection, force: bool) -> Result<BackfillReport> {
    use crate::economy::estimator::compute_cost_micros;
    type CandidateRow = (
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    );

    // ── Step 1: collect candidates ─────────────────────────────────────
    // SELECT before UPDATE so we know the exact span_id set we touched, and
    // so the UPDATE step can use parameterised clauses without re-running
    // the SQL twice. Pull every token bucket that the cache-aware pricing
    // helper needs (input, cache_creation, cache_read, output).
    //
    // Filter modes:
    //   force=false: only NULL/0 cost rows that also have any token > 0.
    //   force=true:  every row with any token > 0, regardless of current cost.
    let select_sql = if force {
        "SELECT span_id, model, input_tokens, cache_creation_input_tokens, \
                cache_read_input_tokens, output_tokens \
         FROM run_usage \
         WHERE (COALESCE(input_tokens,0) > 0 \
                OR COALESCE(cache_creation_input_tokens,0) > 0 \
                OR COALESCE(cache_read_input_tokens,0) > 0 \
                OR COALESCE(output_tokens,0) > 0)"
    } else {
        "SELECT span_id, model, input_tokens, cache_creation_input_tokens, \
                cache_read_input_tokens, output_tokens \
         FROM run_usage \
         WHERE (cost_usd_micros IS NULL OR cost_usd_micros = 0) \
           AND (COALESCE(input_tokens,0) > 0 \
                OR COALESCE(cache_creation_input_tokens,0) > 0 \
                OR COALESCE(cache_read_input_tokens,0) > 0 \
                OR COALESCE(output_tokens,0) > 0)"
    };

    let mut stmt = conn.prepare(select_sql)?;
    let candidates: Vec<CandidateRow> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    drop(stmt);

    let scanned = candidates.len();
    if scanned == 0 {
        return Ok(BackfillReport::default());
    }

    // ── Step 2: compute + UPDATE in one transaction ────────────────────
    // Single transaction so a partial failure leaves the table untouched —
    // the user can re-run without worrying about half-applied state.
    let tx = conn.unchecked_transaction()?;
    let mut updated = 0usize;
    for (span_id, model_opt, input_tokens, cache_creation, cache_read, output_tokens) in
        candidates
    {
        let cost = match compute_cost_micros(
            model_opt.as_deref(),
            input_tokens.unwrap_or(0),
            cache_creation.unwrap_or(0),
            cache_read.unwrap_or(0),
            output_tokens.unwrap_or(0),
        ) {
            Some(c) if c > 0 => c,
            // Either the pricing helper returned None (degenerate / missing
            // fallback pricing) or computed zero. Either way, keep the prior
            // value rather than write a misleading 0.
            _ => continue,
        };

        // In force mode the WHERE filter on cost is intentionally dropped so
        // an existing (potentially wrong) number is overwritten. In the
        // default mode we keep the NULL/0 guard so concurrent writes that
        // priced the row in the meantime are not clobbered.
        if force {
            tx.execute(
                "UPDATE run_usage SET cost_usd_micros = ?1 WHERE span_id = ?2",
                params![cost, span_id],
            )?;
        } else {
            tx.execute(
                "UPDATE run_usage SET cost_usd_micros = ?1 \
                 WHERE span_id = ?2 \
                   AND (cost_usd_micros IS NULL OR cost_usd_micros = 0)",
                params![cost, span_id],
            )?;
        }
        updated += 1;
    }
    tx.commit()?;

    Ok(BackfillReport { scanned, updated })
}

/// Delete telemetry rows older than `cutoff_ts_ms` (milliseconds since the Unix
/// epoch) and return the total number of rows removed.
///
/// Two tables are swept against their respective time columns:
///
/// - `run_usage` by `started_at` (ms epoch) — the per-execution facts.
/// - `usage_totals` by `updated_at` (ms epoch) — the aggregated counters.
///
/// Rows with a `NULL` time column are **kept** (they have no orderable age, so
/// pruning them would be guessing). The two deletes run inside one transaction
/// so a retention sweep is all-or-nothing.
///
/// This is fail-open at the policy layer: the caller (the rt `session_cleanup`
/// hook) decides whether a sweep failure should be swallowed. The function
/// itself returns a [`Result`] and never panics.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure.
pub fn prune_older_than(conn: &Connection, cutoff_ts_ms: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let runs = tx.execute(
        "DELETE FROM run_usage WHERE started_at IS NOT NULL AND started_at < ?1",
        params![cutoff_ts_ms],
    )?;
    let totals = tx.execute(
        "DELETE FROM usage_totals WHERE updated_at IS NOT NULL AND updated_at < ?1",
        params![cutoff_ts_ms],
    )?;
    tx.commit()?;
    Ok(runs + totals)
}

/// Convenience wrapper over [`prune_older_than`]: delete telemetry older than
/// `days` before `now_ts_ms`.
///
/// `now_ts_ms` is passed in (not read from the clock) so callers stay
/// deterministic and testable. `days` is clamped to be non-negative; `days = 0`
/// puts the cutoff at `now_ts_ms`, pruning everything stamped strictly before
/// `now`.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure.
pub fn prune_older_than_days(conn: &Connection, days: i64, now_ts_ms: i64) -> Result<usize> {
    let cutoff = now_ts_ms - days.max(0) * MS_PER_DAY;
    prune_older_than(conn, cutoff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::store::TelemetryStore;
    use tempfile::tempdir;

    fn stamp(conn: &Connection, tool_use_id: &str, agent: &str, updated_at: i64) {
        upsert_attribution(
            conn,
            &RunAttribution {
                session_id: "s1".into(),
                tool_use_id: tool_use_id.into(),
                spec: Some("spec-A".into()),
                wave_id: Some("w1".into()),
                agent_id: Some(agent.into()),
                updated_at: Some(updated_at),
            },
        )
        .unwrap();
    }

    #[test]
    fn session_fallback_picks_most_recent_at_or_before() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::new(dir.path().join("telemetry.db")).unwrap();
        let conn = store.conn();
        stamp(conn, "tu1", "agent-early", 100);
        stamp(conn, "tu2", "agent-mid", 200);
        stamp(conn, "tu3", "agent-late", 300);

        // before_ts = 250 → newest at-or-before is the 200 stamp (agent-mid).
        let got = lookup_attribution_by_session(conn, "s1", Some(250))
            .unwrap()
            .unwrap();
        assert_eq!(got.agent_id.as_deref(), Some("agent-mid"));

        // before_ts = 50 (older than every stamp) → no match.
        assert!(lookup_attribution_by_session(conn, "s1", Some(50)).unwrap().is_none());

        // Unknown session → no match.
        assert!(lookup_attribution_by_session(conn, "other", Some(999)).unwrap().is_none());
    }

    #[test]
    fn session_fallback_unbounded_when_no_ts() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::new(dir.path().join("telemetry.db")).unwrap();
        let conn = store.conn();
        stamp(conn, "tu1", "agent-early", 100);
        stamp(conn, "tu2", "agent-late", 300);

        // No before_ts → the single most-recent stamp regardless of time.
        let got = lookup_attribution_by_session(conn, "s1", None).unwrap().unwrap();
        assert_eq!(got.agent_id.as_deref(), Some("agent-late"));
    }

    fn seed_run(conn: &Connection, span_id: &str, started_at: Option<i64>) {
        record_run(
            conn,
            &RunUsage {
                trace_id: None,
                span_id: span_id.into(),
                parent_span_id: None,
                name: None,
                started_at,
                ended_at: None,
                duration_ms: None,
                attributes: None,
                spec: Some("spec-A".into()),
                phase: Some("EXECUTE".into()),
                model: Some("opus".into()),
                input_tokens: Some(10),
                output_tokens: Some(0),
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
                cost_usd_micros: Some(1),
                is_error: false,
                project_path: None,
                ts_iso: None,
                session_id: Some("s1".into()),
                wave_id: None,
                tool_use_id: None,
                agent_id: None,
            },
        )
        .unwrap();
    }

    fn seed_total(conn: &Connection, session: &str, updated_at: Option<i64>) {
        upsert_usage_metric(
            conn,
            &UsageMetric {
                metric: "claude_code.cost.usage".into(),
                model: Some("opus".into()),
                session_id: Some(session.into()),
                sum: 1.0,
                updated_at,
            },
        )
        .unwrap();
    }

    #[test]
    fn prune_older_than_deletes_only_aged_rows_and_keeps_null_ts() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::new(dir.path().join("telemetry.db")).unwrap();
        let conn = store.conn();

        seed_run(conn, "old", Some(100));
        seed_run(conn, "new", Some(1_000));
        seed_run(conn, "null-ts", None); // never aged out
        seed_total(conn, "old-sess", Some(100));
        seed_total(conn, "new-sess", Some(1_000));
        seed_total(conn, "null-sess", None); // never aged out

        // Cutoff 500: strictly-older rows go (one run + one total).
        let removed = prune_older_than(conn, 500).unwrap();
        assert_eq!(removed, 2);

        let runs: i64 = conn
            .query_row("SELECT COUNT(*) FROM run_usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(runs, 2, "the new + null-ts runs survive");
        let totals: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_totals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(totals, 2, "the new + null-ts totals survive");
    }

    #[test]
    fn backfill_null_costs_prices_null_rows_and_skips_already_priced() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::new(dir.path().join("telemetry.db")).unwrap();
        let conn = store.conn();

        // Seed three rows:
        //  - null-cost: cost_usd_micros = NULL, sonnet tokens → should be priced
        //  - already-priced: cost_usd_micros = 999_999 → untouched in default mode
        //  - all-zero-tokens: cost = NULL, tokens = 0 → skipped (honest NULL)
        let seed = |span_id: &str,
                    cost: Option<i64>,
                    input: Option<i64>,
                    cache_read: Option<i64>,
                    output: Option<i64>| {
            record_run(
                conn,
                &RunUsage {
                    trace_id: None,
                    span_id: span_id.into(),
                    parent_span_id: None,
                    name: None,
                    started_at: None,
                    ended_at: None,
                    duration_ms: None,
                    attributes: None,
                    spec: None,
                    phase: None,
                    model: Some("claude-sonnet-4-6".into()),
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_input_tokens: cache_read,
                    cache_creation_input_tokens: None,
                    cost_usd_micros: cost,
                    is_error: false,
                    project_path: None,
                    ts_iso: None,
                    session_id: Some("s1".into()),
                    wave_id: None,
                    tool_use_id: None,
                    agent_id: None,
                },
            )
            .unwrap();
        };
        seed("null-cost", None, Some(1_000), Some(10_000), Some(500));
        seed(
            "already-priced",
            Some(999_999),
            Some(1_000),
            Some(10_000),
            Some(500),
        );
        seed("all-zero-tokens", None, Some(0), Some(0), Some(0));

        let report = backfill_null_costs(conn, false).unwrap();
        // Two rows passed the filter (null-cost, all-zero-tokens has zero tokens → not selected);
        // only null-cost actually gets a non-zero cost write.
        assert_eq!(report.updated, 1);
        assert_eq!(report.scanned, 1);

        // Cache-aware price for null-cost: 1000*3M + 10000*3M/10 + 500*15M
        //   = 3_000_000_000 + 3_000_000_000 + 7_500_000_000 = 13_500_000_000 raw micros
        //   = 13_500 micros after / 1_000_000.
        let cost_null: Option<i64> = conn
            .query_row(
                "SELECT cost_usd_micros FROM run_usage WHERE span_id = ?1",
                params!["null-cost"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cost_null, Some(13_500));

        // already-priced row: untouched (force=false leaves non-zero costs alone).
        let cost_priced: Option<i64> = conn
            .query_row(
                "SELECT cost_usd_micros FROM run_usage WHERE span_id = ?1",
                params!["already-priced"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cost_priced, Some(999_999));
    }

    #[test]
    fn backfill_null_costs_force_recomputes_existing_rows() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::new(dir.path().join("telemetry.db")).unwrap();
        let conn = store.conn();

        // Seed a row with a wrong pre-existing cost (e.g. legacy pricing path
        // that counted cache_read as fresh input — 10× inflated).
        record_run(
            conn,
            &RunUsage {
                trace_id: None,
                span_id: "inflated".into(),
                parent_span_id: None,
                name: None,
                started_at: None,
                ended_at: None,
                duration_ms: None,
                attributes: None,
                spec: None,
                phase: None,
                model: Some("claude-sonnet-4-6".into()),
                input_tokens: Some(1_000),
                output_tokens: Some(500),
                cache_read_input_tokens: Some(10_000),
                cache_creation_input_tokens: None,
                // Legacy inflated cost: treated cache_read as full input.
                //   (1000 + 10_000)*3M + 500*15M = 33_000_000_000 + 7_500_000_000
                //   → 40_500 micros.
                cost_usd_micros: Some(40_500),
                is_error: false,
                project_path: None,
                ts_iso: None,
                session_id: Some("s1".into()),
                wave_id: None,
                tool_use_id: None,
                agent_id: None,
            },
        )
        .unwrap();

        // Default (non-force) leaves the inflated row alone: it has a non-zero cost.
        let report_noforce = backfill_null_costs(conn, false).unwrap();
        assert_eq!(report_noforce.updated, 0);
        let still_inflated: Option<i64> = conn
            .query_row(
                "SELECT cost_usd_micros FROM run_usage WHERE span_id = ?1",
                params!["inflated"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(still_inflated, Some(40_500));

        // Force recomputes against the cache-aware formula.
        let report_force = backfill_null_costs(conn, true).unwrap();
        assert_eq!(report_force.updated, 1);
        let now_correct: Option<i64> = conn
            .query_row(
                "SELECT cost_usd_micros FROM run_usage WHERE span_id = ?1",
                params!["inflated"],
                |r| r.get(0),
            )
            .unwrap();
        // Correct cache-aware cost: 13_500 (see prior test).
        assert_eq!(now_correct, Some(13_500));
    }

    #[test]
    fn prune_older_than_days_uses_supplied_now() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::new(dir.path().join("telemetry.db")).unwrap();
        let conn = store.conn();

        let now = 100 * MS_PER_DAY;
        seed_run(conn, "ancient", Some(now - 91 * MS_PER_DAY));
        seed_run(conn, "fresh", Some(now - MS_PER_DAY));

        // 90-day window: the 91-day-old run is pruned, the 1-day-old one kept.
        let removed = prune_older_than_days(conn, 90, now).unwrap();
        assert_eq!(removed, 1);
        let runs: i64 = conn
            .query_row("SELECT COUNT(*) FROM run_usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(runs, 1);

        // days = 0 sets the cutoff at `now`, so the remaining 1-day-old run
        // (started_at < now) is pruned too.
        assert_eq!(prune_older_than_days(conn, 0, now).unwrap(), 1);
    }
}
