// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! Integration tests for Wave 6b — SQLite-backed memory writes and FTS5 search.
//!
//! Opens the harness DB (schema applied via `SqliteEventStore::for_project`)
//! and then verifies inserts + FTS5 MATCH queries.  Since `mustard-rt` is a
//! binary-only crate (no `lib.rs`), these tests call `SqliteEventStore` from
//! `mustard-core` to initialise the schema, then operate directly through
//! `rusqlite::Connection` for the Wave-6b DML.

use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::{Connection, params};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helper — open the harness DB (schema applied) and a direct Connection.
// ---------------------------------------------------------------------------

fn open_db(project: &std::path::Path) -> Connection {
    let store = SqliteEventStore::for_project(project).expect("store opens");
    let db_path = store.path().to_path_buf();
    let conn = Connection::open(&db_path).expect("direct connection opens");
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    conn
}

// ---------------------------------------------------------------------------
// INSERT helpers — mirrors memory.rs logic (kept local to avoid pub leakage)
// ---------------------------------------------------------------------------

fn insert_decision(
    conn: &Connection,
    content: &str,
    source: Option<&str>,
    context: Option<&str>,
    at: Option<&str>,
) {
    let at_val = at.unwrap_or("2026-05-20T00:00:00Z");
    conn.execute(
        "INSERT INTO memory_decisions (content, source, context, at) VALUES (?1, ?2, ?3, ?4)",
        params![content, source, context, at_val],
    )
    .expect("insert decision");
}

fn insert_lesson(
    conn: &Connection,
    content: &str,
    source: Option<&str>,
    context: Option<&str>,
    at: Option<&str>,
) {
    let at_val = at.unwrap_or("2026-05-20T00:00:00Z");
    conn.execute(
        "INSERT INTO memory_lessons (content, source, context, at) VALUES (?1, ?2, ?3, ?4)",
        params![content, source, context, at_val],
    )
    .expect("insert lesson");
}

fn upsert_knowledge(conn: &Connection, pattern: &str, confidence: f64, source: Option<&str>) {
    let now = "2026-05-20T00:00:00Z";
    conn.execute(
        "INSERT INTO knowledge_patterns \
         (pattern, confidence, count, last_seen, source, created_at) \
         VALUES (?1, ?2, 1, ?3, ?4, ?3) \
         ON CONFLICT(pattern) DO UPDATE SET \
           confidence = excluded.confidence, \
           count = count + 1, \
           last_seen = excluded.last_seen, \
           source = COALESCE(excluded.source, source)",
        params![pattern, confidence, now, source],
    )
    .expect("upsert knowledge");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn insert_decision_round_trips_and_fts_matches() {
    let dir = tempdir().unwrap();
    let conn = open_db(dir.path());

    insert_decision(
        &conn,
        "serde lenient pays off",
        Some("spec-A"),
        Some("Wave 6b context"),
        Some("2026-05-20T00:00:00Z"),
    );

    // Row is present.
    let content: String = conn
        .query_row(
            "SELECT content FROM memory_decisions WHERE source = 'spec-A'",
            [],
            |r| r.get(0),
        )
        .expect("row exists");
    assert_eq!(content, "serde lenient pays off");

    // FTS5 MATCH on 'serde'.
    let fts_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_decisions_fts \
             WHERE memory_decisions_fts MATCH 'serde'",
            [],
            |r| r.get(0),
        )
        .expect("FTS query");
    assert_eq!(fts_count, 1, "FTS5 must find the decision by 'serde'");
}

#[test]
fn insert_lesson_round_trips_and_fts_matches() {
    let dir = tempdir().unwrap();
    let conn = open_db(dir.path());

    insert_lesson(
        &conn,
        "always verify before recommending",
        Some("retro-B"),
        None,
        Some("2026-05-20T01:00:00Z"),
    );

    // Row is present.
    let source: Option<String> = conn
        .query_row(
            "SELECT source FROM memory_lessons WHERE content LIKE '%verify%'",
            [],
            |r| r.get(0),
        )
        .expect("row exists");
    assert_eq!(source.as_deref(), Some("retro-B"));

    // FTS5 MATCH on 'verify'.
    let fts_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_lessons_fts \
             WHERE memory_lessons_fts MATCH 'verify'",
            [],
            |r| r.get(0),
        )
        .expect("FTS query");
    assert_eq!(fts_count, 1, "FTS5 must find the lesson by 'verify'");
}

#[test]
fn upsert_knowledge_pattern_bumps_count_and_fts_matches() {
    let dir = tempdir().unwrap();
    let conn = open_db(dir.path());

    upsert_knowledge(&conn, "fail-open: always return Ok on store errors", 0.7, Some("spec-A"));
    upsert_knowledge(&conn, "fail-open: always return Ok on store errors", 0.8, Some("spec-B"));

    // count=2, confidence=0.8.
    let (count, confidence): (i64, f64) = conn
        .query_row(
            "SELECT count, confidence FROM knowledge_patterns WHERE pattern LIKE 'fail-open%'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row exists");
    assert_eq!(count, 2);
    assert!((confidence - 0.8).abs() < 1e-9);

    // FTS5 search on `knowledge_patterns_fts` — NOT `knowledge_fts`.
    let fts_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_patterns_fts \
             WHERE knowledge_patterns_fts MATCH 'fail'",
            [],
            |r| r.get(0),
        )
        .expect("FTS query on knowledge_patterns_fts");
    assert_eq!(fts_count, 1, "FTS5 must find the pattern by 'fail'");
}

#[test]
fn decision_at_timestamp_is_preserved() {
    let dir = tempdir().unwrap();
    let conn = open_db(dir.path());

    insert_decision(&conn, "custom timestamp test", None, None, Some("2025-01-15T12:00:00Z"));

    let at: String =
        conn.query_row("SELECT at FROM memory_decisions", [], |r| r.get(0)).unwrap();
    assert_eq!(at, "2025-01-15T12:00:00Z");
}
