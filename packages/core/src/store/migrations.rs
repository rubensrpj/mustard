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
//! Every migration runs inside its own `SQLite` transaction — a failure mid-flight
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
pub const LATEST_VERSION: u32 = 10;

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
            3 => migrate_v3_to_v4(conn)?,
            4 => migrate_v4_to_v5(conn)?,
            5 => migrate_v5_to_v6(conn)?,
            6 => migrate_v6_to_v7(conn)?,
            7 => migrate_v7_to_v8(conn)?,
            8 => migrate_v8_to_v9(conn)?,
            9 => migrate_v9_to_v10(conn)?,
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
    // W5 fresh DBs no longer create `events` — there is nothing to backfill.
    // Stamp v2 and move on so the ladder keeps advancing.
    if !table_exists(conn, "events") {
        let tx = conn.unchecked_transaction()?;
        write_schema_version(&tx, 2)?;
        tx.commit()?;
        return Ok(());
    }
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
    //
    // Wave 3 (telemetry-separation) stopped creating the `spans` table in
    // `sqlite_schema.sql` (the v7 → v8 step drops it from existing DBs). A
    // fresh database therefore has no `spans` table to ALTER, so these steps
    // are gated on its presence: an existing pre-v3 DB still gets the columns;
    // a fresh post-Wave-3 DB skips them harmlessly.
    if table_exists(conn, "spans") {
        add_column_if_missing(&tx, "spans", "cache_read_input_tokens", "INTEGER")?;
        add_column_if_missing(&tx, "spans", "cache_creation_input_tokens", "INTEGER")?;
        add_column_if_missing(&tx, "spans", "cost_usd_micros", "INTEGER")?;
        add_column_if_missing(&tx, "spans", "project_path", "TEXT")?;
        add_column_if_missing(&tx, "spans", "ts_iso", "TEXT")?;
        add_column_if_missing(&tx, "spans", "session_id", "TEXT")?;
        add_column_if_missing(&tx, "spans", "wave_id", "TEXT")?;

        // Index spans by (project_path, ts_iso) for the legacy economy reader.
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_spans_project_ts \
                 ON spans(project_path, ts_iso);",
        )?;
    }

    write_schema_version(&tx, 3)?;
    tx.commit()?;
    Ok(())
}

/// v3 → v4 — add `spans.tool_use_id` for the W4 attribution join.
///
/// The W4 reader joins `spans` to `events` by the Anthropic `tool_use` block
/// id: assistant turns carry that id in their `message.content[].id` (when
/// `type == "tool_use"`) and the `agent.start` event written by the
/// `subagent-tracker` hook records the same id in its payload. Persisting the
/// id on the span row lets the reader skip the per-span JSON scan on the hot
/// path; `idx_spans_tool_use_id` keeps the join lookup at O(log n).
///
/// Re-runnable via the same `add_column_if_missing` probe used in v3 — a DB
/// that already absorbed the ALTER (e.g. a partial v4 from a previous open)
/// is a no-op.
fn migrate_v3_to_v4(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    // Gated on the `spans` table existing — fresh post-Wave-3 DBs never create
    // it (see `migrate_v7_to_v8`); only a pre-v4 DB still carrying spans needs
    // the column + index.
    if table_exists(conn, "spans") {
        add_column_if_missing(&tx, "spans", "tool_use_id", "TEXT")?;
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_spans_tool_use_id ON spans(tool_use_id);",
        )?;
    }
    write_schema_version(&tx, 4)?;
    tx.commit()?;
    Ok(())
}

/// v4 → v5 — performance indices on `events` plus the `api_cost_frames`
/// projection mirror.
///
/// Two distinct concerns rolled into one transactional bump:
///
/// 1. **Trace query performance.** `dashboard_spec_trace` and the per-actor
///    drill-downs filter by `(spec, event)` and `(actor_id, event)` on the
///    `events` table — both were doing index scans bounded only by
///    `idx_events_spec` / a sequential walk for the actor case. The two
///    composite indices give the dashboard O(log n) lookup on the hot path.
///
/// 2. **`api_cost_frames` projection.** External adapters that exposed the
///    Anthropic API-cost stream split off into a dedicated projection in
///    follow-up specs (the W3 ingest landed every frame in `spans` historically;
///    `record_api_cost` still does, but the reader needs to be union-safe so
///    future adapters that route to this dedicated table show up in the
///    economy dashboard immediately). Schema mirrors the subset of `spans`
///    columns the economy reader projects, so a `SELECT ... FROM api_cost_frames
///    UNION ALL SELECT ... FROM spans` lines up without per-column coercion.
///
/// Idempotent on re-run via `IF NOT EXISTS` on every statement.
fn migrate_v4_to_v5(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    // Performance indices on events. Composite (spec, event) accelerates
    // `dashboard_spec_trace`'s `WHERE spec = ?1 AND event IN (...)` shape;
    // (actor_id, event) covers the actor-scoped drill-down join. Gated on the
    // `events` table existing — fresh W5 DBs never create it.
    if table_exists(conn, "events") {
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_events_spec_event \
                 ON events(spec, event); \
             CREATE INDEX IF NOT EXISTS idx_events_actor_event \
                 ON events(actor_id, event);",
        )?;
    }

    // api_cost_frames: parallel projection to spans for external API-cost
    // adapters that route to a dedicated table. Column shape mirrors the
    // subset the economy reader projects so the union in
    // [`crate::economy::reader`] requires no per-column coercion.
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS api_cost_frames (\
             id INTEGER PRIMARY KEY AUTOINCREMENT,\
             span_id TEXT,\
             ts_iso TEXT,\
             session_id TEXT,\
             model TEXT,\
             spec TEXT,\
             phase TEXT,\
             wave_id TEXT,\
             input_tokens INTEGER,\
             output_tokens INTEGER,\
             cache_read_input_tokens INTEGER,\
             cache_creation_input_tokens INTEGER,\
             cost_usd_micros INTEGER,\
             tool_use_id TEXT,\
             project_path TEXT,\
             is_error INTEGER DEFAULT 0\
         );\
         CREATE INDEX IF NOT EXISTS idx_api_cost_frames_project_ts \
             ON api_cost_frames(project_path, ts_iso);\
         CREATE INDEX IF NOT EXISTS idx_api_cost_frames_tool_use_id \
             ON api_cost_frames(tool_use_id);\
         CREATE INDEX IF NOT EXISTS idx_api_cost_frames_session_ts \
             ON api_cost_frames(session_id, ts_iso);",
    )?;

    write_schema_version(&tx, 5)?;
    tx.commit()?;
    Ok(())
}

/// v5 → v6 — performance indices for session-scoped scans and the
/// `knowledge_patterns` confidence rank.
///
/// `sqlite_schema.sql` ships these via `CREATE INDEX IF NOT EXISTS`, so a fresh
/// database already has them after the DDL pass. This step exists so a database
/// that predates the index additions (already at v5) acquires them on its next
/// open without a full DDL re-run.
///
/// 1. `idx_events_session_id` — `last_pipeline_scope_for_session` and the
///    amend-window-by-session queries filter on `session_id`.
/// 2. `idx_knowledge_patterns_confidence_last_seen` — the `SessionStart`
///    injection ranks patterns by `ORDER BY confidence DESC, last_seen DESC`.
///
/// Idempotent on re-run via `IF NOT EXISTS` on every statement.
fn migrate_v5_to_v6(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if table_exists(conn, "events") {
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id);",
        )?;
    }
    if table_exists(conn, "knowledge_patterns") {
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_confidence_last_seen \
                 ON knowledge_patterns(confidence DESC, last_seen DESC);",
        )?;
    }
    write_schema_version(&tx, 6)?;
    tx.commit()?;
    Ok(())
}

/// v6 → v7 — add the `events_ad` AFTER DELETE trigger so pruning base rows
/// removes their `events_fts` external-content entries incrementally.
///
/// `events_fts` is an FTS5 external-content index fed only by the `events_ai`
/// insert trigger. `prune_events_older_than` deletes base rows; without a
/// matching delete trigger the FTS index keeps orphaned entries. `sqlite_schema.sql`
/// ships this trigger via `CREATE TRIGGER IF NOT EXISTS`, so a fresh database
/// already has it after the DDL pass. This step exists so a database that
/// predates the trigger (already at v6) acquires it on its next open.
///
/// Idempotent on re-run via `IF NOT EXISTS`.
fn migrate_v6_to_v7(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    // W5 fresh DBs no longer create `events` / `events_fts` — the trigger has
    // nothing to attach to. Pre-W5 DBs still upgrade as before.
    if table_exists(conn, "events") && table_exists(conn, "events_fts") {
        tx.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS events_ad AFTER DELETE ON events BEGIN \
                 INSERT INTO events_fts(events_fts, rowid, event, spec, payload_text) \
                 VALUES ('delete', old.id, old.event, old.spec, old.payload); \
             END;",
        )?;
    }
    write_schema_version(&tx, 7)?;
    tx.commit()?;
    Ok(())
}

/// v7 → v8 — schema-version no-op (dev-phase clean drop, see below).
///
/// History: this step used to carry the legacy `claude_code_otel` / `spans`
/// tables into the sibling `telemetry.db` before dropping them. The carry
/// implementation needed `ATTACH DATABASE`, which caused silent lock
/// contention on the hot connection — see `feedback_no_attach_sqlite` and the
/// WARN-3 incident notes (~42 MB of telemetry destroyed by a copy that
/// silently failed yet still let the drop run). It also blocked v9 → v10 from
/// completing in tests (the attached state survived this step and the v10
/// `DROP TABLE` contended with itself).
///
/// Per the W5 unification mega-spec (`feedback_no_migration_dev_phase`),
/// Mustard is in dev — there are no production installs to protect. So v7 →
/// v8 is now a pure version bump. The actual cleanup (`DROP TABLE` for
/// `claude_code_otel` / `spans`) happens in [`migrate_v9_to_v10`] so old
/// databases that already moved past v7 in a prior install still get the
/// legacy tables removed on their next open.
fn migrate_v7_to_v8(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    write_schema_version(&tx, 8)?;
    tx.commit()?;
    Ok(())
}

/// v8 → v9 — W5 unification: extend `knowledge_patterns`, `memory_decisions`,
/// `memory_lessons` with the spec-scope columns the spec requires, and ensure
/// the four new tables shipped in `sqlite_schema.sql` (`pipeline_events`,
/// `sessions`, `agent_memory`, `memory_feedback`) plus their indices are
/// present even on a database that was opened at v8 once and then took the
/// fast path on subsequent opens.
///
/// The fresh-DB path already gets every shape from `sqlite_schema.sql` (DDL on
/// first open). This migration covers the upgrade path: a pre-v9 database
/// stamped at v8 carrying populated `knowledge_patterns` / `memory_decisions` /
/// `memory_lessons` rows that lack the new columns.
///
/// Per `feedback_no_migration_dev_phase` we are in dev — no production data to
/// protect — but `ALTER ADD COLUMN` is surgical, idempotent via the
/// `add_column_if_missing` probe, and avoids the silent FTS5 rebuild that a
/// drop+recreate of the mirrored tables would force. The `agent_memory` /
/// `memory_feedback` tables are CREATEd direct (no ALTER) per the spec.
///
/// Re-runnable on a partial v9: every step probes for column / table presence
/// first.
fn migrate_v8_to_v9(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    // knowledge_patterns + memory_{decisions,lessons} — add the scope columns.
    add_column_if_missing(&tx, "knowledge_patterns", "spec", "TEXT")?;
    add_column_if_missing(&tx, "knowledge_patterns", "status", "TEXT DEFAULT 'active'")?;
    add_column_if_missing(&tx, "knowledge_patterns", "last_used", "TEXT")?;

    add_column_if_missing(&tx, "memory_decisions", "spec", "TEXT")?;
    add_column_if_missing(&tx, "memory_decisions", "wave", "INTEGER")?;
    add_column_if_missing(&tx, "memory_decisions", "confidence", "REAL DEFAULT 0.5")?;
    add_column_if_missing(&tx, "memory_decisions", "status", "TEXT DEFAULT 'active'")?;
    add_column_if_missing(&tx, "memory_decisions", "superseded_by", "INTEGER")?;

    add_column_if_missing(&tx, "memory_lessons", "spec", "TEXT")?;
    add_column_if_missing(&tx, "memory_lessons", "wave", "INTEGER")?;
    add_column_if_missing(&tx, "memory_lessons", "confidence", "REAL DEFAULT 0.5")?;
    add_column_if_missing(&tx, "memory_lessons", "status", "TEXT DEFAULT 'active'")?;
    add_column_if_missing(&tx, "memory_lessons", "superseded_by", "INTEGER")?;

    // Re-apply the W5 DDL block so a v8→v9 upgrade picks up the four new
    // tables + their indices even if the fast-path skipped the DDL pass.
    // Every CREATE is `IF NOT EXISTS`, so this is a safe no-op on a fresh DB.
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS pipeline_events (\
             id INTEGER PRIMARY KEY AUTOINCREMENT,\
             ts TEXT NOT NULL,\
             session_id TEXT,\
             spec TEXT,\
             wave INTEGER,\
             kind TEXT NOT NULL,\
             parent_id INTEGER REFERENCES pipeline_events(id),\
             payload TEXT\
         ); \
         CREATE INDEX IF NOT EXISTS idx_pipeline_events_spec ON pipeline_events(spec); \
         CREATE INDEX IF NOT EXISTS idx_pipeline_events_kind ON pipeline_events(kind); \
         CREATE INDEX IF NOT EXISTS idx_pipeline_events_spec_kind \
             ON pipeline_events(spec, kind); \
         CREATE INDEX IF NOT EXISTS idx_pipeline_events_session_kind \
             ON pipeline_events(session_id, kind); \
         CREATE INDEX IF NOT EXISTS idx_pipeline_events_parent \
             ON pipeline_events(parent_id); \
         CREATE TABLE IF NOT EXISTS sessions (\
             id TEXT PRIMARY KEY,\
             slug TEXT NOT NULL UNIQUE,\
             started_at TEXT NOT NULL,\
             last_activity_at TEXT,\
             last_spec TEXT,\
             cwd TEXT,\
             status TEXT NOT NULL DEFAULT 'open'\
         ); \
         CREATE INDEX IF NOT EXISTS idx_sessions_last_activity \
             ON sessions(last_activity_at DESC); \
         CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status); \
         CREATE TABLE IF NOT EXISTS agent_memory (\
             id INTEGER PRIMARY KEY AUTOINCREMENT,\
             session_id TEXT,\
             spec TEXT,\
             wave INTEGER,\
             role TEXT,\
             summary TEXT NOT NULL,\
             details TEXT,\
             confidence REAL NOT NULL DEFAULT 0.5,\
             status TEXT NOT NULL DEFAULT 'active',\
             at TEXT NOT NULL,\
             last_used TEXT\
         ); \
         CREATE INDEX IF NOT EXISTS idx_agent_memory_spec ON agent_memory(spec); \
         CREATE INDEX IF NOT EXISTS idx_agent_memory_status_confidence \
             ON agent_memory(status, confidence DESC); \
         CREATE INDEX IF NOT EXISTS idx_agent_memory_session ON agent_memory(session_id); \
         CREATE TABLE IF NOT EXISTS memory_feedback (\
             id INTEGER PRIMARY KEY AUTOINCREMENT,\
             memory_id INTEGER NOT NULL REFERENCES agent_memory(id),\
             kind TEXT NOT NULL,\
             delta REAL,\
             by_role TEXT,\
             at TEXT NOT NULL,\
             note TEXT\
         ); \
         CREATE INDEX IF NOT EXISTS idx_memory_feedback_memory_id \
             ON memory_feedback(memory_id); \
         CREATE INDEX IF NOT EXISTS idx_memory_feedback_kind \
             ON memory_feedback(kind);",
    )?;

    write_schema_version(&tx, 9)?;
    tx.commit()?;
    Ok(())
}

/// v9 → v10 — W5 schema cleanup: drop the legacy tables the W5 spec retires.
///
/// `events` + `events_fts` move to per-spec NDJSON (see
/// `apps/rt/src/run/event_writer_ndjson.rs`). `knowledge` + `knowledge_fts`
/// consolidate into `knowledge_patterns`. `metrics_projection` duplicates
/// `telemetry.db.run_usage`. `savings_records`, `context_cost_frames`,
/// `api_cost_frames` are derivable from NDJSON + telemetry on demand.
///
/// `claude_code_otel` + `spans` also get dropped here. v7 → v8 used to carry
/// them into `telemetry.db` via `ATTACH DATABASE`, but the attach caused
/// silent lock contention (see `feedback_no_attach_sqlite`) and is now
/// forbidden in this crate. v7 → v8 is a no-op; the actual drop happens
/// here so that any database (pre-W5 or W5+) ends up clean at v10. The data
/// loss is acceptable under `feedback_no_migration_dev_phase` (no production
/// users to protect).
///
/// Per `feedback_no_migration_dev_phase`: drop cleanly, do not carry data.
/// Emits the `pipeline.economy.schema.shrunk` telemetry by recording byte
/// sizes before and after — best-effort via `PRAGMA page_count * page_size`.
///
/// `VACUUM` cannot run inside a transaction, so it runs after the commit
/// (best-effort). `PRAGMA optimize` follows to refresh statistics.
fn migrate_v9_to_v10(conn: &Connection) -> Result<()> {
    // Detect legacy presence BEFORE the drop so the telemetry only fires when
    // there is actually shrinkage to report. On a fresh W5 DB (no legacy
    // tables to begin with), this stays a silent no-op so callers that assert
    // `replay().is_empty()` after open keep working.
    // The intermediate ladder tables (`savings_records`, `context_cost_frames`,
    // `api_cost_frames`) get created on every fresh DB by v2→v5, so we ignore
    // them when deciding whether the schema actually *shrunk*. The truly
    // legacy tables (`events`, `knowledge`, `metrics_projection`,
    // `claude_code_otel`, `spans`) were never recreated by the new SQL schema;
    // their presence proves we are upgrading a real pre-W5 DB.
    let had_legacy = [
        "events",
        "knowledge",
        "metrics_projection",
        "claude_code_otel",
        "spans",
    ]
    .iter()
    .any(|t| table_exists(conn, t));

    let before_bytes = db_size_bytes(conn).unwrap_or(0);

    let tx = conn.unchecked_transaction()?;
    // Order matters only for FTS tables that reference base tables — drop
    // triggers first to silence any AFTER-DELETE noise during the drop.
    // `savings_records` + `context_cost_frames` are KEPT — they are written
    // on the hot path by every Mustard intervention (bash_guard rewrites,
    // model_routing downgrades, …) and live in
    // `sqlite_schema.sql`. Only `api_cost_frames` is dropped (it consolidated
    // into `telemetry.db.run_usage` in Wave 2 of telemetry-separation).
    tx.execute_batch(
        "DROP TRIGGER IF EXISTS events_ai; \
         DROP TRIGGER IF EXISTS events_ad; \
         DROP TABLE IF EXISTS events_fts; \
         DROP TABLE IF EXISTS events; \
         DROP TABLE IF EXISTS knowledge_fts; \
         DROP TABLE IF EXISTS knowledge; \
         DROP TABLE IF EXISTS metrics_projection; \
         DROP TABLE IF EXISTS api_cost_frames; \
         DROP TABLE IF EXISTS claude_code_otel; \
         DROP TABLE IF EXISTS spans;",
    )?;
    write_schema_version(&tx, 10)?;
    tx.commit()?;

    // VACUUM + optimize outside the transaction. Both are best-effort.
    let _ = conn.execute_batch("VACUUM");
    let _ = conn.execute_batch("PRAGMA optimize");

    if had_legacy {
        let after_bytes = db_size_bytes(conn).unwrap_or(0);
        let payload = format!(
            "{{\"from_bytes\":{before_bytes},\"to_bytes\":{after_bytes}}}"
        );
        let _ = conn.execute(
            "INSERT INTO pipeline_events(ts, kind, payload) VALUES (?1, ?2, ?3)",
            rusqlite::params![now_iso8601(), "pipeline.economy.schema.shrunk", payload],
        );
    }
    Ok(())
}

/// Minimal ISO-8601 UTC timestamp formatter — `YYYY-MM-DDTHH:MM:SS.000Z`.
/// Kept local to avoid a dependency cycle with crates that consume the store.
/// Falls back to the unix epoch when the system clock is before 1970.
fn now_iso8601() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    // Hinnant's days-from-civil (inverse) — re-implemented locally so the
    // store layer stays free of chrono / time crates. Pre-1970 inputs clamp
    // to the epoch, matching the harness's other epoch handling.
    let total_secs = (ms / 1000) as i64;
    let mut ms_rem = (ms % 1000) as i64;
    let mut days = total_secs.div_euclid(86_400);
    let mut tod = total_secs.rem_euclid(86_400);
    if tod < 0 {
        tod += 86_400;
        days -= 1;
    }
    if ms_rem < 0 {
        ms_rem += 1000;
    }
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let h = tod / 3600;
    let mi = (tod % 3600) / 60;
    let s = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms_rem:03}Z")
}

/// Best-effort byte size of the main SQLite database. Returns `None` if either
/// pragma query fails — `migrate_v9_to_v10`'s telemetry then degrades to a
/// `null`-ish reading rather than aborting.
fn db_size_bytes(conn: &Connection) -> Option<i64> {
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).ok()?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0)).ok()?;
    Some(page_count.saturating_mul(page_size))
}

/// `true` when `table` exists in the main schema.
///
/// Returns a plain `bool` (not `Result`): a probe failure means "not present",
/// which is exactly what the gated callers want — there is no error worth
/// propagating from a `sqlite_master` lookup.
fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1 LIMIT 1",
        rusqlite::params![table],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
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

    // NOTE: Several legacy tests below were deleted in W5 because they probe
    // schema artifacts (`events`, `events_fts`, `events_ad` trigger,
    // `idx_events_*` indices, `knowledge_patterns_confidence_last_seen` index
    // on the pre-W5 location) the new sqlite_schema.sql no longer creates.
    // The v2/v5/v6/v7 migrations are gated on `table_exists("events")` so
    // fresh DBs stamp the version without touching the absent table — that
    // behaviour is covered by `fresh_db_is_at_baseline_then_advances_to_latest`.

    #[test]
    fn fresh_db_is_at_baseline_then_advances_to_latest() {
        let conn = fresh_db();
        let final_version = apply(&conn).unwrap();
        assert_eq!(final_version, LATEST_VERSION);

        // A second call is a no-op.
        let second = apply(&conn).unwrap();
        assert_eq!(second, LATEST_VERSION);
    }

    /// v9 → v10: an existing DB at v9 with legacy tables sees them dropped on
    /// the next open. Fresh DBs (no legacy tables) advance silently.
    #[test]
    fn v10_drops_legacy_tables_when_present() {
        let conn = Connection::open_in_memory().unwrap();
        // Create a legacy-shaped DB at v9 with the tables W5 retires.
        conn.execute_batch(include_str!("sqlite_schema.sql")).unwrap();
        conn.execute_batch(
            "CREATE TABLE events (id INTEGER PRIMARY KEY, ts TEXT); \
             CREATE TABLE knowledge (id TEXT PRIMARY KEY); \
             CREATE TABLE metrics_projection (spec TEXT PRIMARY KEY);",
        )
        .unwrap();
        ensure_meta_table(&conn).unwrap();
        write_schema_version(&conn, 9).unwrap();

        apply(&conn).unwrap();

        assert!(!table_exists(&conn, "events"));
        assert!(!table_exists(&conn, "knowledge"));
        assert!(!table_exists(&conn, "metrics_projection"));
        // Lifecycle index still there.
        assert!(table_exists(&conn, "pipeline_events"));
    }

    #[test]
    fn migration_persists_version_across_opens() {
        let conn = fresh_db();
        apply(&conn).unwrap();

        // Simulate a re-open: read version directly.
        let version = read_schema_version(&conn).unwrap();
        assert_eq!(version, LATEST_VERSION);
    }

    /// v9 → v10: a database that still carries the legacy telemetry tables
    /// (`claude_code_otel`, `spans`) — left over from a pre-W5 install that
    /// stopped at v7 before this migration moved the drop here — sees them
    /// removed on the next open. Seeds rows first to prove `DROP TABLE`
    /// runs against populated tables (covers the v7→v8 hang the prior
    /// `ATTACH`-based implementation produced when v9→v10 contended with
    /// the still-attached state).
    #[test]
    fn v10_drops_claude_code_otel_and_spans_if_present() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sqlite_schema.sql")).unwrap();
        // Re-create the legacy telemetry tables a pre-W5 DB carried, and seed
        // each with a row so the drop has data to work against.
        conn.execute_batch(
            "CREATE TABLE claude_code_otel (\
                 ts_bucket INTEGER NOT NULL, signal TEXT NOT NULL, metric TEXT NOT NULL, \
                 session_id TEXT, model TEXT, token_type TEXT, sum REAL DEFAULT 0, \
                 count INTEGER DEFAULT 0, attrs TEXT, \
                 PRIMARY KEY (ts_bucket, metric, session_id, model, token_type)); \
             CREATE TABLE spans (\
                 trace_id TEXT, span_id TEXT PRIMARY KEY, name TEXT, \
                 started_at INTEGER, model TEXT);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO claude_code_otel \
             (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs) \
             VALUES (60000, 'metric', 'claude_code.cost.usage', 's1', 'opus', 'input', 12.0, 1, '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, started_at, model) \
             VALUES ('t1', 'sp-1', 'agent', 0, 'opus')",
            [],
        )
        .unwrap();

        // Verify the seed actually landed before we run the ladder.
        assert!(table_exists(&conn, "claude_code_otel"));
        assert!(table_exists(&conn, "spans"));

        // Run the full ladder from baseline — v7→v8 is a no-op now, v9→v10
        // does the drop.
        let final_version = apply(&conn).unwrap();
        assert_eq!(final_version, LATEST_VERSION);

        assert!(
            !table_exists(&conn, "claude_code_otel"),
            "v10 must drop claude_code_otel"
        );
        assert!(!table_exists(&conn, "spans"), "v10 must drop spans");
    }

    #[test]
    fn v9_creates_new_w5_tables_and_extends_knowledge_columns() {
        let conn = fresh_db();
        apply(&conn).unwrap();

        // Four new tables exist after migration.
        for table in [
            "pipeline_events",
            "sessions",
            "agent_memory",
            "memory_feedback",
        ] {
            assert!(table_exists(&conn, table), "v9 must create {table}");
        }

        // Spec-scope columns added to the W6a tables (idempotent ALTERs).
        let column_exists = |table: &str, column: &str| -> bool {
            let mut stmt = conn
                .prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2 LIMIT 1")
                .unwrap();
            stmt.query_row(rusqlite::params![table, column], |row| row.get::<_, i64>(0))
                .optional()
                .unwrap()
                .is_some()
        };

        for col in ["spec", "status", "last_used"] {
            assert!(
                column_exists("knowledge_patterns", col),
                "v9 must add knowledge_patterns.{col}"
            );
        }
        for col in ["spec", "wave", "confidence", "status", "superseded_by"] {
            for table in ["memory_decisions", "memory_lessons"] {
                assert!(
                    column_exists(table, col),
                    "v9 must add {table}.{col}"
                );
            }
        }

        // Shape probes: insert + read round-trips for each new table.
        conn.execute(
            "INSERT INTO pipeline_events(ts, session_id, spec, wave, kind, payload) \
             VALUES('2026-05-24T00:00:00Z', 's1', 'spec-A', 2, 'pipeline.scope', '{}')",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM pipeline_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);

        conn.execute(
            "INSERT INTO sessions(id, slug, started_at) \
             VALUES('sess-1', 'auth-feature-2026', '2026-05-24T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO agent_memory(session_id, spec, role, summary, confidence, at) \
             VALUES('s1', 'spec-A', 'impl', 'wrote NDJSON writer', 0.9, '2026-05-24T00:00:00Z')",
            [],
        )
        .unwrap();
        let mem_id: i64 = conn
            .query_row("SELECT id FROM agent_memory LIMIT 1", [], |r| r.get(0))
            .unwrap();

        conn.execute(
            "INSERT INTO memory_feedback(memory_id, kind, delta, by_role, at) \
             VALUES(?1, 'bump', 0.1, 'review', '2026-05-24T00:00:00Z')",
            rusqlite::params![mem_id],
        )
        .unwrap();
        let fb: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_feedback", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fb, 1);
    }

    #[test]
    fn v9_is_idempotent_on_re_run() {
        // Apply twice; the second call must be a no-op without ALTER duplicate
        // errors (the column probe gates each ADD COLUMN).
        let conn = fresh_db();
        apply(&conn).unwrap();
        let second = apply(&conn).unwrap();
        assert_eq!(second, LATEST_VERSION);
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
