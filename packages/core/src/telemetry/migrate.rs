//! One-shot, idempotent migration that builds `telemetry.db` from the data
//! still living in `mustard.db`.
//!
//! This wave is **additive**: it reads the legacy `claude_code_otel` and
//! `spans` tables (plus `events` for the attribution backfill) out of
//! `mustard.db` and writes the reduced/attributed equivalents into the
//! dedicated telemetry tables. It does **not** drop the legacy tables or touch
//! `store/sqlite_schema.sql` — that cleanup is deferred to a later wave once
//! the readers and writers have switched over, so the workspace keeps building
//! between waves.
//!
//! Idempotency is guaranteed two ways: the migration is skipped entirely when
//! the telemetry tables are already populated, and every INSERT uses
//! `INSERT OR IGNORE` so a partial run can be safely re-driven.
//!
//! `mustard.db` is attached **only here**, inside the one-time migration — the
//! reader path never attaches it, preserving the independence of the two
//! databases.

use rusqlite::{Connection, params};

use crate::error::{Error, Result};

/// Build `telemetry.db` (on `conn`) from the legacy telemetry in the
/// `mustard.db` at `mustard_db_path`.
///
/// Steps (all guarded for idempotency):
///
/// 1. Skip when the telemetry tables already hold rows.
/// 2. `ATTACH` the source `mustard.db` read-side.
/// 3. Aggregate `claude_code_otel` → `usage_totals` by
///    `(metric, model, session_id)`, summing `sum` and taking `MAX(ts_bucket)`
///    as `updated_at`.
/// 4. Copy `spans` → `run_usage`, backfilling `spec` / `wave_id` / `agent_id`
///    by correlating with `events(agent.start)` — the same logic as the W4
///    attribution CTE in `economy::reader`, replicated here as a one-time
///    backfill.
/// 5. `DETACH` the source.
///
/// Returns the number of `run_usage` rows written (0 when skipped).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] for any database failure. A missing source
/// `mustard.db` is treated as "nothing to migrate" (returns `Ok(0)`) rather
/// than an error — fail-open.
pub fn migrate_from_mustard_db(conn: &Connection, mustard_db_path: &str) -> Result<usize> {
    // Step 1: skip only when BOTH telemetry tables are already populated — see
    // NOTE-4. Checking them together would let a partial prior run (e.g. the
    // OTEL half copied, the spans half not) skip the missing half forever; the
    // per-table gates inside `migrate_attached` complete only the absent side.
    // INSERT OR IGNORE keeps each side a no-op when its rows already exist.
    if usage_totals_populated(conn)? && run_usage_populated(conn)? {
        return Ok(0);
    }

    // Fail-open: a source path that does not exist (fresh project, telemetry
    // before any harness run) means there is simply nothing to migrate.
    if !std::path::Path::new(mustard_db_path).exists() {
        return Ok(0);
    }

    // Step 2: attach the source read-side. The path is interpolated through a
    // bound parameter to avoid quoting pitfalls.
    conn.execute("ATTACH DATABASE ?1 AS src", params![mustard_db_path])
        .map_err(Error::from)?;

    // Run the body and ALWAYS detach, even on error.
    let result = migrate_attached(conn);
    let _ = conn.execute_batch("DETACH DATABASE src");
    result
}

/// Carry the legacy telemetry out of an **already-open** `mustard.db` into its
/// sibling `telemetry.db`, given only the open `mustard.db` connection.
///
/// This is the variant called from the schema migration ladder
/// (`store::migrations::migrate_v7_to_v8`), which holds the `mustard.db`
/// connection but not its path or the telemetry path. It:
///
/// 1. Reads the `mustard.db` file path back off the connection
///    (`PRAGMA database_list`, the `main` schema row).
/// 2. Resolves the sibling `telemetry.db` the same way [`crate::telemetry::store`]
///    does — `MUSTARD_TELEMETRY_DB_PATH` if set and non-blank, else
///    `{mustard_dir}/telemetry.db` (they live side by side in `.harness/`).
/// 3. Opens the telemetry store and runs [`migrate_from_mustard_db`] against the
///    `mustard.db` path (the copy attaches the source read-side itself).
///
/// Returns the number of `run_usage` rows the copy wrote (0 when skipped or
/// nothing to carry).
///
/// # Errors
///
/// Returns [`Error::Sqlite`] / [`Error::Io`] if the connection path cannot be
/// read, the telemetry store cannot be opened, or the copy fails. The caller
/// (the migration ladder) treats any error as fail-open: it does NOT drop the
/// legacy tables and leaves the schema version unadvanced so the next open
/// retries — no data is lost.
pub fn migrate_into_telemetry_db(mustard_conn: &Connection) -> Result<usize> {
    // Step 1: read the on-disk path of the `main` database off the connection.
    // `PRAGMA database_list` yields (seq, name, file) rows; `main` is the open
    // mustard.db. An in-memory or temp database reports an empty file — nothing
    // to migrate from, so treat it as a no-op.
    let mustard_path: String = mustard_conn
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .map_err(Error::from)?
        .unwrap_or_default();
    if mustard_path.trim().is_empty() {
        return Ok(0);
    }

    // Step 2: resolve the sibling telemetry.db. Env override wins (matches
    // `telemetry::store::resolve_db_path`); otherwise it sits next to mustard.db.
    let telemetry_path = match std::env::var("MUSTARD_TELEMETRY_DB_PATH") {
        Ok(v) if !v.trim().is_empty() => std::path::PathBuf::from(v),
        _ => std::path::Path::new(&mustard_path)
            .parent()
            .map(|dir| dir.join("telemetry.db"))
            .unwrap_or_else(|| std::path::PathBuf::from("telemetry.db")),
    };

    // Step 3: open the telemetry store (creating it if absent) and run the
    // copy. The copy reads FROM the mustard.db path via its own ATTACH — it does
    // not reuse `mustard_conn` (a fresh read-side attach keeps the two opens
    // independent and avoids nesting the migration's open transaction).
    let store = crate::telemetry::store::TelemetryStore::new(telemetry_path)?;
    migrate_from_mustard_db(store.conn(), &mustard_path)
}

/// Body of the migration, run while `src` (the source `mustard.db`) is attached.
fn migrate_attached(conn: &Connection) -> Result<usize> {
    // Step 3: aggregate the OTEL counters, but only when the destination half
    // is still empty (NOTE-4: independent idempotency) and the legacy source
    // table exists. INSERT OR IGNORE makes this safe even if both hold.
    if !usage_totals_populated(conn)? && table_exists(conn, "src", "claude_code_otel") {
        conn.execute_batch(
            "INSERT OR IGNORE INTO usage_totals (metric, model, session_id, sum, updated_at) \
             SELECT metric, model, session_id, SUM(sum), MAX(ts_bucket) \
             FROM src.claude_code_otel \
             GROUP BY metric, model, session_id;",
        )
        .map_err(Error::from)?;
    }

    // Step 4: copy spans → run_usage with the attribution backfill, again only
    // when the destination half is empty. Absent `spans` (a database predating
    // the projection) is a no-op.
    if !run_usage_populated(conn)? && table_exists(conn, "src", "spans") {
        conn.execute_batch(&copy_spans_sql()).map_err(Error::from)?;
    }

    let written: i64 = conn
        .query_row("SELECT COUNT(*) FROM run_usage", [], |r| r.get(0))
        .map_err(Error::from)?;
    Ok(usize::try_from(written.max(0)).unwrap_or(0))
}

/// The `INSERT OR IGNORE INTO run_usage SELECT ... FROM src.spans` statement,
/// with the spec / wave_id / agent_id attribution backfill inlined.
///
/// The backfill replicates the W4 CTE in `economy::reader::attribution_cte`:
/// for each span, two correlated subqueries walk the join legs in priority
/// order against `src.events` —
///
/// - **primary**: `JSON_EXTRACT(events.payload,'$.tool_use_id') = span.tool_use_id`
/// - **fallback**: the most-recent `agent.start` with the same `session_id`
///   whose `ts <= span.ts_iso`
///
/// and the resolved value is `COALESCE`'d with the span's own column so a span
/// recorded without any `agent.start` still keeps its native attribution:
///
/// - `agent_id` = payload.agent_id ?? payload.subagentType ?? events.actor_id
/// - `spec`     = payload.spec_id  ?? events.spec ?? span.spec
/// - `wave_id`  = payload.wave_id  ?? CAST(events.wave AS TEXT) ?? span.wave_id
fn copy_spans_sql() -> String {
    // Each backfill column is built from a primary-leg subquery COALESCE'd with
    // a fallback-leg subquery, then COALESCE'd with the span's own column.
    let agent_primary = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.agent_id'), \
                                          JSON_EXTRACT(ev.payload,'$.subagentType'), ev.actor_id) \
                          FROM src.events ev \
                          WHERE ev.event = 'agent.start' AND s.tool_use_id IS NOT NULL \
                            AND JSON_EXTRACT(ev.payload,'$.tool_use_id') = s.tool_use_id LIMIT 1)";
    let agent_fallback = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.agent_id'), \
                                           JSON_EXTRACT(ev.payload,'$.subagentType'), ev.actor_id) \
                           FROM src.events ev \
                           WHERE ev.event = 'agent.start' AND ev.session_id IS NOT NULL \
                             AND s.session_id IS NOT NULL AND ev.session_id = s.session_id \
                             AND ev.ts <= s.ts_iso ORDER BY ev.ts DESC LIMIT 1)";
    let spec_primary = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.spec_id'), ev.spec) \
                         FROM src.events ev \
                         WHERE ev.event = 'agent.start' AND s.tool_use_id IS NOT NULL \
                           AND JSON_EXTRACT(ev.payload,'$.tool_use_id') = s.tool_use_id LIMIT 1)";
    let spec_fallback = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.spec_id'), ev.spec) \
                          FROM src.events ev \
                          WHERE ev.event = 'agent.start' AND ev.session_id IS NOT NULL \
                            AND s.session_id IS NOT NULL AND ev.session_id = s.session_id \
                            AND ev.ts <= s.ts_iso ORDER BY ev.ts DESC LIMIT 1)";
    let wave_primary = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.wave_id'), CAST(ev.wave AS TEXT)) \
                         FROM src.events ev \
                         WHERE ev.event = 'agent.start' AND s.tool_use_id IS NOT NULL \
                           AND JSON_EXTRACT(ev.payload,'$.tool_use_id') = s.tool_use_id LIMIT 1)";
    let wave_fallback = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.wave_id'), CAST(ev.wave AS TEXT)) \
                          FROM src.events ev \
                          WHERE ev.event = 'agent.start' AND ev.session_id IS NOT NULL \
                            AND s.session_id IS NOT NULL AND ev.session_id = s.session_id \
                            AND ev.ts <= s.ts_iso ORDER BY ev.ts DESC LIMIT 1)";

    format!(
        "INSERT OR IGNORE INTO run_usage \
            (trace_id, span_id, parent_span_id, name, started_at, ended_at, \
             duration_ms, attributes, spec, phase, model, input_tokens, \
             output_tokens, cache_read_input_tokens, cache_creation_input_tokens, \
             cost_usd_micros, is_error, project_path, ts_iso, session_id, \
             wave_id, tool_use_id, agent_id) \
         SELECT s.trace_id, s.span_id, s.parent_span_id, s.name, s.started_at, s.ended_at, \
                s.duration_ms, s.attributes, \
                COALESCE({spec_primary}, {spec_fallback}, s.spec) AS attr_spec, \
                s.phase, s.model, s.input_tokens, s.output_tokens, \
                s.cache_read_input_tokens, s.cache_creation_input_tokens, \
                s.cost_usd_micros, s.is_error, s.project_path, s.ts_iso, s.session_id, \
                COALESCE({wave_primary}, {wave_fallback}, s.wave_id) AS attr_wave, \
                s.tool_use_id, \
                COALESCE({agent_primary}, {agent_fallback}) AS attr_agent \
         FROM src.spans s;"
    )
}

/// `true` when `usage_totals` already holds at least one row.
fn usage_totals_populated(conn: &Connection) -> Result<bool> {
    let usage: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_totals", [], |r| r.get(0))
        .map_err(Error::from)?;
    Ok(usage > 0)
}

/// `true` when `run_usage` already holds at least one row.
fn run_usage_populated(conn: &Connection) -> Result<bool> {
    let runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM run_usage", [], |r| r.get(0))
        .map_err(Error::from)?;
    Ok(runs > 0)
}

/// `true` when `schema.table` exists in the attached database `schema`.
///
/// Returns `bool` (not `Result`): a query failure here means "table not
/// reachable", which is exactly the `false` the callers want — there is no
/// error worth propagating.
fn table_exists(conn: &Connection, schema: &str, table: &str) -> bool {
    // `schema` is a crate-internal literal ("src"); `table` is a literal too.
    let sql = format!("SELECT 1 FROM {schema}.sqlite_master WHERE type='table' AND name=?1 LIMIT 1");
    let found: Option<i64> = conn.query_row(&sql, params![table], |r| r.get(0)).ok();
    found.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite_store::SqliteEventStore;
    use crate::telemetry::store::TelemetryStore;
    use rusqlite::Connection;
    use std::path::Path;
    use tempfile::tempdir;

    /// Create a `mustard.db` (with the full harness schema) at `path`, then seed
    /// the legacy telemetry tables directly.
    fn seed_mustard_db(path: &Path) {
        // Open through SqliteEventStore so the events schema + migrations apply.
        let _store = SqliteEventStore::new(path).unwrap();
        let conn = Connection::open(path).unwrap();

        // Wave 3 stopped shipping `spans` / `claude_code_otel` in the harness
        // schema and the v8 migration drops them, so a freshly-opened store no
        // longer has them. This migration test models a pre-Wave-3 source DB
        // that still carries the legacy telemetry, so we recreate the two
        // tables (with the columns the backfill reads) before seeding them.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS claude_code_otel (\
                 ts_bucket INTEGER NOT NULL, signal TEXT NOT NULL, metric TEXT NOT NULL, \
                 session_id TEXT, model TEXT, token_type TEXT, sum REAL DEFAULT 0, \
                 count INTEGER DEFAULT 0, attrs TEXT, \
                 PRIMARY KEY (ts_bucket, metric, session_id, model, token_type)); \
             CREATE TABLE IF NOT EXISTS spans (\
                 trace_id TEXT, span_id TEXT PRIMARY KEY, parent_span_id TEXT, name TEXT, \
                 started_at INTEGER, ended_at INTEGER, duration_ms INTEGER, attributes TEXT, \
                 spec TEXT, phase TEXT, model TEXT, input_tokens INTEGER, output_tokens INTEGER, \
                 is_error INTEGER, cache_read_input_tokens INTEGER, \
                 cache_creation_input_tokens INTEGER, cost_usd_micros INTEGER, \
                 project_path TEXT, ts_iso TEXT, session_id TEXT, wave_id TEXT, tool_use_id TEXT);",
        )
        .unwrap();

        // 5 distinct usage_totals after aggregation:
        //  (cost.usage, opus, s1)   sum 30 across two minute-buckets
        //  (cost.usage, sonnet, s1) sum 5
        //  (cost.usage, opus, s2)   sum 7
        //  (session.count, NULL, s1) sum 2
        //  (active_time.total, NULL, s1) sum 120
        let otel = [
            (60_000i64, "claude_code.cost.usage", Some("opus"), Some("s1"), Some("input"), 10.0),
            (120_000, "claude_code.cost.usage", Some("opus"), Some("s1"), Some("input"), 20.0),
            (60_000, "claude_code.cost.usage", Some("sonnet"), Some("s1"), Some("input"), 5.0),
            (60_000, "claude_code.cost.usage", Some("opus"), Some("s2"), Some("input"), 7.0),
            (60_000, "claude_code.session.count", None, Some("s1"), None, 2.0),
            (60_000, "claude_code.active_time.total", None, Some("s1"), None, 120.0),
        ];
        for (bucket, metric, model, session, ttype, sum) in otel {
            conn.execute(
                "INSERT INTO claude_code_otel \
                 (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs) \
                 VALUES (?1, 'metric', ?2, ?3, ?4, ?5, ?6, 1, '{}')",
                params![bucket, metric, session, model, ttype, sum],
            )
            .unwrap();
        }

        // agent.start events for the backfill: span sp-tu matches by tool_use_id,
        // span sp-sess matches by the session+ts fallback, span sp-orphan keeps
        // its own native columns (no agent.start to join).
        conn.execute(
            "INSERT INTO events (ts, session_id, wave, spec, event, actor_kind, actor_id, payload) \
             VALUES ('2026-05-22T10:00:00Z', 's1', 0, NULL, 'agent.start', 'agent', 'fallback-actor', \
                     '{\"session_id\":\"s1\"}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (ts, session_id, wave, spec, event, actor_kind, actor_id, payload) \
             VALUES ('2026-05-22T11:00:00Z', 's1', 0, NULL, 'agent.start', 'agent', 'tu-actor', \
                     '{\"tool_use_id\":\"tu-1\",\"agent_id\":\"core-impl\",\"spec_id\":\"spec-A\",\"wave_id\":\"w1\"}')",
            [],
        )
        .unwrap();

        // spans → run_usage.
        // sp-tu: has tool_use_id 'tu-1' → primary leg attributes to core-impl/spec-A/w1.
        conn.execute(
            "INSERT INTO spans (span_id, spec, model, input_tokens, output_tokens, \
             cost_usd_micros, is_error, ts_iso, session_id, tool_use_id) \
             VALUES ('sp-tu', NULL, 'opus', 1000, 500, 25000, 0, '2026-05-22T11:30:00Z', 's1', 'tu-1')",
            [],
        )
        .unwrap();
        // sp-sess: no tool_use_id, falls back to most-recent agent.start in s1
        // with ts <= span ts → the tu-1 event (11:00) wins (most recent ≤ 11:45).
        conn.execute(
            "INSERT INTO spans (span_id, spec, model, input_tokens, output_tokens, \
             cost_usd_micros, is_error, ts_iso, session_id) \
             VALUES ('sp-sess', NULL, 'opus', 200, 100, 5000, 0, '2026-05-22T11:45:00Z', 's1')",
            [],
        )
        .unwrap();
        // sp-orphan: no session match (different session, no agent.start) → keeps
        // its own native spec/wave_id; agent stays NULL.
        conn.execute(
            "INSERT INTO spans (span_id, spec, model, input_tokens, output_tokens, \
             cost_usd_micros, is_error, ts_iso, session_id, wave_id) \
             VALUES ('sp-orphan', 'spec-Z', 'sonnet', 50, 10, 1000, 0, '2026-05-22T09:00:00Z', 's9', 'w9')",
            [],
        )
        .unwrap();
    }

    fn telemetry_at(dir: &Path) -> TelemetryStore {
        TelemetryStore::new(dir.join("telemetry.db")).unwrap()
    }

    #[test]
    fn aggregation_preserves_the_five_usage_totals() {
        let dir = tempdir().unwrap();
        let mustard = dir.path().join("mustard.db");
        seed_mustard_db(&mustard);
        let store = telemetry_at(dir.path());

        migrate_from_mustard_db(store.conn(), &mustard.to_string_lossy()).unwrap();

        let conn = store.conn();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_totals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 5, "5 distinct (metric, model, session) groups");

        // (cost.usage, opus, s1) summed across the two minute-buckets = 30,
        // updated_at = MAX(ts_bucket) = 120_000.
        let (sum, updated): (f64, i64) = conn
            .query_row(
                "SELECT sum, updated_at FROM usage_totals \
                 WHERE metric='claude_code.cost.usage' AND model='opus' AND session_id='s1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((sum - 30.0).abs() < f64::EPSILON);
        assert_eq!(updated, 120_000);
    }

    #[test]
    fn backfill_attributes_spec_wave_agent_like_the_cte() {
        let dir = tempdir().unwrap();
        let mustard = dir.path().join("mustard.db");
        seed_mustard_db(&mustard);
        let store = telemetry_at(dir.path());

        migrate_from_mustard_db(store.conn(), &mustard.to_string_lossy()).unwrap();
        let conn = store.conn();

        let row = |span: &str| -> (Option<String>, Option<String>, Option<String>) {
            conn.query_row(
                "SELECT spec, wave_id, agent_id FROM run_usage WHERE span_id = ?1",
                params![span],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };

        // Primary leg (tool_use_id match).
        assert_eq!(
            row("sp-tu"),
            (Some("spec-A".into()), Some("w1".into()), Some("core-impl".into()))
        );
        // Fallback leg (session + ts window) — same agent.start wins.
        assert_eq!(
            row("sp-sess"),
            (Some("spec-A".into()), Some("w1".into()), Some("core-impl".into()))
        );
        // No join → native columns kept, agent NULL.
        assert_eq!(
            row("sp-orphan"),
            (Some("spec-Z".into()), Some("w9".into()), None)
        );
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = tempdir().unwrap();
        let mustard = dir.path().join("mustard.db");
        seed_mustard_db(&mustard);
        let store = telemetry_at(dir.path());
        let path = mustard.to_string_lossy().into_owned();

        let first = migrate_from_mustard_db(store.conn(), &path).unwrap();
        assert_eq!(first, 3, "three spans copied");
        // Second run is skipped (already populated) — no duplicate rows.
        let second = migrate_from_mustard_db(store.conn(), &path).unwrap();
        assert_eq!(second, 0);
        let runs: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM run_usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(runs, 3);
    }

    #[test]
    fn missing_source_is_noop_not_error() {
        let dir = tempdir().unwrap();
        let store = telemetry_at(dir.path());
        let written =
            migrate_from_mustard_db(store.conn(), &dir.path().join("absent.db").to_string_lossy())
                .unwrap();
        assert_eq!(written, 0);
    }

    #[test]
    fn telemetry_db_does_not_attach_mustard_db_on_reads() {
        // After migration the reader path must not reference `src` / mustard.db.
        // Detaching here (it is already detached) and then reading proves the
        // reads stand alone on telemetry.db.
        let dir = tempdir().unwrap();
        let mustard = dir.path().join("mustard.db");
        seed_mustard_db(&mustard);
        let store = telemetry_at(dir.path());
        migrate_from_mustard_db(store.conn(), &mustard.to_string_lossy()).unwrap();

        // No attached database named 'src' should remain.
        let attached: Vec<String> = {
            let mut stmt = store.conn().prepare("PRAGMA database_list").unwrap();
            let names = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .filter_map(std::result::Result::ok)
                .collect();
            names
        };
        assert!(!attached.iter().any(|n| n == "src"), "src must be detached");

        // Reads succeed without re-attaching.
        use crate::telemetry::reader;
        // cost.usage total = opus/s1 30 + sonnet/s1 5 + opus/s2 7 = 42.
        assert!((reader::cost_total(store.conn()).unwrap() - 42.0).abs() < f64::EPSILON);
        assert_eq!(reader::runs_by_spec(store.conn()).unwrap().len(), 2); // spec-A, spec-Z
    }
}
