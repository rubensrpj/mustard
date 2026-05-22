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
    fn prune_older_than_days_uses_supplied_now() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::new(dir.path().join("telemetry.db")).unwrap();
        let conn = store.conn();

        let now = 100 * MS_PER_DAY;
        seed_run(conn, "ancient", Some(now - 91 * MS_PER_DAY));
        seed_run(conn, "fresh", Some(now - 1 * MS_PER_DAY));

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
