//! Wave 8 (2026-05-21, spec
//! `2026-05-20-economia-moat-unification/wave-8-visao-geral-revamp`) — regression
//! test for the `top_files_today` ranking on the Visão Geral page.
//!
//! Before this wave the file ranking dropped to zero immediately after a spec
//! moved to `completed/` because the underlying replay path mixed the session
//! rotation with a UTC-midnight cut. The dashboard now overrides the ranking
//! with a session-agnostic SQL aggregation; this test pins the contract:
//! `tool.use` events from **every** session of the current UTC day must be
//! counted, including events whose spec has already been closed.

use rusqlite::Connection;
use mustard_dashboard_lib::spec_views::{top_files_today_impl, FileCount};

/// Minimal schema mirroring the subset of `events` columns the production DB
/// exposes. `payload` is a TEXT column the production code reads via
/// `json_extract`, so we store JSON strings the same way.
const SCHEMA: &str = r#"
CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  session_id TEXT,
  wave INTEGER,
  spec TEXT,
  event TEXT NOT NULL,
  actor_kind TEXT,
  actor_id TEXT,
  payload TEXT
);
"#;

/// Insert one `tool.use` event with `today_iso` formatted as `YYYY-MM-DDTHH:MM:SSZ`.
fn insert_tool_use(
    conn: &Connection,
    ts: &str,
    session_id: &str,
    spec: &str,
    file_path: &str,
) {
    let payload = format!(r#"{{"file_path":"{file_path}"}}"#);
    conn.execute(
        "INSERT INTO events (ts, session_id, spec, event, payload) VALUES (?,?,?,?,?)",
        rusqlite::params![ts, session_id, spec, "tool.use", payload],
    )
    .expect("insert tool.use");
}

/// Format today's UTC date as `YYYY-MM-DD` so the inserted rows fall inside the
/// `WHERE ts >= date('now')` window the production query uses.
fn today_utc() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%d").to_string()
}

/// AC-7: `top_files_today` must aggregate across every session of today even
/// after the spec that produced part of those events has been CLOSEd.
///
/// Scenario:
///   - session `s1` (now closed) touched `src/a.rs` twice and `src/b.rs` once
///   - session `s2` (current) touched `src/a.rs` once and `src/c.rs` once
///
/// Expected ranking: a.rs=3, b.rs=1, c.rs=1 — independent of which spec or
/// session owned the edits. Without the fix the closed-session rows would
/// silently disappear.
#[test]
fn test_top_files_today_post_close() {
    let conn = Connection::open_in_memory().expect("open in-memory");
    conn.execute_batch(SCHEMA).expect("apply schema");

    let today = today_utc();
    let ts1 = format!("{today}T08:00:00Z");
    let ts2 = format!("{today}T08:01:00Z");
    let ts3 = format!("{today}T08:02:00Z");
    let ts4 = format!("{today}T09:00:00Z");
    let ts5 = format!("{today}T09:01:00Z");

    // Closed spec — session s1
    insert_tool_use(&conn, &ts1, "s1", "old-spec", "src/a.rs");
    insert_tool_use(&conn, &ts2, "s1", "old-spec", "src/a.rs");
    insert_tool_use(&conn, &ts3, "s1", "old-spec", "src/b.rs");
    // Active spec — session s2 (post-CLOSE)
    insert_tool_use(&conn, &ts4, "s2", "new-spec", "src/a.rs");
    insert_tool_use(&conn, &ts5, "s2", "new-spec", "src/c.rs");

    let result: Vec<FileCount> = top_files_today_impl(&conn).expect("query ok");

    // Order: descending count, then path asc as a tiebreaker.
    let by_path: std::collections::HashMap<&str, i64> = result
        .iter()
        .map(|f| (f.path.as_str(), f.count))
        .collect();

    assert_eq!(by_path.get("src/a.rs").copied(), Some(3), "a.rs aggregates both sessions");
    assert_eq!(by_path.get("src/b.rs").copied(), Some(1), "b.rs from closed session preserved");
    assert_eq!(by_path.get("src/c.rs").copied(), Some(1), "c.rs from active session present");
    assert_eq!(result.len(), 3, "no other entries leak in");
    assert_eq!(result[0].path, "src/a.rs", "highest count ranks first");
}

/// Sanity check: yesterday's edits must NOT contribute to today's ranking.
/// Guards against the opposite bug where the window filter is dropped along
/// with the session filter.
#[test]
fn test_top_files_today_excludes_other_days() {
    let conn = Connection::open_in_memory().expect("open in-memory");
    conn.execute_batch(SCHEMA).expect("apply schema");

    // A row clearly in the past — 2020 is well before any plausible today.
    insert_tool_use(&conn, "2020-01-01T10:00:00Z", "s0", "old", "src/old.rs");
    let today = today_utc();
    insert_tool_use(&conn, &format!("{today}T10:00:00Z"), "s1", "new", "src/new.rs");

    let result = top_files_today_impl(&conn).expect("query ok");
    assert!(result.iter().all(|f| f.path != "src/old.rs"), "old.rs must not leak in");
    assert!(result.iter().any(|f| f.path == "src/new.rs"), "new.rs must be present");
}
