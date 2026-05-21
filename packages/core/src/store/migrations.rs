//! Schema migration runner for the harness store.
//!
//! [`SqliteEventStore::new`](super::sqlite_store::SqliteEventStore::new) calls
//! [`apply`] right after the idempotent `CREATE TABLE` block: the schema gets
//! the *shape* in place, this module advances the *data* through versioned
//! transformations that cannot be expressed as `CREATE IF NOT EXISTS`.
//!
//! # Versioning
//!
//! The current version is stored in `_mustard_meta(key='schema_version')`. A
//! fresh database has no row; [`apply`] treats that as version `1` (the
//! pre-migration baseline shipped with `sqlite_schema.sql`). Each migration
//! step bumps the row to the next version, transactionally with its own work.
//!
//! # Adding a migration
//!
//! 1. Bump [`LATEST_VERSION`].
//! 2. Add a `migrate_vN_to_vN_plus_1(conn) -> Result<()>` function.
//! 3. Append the call to the `match` arm in [`apply`] with the source version.
//! 4. Cover the new step with a unit test that seeds the previous state and
//!    asserts the post-state plus `_mustard_meta`.
//!
//! Every migration runs inside its own SQLite transaction — a failure mid-flight
//! leaves the database at the previous version, never half-applied. The runner
//! itself is fail-open at the *open* layer: [`SqliteEventStore::new`] still
//! returns [`Error::Sqlite`] if a migration fails, callers degrade safely.
//!
//! # Why `__orphan__`
//!
//! Pre-v2 the harness wrote events with `spec = NULL` from any emitter that
//! did not resolve a spec context (six call sites identified in the 2026-05-20
//! attribution audit — `tracker.rs`, `emit_event.rs`, `statusline.rs`,
//! `session_start.rs`, `spec_link.rs`, `memory.rs`). Projections that filter by
//! `spec = ?1` skipped those events silently, producing the "UNKNOWN / 0 ACs / 0
//! tools" symptom in the dashboard. The v2 migration backfills each NULL row to
//! the most recent `pipeline.scope` event in the same session (the spec that
//! was *active* when the orphan was written) and tags the unrecoverable rows
//! `'__orphan__'` so projections can group them under a single bucket instead
//! of losing them to `IS NULL` filters.

use crate::error::Result;
use rusqlite::{Connection, OptionalExtension};

/// Latest schema version this runner knows how to migrate *to*.
///
/// Bump this when adding a new `migrate_vN_to_vN_plus_1` step and append the
/// call inside [`apply`]. A database with `_mustard_meta.schema_version` equal
/// to [`LATEST_VERSION`] is a no-op on open.
pub const LATEST_VERSION: u32 = 3;

/// Sentinel spec name for events that could not be attributed by the v2
/// backfill — typically pre-pipeline events or rows missing `session_id`.
///
/// Projections can filter on this value to surface "orphaned telemetry"
/// without losing the events to a `spec IS NULL` filter.
pub const ORPHAN_SPEC: &str = "__orphan__";

/// Apply every outstanding migration to `conn`, advancing the database from
/// its stored version to [`LATEST_VERSION`].
///
/// Idempotent: a database already at the latest version is a no-op.
///
/// # Errors
///
/// Returns [`Error::Sqlite`](crate::error::Error::Sqlite) if the meta table
/// cannot be created, the version cannot be read, or any migration step fails.
/// A failing step leaves the database at the previous version (transactional).
pub fn apply(conn: &Connection) -> Result<u32> {
    ensure_meta_table(conn)?;
    let mut current = read_schema_version(conn)?;
    while current < LATEST_VERSION {
        match current {
            1 => migrate_v1_to_v2(conn)?,
            2 => migrate_v2_to_v3(conn)?,
            // Future migrations append here. The `LATEST_VERSION` constant is
            // the only invariant — every step must move the version forward.
            other => {
                return Err(crate::error::Error::Sqlite(format!(
                    "no migration registered for schema version {other}"
                )));
            }
        }
        current = read_schema_version(conn)?;
    }
    Ok(current)
}

/// Create the meta table if it does not exist. Idempotent; safe on every open.
fn ensure_meta_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _mustard_meta (\
           key TEXT PRIMARY KEY,\
           value TEXT NOT NULL\
         );",
    )?;
    Ok(())
}

/// Read the stored schema version. Treats an absent row as `1` (baseline) so a
/// freshly-created database lines up with the migration ladder.
fn read_schema_version(conn: &Connection) -> Result<u32> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM _mustard_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|s| s.parse().ok()).unwrap_or(1))
}

/// Persist the new schema version. `INSERT OR REPLACE` is safe — the PK is
/// `'schema_version'` so we always end with exactly one row. Accepts any
/// `rusqlite::Connection`-shaped handle so the helper can be called inside an
/// open transaction (`Transaction` derefs to `Connection`) without copying.
fn write_schema_version(conn: &Connection, version: u32) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO _mustard_meta(key, value) VALUES('schema_version', ?1)",
        rusqlite::params![version.to_string()],
    )?;
    Ok(())
}

/// v1 → v2 — backfill `events.spec` for rows with `NULL` attribution.
///
/// Algorithm (executed inside a single transaction):
///
/// 1. For every orphan with a non-null `session_id`, set `spec` to the most
///    recent `pipeline.scope` event in the same session whose `ts <= orphan.ts`.
///    This matches the harness's runtime semantics — the spec that was active
///    when the orphan was emitted.
/// 2. Any remaining orphans (no `pipeline.scope` ancestor, or no
///    `session_id`) are tagged [`ORPHAN_SPEC`]. They remain queryable but
///    cleanly grouped.
/// 3. Bump `_mustard_meta.schema_version` to `2`.
///
/// The migration is *only* `UPDATE` work — no schema change. Hot path stays
/// the unchanged `INSERT INTO events`; new code paths just need to populate
/// `spec` going forward (covered by the attribution hardening in `apps/rt`).
fn migrate_v1_to_v2(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    // Step 1: backfill orphans whose session has a pipeline.scope ancestor.
    // SQLite's correlated subquery is fine here — the volume is bounded by the
    // event table size, and `idx_events_event` + `idx_events_ts` cover the scan.
    tx.execute_batch(
        "UPDATE events \
         SET spec = ( \
             SELECT scope_ev.spec \
             FROM events AS scope_ev \
             WHERE scope_ev.event = 'pipeline.scope' \
               AND scope_ev.session_id = events.session_id \
               AND scope_ev.ts <= events.ts \
               AND scope_ev.spec IS NOT NULL \
             ORDER BY scope_ev.ts DESC, scope_ev.id DESC \
             LIMIT 1 \
         ) \
         WHERE events.spec IS NULL \
           AND events.session_id IS NOT NULL;",
    )?;

    // Step 2: any orphan that could not be resolved gets the sentinel.
    tx.execute_batch(
        "UPDATE events SET spec = '__orphan__' WHERE spec IS NULL;",
    )?;

    // Step 3: commit + bump version (inside the transaction so a failure rolls
    // back the data updates and the version stays at 1).
    write_schema_version(&tx, 2)?;

    tx.commit()?;
    Ok(())
}

/// v2 → v3 — add the economy-domain tables consumed by
/// [`crate::economy::writer`] and [`crate::economy::reader`].
///
/// Creates two new tables (`savings_records`, `context_cost_frames`) with
/// their secondary indices. The pre-existing `spans` table — already shipped
/// in `sqlite_schema.sql` — covers both [`SpanRecord`](crate::economy::model::SpanRecord)
/// and [`ApiCostFrame`](crate::economy::model::ApiCostFrame); this migration
/// adds two extra columns to `spans` only if they are absent, so the writer
/// can persist `cache_read_input_tokens` and `cache_creation_input_tokens`
/// (fields Anthropic added after the legacy schema was frozen).
///
/// Every statement is `IF NOT EXISTS` / a guarded `ALTER` so the migration is
/// safe to re-run on a database that already absorbed a partial v3.
fn migrate_v2_to_v3(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    // savings_records: one row per intervention. Indices match the two
    // dominant scan paths — by project+time (dashboard list) and by spec+time
    // (spec drill-down).
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS savings_records (\
             id INTEGER PRIMARY KEY AUTOINCREMENT,\
             ts INTEGER NOT NULL,\
             source TEXT NOT NULL,\
             tokens_saved INTEGER NOT NULL,\
             model_target TEXT,\
             project_path TEXT NOT NULL,\
             spec_id TEXT,\
             wave_id TEXT,\
             agent_id TEXT,\
             payload TEXT\
         );\
         CREATE INDEX IF NOT EXISTS idx_savings_records_project_ts \
             ON savings_records(project_path, ts);\
         CREATE INDEX IF NOT EXISTS idx_savings_records_spec_ts \
             ON savings_records(spec_id, ts);",
    )?;

    // context_cost_frames: one row per agent dispatch. Indices match the two
    // dominant scan paths — by project+time and by agent+time.
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS context_cost_frames (\
             id INTEGER PRIMARY KEY AUTOINCREMENT,\
             ts INTEGER NOT NULL,\
             agent_id TEXT NOT NULL,\
             wave_id TEXT,\
             spec_id TEXT,\
             project_path TEXT NOT NULL,\
             prompt_size_bytes INTEGER,\
             prefix_stable_bytes INTEGER,\
             slice_bytes INTEGER,\
             recipe_bytes INTEGER,\
             wave_slice_bytes INTEGER,\
             return_size_bytes INTEGER,\
             retry_overhead_bytes INTEGER\
         );\
         CREATE INDEX IF NOT EXISTS idx_context_cost_frames_project_ts \
             ON context_cost_frames(project_path, ts);\
         CREATE INDEX IF NOT EXISTS idx_context_cost_frames_agent_ts \
             ON context_cost_frames(agent_id, ts);",
    )?;

    // Add cache_* columns + project_path + ts_iso + cost_usd_micros to spans
    // only if absent. SQLite has no `ADD COLUMN IF NOT EXISTS`, so we probe
    // the schema via `pragma_table_info` first and ignore duplicate-column
    // errors as a belt-and-braces fallback.
    add_column_if_missing(&tx, "spans", "cache_read_input_tokens", "INTEGER")?;
    add_column_if_missing(&tx, "spans", "cache_creation_input_tokens", "INTEGER")?;
    add_column_if_missing(&tx, "spans", "cost_usd_micros", "INTEGER")?;
    add_column_if_missing(&tx, "spans", "project_path", "TEXT")?;
    add_column_if_missing(&tx, "spans", "ts_iso", "TEXT")?;
    add_column_if_missing(&tx, "spans", "session_id", "TEXT")?;
    add_column_if_missing(&tx, "spans", "wave_id", "TEXT")?;

    // Index spans by (project_path, ts_iso) for the economy reader, in
    // addition to the legacy (spec, phase, started_at) indices already in
    // sqlite_schema.sql.
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_spans_project_ts \
             ON spans(project_path, ts_iso);",
    )?;

    write_schema_version(&tx, 3)?;
    tx.commit()?;
    Ok(())
}

/// Add `column` to `table` with `decl` only if the column does not already
/// exist. Probes `pragma_table_info(table)` and issues `ALTER TABLE … ADD
/// COLUMN` exactly once.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> Result<()> {
    let exists: bool = {
        let mut stmt = conn.prepare(
            "SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2 LIMIT 1",
        )?;
        stmt.query_row(rusqlite::params![table, column], |row| row.get::<_, i64>(0))
            .optional()?
            .is_some()
    };
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl};"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Build an in-memory database with the production schema applied so each
    /// test starts from the same baseline as a real open.
    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sqlite_schema.sql")).unwrap();
        conn
    }

    /// Seed one event row directly — bypasses `SqliteEventStore::append` so we
    /// can write `spec = NULL` like the pre-fix call sites used to.
    fn seed_event(
        conn: &Connection,
        ts: &str,
        session_id: Option<&str>,
        event: &str,
        spec: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO events(ts, session_id, wave, spec, event, actor_kind, actor_id, payload) \
             VALUES(?1, ?2, 0, ?3, ?4, 'hook', 'test', '{}')",
            rusqlite::params![ts, session_id, spec, event],
        )
        .unwrap();
    }

    #[test]
    fn fresh_db_is_at_baseline_then_advances_to_latest() {
        let conn = fresh_db();
        let final_version = apply(&conn).unwrap();
        assert_eq!(final_version, LATEST_VERSION);

        // A second call is a no-op.
        let second = apply(&conn).unwrap();
        assert_eq!(second, LATEST_VERSION);
    }

    #[test]
    fn backfill_attributes_orphan_to_active_scope_in_same_session() {
        let conn = fresh_db();

        // Session s1 opens a spec at t=0, emits tool.use at t=1 with NULL spec.
        seed_event(&conn, "2026-05-20T10:00:00Z", Some("s1"), "pipeline.scope", Some("feature-A"));
        seed_event(&conn, "2026-05-20T10:01:00Z", Some("s1"), "tool.use", None);
        // Different session s2 emits unrelated tool.use — should not match s1's scope.
        seed_event(&conn, "2026-05-20T10:01:30Z", Some("s2"), "tool.use", None);

        apply(&conn).unwrap();

        let spec_for_s1_tool: String = conn
            .query_row(
                "SELECT spec FROM events WHERE session_id = 's1' AND event = 'tool.use'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(spec_for_s1_tool, "feature-A");

        let spec_for_s2_tool: String = conn
            .query_row(
                "SELECT spec FROM events WHERE session_id = 's2' AND event = 'tool.use'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(spec_for_s2_tool, ORPHAN_SPEC);
    }

    #[test]
    fn backfill_picks_most_recent_scope_when_multiple_present() {
        let conn = fresh_db();

        seed_event(&conn, "2026-05-20T10:00:00Z", Some("s1"), "pipeline.scope", Some("feature-A"));
        seed_event(&conn, "2026-05-20T11:00:00Z", Some("s1"), "pipeline.scope", Some("feature-B"));
        // Orphan at t=11:30 — should pick feature-B (more recent).
        seed_event(&conn, "2026-05-20T11:30:00Z", Some("s1"), "tool.use", None);
        // Orphan at t=10:30 — should pick feature-A (B not yet active).
        seed_event(&conn, "2026-05-20T10:30:00Z", Some("s1"), "tool.use", None);

        apply(&conn).unwrap();

        let mut stmt = conn
            .prepare("SELECT ts, spec FROM events WHERE event = 'tool.use' ORDER BY ts")
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(std::result::Result::unwrap)
            .collect();
        assert_eq!(rows[0], ("2026-05-20T10:30:00Z".into(), "feature-A".into()));
        assert_eq!(rows[1], ("2026-05-20T11:30:00Z".into(), "feature-B".into()));
    }

    #[test]
    fn orphan_without_session_falls_back_to_sentinel() {
        let conn = fresh_db();
        seed_event(&conn, "2026-05-20T10:00:00Z", None, "tool.use", None);
        apply(&conn).unwrap();

        let spec: String = conn
            .query_row("SELECT spec FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(spec, ORPHAN_SPEC);
    }

    #[test]
    fn already_attributed_rows_are_left_untouched() {
        let conn = fresh_db();
        seed_event(&conn, "2026-05-20T10:00:00Z", Some("s1"), "tool.use", Some("feature-Z"));
        apply(&conn).unwrap();

        let spec: String = conn
            .query_row("SELECT spec FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(spec, "feature-Z");
    }

    #[test]
    fn migration_persists_version_across_opens() {
        let conn = fresh_db();
        apply(&conn).unwrap();

        // Simulate a re-open: read version directly.
        let version = read_schema_version(&conn).unwrap();
        assert_eq!(version, LATEST_VERSION);
    }

    #[test]
    fn write_schema_version_overwrites_existing_row() {
        let conn = fresh_db();
        ensure_meta_table(&conn).unwrap();
        write_schema_version(&conn, 1).unwrap();
        write_schema_version(&conn, 2).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _mustard_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let version = read_schema_version(&conn).unwrap();
        assert_eq!(version, 2);
    }
}
