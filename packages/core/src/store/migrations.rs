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
pub const LATEST_VERSION: u32 = 8;

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
    // (actor_id, event) covers the actor-scoped drill-down join.
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_events_spec_event \
             ON events(spec, event); \
         CREATE INDEX IF NOT EXISTS idx_events_actor_event \
             ON events(actor_id, event);",
    )?;

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
    tx.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id); \
         CREATE INDEX IF NOT EXISTS idx_knowledge_patterns_confidence_last_seen \
             ON knowledge_patterns(confidence DESC, last_seen DESC);",
    )?;
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
    tx.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS events_ad AFTER DELETE ON events BEGIN \
             INSERT INTO events_fts(events_fts, rowid, event, spec, payload_text) \
             VALUES ('delete', old.id, old.event, old.spec, old.payload); \
         END;",
    )?;
    write_schema_version(&tx, 7)?;
    tx.commit()?;
    Ok(())
}

/// v7 → v8 — carry the legacy telemetry into `telemetry.db`, then drop the
/// retired tables `claude_code_otel` and `spans`.
///
/// Telemetry moved to a dedicated `telemetry.db` (Wave 3 of
/// `2026-05-22-telemetry-separation`): `usage_totals` replaces
/// `claude_code_otel`, `run_usage` replaces `spans`, and every reader now goes
/// through `crate::telemetry::reader`. The two legacy tables in `mustard.db`
/// stopped receiving writes in Wave 2 and stopped being read in Wave 3, so they
/// are now dead weight — historically ~42 MB on a long-lived install.
///
/// # Copy on the connection that already owns `mustard.db`
///
/// This migration runs on `conn` **inside** `SqliteEventStore::new`, while
/// `conn` already holds `mustard.db` open. The original implementation copied by
/// opening a *second* connection (a `TelemetryStore`) that `ATTACH`ed
/// `mustard.db` by path — that second reader contended with `conn`, and the
/// source-table probe in `telemetry::migrate` swallowed the resulting busy/lock
/// error as "table absent", so the copy silently skipped both tables yet the
/// drop still ran → ~42 MB of telemetry destroyed without being copied. The fix
/// inverts the attach direction: `conn` (which owns `mustard.db`) `ATTACH`es the
/// sibling `telemetry.db` (which nothing else holds), so the source reads come
/// off the connection that is guaranteed to see the data.
///
/// 1. Resolve the sibling `telemetry.db` path (env `MUSTARD_TELEMETRY_DB_PATH`
///    else `{mustard_dir}/telemetry.db`) and open a `TelemetryStore` once to
///    create/migrate its schema, then drop that handle.
/// 2. On `conn`, `ATTACH DATABASE '<telemetry.db>' AS tel`.
/// 3. Copy `main.claude_code_otel` → `tel.usage_totals` and `main.spans` →
///    `tel.run_usage` (with the attribution backfill). `main` is owned by `conn`
///    so its rows are read correctly.
/// 4. `DETACH tel`.
///
/// # Never drop unverified (WARN-3 hardening)
///
/// Before dropping each legacy table, the guard uses **error-propagating**
/// queries (never `.ok()`-swallowed) to learn whether the source table exists
/// and how many rows it has, and to count what the destination now holds. A
/// source table is dropped **only** when it is genuinely absent OR its rows are
/// verifiably present in the destination. If a source table exists with N>0 rows
/// but the destination is still empty (a silently-failed copy), the migration
/// returns `Ok(())` **without** dropping and **without** advancing to v8:
/// fail-open leaves the data and the next open retries. Any error during the
/// copy/attach short-circuits to the same leave-the-data outcome.
///
/// `VACUUM` cannot run inside a transaction, so it runs **after** the commit,
/// best-effort: a failure leaves unreclaimed pages (a space-only concern).
///
/// Idempotent: the copy is `INSERT OR IGNORE` per table, `DROP TABLE IF EXISTS`
/// is a no-op once the tables are gone, and the version gate prevents a re-run.
fn migrate_v7_to_v8(conn: &Connection) -> Result<()> {
    // Resolve the sibling telemetry.db off `conn`'s own `main` path. An
    // in-memory / temp database reports an empty file — there is no sibling to
    // copy into and (being in-memory) no persisted history to lose, so just drop
    // and advance. This matches the prior fresh-DB behaviour.
    let Some(telemetry_path) = sibling_telemetry_path(conn)? else {
        return drop_legacy_and_stamp(conn);
    };

    // Ensure the telemetry schema exists in that file, then release the handle
    // so nothing else holds telemetry.db while `conn` attaches it. Any failure
    // is fail-open: leave the legacy tables and the version at v7.
    if crate::telemetry::store::TelemetryStore::new(&telemetry_path).is_err() {
        return Ok(());
    }
    let telemetry_str = telemetry_path.to_string_lossy().into_owned();

    // Attach telemetry.db on the connection that already owns mustard.db, run
    // the copy + verified drop, and ALWAYS detach. The copy reads source rows
    // off `main` (owned by `conn`) so a competing reader can never mask them.
    if conn
        .execute("ATTACH DATABASE ?1 AS tel", rusqlite::params![telemetry_str])
        .is_err()
    {
        return Ok(());
    }
    let result = copy_then_verified_drop(conn);
    let _ = conn.execute_batch("DETACH DATABASE tel");

    // A copy/guard failure is fail-open: do NOT advance the version, leave the
    // legacy tables, retry next open. Only VACUUM after a successful, committed
    // drop (signalled by `Ok(true)`).
    match result {
        Ok(true) => {
            let _ = conn.execute_batch("VACUUM");
            Ok(())
        }
        Ok(false) | Err(_) => Ok(()),
    }
}

/// Resolve the sibling `telemetry.db` for the `mustard.db` open on `conn`.
///
/// Mirrors `telemetry::store`/`telemetry::migrate`: `MUSTARD_TELEMETRY_DB_PATH`
/// wins when set and non-blank, otherwise `{mustard_dir}/telemetry.db`. Returns
/// `Ok(None)` when `conn`'s `main` database is in-memory / temp (empty file
/// path) — there is no sibling and no persisted history to carry.
fn sibling_telemetry_path(conn: &Connection) -> Result<Option<std::path::PathBuf>> {
    let mustard_path: String = conn
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |r| r.get::<_, Option<String>>(0),
        )?
        .unwrap_or_default();
    if mustard_path.trim().is_empty() {
        return Ok(None);
    }
    let path = match std::env::var("MUSTARD_TELEMETRY_DB_PATH") {
        Ok(v) if !v.trim().is_empty() => std::path::PathBuf::from(v),
        _ => std::path::Path::new(&mustard_path).parent().map_or_else(
            || std::path::PathBuf::from("telemetry.db"),
            |dir| dir.join("telemetry.db"),
        ),
    };
    Ok(Some(path))
}

/// Copy the two legacy tables from `main` (owned by `conn`) into the attached
/// `tel` schema, then drop **only** the tables whose data is verifiably present
/// in the destination (or genuinely absent at the source).
///
/// Returns `Ok(true)` when the drop ran and the version was stamped to 8,
/// `Ok(false)` when a source table held rows the destination never received (a
/// failed copy) — in that case nothing is dropped and the version stays at v7 so
/// the next open retries. Propagates any SQLite error (the caller treats it as
/// fail-open: leave the data).
fn copy_then_verified_drop(conn: &Connection) -> Result<bool> {
    // Source row counts BEFORE the copy. `count_main_table` propagates errors
    // (never swallows a lock as "absent") and returns `None` only when the table
    // is genuinely missing from `main`.
    let otel_before = count_main_table(conn, "claude_code_otel")?;
    let spans_before = count_main_table(conn, "spans")?;

    // Destination counts BEFORE the copy, so we can tell "copy added the rows"
    // from "rows were already there".
    let totals_dest_before = count_tel_table(conn, "usage_totals")?;
    let runs_dest_before = count_tel_table(conn, "run_usage")?;

    // Copy each side only when its source exists. `main.` sources are read off
    // the owning connection; `tel.` is the freshly-attached sibling.
    if otel_before.is_some() {
        conn.execute_batch(
            "INSERT OR IGNORE INTO tel.usage_totals (metric, model, session_id, sum, updated_at) \
             SELECT metric, model, session_id, SUM(sum), MAX(ts_bucket) \
             FROM main.claude_code_otel \
             GROUP BY metric, model, session_id;",
        )?;
    }
    if spans_before.is_some() {
        conn.execute_batch(&copy_spans_into_tel_sql())?;
    }

    // Destination counts AFTER the copy.
    let totals_dest_after = count_tel_table(conn, "usage_totals")?;
    let runs_dest_after = count_tel_table(conn, "run_usage")?;

    // Per-table drop decision. A table is safe to drop when it is genuinely
    // absent at the source, OR it had no rows, OR the destination now carries
    // rows (it received the data — INSERT OR IGNORE may legitimately add 0 if
    // the dest already held them, which is why we accept "dest non-empty" rather
    // than "dest grew by exactly N").
    let otel_safe = drop_is_safe(otel_before, totals_dest_before, totals_dest_after);
    let spans_safe = drop_is_safe(spans_before, runs_dest_before, runs_dest_after);

    // If either side has un-copied rows, abort the drop entirely: leave BOTH
    // legacy tables and the version at v7 so the next open retries the whole
    // step. Dropping only the verified half would advance nothing anyway (we do
    // not stamp v8 unless both are safe), so keep it simple and atomic.
    if !otel_safe || !spans_safe {
        return Ok(false);
    }

    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "DROP TABLE IF EXISTS claude_code_otel; \
         DROP TABLE IF EXISTS spans;",
    )?;
    write_schema_version(&tx, 8)?;
    tx.commit()?;
    Ok(true)
}

/// Decide whether a legacy source table is safe to drop.
///
/// `source` is the source row count (`None` = table absent), `dest_before` /
/// `dest_after` bracket the copy. Safe when the source is absent, the source was
/// empty, or the destination now holds rows. Unsafe only when the source had
/// rows (`Some(n > 0)`) yet the destination is still empty after the copy — a
/// silent skip we must never let trigger a drop.
fn drop_is_safe(source: Option<i64>, _dest_before: i64, dest_after: i64) -> bool {
    match source {
        None => true,            // genuinely absent
        Some(n) if n <= 0 => true, // nothing to lose
        Some(_) => dest_after > 0, // copied iff dest received rows
    }
}

/// Row count of `main.<table>`, or `None` when the table does not exist.
///
/// Unlike a `.ok()`-swallowing probe, this **propagates** any error other than
/// "no such table": a lock/busy failure surfaces as `Err` (the caller leaves the
/// data) instead of masquerading as "absent" (which would green-light a drop).
fn count_main_table(conn: &Connection, table: &str) -> Result<Option<i64>> {
    if !table_exists(conn, table) {
        return Ok(None);
    }
    // `table` is a crate-internal literal, not user input.
    let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM main.{table}"), [], |r| r.get(0))?;
    Ok(Some(n))
}

/// Row count of an attached `tel.<table>`. Errors propagate.
fn count_tel_table(conn: &Connection, table: &str) -> Result<i64> {
    let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM tel.{table}"), [], |r| r.get(0))?;
    Ok(n)
}

/// Drop the legacy tables and stamp v8 with no copy — used only when there is no
/// sibling telemetry.db to copy into (in-memory / temp `main`), where there is
/// no persisted history to lose.
fn drop_legacy_and_stamp(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "DROP TABLE IF EXISTS claude_code_otel; \
         DROP TABLE IF EXISTS spans;",
    )?;
    write_schema_version(&tx, 8)?;
    tx.commit()?;
    let _ = conn.execute_batch("VACUUM");
    Ok(())
}

/// The `INSERT OR IGNORE INTO tel.run_usage SELECT ... FROM main.spans`
/// statement, with the same spec / wave_id / agent_id attribution backfill as
/// [`crate::telemetry::migrate`], but reading the source off `main` (the
/// `mustard.db` owned by `conn`) and writing into the attached `tel.run_usage`.
///
/// Source = `main` is the load-bearing change vs. the collector-startup path:
/// `conn` already holds `mustard.db`, so its spans / events rows read correctly
/// even while the migration's own write transaction is in flight. Attribution
/// semantics are identical to the collector path:
///
/// - `agent_id` = payload.agent_id ?? payload.subagentType ?? events.actor_id
/// - `spec`     = payload.spec_id  ?? events.spec ?? span.spec
/// - `wave_id`  = payload.wave_id  ?? CAST(events.wave AS TEXT) ?? span.wave_id
fn copy_spans_into_tel_sql() -> String {
    let agent_primary = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.agent_id'), \
                                          JSON_EXTRACT(ev.payload,'$.subagentType'), ev.actor_id) \
                          FROM main.events ev \
                          WHERE ev.event = 'agent.start' AND s.tool_use_id IS NOT NULL \
                            AND JSON_EXTRACT(ev.payload,'$.tool_use_id') = s.tool_use_id LIMIT 1)";
    let agent_fallback = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.agent_id'), \
                                           JSON_EXTRACT(ev.payload,'$.subagentType'), ev.actor_id) \
                           FROM main.events ev \
                           WHERE ev.event = 'agent.start' AND ev.session_id IS NOT NULL \
                             AND s.session_id IS NOT NULL AND ev.session_id = s.session_id \
                             AND ev.ts <= s.ts_iso ORDER BY ev.ts DESC LIMIT 1)";
    let spec_primary = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.spec_id'), ev.spec) \
                         FROM main.events ev \
                         WHERE ev.event = 'agent.start' AND s.tool_use_id IS NOT NULL \
                           AND JSON_EXTRACT(ev.payload,'$.tool_use_id') = s.tool_use_id LIMIT 1)";
    let spec_fallback = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.spec_id'), ev.spec) \
                          FROM main.events ev \
                          WHERE ev.event = 'agent.start' AND ev.session_id IS NOT NULL \
                            AND s.session_id IS NOT NULL AND ev.session_id = s.session_id \
                            AND ev.ts <= s.ts_iso ORDER BY ev.ts DESC LIMIT 1)";
    let wave_primary = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.wave_id'), CAST(ev.wave AS TEXT)) \
                         FROM main.events ev \
                         WHERE ev.event = 'agent.start' AND s.tool_use_id IS NOT NULL \
                           AND JSON_EXTRACT(ev.payload,'$.tool_use_id') = s.tool_use_id LIMIT 1)";
    let wave_fallback = "(SELECT COALESCE(JSON_EXTRACT(ev.payload,'$.wave_id'), CAST(ev.wave AS TEXT)) \
                          FROM main.events ev \
                          WHERE ev.event = 'agent.start' AND ev.session_id IS NOT NULL \
                            AND s.session_id IS NOT NULL AND ev.session_id = s.session_id \
                            AND ev.ts <= s.ts_iso ORDER BY ev.ts DESC LIMIT 1)";

    format!(
        "INSERT OR IGNORE INTO tel.run_usage \
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
         FROM main.spans s;"
    )
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
    fn v5_creates_event_composite_indices_and_api_cost_frames_table() {
        let conn = fresh_db();
        apply(&conn).unwrap();

        // Composite indices on events visible in sqlite_master.
        let has_spec_event: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_events_spec_event'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(has_spec_event, "v5 must create idx_events_spec_event");

        let has_actor_event: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_events_actor_event'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(has_actor_event, "v5 must create idx_events_actor_event");

        // api_cost_frames table accepts the shape the economy reader projects.
        conn.execute(
            "INSERT INTO api_cost_frames(span_id, ts_iso, session_id, model, spec, \
                                         input_tokens, output_tokens, cost_usd_micros, \
                                         tool_use_id, project_path) \
             VALUES('req-x', '2026-05-21T00:00:00Z', 's1', 'opus', 'spec-A', \
                    100, 50, 1000, NULL, '/tmp/p')",
            [],
        )
        .unwrap();
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM api_cost_frames", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 1);
    }

    #[test]
    fn v6_creates_session_and_knowledge_rank_indices() {
        let conn = fresh_db();
        apply(&conn).unwrap();
        for idx in [
            "idx_events_session_id",
            "idx_knowledge_patterns_confidence_last_seen",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type='index' AND name=?1",
                    rusqlite::params![idx],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
                .unwrap()
                .is_some();
            assert!(exists, "v6 must create {idx}");
        }
    }

    #[test]
    fn v8_drops_legacy_telemetry_tables() {
        // Seed a database that still has the legacy tables (as a pre-Wave-3 DB
        // would), at version 7, then advance: v8 must drop both.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sqlite_schema.sql")).unwrap();
        // Re-create the legacy tables the current schema no longer ships.
        conn.execute_batch(
            "CREATE TABLE spans (span_id TEXT PRIMARY KEY, spec TEXT); \
             CREATE TABLE claude_code_otel (ts_bucket INTEGER, metric TEXT);",
        )
        .unwrap();
        ensure_meta_table(&conn).unwrap();
        write_schema_version(&conn, 7).unwrap();

        let final_version = apply(&conn).unwrap();
        assert_eq!(final_version, LATEST_VERSION);
        assert!(!table_exists(&conn, "spans"), "v8 must drop spans");
        assert!(
            !table_exists(&conn, "claude_code_otel"),
            "v8 must drop claude_code_otel"
        );
    }

    #[test]
    fn v8_copies_history_into_telemetry_db_before_dropping() {
        // WARN-3 regression: a file-based mustard.db at v7 carrying legacy
        // telemetry must copy that data into the sibling telemetry.db BEFORE the
        // tables are dropped. After `apply`, telemetry.db holds the rows AND the
        // mustard.db legacy tables are gone — proving copy-before-drop.
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let mustard = dir.path().join("mustard.db");

        // Build a file-based mustard.db with the production schema, then pin it
        // at v7 and re-create + seed the legacy tables a pre-Wave-3 DB carried.
        let conn = Connection::open(&mustard).unwrap();
        conn.execute_batch(include_str!("sqlite_schema.sql")).unwrap();
        conn.execute_batch(
            "CREATE TABLE spans (\
                 trace_id TEXT, span_id TEXT PRIMARY KEY, parent_span_id TEXT, name TEXT, \
                 started_at INTEGER, ended_at INTEGER, duration_ms INTEGER, attributes TEXT, \
                 spec TEXT, phase TEXT, model TEXT, input_tokens INTEGER, output_tokens INTEGER, \
                 is_error INTEGER, cache_read_input_tokens INTEGER, \
                 cache_creation_input_tokens INTEGER, cost_usd_micros INTEGER, \
                 project_path TEXT, ts_iso TEXT, session_id TEXT, wave_id TEXT, tool_use_id TEXT); \
             CREATE TABLE claude_code_otel (\
                 ts_bucket INTEGER NOT NULL, signal TEXT NOT NULL, metric TEXT NOT NULL, \
                 session_id TEXT, model TEXT, token_type TEXT, sum REAL DEFAULT 0, \
                 count INTEGER DEFAULT 0, attrs TEXT, \
                 PRIMARY KEY (ts_bucket, metric, session_id, model, token_type));",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spans (span_id, spec, model, input_tokens, output_tokens, \
             cost_usd_micros, is_error, ts_iso, session_id) \
             VALUES ('sp-1', 'spec-A', 'opus', 1000, 500, 25000, 0, '2026-05-22T11:00:00Z', 's1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO claude_code_otel \
             (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs) \
             VALUES (60000, 'metric', 'claude_code.cost.usage', 's1', 'opus', 'input', 12.0, 1, '{}')",
            [],
        )
        .unwrap();
        ensure_meta_table(&conn).unwrap();
        write_schema_version(&conn, 7).unwrap();

        // Advance to v8 — this is where the copy-then-drop happens.
        let final_version = apply(&conn).unwrap();
        assert_eq!(final_version, LATEST_VERSION);

        // mustard.db legacy tables are gone (drop ran).
        assert!(!table_exists(&conn, "spans"), "v8 must drop spans");
        assert!(
            !table_exists(&conn, "claude_code_otel"),
            "v8 must drop claude_code_otel"
        );

        // The sibling telemetry.db received the history BEFORE the drop.
        let telemetry = dir.path().join("telemetry.db");
        assert!(telemetry.exists(), "telemetry.db must have been created");
        let tconn = Connection::open(&telemetry).unwrap();
        let runs: i64 = tconn
            .query_row("SELECT COUNT(*) FROM run_usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(runs, 1, "the span must be copied into run_usage");
        let totals: i64 = tconn
            .query_row("SELECT COUNT(*) FROM usage_totals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(totals, 1, "the otel counter must be copied into usage_totals");
        let spec: String = tconn
            .query_row(
                "SELECT spec FROM run_usage WHERE span_id = 'sp-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(spec, "spec-A", "native span attribution preserved");
    }

    /// Seed a file-based `mustard.db` pinned at v7, carrying the legacy
    /// `claude_code_otel` + `spans` tables with one row each. Used by the real-
    /// path regression below.
    fn seed_v7_mustard_db_with_legacy(mustard: &std::path::Path) {
        let conn = Connection::open(mustard).unwrap();
        conn.execute_batch(include_str!("sqlite_schema.sql")).unwrap();
        conn.execute_batch(
            "CREATE TABLE spans (\
                 trace_id TEXT, span_id TEXT PRIMARY KEY, parent_span_id TEXT, name TEXT, \
                 started_at INTEGER, ended_at INTEGER, duration_ms INTEGER, attributes TEXT, \
                 spec TEXT, phase TEXT, model TEXT, input_tokens INTEGER, output_tokens INTEGER, \
                 is_error INTEGER, cache_read_input_tokens INTEGER, \
                 cache_creation_input_tokens INTEGER, cost_usd_micros INTEGER, \
                 project_path TEXT, ts_iso TEXT, session_id TEXT, wave_id TEXT, tool_use_id TEXT); \
             CREATE TABLE claude_code_otel (\
                 ts_bucket INTEGER NOT NULL, signal TEXT NOT NULL, metric TEXT NOT NULL, \
                 session_id TEXT, model TEXT, token_type TEXT, sum REAL DEFAULT 0, \
                 count INTEGER DEFAULT 0, attrs TEXT, \
                 PRIMARY KEY (ts_bucket, metric, session_id, model, token_type));",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spans (span_id, spec, model, input_tokens, output_tokens, \
             cost_usd_micros, is_error, ts_iso, session_id) \
             VALUES ('sp-1', 'spec-A', 'opus', 1000, 500, 25000, 0, '2026-05-22T11:00:00Z', 's1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO claude_code_otel \
             (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs) \
             VALUES (60000, 'metric', 'claude_code.cost.usage', 's1', 'opus', 'input', 12.0, 1, '{}')",
            [],
        )
        .unwrap();
        ensure_meta_table(&conn).unwrap();
        write_schema_version(&conn, 7).unwrap();
    }

    #[test]
    fn v8_real_open_copies_before_drop_when_conn_owns_mustard_db() {
        // The WARN-3 data-loss regression. The bug only reproduces through the
        // REAL open path (`SqliteEventStore::new`), because the copy ran on a
        // second connection that contended with the migration's own connection
        // and silently skipped the source tables — yet the drop still ran.
        // Here we open the seeded v7 DB via that real path and assert the
        // sibling telemetry.db received the rows AND only then the legacy tables
        // are gone.
        use crate::store::sqlite_store::SqliteEventStore;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let mustard = dir.path().join("mustard.db");
        seed_v7_mustard_db_with_legacy(&mustard);

        // Real open — triggers migrate_v7_to_v8 with conn holding mustard.db.
        let store = SqliteEventStore::new(&mustard).unwrap();

        // Legacy tables dropped on the mustard.db now owned by the store.
        assert!(!table_exists(store.conn(), "spans"), "v8 must drop spans");
        assert!(
            !table_exists(store.conn(), "claude_code_otel"),
            "v8 must drop claude_code_otel"
        );

        // The sibling telemetry.db got the data BEFORE the drop — the property
        // the silent-skip bug violated (it dropped with telemetry.db empty).
        let telemetry = dir.path().join("telemetry.db");
        assert!(telemetry.exists(), "telemetry.db must exist");
        let tconn = Connection::open(&telemetry).unwrap();
        let runs: i64 = tconn
            .query_row("SELECT COUNT(*) FROM run_usage", [], |r| r.get(0))
            .unwrap();
        let totals: i64 = tconn
            .query_row("SELECT COUNT(*) FROM usage_totals", [], |r| r.get(0))
            .unwrap();
        assert!(runs > 0, "spans must be copied into run_usage before drop");
        assert!(totals > 0, "otel must be copied into usage_totals before drop");
        let spec: String = tconn
            .query_row("SELECT spec FROM run_usage WHERE span_id = 'sp-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(spec, "spec-A", "native span attribution preserved");
    }

    #[test]
    fn drop_guard_blocks_drop_when_source_has_rows_but_dest_empty() {
        // The pure decision the WARN-3 guard turns on: a source table with rows
        // whose destination stayed empty after the copy is a FAILED copy → must
        // NOT be dropped. Absent or empty sources, and dest-received-rows, are
        // all safe.
        assert!(!drop_is_safe(Some(5), 0, 0), "rows present, dest empty → unsafe");
        assert!(drop_is_safe(Some(5), 0, 5), "rows present, dest filled → safe");
        assert!(drop_is_safe(None, 0, 0), "source absent → safe");
        assert!(drop_is_safe(Some(0), 0, 0), "source empty → safe");
    }

    #[test]
    fn v8_does_not_drop_when_copy_cannot_reach_dest() {
        // End-to-end fail-open: a v7 mustard.db whose source tables hold rows,
        // attached to a telemetry.db whose destination tables are MISSING. The
        // guard's destination count then errors (never silently "absent"), so
        // `copy_then_verified_drop` propagates Err → the caller leaves the data.
        // Assert the source tables survive and the version stays at v7.
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let mustard = dir.path().join("mustard.db");
        seed_v7_mustard_db_with_legacy(&mustard);

        // A telemetry.db with NO run_usage / usage_totals tables.
        let telemetry = dir.path().join("telemetry.db");
        Connection::open(&telemetry).unwrap();

        let conn = Connection::open(&mustard).unwrap();
        conn.execute(
            "ATTACH DATABASE ?1 AS tel",
            rusqlite::params![telemetry.to_string_lossy()],
        )
        .unwrap();
        let outcome = copy_then_verified_drop(&conn);
        let _ = conn.execute_batch("DETACH DATABASE tel");

        assert!(outcome.is_err(), "missing dest tables must surface as Err, not a silent drop");
        assert!(table_exists(&conn, "spans"), "spans must NOT be dropped");
        assert!(
            table_exists(&conn, "claude_code_otel"),
            "claude_code_otel must NOT be dropped"
        );
        assert_eq!(read_schema_version(&conn).unwrap(), 7, "version stays v7");
    }

    #[test]
    fn v8_is_idempotent_when_tables_already_absent() {
        // A fresh DB never has the legacy tables; advancing to v8 is a no-op.
        let conn = fresh_db();
        assert!(!table_exists(&conn, "spans"));
        let final_version = apply(&conn).unwrap();
        assert_eq!(final_version, LATEST_VERSION);
        assert!(!table_exists(&conn, "spans"));
        assert!(!table_exists(&conn, "claude_code_otel"));
    }

    #[test]
    fn v7_creates_events_delete_trigger() {
        let conn = fresh_db();
        apply(&conn).unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='events_ad'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(exists, "v7 must create the events_ad delete trigger");
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
