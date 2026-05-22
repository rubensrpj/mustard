//! Writer side of the economy domain.
//!
//! Four `record_*` functions, one per record type. Each takes a borrowed
//! [`Connection`] (callers own the open store) and the domain record, opens a
//! short transaction, and issues an `INSERT`. Failures bubble back as
//! [`Result`] — never panic, never log a stack trace; the call site decides
//! whether to fail open via [`fail_open`](crate::error::fail_open).
//!
//! The four entry points map to:
//!
//! | Function | Target table | Source |
//! |---|---|---|
//! | [`record_run`] | `run_usage` (telemetry.db) | internal estimator (W1) |
//! | [`record_api_cost`] | `run_usage` (telemetry.db) | external adapter (OTEL/JSONL, W3) |
//! | [`record_savings`] | `savings_records` (mustard.db) | every Mustard intervention |
//! | [`record_context_cost`] | `context_cost_frames` (mustard.db) | `apps/rt` dispatch hooks (W2) |
//!
//! `record_run` and `record_api_cost` share the same `run_usage` row because
//! semantically they store the same thing (one Anthropic request's worth of
//! tokens + price); only the call site signals provenance. As of Wave 2
//! (telemetry-separation) both delegate to
//! [`crate::telemetry::writer::record_run`], so `conn` must be a `telemetry.db`
//! connection. See [`ApiCostFrame`](super::model::ApiCostFrame) for the alias
//! rationale.

use rusqlite::{Connection, params};
use serde_json::Value;

use crate::error::{Error, Result};

use super::model::{ApiCostFrame, ContextCostFrame, SavingsRecord, SpanRecord};
use crate::telemetry::model::RunUsage;

/// Persist a [`SpanRecord`] as one `run_usage` row in the telemetry store.
///
/// Wave 2 (telemetry-separation): the run no longer lands in `mustard.db`'s
/// `spans` — it is mapped onto a [`RunUsage`] and written to `telemetry.db` via
/// the dedicated telemetry writer. `conn` is therefore expected to be a
/// `telemetry.db` connection (the legacy `spans` columns the reader projected
/// carry over 1:1). `record.span_id` is the primary key; a re-record of the
/// same id is an idempotent upsert.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure.
pub fn record_run(conn: &Connection, rec: SpanRecord) -> Result<()> {
    crate::telemetry::writer::record_run(conn, &span_to_run(rec))
}

/// Persist an [`ApiCostFrame`] — semantically equivalent to
/// [`record_run`] but signals the call site is an external adapter (OTEL,
/// JSONL ingest in W3) rather than the internal estimator.
///
/// Wires into the same `run_usage` row in `telemetry.db`. See module docs.
///
/// # Errors
///
/// Same as [`record_run`].
pub fn record_api_cost(conn: &Connection, rec: ApiCostFrame) -> Result<()> {
    crate::telemetry::writer::record_run(conn, &span_to_run(rec))
}

/// Persist a [`SavingsRecord`] into `savings_records`.
///
/// The `extra` map is stored as a JSON `TEXT` payload so adapter-specific
/// fields are not lost; the dashboard reads it when surfacing drill-downs.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure, or [`Error::Parse`] if
/// the `extra` map cannot be serialized (in practice unreachable — a
/// `serde_json::Map` is always serializable).
pub fn record_savings(conn: &Connection, rec: SavingsRecord) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let payload_json = serde_json::to_string(&Value::Object(rec.extra)).map_err(Error::from)?;
    tx.execute(
        "INSERT INTO savings_records \
            (ts, source, tokens_saved, model_target, project_path, \
             spec_id, wave_id, agent_id, payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            iso_to_epoch_ms(&rec.ts),
            rec.source.as_str(),
            rec.tokens_saved,
            rec.model_target,
            rec.project_path.as_path().to_string_lossy().into_owned(),
            rec.spec_id.map(|s| s.0),
            rec.wave_id.map(|w| w.0),
            rec.agent_id.map(|a| a.0),
            payload_json,
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// Persist a [`ContextCostFrame`] into `context_cost_frames`.
///
/// All `*_bytes` fields are optional — call sites that have not yet
/// instrumented the breakdown can record a partial frame and the dashboard
/// renders the columns it has.
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure.
pub fn record_context_cost(conn: &Connection, rec: ContextCostFrame) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO context_cost_frames \
            (ts, agent_id, wave_id, spec_id, project_path, \
             prompt_size_bytes, prefix_stable_bytes, slice_bytes, \
             recipe_bytes, wave_slice_bytes, return_size_bytes, \
             retry_overhead_bytes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            iso_to_epoch_ms(&rec.ts),
            rec.agent_id.0,
            rec.wave_id.map(|w| w.0),
            rec.spec_id.map(|s| s.0),
            rec.project_path.as_path().to_string_lossy().into_owned(),
            rec.prompt_size_bytes,
            rec.prefix_stable_bytes,
            rec.slice_bytes,
            rec.recipe_bytes,
            rec.wave_slice_bytes,
            rec.return_size_bytes,
            rec.retry_overhead_bytes,
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// Map a [`SpanRecord`] onto a [`RunUsage`] for the telemetry `run_usage`
/// table (Wave 2 telemetry-separation).
///
/// The legacy `spans` columns carry over 1:1; `started_at` is derived from the
/// ISO timestamp, and the W4 `tool_use_id` attribution key is pulled out of the
/// lenient `extra` map (set by W3 adapters when the upstream payload exposes
/// Anthropic's tool_use id). Adapters that have already resolved `spec` /
/// `wave_id` / `agent_id` (the Wave 2 write-time attribution) carry them on the
/// record; the rest stay `None`.
fn span_to_run(rec: SpanRecord) -> RunUsage {
    let tool_use_id = rec
        .extra
        .get("tool_use_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let wave_id = rec
        .extra
        .get("wave_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let agent_id = rec
        .extra
        .get("agent_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    RunUsage {
        trace_id: None,
        span_id: rec.span_id,
        parent_span_id: None,
        name: None,
        started_at: Some(iso_to_epoch_ms(&rec.ts)),
        ended_at: None,
        duration_ms: None,
        attributes: None,
        spec: rec.spec,
        phase: rec.phase,
        model: rec.model,
        input_tokens: rec.input_tokens,
        output_tokens: rec.output_tokens,
        cache_read_input_tokens: rec.cache_read_input_tokens,
        cache_creation_input_tokens: rec.cache_creation_input_tokens,
        cost_usd_micros: rec.cost_usd_micros,
        is_error: rec.is_error,
        project_path: None,
        ts_iso: Some(rec.ts),
        session_id: rec.session_id,
        wave_id,
        tool_use_id,
        agent_id,
    }
}

/// Best-effort ISO-8601 → epoch-millis converter for the timestamp column.
///
/// Returns `0` on a malformed timestamp — the column is non-null, and a
/// readable diagnostic from the SQL parse failure is worse than a stable
/// sentinel for hooks that fail open. Adapters that have a higher-precision
/// timestamp source should set it themselves before calling the writer.
fn iso_to_epoch_ms(ts: &str) -> i64 {
    // The crate is `jiff`-free here (jiff is a workspace dep but not pulled
    // into core today); a naive ISO parse covers the format Mustard's hooks
    // emit (`YYYY-MM-DDTHH:MM:SS[.sss]Z`). Anything fancier — timezones,
    // sub-millisecond — is delegated to the W3 OTEL/JSONL adapters.
    fn parse(ts: &str) -> Option<i64> {
        // YYYY MM DD HH MM SS [millis]
        let bytes = ts.as_bytes();
        if bytes.len() < 19 {
            return None;
        }
        let y: i64 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
        let mo: i64 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
        let d: i64 = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
        let h: i64 = std::str::from_utf8(&bytes[11..13]).ok()?.parse().ok()?;
        let mi: i64 = std::str::from_utf8(&bytes[14..16]).ok()?.parse().ok()?;
        let s: i64 = std::str::from_utf8(&bytes[17..19]).ok()?.parse().ok()?;
        let mut millis = 0i64;
        if bytes.len() >= 23 && bytes[19] == b'.' {
            millis = std::str::from_utf8(&bytes[20..23]).ok()?.parse().ok()?;
        }
        // Days since Unix epoch by the proleptic Gregorian conversion
        // (Howard Hinnant's date algorithm, simplified for positive years).
        let year = if mo <= 2 { y - 1 } else { y };
        let era = year.div_euclid(400);
        let yoe = year - era * 400;
        let m = if mo > 2 { mo - 3 } else { mo + 9 };
        let doy = (153 * m + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        let secs = days * 86_400 + h * 3600 + mi * 60 + s;
        Some(secs * 1_000 + millis)
    }
    parse(ts).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::scope::{AgentId, ProjectPath, SpecId, WaveId};
    use crate::store::sqlite_store::SqliteEventStore;
    use rusqlite::Connection;
    use serde_json::Map;
    use tempfile::tempdir;

    fn fresh_conn(dir: &std::path::Path) -> Connection {
        // Open through SqliteEventStore so the schema + migrations are applied.
        let _store = SqliteEventStore::new(dir.join("mustard.db")).unwrap();
        Connection::open(dir.join("mustard.db")).unwrap()
    }

    #[test]
    fn record_run_inserts_one_run_usage_row() {
        // Wave 2: `record_run` now writes `run_usage` in telemetry.db, so the
        // connection is a TelemetryStore connection (not mustard.db).
        let dir = tempdir().unwrap();
        let conn = crate::telemetry::store::TelemetryStore::new(dir.path().join("telemetry.db"))
            .unwrap()
            .into_connection();
        let rec = SpanRecord {
            ts: "2026-05-21T00:00:00Z".into(),
            session_id: Some("s-1".into()),
            span_id: "req-1".into(),
            model: Some("claude-opus-4-7".into()),
            spec: Some("spec-A".into()),
            phase: Some("EXECUTE".into()),
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_input_tokens: Some(800),
            cache_creation_input_tokens: Some(0),
            cost_usd_micros: Some(25_000),
            is_error: false,
            extra: Map::new(),
        };
        record_run(&conn, rec).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM run_usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn record_savings_inserts_one_row() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(dir.path());
        let rec = SavingsRecord {
            ts: "2026-05-21T00:00:00Z".into(),
            source: super::super::model::SavingsSource::RtkRewrite,
            tokens_saved: 1200,
            model_target: Some("claude-3-5-sonnet".into()),
            project_path: ProjectPath::new("/tmp/p"),
            spec_id: Some(SpecId::new("spec-A")),
            wave_id: Some(WaveId::new("w1")),
            agent_id: Some(AgentId::new("explore")),
            extra: Map::new(),
        };
        record_savings(&conn, rec).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM savings_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn record_context_cost_inserts_one_row() {
        let dir = tempdir().unwrap();
        let conn = fresh_conn(dir.path());
        let rec = ContextCostFrame {
            ts: "2026-05-21T00:00:00Z".into(),
            agent_id: AgentId::new("core-impl"),
            wave_id: Some(WaveId::new("w1")),
            spec_id: Some(SpecId::new("spec-A")),
            project_path: ProjectPath::new("/tmp/p"),
            prompt_size_bytes: Some(20_000),
            prefix_stable_bytes: Some(15_000),
            slice_bytes: Some(3_000),
            recipe_bytes: Some(500),
            wave_slice_bytes: Some(1_500),
            return_size_bytes: Some(800),
            retry_overhead_bytes: Some(0),
            extra: Map::new(),
        };
        record_context_cost(&conn, rec).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM context_cost_frames", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn iso_to_epoch_ms_known_value() {
        // 1970-01-01T00:00:00Z is the epoch — the canonical sentinel.
        assert_eq!(iso_to_epoch_ms("1970-01-01T00:00:00Z"), 0);
        // 2026-05-21T00:00:00Z = 1779321600 seconds since the epoch.
        assert_eq!(iso_to_epoch_ms("2026-05-21T00:00:00Z"), 1_779_321_600_000);
        // Millisecond precision is preserved.
        assert_eq!(
            iso_to_epoch_ms("2026-05-21T00:00:00.123Z"),
            1_779_321_600_123
        );
    }

    #[test]
    fn iso_to_epoch_ms_malformed_is_zero() {
        assert_eq!(iso_to_epoch_ms("not-a-date"), 0);
        assert_eq!(iso_to_epoch_ms(""), 0);
    }
}
