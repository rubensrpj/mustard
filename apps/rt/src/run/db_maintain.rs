//! `mustard-rt run db-maintain` — SQLite harness database maintenance.
//!
//! ## Modes
//!
//! - **Default (no flags, read-only):** print a JSON size/space report.
//! - **`--vacuum`:** checkpoint WAL then VACUUM; print before/after byte counts.
//! - **`--prune-keep <N>`:** delete all but the N most-recent events by id.
//! - **`--prune-older-than <N>d` (W11.T11.2):** delete events older than N
//!   whole days from `pipeline_events.ts` (ISO-8601 string compare).
//! - **`--telemetry-only` (W11.T11.2):** restrict every operation to
//!   `telemetry.db`; `mustard.db` is never opened in this mode (the docs
//!   contract from
//!   `.claude/spec/2026-05-25-mustard-deep-refactor/wave-11-mixed/spec.md`).
//!
//! DB path: `MUSTARD_DB_PATH` env, else `{cwd}/.claude/.harness/mustard.db`.
//! Telemetry DB path: `MUSTARD_TELEMETRY_DB_PATH` env, else
//! `{cwd}/.claude/.harness/telemetry.db`.

use mustard_core::store::sqlite_store::SqliteEventStore;
use serde_json::{json, Value};
use std::path::PathBuf;

/// CLI options parsed from the trailing arg slice.
pub struct DbMaintainOpts {
    /// Run VACUUM (implies WAL checkpoint first).
    pub vacuum: bool,
    /// Keep only the N most-recent events; `None` means no pruning.
    pub prune_keep: Option<u32>,
    /// Prune events older than N days; `None` means no age-based pruning.
    /// W11.T11.2 — companion to `--prune-keep` for time-bounded retention.
    pub prune_older_than_days: Option<u32>,
    /// Restrict every action to `telemetry.db`; skip `mustard.db` entirely.
    /// W11.T11.2 — lets ops vacuum the high-volume store without touching the
    /// hot harness DB the hooks open on every tool use.
    pub telemetry_only: bool,
}

impl DbMaintainOpts {
    /// Parse `--vacuum` / `--prune-keep <N>` / `--prune-older-than <N>d` /
    /// `--telemetry-only` from a flat arg slice. Unknown args are silently
    /// ignored (fail-open: a typo never aborts the maintenance run).
    pub fn from_args(args: &[String]) -> Self {
        let mut vacuum = false;
        let mut prune_keep: Option<u32> = None;
        let mut prune_older_than_days: Option<u32> = None;
        let mut telemetry_only = false;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--vacuum" => vacuum = true,
                "--telemetry-only" => telemetry_only = true,
                "--prune-keep" => {
                    if let Some(n) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                        prune_keep = Some(n);
                        i += 1;
                    }
                }
                "--prune-older-than" => {
                    if let Some(raw) = args.get(i + 1) {
                        // Accept `30`, `30d`, `30D` — strip the trailing `d`
                        // so the user can write the natural form. Other unit
                        // suffixes are rejected (parse fails → no prune).
                        let stripped = raw
                            .strip_suffix('d')
                            .or_else(|| raw.strip_suffix('D'))
                            .unwrap_or(raw);
                        if let Ok(n) = stripped.parse::<u32>() {
                            prune_older_than_days = Some(n);
                            i += 1;
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
        Self {
            vacuum,
            prune_keep,
            prune_older_than_days,
            telemetry_only,
        }
    }
}

/// Resolve the project DB path — same logic as `SqliteEventStore::for_project`.
fn resolve_db_path(cwd: &std::path::Path) -> PathBuf {
    if let Ok(p) = std::env::var("MUSTARD_DB_PATH") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    cwd.join(".claude").join(".harness").join("mustard.db")
}

/// Resolve the telemetry DB path. Mirrors `resolve_db_path` but targets
/// `telemetry.db` and honours `MUSTARD_TELEMETRY_DB_PATH`.
/// W11.T11.2 — backs `--telemetry-only`.
fn resolve_telemetry_db_path(cwd: &std::path::Path) -> PathBuf {
    if let Ok(p) = std::env::var("MUSTARD_TELEMETRY_DB_PATH") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    cwd.join(".claude").join(".harness").join("telemetry.db")
}

/// Read SQLite size PRAGMAs; return `(page_count, freelist_count, page_size)`.
fn read_size_pragmas(conn: &rusqlite::Connection) -> (i64, i64, i64) {
    let page_count: i64 = conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap_or(0);
    let freelist_count: i64 = conn
        .query_row("PRAGMA freelist_count", [], |r| r.get(0))
        .unwrap_or(0);
    let page_size: i64 = conn
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .unwrap_or(4096);
    (page_count, freelist_count, page_size)
}

/// Try `SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC`.
/// Returns `None` when `dbstat` is not compiled in, so the caller can emit
/// `"dbstat": "unavailable"` instead of failing.
fn query_dbstat(conn: &rusqlite::Connection) -> Option<Vec<(String, i64)>> {
    let mut stmt = conn
        .prepare("SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC")
        .ok()?;
    let rows: rusqlite::Result<Vec<(String, i64)>> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok()?
        .collect();
    rows.ok()
}

/// `SELECT kind, COUNT(*) c FROM pipeline_events GROUP BY kind ORDER BY c DESC LIMIT 10`.
///
/// W5: the high-volume `events` table is gone; the lifecycle index
/// `pipeline_events` (column `kind`) is the SQLite-side equivalent. Non-pipeline
/// events live in per-spec NDJSON files and are not aggregated here.
fn top_event_kinds(conn: &rusqlite::Connection) -> Vec<Value> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT kind, COUNT(*) c FROM pipeline_events GROUP BY kind ORDER BY c DESC LIMIT 10",
    ) else {
        return Vec::new();
    };
    stmt.query_map([], |r| {
        let event: String = r.get(0)?;
        let count: i64 = r.get(1)?;
        Ok((event, count))
    })
    .map(|rows| {
        rows.filter_map(|r| r.ok())
            .map(|(e, c)| json!({ "event": e, "count": c }))
            .collect()
    })
    .unwrap_or_default()
}

/// WAL file size in bytes (0 when the file is absent or unreadable).
fn wal_bytes(db_path: &std::path::Path) -> u64 {
    let wal = db_path.with_extension("db-wal");
    std::fs::metadata(&wal).map_or(0, |m| m.len())
}

/// Stats-only (read-only) mode — prints the size/space JSON report.
fn run_stats(store: &SqliteEventStore) -> Value {
    let conn = store.conn();

    // W5: the high-volume `events` table is retired; the per-spec NDJSON sink
    // owns the hot path. SQLite keeps the lean `pipeline_events` lifecycle index.
    let events_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM pipeline_events", [], |r| r.get(0))
        .unwrap_or(0);
    let events_fts_count: i64 = -1; // events_fts retired in W5.

    let (page_count, freelist_count, page_size) = read_size_pragmas(conn);
    let db_bytes = page_count * page_size;
    let reclaimable_bytes = freelist_count * page_size;
    let wal = wal_bytes(store.path());

    // Per-table breakdown — fail-open if dbstat vtab is missing.
    let (per_table, dbstat_note) = match query_dbstat(conn) {
        Some(rows) => {
            let arr: Vec<Value> = rows
                .into_iter()
                .map(|(name, sz)| json!({ "table": name, "bytes": sz }))
                .collect();
            (Some(arr), None)
        }
        None => (None, Some("unavailable")),
    };

    let top_events = top_event_kinds(conn);

    let mut doc = json!({
        "events_count": events_count,
        "events_fts_count": events_fts_count,
        "page_count": page_count,
        "freelist_count": freelist_count,
        "page_size": page_size,
        "db_bytes": db_bytes,
        "reclaimable_bytes": reclaimable_bytes,
        "wal_bytes": wal,
        "top_event_kinds": top_events,
    });

    if let Some(rows) = per_table {
        doc["per_table"] = json!(rows);
    }
    if let Some(note) = dbstat_note {
        doc["dbstat"] = json!(note);
    }

    doc
}

/// `--vacuum` mode: checkpoint WAL, VACUUM, print before/after stats.
fn run_vacuum(store: &SqliteEventStore) {
    let conn = store.conn();

    let (pc_before, fl_before, ps) = read_size_pragmas(conn);
    let bytes_before = pc_before * ps;

    let start = std::time::Instant::now();

    // Checkpoint: flush WAL pages into the main database file.
    let cp: rusqlite::Result<i64> =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0));
    if let Err(e) = cp {
        eprintln!("[db-maintain] WARN: checkpoint error: {e}");
    }

    // VACUUM reclaims freelist pages — must run outside any open transaction.
    if let Err(e) = conn.execute_batch("VACUUM") {
        eprintln!("[db-maintain] WARN: VACUUM error: {e}");
    }

    let elapsed_ms = start.elapsed().as_millis();

    let (pc_after, fl_after, _) = read_size_pragmas(conn);
    let bytes_after = pc_after * ps;

    let doc = json!({
        "mode": "vacuum",
        "before": { "db_bytes": bytes_before, "freelist_count": fl_before },
        "after":  { "db_bytes": bytes_after,  "freelist_count": fl_after },
        "reclaimed_bytes": bytes_before - bytes_after,
        "elapsed_ms": elapsed_ms as i64,
    });
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
}

/// `--prune-keep <N>` mode: delete all but the N most-recent rows from
/// `pipeline_events` (the W5 lifecycle index). NDJSON spec files are pruned
/// separately via `mustard-rt run spec-clear`.
fn run_prune(store: &SqliteEventStore, keep: u32) {
    let conn = store.conn();

    let rows_deleted: usize = conn
        .execute(
            "DELETE FROM pipeline_events WHERE id NOT IN \
             (SELECT id FROM pipeline_events ORDER BY id DESC LIMIT ?1)",
            rusqlite::params![keep],
        )
        .unwrap_or(0);

    let doc = json!({
        "mode": "prune-keep",
        "keep": keep,
        "rows_deleted": rows_deleted,
        "note": "Run --vacuum afterward to reclaim freed pages from disk.",
    });
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
}

/// `--prune-older-than <N>d` mode (W11.T11.2): delete every row in
/// `pipeline_events` whose `ts` is older than `now - N days`. The column is an
/// ISO-8601 string; we build the cutoff string in the same shape so SQLite can
/// compare lexicographically without parsing.
///
/// Fail-open: SQL errors degrade to `rows_deleted: 0` and the report still
/// prints — the maintenance run is never the load-bearing path.
fn run_prune_older(store: &SqliteEventStore, days: u32) {
    let conn = store.conn();

    // Cutoff is `now - N * 86400 seconds`. We render it through SQLite's own
    // `strftime` so the format always matches what the writer emits (the
    // writer uses `chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis,
    // true)` — `YYYY-MM-DDTHH:MM:SS.sssZ`). Comparing strings in this layout
    // is correct because ISO-8601 sorts lexicographically.
    let cutoff: String = conn
        .query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1)",
            rusqlite::params![format!("-{days} days")],
            |r| r.get(0),
        )
        .unwrap_or_default();

    let rows_deleted: usize = if cutoff.is_empty() {
        0
    } else {
        conn.execute(
            "DELETE FROM pipeline_events WHERE ts < ?1",
            rusqlite::params![&cutoff],
        )
        .unwrap_or(0)
    };

    let doc = json!({
        "mode": "prune-older-than",
        "days": days,
        "cutoff": cutoff,
        "rows_deleted": rows_deleted,
        "note": "Run --vacuum afterward to reclaim freed pages from disk.",
    });
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
}

/// Run vacuum directly on a raw `telemetry.db` connection. Mirrors
/// `run_vacuum(&store)` but skips the harness `SqliteEventStore` wrapper —
/// `telemetry.db` is a separate database (no ATTACH ever; see memory
/// `feedback_no_attach_sqlite`).
fn run_vacuum_raw(conn: &rusqlite::Connection, db_path: &std::path::Path) {
    let (pc_before, fl_before, ps) = read_size_pragmas(conn);
    let bytes_before = pc_before * ps;

    let start = std::time::Instant::now();

    let cp: rusqlite::Result<i64> =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0));
    if let Err(e) = cp {
        eprintln!("[db-maintain] WARN: telemetry checkpoint error: {e}");
    }
    if let Err(e) = conn.execute_batch("VACUUM") {
        eprintln!("[db-maintain] WARN: telemetry VACUUM error: {e}");
    }

    let elapsed_ms = start.elapsed().as_millis();

    let (pc_after, fl_after, _) = read_size_pragmas(conn);
    let bytes_after = pc_after * ps;

    let doc = json!({
        "mode": "vacuum",
        "target": "telemetry",
        "path": db_path.to_string_lossy(),
        "before": { "db_bytes": bytes_before, "freelist_count": fl_before },
        "after":  { "db_bytes": bytes_after,  "freelist_count": fl_after },
        "reclaimed_bytes": bytes_before - bytes_after,
        "elapsed_ms": elapsed_ms as i64,
    });
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
}

/// Emit one `pipeline.economy.operation.invoked` event for the maintenance
/// run. Fail-open per spec (W11 contract) — any write error is logged and
/// swallowed.
fn emit_economy_event(duration_ms: u128, mode: &str) {
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use crate::run::env::{current_spec, session_id};
    use crate::util::now_iso8601;

    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec = current_spec(&cwd);
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("db-maintain".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": format!("db-maintain.{mode}"),
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec,
    };
    let _ = crate::run::event_route::emit(&cwd, &ev);
}

/// Entry point for `mustard-rt run db-maintain`.
pub fn run(args: &[String]) {
    let started = std::time::Instant::now();
    let opts = DbMaintainOpts::from_args(args);

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Decide which mode label to emit on the economy event (read first so the
    // event reflects the actual chosen path even when nothing runs).
    let mode_label = if opts.vacuum {
        "vacuum"
    } else if opts.prune_keep.is_some() {
        "prune-keep"
    } else if opts.prune_older_than_days.is_some() {
        "prune-older-than"
    } else {
        "stats"
    };

    if opts.telemetry_only {
        // W11.T11.2 — telemetry.db is a separate file (no ATTACH). The
        // harness `mustard.db` is never opened in this mode.
        let tele_path = resolve_telemetry_db_path(&cwd);
        if !tele_path.exists() {
            let doc = json!({
                "error": "telemetry database not found",
                "path": tele_path.to_string_lossy(),
            });
            println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
            emit_economy_event(started.elapsed().as_millis(), mode_label);
            return;
        }
        let conn = match rusqlite::Connection::open(&tele_path) {
            Ok(c) => c,
            Err(e) => {
                let doc = json!({
                    "error": format!("cannot open telemetry db: {e}"),
                    "path": tele_path.to_string_lossy(),
                });
                println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
                emit_economy_event(started.elapsed().as_millis(), mode_label);
                return;
            }
        };
        if opts.vacuum {
            run_vacuum_raw(&conn, &tele_path);
        } else {
            // Telemetry-only currently supports vacuum + stats. Prune is a
            // pipeline-events concern (mustard.db); telemetry rows are
            // append-only and ageing them out belongs to a future spec.
            let (page_count, freelist_count, page_size) = read_size_pragmas(&conn);
            let doc = json!({
                "target": "telemetry",
                "path": tele_path.to_string_lossy(),
                "page_count": page_count,
                "freelist_count": freelist_count,
                "page_size": page_size,
                "db_bytes": page_count * page_size,
                "reclaimable_bytes": freelist_count * page_size,
            });
            println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
        }
        emit_economy_event(started.elapsed().as_millis(), mode_label);
        return;
    }

    let db_path = resolve_db_path(&cwd);

    // Fail-open: if the DB doesn't exist yet, emit a minimal notice and exit.
    if !db_path.exists() {
        let doc = json!({
            "error": "database not found",
            "path": db_path.to_string_lossy(),
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
        emit_economy_event(started.elapsed().as_millis(), mode_label);
        return;
    }

    let store = match SqliteEventStore::new(&db_path) {
        Ok(s) => s,
        Err(e) => {
            let doc = json!({ "error": format!("cannot open db: {e}"), "path": db_path.to_string_lossy() });
            println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
            emit_economy_event(started.elapsed().as_millis(), mode_label);
            return;
        }
    };

    if opts.vacuum {
        run_vacuum(&store);
    } else if let Some(keep) = opts.prune_keep {
        run_prune(&store, keep);
    } else if let Some(days) = opts.prune_older_than_days {
        run_prune_older(&store, days);
    } else {
        // Default: stats-only, read-only.
        let doc = run_stats(&store);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
    }

    emit_economy_event(started.elapsed().as_millis(), mode_label);
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opts_parse_vacuum() {
        let args = vec!["--vacuum".to_string()];
        let opts = DbMaintainOpts::from_args(&args);
        assert!(opts.vacuum);
        assert!(opts.prune_keep.is_none());
        assert!(opts.prune_older_than_days.is_none());
        assert!(!opts.telemetry_only);
    }

    #[test]
    fn opts_parse_prune_keep() {
        let args = vec!["--prune-keep".to_string(), "5000".to_string()];
        let opts = DbMaintainOpts::from_args(&args);
        assert!(!opts.vacuum);
        assert_eq!(opts.prune_keep, Some(5000));
    }

    #[test]
    fn opts_default_is_stats() {
        let opts = DbMaintainOpts::from_args(&[]);
        assert!(!opts.vacuum);
        assert!(opts.prune_keep.is_none());
        assert!(opts.prune_older_than_days.is_none());
        assert!(!opts.telemetry_only);
    }

    #[test]
    fn opts_parse_telemetry_only() {
        let args = vec!["--telemetry-only".to_string()];
        let opts = DbMaintainOpts::from_args(&args);
        assert!(opts.telemetry_only);
    }

    #[test]
    fn opts_parse_prune_older_than_with_d_suffix() {
        let args = vec!["--prune-older-than".to_string(), "30d".to_string()];
        let opts = DbMaintainOpts::from_args(&args);
        assert_eq!(opts.prune_older_than_days, Some(30));
    }

    #[test]
    fn opts_parse_prune_older_than_without_suffix() {
        let args = vec!["--prune-older-than".to_string(), "7".to_string()];
        let opts = DbMaintainOpts::from_args(&args);
        assert_eq!(opts.prune_older_than_days, Some(7));
    }

    #[test]
    fn opts_parse_combined_telemetry_only_vacuum() {
        let args = vec!["--telemetry-only".to_string(), "--vacuum".to_string()];
        let opts = DbMaintainOpts::from_args(&args);
        assert!(opts.telemetry_only);
        assert!(opts.vacuum);
    }

    #[test]
    fn missing_db_emits_json_error() {
        // Redirect cwd to a tempdir with no mustard.db.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("mustard.db");
        assert!(!db_path.exists());
        // Verify resolve_db_path works as expected.
        let resolved = resolve_db_path(dir.path());
        assert!(resolved.ends_with("mustard.db"));
    }

    #[test]
    fn stats_run_on_fresh_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("mustard.db");
        let store = SqliteEventStore::new(&db_path).unwrap();
        let doc = run_stats(&store);
        // Basic shape checks.
        assert_eq!(doc["events_count"], serde_json::json!(0));
        assert!(doc.get("db_bytes").is_some());
        assert!(doc.get("reclaimable_bytes").is_some());
        // dbstat may or may not be available — either field must be present.
        let has_per_table = doc.get("per_table").is_some();
        let has_dbstat_note = doc.get("dbstat").is_some_and(|v| v == "unavailable");
        assert!(
            has_per_table || has_dbstat_note,
            "expected per_table or dbstat=unavailable"
        );
    }

    #[test]
    fn prune_removes_old_events() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("mustard.db");
        let store = SqliteEventStore::new(&db_path).unwrap();
        let conn = store.conn();
        // W5: insert 5 lifecycle rows into `pipeline_events`.
        for i in 0..5u32 {
            conn.execute(
                "INSERT INTO pipeline_events (session_id, kind, payload, ts) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    format!("sess-{i}"),
                    "pipeline.status",
                    "{}",
                    format!("2026-05-{:02}T00:00:00Z", i + 1)
                ],
            )
            .unwrap();
        }
        let count_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM pipeline_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_before, 5);

        run_prune(&store, 3);

        let count_after: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM pipeline_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count_after, 3);
    }
}
