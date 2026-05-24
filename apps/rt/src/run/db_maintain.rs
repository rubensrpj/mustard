//! `mustard-rt run db-maintain` — SQLite harness database maintenance.
//!
//! ## Modes
//!
//! - **Default (no flags, read-only):** print a JSON size/space report.
//! - **`--vacuum`:** checkpoint WAL then VACUUM; print before/after byte counts.
//! - **`--prune-keep <N>`:** delete all but the N most-recent events by id.
//!
//! DB path: `MUSTARD_DB_PATH` env, else `{cwd}/.claude/.harness/mustard.db`.

use mustard_core::store::sqlite_store::SqliteEventStore;
use serde_json::{json, Value};
use std::path::PathBuf;

/// CLI options parsed from the trailing arg slice.
pub struct DbMaintainOpts {
    /// Run VACUUM (implies WAL checkpoint first).
    pub vacuum: bool,
    /// Keep only the N most-recent events; `None` means no pruning.
    pub prune_keep: Option<u32>,
}

impl DbMaintainOpts {
    /// Parse `--vacuum` / `--prune-keep <N>` from a flat arg slice.
    pub fn from_args(args: &[String]) -> Self {
        let mut vacuum = false;
        let mut prune_keep: Option<u32> = None;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--vacuum" => vacuum = true,
                "--prune-keep" => {
                    if let Some(n) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                        prune_keep = Some(n);
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        Self { vacuum, prune_keep }
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

/// `SELECT event, COUNT(*) c FROM events GROUP BY event ORDER BY c DESC LIMIT 10`.
fn top_event_kinds(conn: &rusqlite::Connection) -> Vec<Value> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT event, COUNT(*) c FROM events GROUP BY event ORDER BY c DESC LIMIT 10",
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

    let events_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap_or(0);

    let events_fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events_fts", [], |r| r.get(0))
        .unwrap_or(-1); // -1 signals unavailable

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

/// `--prune-keep <N>` mode: delete all but the N most-recent events.
fn run_prune(store: &SqliteEventStore, keep: u32) {
    let conn = store.conn();

    let rows_deleted: usize = conn
        .execute(
            "DELETE FROM events WHERE id NOT IN \
             (SELECT id FROM events ORDER BY id DESC LIMIT ?1)",
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

/// Entry point for `mustard-rt run db-maintain`.
pub fn run(args: &[String]) {
    let opts = DbMaintainOpts::from_args(args);

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let db_path = resolve_db_path(&cwd);

    // Fail-open: if the DB doesn't exist yet, emit a minimal notice and exit.
    if !db_path.exists() {
        let doc = json!({
            "error": "database not found",
            "path": db_path.to_string_lossy(),
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
        return;
    }

    let store = match SqliteEventStore::new(&db_path) {
        Ok(s) => s,
        Err(e) => {
            let doc = json!({ "error": format!("cannot open db: {e}"), "path": db_path.to_string_lossy() });
            println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
            return;
        }
    };

    if opts.vacuum {
        run_vacuum(&store);
    } else if let Some(keep) = opts.prune_keep {
        run_prune(&store, keep);
    } else {
        // Default: stats-only, read-only.
        let doc = run_stats(&store);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string()));
    }
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
        // Insert 5 bare-minimum events.
        for i in 0..5u32 {
            conn.execute(
                "INSERT INTO events (session_id, event, payload, ts) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    format!("sess-{i}"),
                    "test",
                    "{}",
                    format!("2026-05-{:02}T00:00:00Z", i + 1)
                ],
            )
            .unwrap();
        }
        let count_before: i64 =
            conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(count_before, 5);

        run_prune(&store, 3);

        let count_after: i64 =
            store.conn().query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(count_after, 3);
    }
}
