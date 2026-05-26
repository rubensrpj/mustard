//! Integration test for spec
//! `2026-05-19-dashboard-phase-from-sqlite` — Wave 1.
//!
//! Verifies that `specs_from_db` derives each `SpecRow.phase` from the most
//! recent `pipeline.phase` event for the spec (`payload.to`), not from the
//! `specs.phase` projection column. Event ordering follows insertion `id`
//! (the SQL uses `ORDER BY e.id DESC LIMIT 1`).
//!
//! Seeding uses raw `INSERT` statements against the schema rather than
//! `SqliteEventStore::append`, matching the convention of the sibling
//! `db_test.rs`: keeps the dashboard test-deps minimal (no `mustard-core`
//! dev-dependency) while exercising the exact query path the dashboard runs.

use mustard_dashboard_lib::db::specs_from_db;
use rusqlite::Connection;

/// Minimal subset of `sqlite_schema.sql` needed for `specs_from_db`. Mirrors
/// the schema in `tests/db_test.rs` — the FTS table and trigger are kept so
/// row inserts behave identically to the production store.
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
CREATE VIRTUAL TABLE events_fts USING fts5(event, spec, payload_text, content='events', content_rowid='id');
CREATE TRIGGER events_ai AFTER INSERT ON events BEGIN
  INSERT INTO events_fts(rowid, event, spec, payload_text) VALUES (new.id, new.event, new.spec, new.payload);
END;
CREATE TABLE specs (
  name TEXT PRIMARY KEY, status TEXT, phase TEXT, started_at TEXT, completed_at TEXT, affected_files TEXT
);
"#;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn
}

fn insert_spec(conn: &Connection, name: &str, projection_phase: Option<&str>) {
    conn.execute(
        "INSERT INTO specs (name, status, phase, started_at, completed_at, affected_files) \
         VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            name,
            "active",
            projection_phase,
            "2026-05-19T00:00:00Z",
            rusqlite::types::Null,
            "[]"
        ],
    )
    .unwrap();
}

fn insert_phase_event(conn: &Connection, spec: &str, to: &str, ts: &str) {
    let payload = format!(r#"{{"from":null,"to":"{}"}}"#, to);
    conn.execute(
        "INSERT INTO events (ts, session_id, spec, event, payload) VALUES (?,?,?,?,?)",
        rusqlite::params![ts, "s1", spec, "pipeline.phase", payload],
    )
    .unwrap();
}

#[test]
fn phase_comes_from_latest_pipeline_phase_event() {
    // Spec `demo` has only one pipeline.phase event → that's the phase.
    let conn = setup();
    insert_spec(&conn, "demo", None);
    insert_phase_event(&conn, "demo", "ANALYZE", "2026-05-19T10:00:00Z");

    let rows = specs_from_db(&conn).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "demo");
    assert_eq!(rows[0].phase.as_deref(), Some("ANALYZE"));
}

#[test]
fn phase_uses_freshest_event_by_id() {
    // Multiple transitions: the latest insertion wins, not lexical order on `to`.
    let conn = setup();
    insert_spec(&conn, "demo", None);
    insert_phase_event(&conn, "demo", "ANALYZE", "2026-05-19T10:00:00Z");
    insert_phase_event(&conn, "demo", "PLAN", "2026-05-19T11:00:00Z");
    insert_phase_event(&conn, "demo", "EXECUTE", "2026-05-19T12:00:00Z");

    let rows = specs_from_db(&conn).unwrap();
    let demo = rows.iter().find(|r| r.name == "demo").expect("demo row");
    assert_eq!(demo.phase.as_deref(), Some("EXECUTE"));
}

#[test]
fn phase_ignores_specs_projection_column() {
    // The `specs.phase` projection column is stale on purpose here — the
    // derived value (from the event) must win.
    let conn = setup();
    insert_spec(&conn, "demo", Some("CLOSE"));
    insert_phase_event(&conn, "demo", "ANALYZE", "2026-05-19T10:00:00Z");

    let rows = specs_from_db(&conn).unwrap();
    let demo = rows.iter().find(|r| r.name == "demo").expect("demo row");
    assert_eq!(demo.phase.as_deref(), Some("ANALYZE"));
}

#[test]
fn phase_is_none_when_no_pipeline_phase_event_recorded() {
    // No event recorded for this spec → phase = None, even if the projection
    // column happens to be populated by some other code path.
    let conn = setup();
    insert_spec(&conn, "lonely", Some("PLAN"));

    let rows = specs_from_db(&conn).unwrap();
    let lonely = rows
        .iter()
        .find(|r| r.name == "lonely")
        .expect("lonely row");
    assert!(lonely.phase.is_none(), "expected None, got {:?}", lonely.phase);
}

#[test]
fn phase_event_for_other_spec_does_not_leak() {
    // Events for other specs must not bleed into the queried spec's phase.
    let conn = setup();
    insert_spec(&conn, "demo", None);
    insert_spec(&conn, "neighbour", None);
    insert_phase_event(&conn, "neighbour", "EXECUTE", "2026-05-19T12:00:00Z");
    insert_phase_event(&conn, "demo", "ANALYZE", "2026-05-19T10:00:00Z");

    let rows = specs_from_db(&conn).unwrap();
    let demo = rows.iter().find(|r| r.name == "demo").expect("demo row");
    assert_eq!(demo.phase.as_deref(), Some("ANALYZE"));
    let neighbour = rows.iter().find(|r| r.name == "neighbour").expect("neighbour row");
    assert_eq!(neighbour.phase.as_deref(), Some("EXECUTE"));
}
