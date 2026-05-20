// Integration tests for the pipeline_amend_window table and its projection
// methods. Each test opens a fresh temp store so they are fully independent.
//
// `.unwrap()` / `.expect()` are the intended test-failure mechanism here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mustard_core::io::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, SCHEMA_VERSION,
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn open_store(dir: &std::path::Path) -> SqliteEventStore {
    SqliteEventStore::new(dir.join("test.db")).expect("store must open")
}

fn task_complete_event(spec: &str, session: &str, files: &[&str]) -> HarnessEvent {
    HarnessEvent {
        v: SCHEMA_VERSION,
        ts: "2026-05-20T00:00:00.000Z".to_string(),
        session_id: session.to_string(),
        wave: 1,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("test".to_string()),
            actor_type: None,
        },
        event: "pipeline.task.complete".to_string(),
        payload: json!({
            "name": "wave-task",
            "files_modified": files,
        }),
        spec: Some(spec.to_string()),
    }
}

// ---------------------------------------------------------------------------
// AC-1: schema has pipeline_amend_window with the expected 10 columns
// ---------------------------------------------------------------------------

/// AC-1: Confirms that the `pipeline_amend_window` table exists in the schema
/// with all 10 declared columns and that a basic INSERT/SELECT round-trip works.
#[test]
fn schema_amend_window_present() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // Applying the schema is done by SqliteEventStore::new.
    SqliteEventStore::new(&db_path).expect("store must open");

    let conn = Connection::open(&db_path).unwrap();

    // PRAGMA table_info returns one row per column.
    let col_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('pipeline_amend_window')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(col_count, 10, "pipeline_amend_window must have exactly 10 columns");

    // INSERT/SELECT round-trip verifies column names and constraints.
    conn.execute(
        "INSERT INTO pipeline_amend_window \
         (spec_id, session_id, closed_at, pipeline_file_set, subprojects) \
         VALUES ('spec-1', 'sess-1', '2026-05-20T00:00:00Z', '[\"a.rs\"]', '[\"core\"]')",
        [],
    )
    .unwrap();

    let status: String = conn
        .query_row(
            "SELECT status FROM pipeline_amend_window WHERE spec_id = 'spec-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "open", "default status must be 'open'");
}

// ---------------------------------------------------------------------------
// AC-2: amend_window_pipeline_file_set deduplicates across overlapping waves
// ---------------------------------------------------------------------------

/// AC-2: Three `pipeline.task.complete` events for the same spec with
/// overlapping `files_modified` lists must be unioned and deduplicated.
///
/// wave1 = [a.rs, b.rs], wave2 = [b.rs, c.rs], wave3 = [c.rs, d.rs]
/// Expected result = ["a.rs", "b.rs", "c.rs", "d.rs"] sorted.
#[test]
fn amend_window_projection_union_dedupes() {
    let dir = tempdir().unwrap();
    let store = open_store(dir.path());

    use mustard_core::io::event_store::EventSink;

    let spec = "spec-union";
    let session = "sess-union";

    store
        .append(&task_complete_event(spec, session, &["a.rs", "b.rs"]))
        .unwrap();
    store
        .append(&task_complete_event(spec, session, &["b.rs", "c.rs"]))
        .unwrap();
    store
        .append(&task_complete_event(spec, session, &["c.rs", "d.rs"]))
        .unwrap();

    let file_set = store.amend_window_pipeline_file_set(spec).unwrap();
    assert_eq!(file_set, vec!["a.rs", "b.rs", "c.rs", "d.rs"]);
}

// ---------------------------------------------------------------------------
// AC-7: amend_window_for_session returns the inserted window
// ---------------------------------------------------------------------------

/// AC-7: After inserting a synthetic `pipeline_amend_window` row,
/// `amend_window_for_session` must return `Some(window)` with a matching
/// `pipeline_file_set`.
#[test]
fn amend_window_open_on_complete() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = SqliteEventStore::new(&db_path).expect("store must open");

    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "INSERT INTO pipeline_amend_window \
         (spec_id, session_id, closed_at, pipeline_file_set, subprojects, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "spec-ac7",
            "sess-ac7",
            "2026-05-20T00:00:00Z",
            r#"["src/main.rs","src/lib.rs"]"#,
            r#"["core"]"#,
            "open",
        ],
    )
    .unwrap();

    let window = store
        .amend_window_for_session("sess-ac7")
        .unwrap()
        .expect("window must be Some for an open session");

    assert_eq!(window.spec_id, "spec-ac7");
    assert_eq!(window.session_id, "sess-ac7");
    assert_eq!(window.status, "open");
    assert_eq!(
        window.pipeline_file_set,
        vec!["src/main.rs", "src/lib.rs"]
    );
    assert_eq!(window.subprojects, vec!["core"]);
    assert!(!window.drift_emitted);
}

/// Confirms that `amend_window_for_session` returns `None` for an unknown
/// session rather than an error — fail-open behaviour.
#[test]
fn amend_window_missing_session_returns_none() {
    let dir = tempdir().unwrap();
    let store = open_store(dir.path());
    let result = store.amend_window_for_session("nonexistent-session").unwrap();
    assert!(result.is_none(), "unknown session must return None, not Err");
}
