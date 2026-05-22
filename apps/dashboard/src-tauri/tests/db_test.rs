use rusqlite::Connection;
use mustard_dashboard_lib::db::{
    knowledge_from_db, metrics_from_db, recent_events_from_db, specs_from_db,
};

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
CREATE TABLE knowledge (
  id TEXT PRIMARY KEY, type TEXT, name TEXT, description TEXT,
  confidence REAL, created_at TEXT, updated_at TEXT, source TEXT
);
CREATE VIRTUAL TABLE knowledge_fts USING fts5(id UNINDEXED, name, description);
CREATE TABLE spans (
  trace_id TEXT, span_id TEXT PRIMARY KEY, parent_span_id TEXT, name TEXT,
  started_at INTEGER, ended_at INTEGER, duration_ms INTEGER, attributes TEXT,
  spec TEXT, phase TEXT, model TEXT, input_tokens INTEGER, output_tokens INTEGER, is_error INTEGER
);
"#;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    // 3 events; one is agent.start
    conn.execute(
        "INSERT INTO events (ts, session_id, event, payload) VALUES (?,?,?,?)",
        rusqlite::params![
            "2026-05-10T10:00:00Z",
            "s1",
            "agent.start",
            r#"{"summary":"first"}"#
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO events (ts, session_id, event, payload) VALUES (?,?,?,?)",
        rusqlite::params![
            "2026-05-11T10:00:00Z",
            "s1",
            "tool.use",
            r#"{"description":"second"}"#
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO events (ts, session_id, event, payload) VALUES (?,?,?,?)",
        rusqlite::params!["2026-05-12T10:00:00Z", "s2", "session.end", "{}"],
    )
    .unwrap();
    // 2 specs
    conn.execute(
        "INSERT INTO specs (name, status, phase, started_at, completed_at, affected_files) VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            "spec-a",
            "completed",
            "CLOSE",
            "2026-05-09",
            "2026-05-10",
            r#"["a.rs","b.rs"]"#
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO specs (name, status, phase, started_at, completed_at, affected_files) VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            "spec-b",
            "active",
            "EXECUTE",
            "2026-05-11",
            rusqlite::types::Null,
            r#"["c.rs"]"#
        ],
    )
    .unwrap();
    // 2 knowledge
    conn.execute(
        "INSERT INTO knowledge (id, type, name, description, confidence) VALUES (?,?,?,?,?)",
        rusqlite::params!["k1", "pattern", "Pat A", "desc A", 0.9],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO knowledge (id, type, name, description, confidence) VALUES (?,?,?,?,?)",
        rusqlite::params!["k2", "convention", "Conv B", "desc B", 0.5],
    )
    .unwrap();
    // 1 span — 100 + 200 tokens
    conn.execute(
        "INSERT INTO spans (span_id, name, started_at, input_tokens, output_tokens) VALUES (?,?,?,?,?)",
        rusqlite::params!["sp1", "test", 0i64, 100, 200],
    )
    .unwrap();
    conn
}

#[test]
fn metrics_counts_events_and_tokens() {
    let conn = setup();
    // Token totals now come from the dedicated telemetry.db (`run_usage`), not
    // the legacy in-memory `spans` table. With no telemetry store wired in this
    // unit test we pass `None`, so the token counters degrade to 0 (fail-soft);
    // the event/agent counts still come from the in-memory `events` table.
    let m = metrics_from_db(&conn, None).unwrap();
    assert_eq!(m.total_events, 3);
    assert_eq!(m.agents_dispatched, 1);
    assert_eq!(m.tokens_total, 0);
}

#[test]
fn knowledge_high_confidence() {
    let conn = setup();
    let k = knowledge_from_db(&conn).unwrap();
    assert_eq!(k.high_confidence_count, 1);
    assert_eq!(k.patterns_count, 1);
    assert_eq!(k.conventions_count, 1);
}

#[test]
fn specs_ordered_by_completion_desc() {
    let conn = setup();
    let s = specs_from_db(&conn).unwrap();
    assert_eq!(s.len(), 2);
    // spec-b (no completed_at, started 2026-05-11) should come before spec-a (completed 2026-05-10)
    assert_eq!(s[0].name, "spec-b");
    assert_eq!(s[1].name, "spec-a");
    assert_eq!(
        s[1].affected_files,
        vec!["a.rs".to_string(), "b.rs".to_string()]
    );
}

#[test]
fn recent_events_desc_order() {
    let conn = setup();
    let r = recent_events_from_db(&conn, 10).unwrap();
    assert_eq!(r.len(), 3);
    // newest first
    assert_eq!(r[0].ts.as_deref(), Some("2026-05-12T10:00:00Z"));
    assert_eq!(r[2].ts.as_deref(), Some("2026-05-10T10:00:00Z"));
}
