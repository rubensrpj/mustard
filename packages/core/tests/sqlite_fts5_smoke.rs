// Integration test — same lint carve-out as parity.rs: `.unwrap()` and
// `.expect()` are the intended test-failure mechanism here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Smoke test for Wave 6a: confirms that FTS5 is compiled into the bundled
//! SQLite and that the three new virtual tables + their sync triggers work
//! end-to-end.
//!
//! Strategy: open a temp store through `SqliteEventStore::new` (which applies
//! the full schema), then open a *parallel* `rusqlite::Connection` to the same
//! file for the INSERT/SELECT work. The store's `conn` field is private — this
//! approach avoids changing the public API while still validating the schema.

use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::Connection;
use tempfile::tempdir;

/// Confirms that `memory_decisions_fts` is wired correctly: inserting 3 rows
/// via the `memory_decisions` table auto-populates the FTS5 index (through the
/// `memory_decisions_ai` trigger), and a MATCH query returns the right hit count.
#[test]
fn fts5_basic_match() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Apply the schema (including all Wave 6a tables + triggers).
    SqliteEventStore::new(&db_path).expect("store must open");

    // Open a parallel connection for INSERT/SELECT — the store's conn is private.
    let conn = Connection::open(&db_path).expect("parallel connection must open");

    conn.execute(
        "INSERT INTO memory_decisions (content, source, context, at) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            "use serde lenient for boundary models",
            "spec-A",
            "approved at PLAN",
            "2026-05-20T00:00:00Z"
        ],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO memory_decisions (content, source, context, at) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            "never delete in-flight migrations",
            "spec-B",
            "PLAN review",
            "2026-05-20T00:01:00Z"
        ],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO memory_decisions (content, source, context, at) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            "FTS5 over sqlite-vec",
            "spec-A",
            "approved",
            "2026-05-20T00:02:00Z"
        ],
    )
    .unwrap();

    // "serde" appears in only the first row — expect exactly 1 hit.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_decisions_fts \
             WHERE memory_decisions_fts MATCH ?1",
            ["serde"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "expected one hit for 'serde' via FTS5");
}

/// Confirms that `memory_lessons_fts` triggers fire symmetrically.
#[test]
fn fts5_memory_lessons_trigger() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("lessons.db");
    SqliteEventStore::new(&db_path).expect("store must open");
    let conn = Connection::open(&db_path).unwrap();

    conn.execute(
        "INSERT INTO memory_lessons (content, source, context, at) \
         VALUES ('prefer trait abstractions over concrete types', 'lessons', 'design', '2026-05-20T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_lessons (content, source, context, at) \
         VALUES ('measure before optimising', 'lessons', 'perf', '2026-05-20T00:01:00Z')",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_lessons_fts WHERE memory_lessons_fts MATCH ?1",
            ["trait"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "expected one hit for 'trait'");
}

/// Confirms that `knowledge_patterns_fts` triggers fire on INSERT.
#[test]
fn fts5_knowledge_patterns_trigger() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("patterns.db");
    SqliteEventStore::new(&db_path).expect("store must open");
    let conn = Connection::open(&db_path).unwrap();

    conn.execute(
        "INSERT INTO knowledge_patterns (pattern, confidence, count, last_seen, source, created_at) \
         VALUES ('fail-open error handling', 0.9, 5, '2026-05-20T00:00:00Z', 'session-x', '2026-05-20T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO knowledge_patterns (pattern, confidence, count, last_seen, source, created_at) \
         VALUES ('trait-backed IO abstraction', 0.8, 3, '2026-05-20T00:01:00Z', 'session-y', '2026-05-20T00:01:00Z')",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_patterns_fts WHERE knowledge_patterns_fts MATCH ?1",
            ["fail"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "expected one hit for 'fail'");

    // Both rows share "handling" / not. "trait" is unique to second row.
    let count2: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM knowledge_patterns_fts WHERE knowledge_patterns_fts MATCH ?1",
            ["trait"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count2, 1, "expected one hit for 'trait'");
}

/// Confirms the DELETE trigger removes rows from the FTS index.
#[test]
fn fts5_delete_trigger_removes_from_index() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("delete_test.db");
    SqliteEventStore::new(&db_path).expect("store must open");
    let conn = Connection::open(&db_path).unwrap();

    conn.execute(
        "INSERT INTO memory_decisions (content, source, context, at) \
         VALUES ('ephemeral decision', 'src', 'ctx', '2026-05-20T00:00:00Z')",
        [],
    )
    .unwrap();

    // Verify it was indexed.
    let before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_decisions_fts WHERE memory_decisions_fts MATCH ?1",
            ["ephemeral"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before, 1);

    // Delete the row — the `memory_decisions_ad` trigger should remove it from FTS.
    conn.execute("DELETE FROM memory_decisions WHERE content = 'ephemeral decision'", [])
        .unwrap();

    let after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_decisions_fts WHERE memory_decisions_fts MATCH ?1",
            ["ephemeral"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after, 0, "deleted row must be removed from FTS index");
}
