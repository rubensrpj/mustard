//! Integration test for Wave 3b of spec
//! `2026-05-19-pipeline-state-from-sqlite`.
//!
//! Verifies that `pipelines_from_db` and `active_pipelines_from_db` correctly
//! fold the pipeline.* event stream into `PipelineSummary` / `ActivePipeline`
//! records. Seeds via raw SQL inserts (no mustard-core dependency) so the
//! dashboard's test deps stay minimal — same convention as `db_test.rs` and
//! `specs_phase_from_events_test.rs`.

use mustard_dashboard_lib::db::{active_pipelines_from_db, pipelines_from_db};
use rusqlite::Connection;

/// Minimal schema subset needed for the pipeline aggregation queries.
/// Must match the full production schema for the columns we touch.
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

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn
}

fn insert_event(conn: &Connection, spec: &str, event: &str, payload: &str, ts: &str) {
    conn.execute(
        "INSERT INTO events (ts, session_id, spec, event, payload) VALUES (?,?,?,?,?)",
        rusqlite::params![ts, "s-demo", spec, event, payload],
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Scenario: spec "demo" with a full pipeline sequence matching the spec AC.
// Events:
//   pipeline.scope    → scope=full, lang=pt, model=opus, is_wave_plan=true, total_waves=3
//   pipeline.status   → to="approved"
//   pipeline.task.dispatch → name="t1", agent="rt-impl", wave=1
//   pipeline.task.complete → name="t1", wave=1
//   pipeline.wave.complete → wave=1
//   pipeline.status   → to="implementing"
// ---------------------------------------------------------------------------

fn seed_demo(conn: &Connection) {
    insert_event(
        conn, "demo", "pipeline.scope",
        r#"{"scope":"full","lang":"pt","model":"opus","isWavePlan":true,"totalWaves":3}"#,
        "2026-05-20T10:00:00Z",
    );
    insert_event(
        conn, "demo", "pipeline.status",
        r#"{"to":"approved"}"#,
        "2026-05-20T10:01:00Z",
    );
    insert_event(
        conn, "demo", "pipeline.task.dispatch",
        r#"{"name":"t1","agent":"rt-impl","wave":1}"#,
        "2026-05-20T10:02:00Z",
    );
    insert_event(
        conn, "demo", "pipeline.task.complete",
        r#"{"name":"t1","wave":1}"#,
        "2026-05-20T10:03:00Z",
    );
    insert_event(
        conn, "demo", "pipeline.wave.complete",
        r#"{"wave":1}"#,
        "2026-05-20T10:04:00Z",
    );
    insert_event(
        conn, "demo", "pipeline.status",
        r#"{"to":"implementing"}"#,
        "2026-05-20T10:05:00Z",
    );
}

// ---------------------------------------------------------------------------
// pipelines_from_db tests
// ---------------------------------------------------------------------------

#[test]
fn pipelines_from_db_returns_one_summary() {
    let conn = setup();
    seed_demo(&conn);
    let summaries = pipelines_from_db(&conn);
    assert_eq!(summaries.len(), 1, "expected exactly 1 PipelineSummary");
    let s = &summaries[0];
    assert_eq!(s.spec_name, "demo");
    assert_eq!(s.status, "implementing");
    assert_eq!(s.scope, "full");
}

#[test]
fn pipelines_from_db_empty_when_no_events() {
    let conn = setup();
    let summaries = pipelines_from_db(&conn);
    assert!(summaries.is_empty(), "expected empty when no pipeline events");
}

#[test]
fn pipelines_from_db_multiple_specs_independent() {
    let conn = setup();
    seed_demo(&conn);
    // Add a second spec with only a status event.
    insert_event(
        &conn, "other-spec", "pipeline.status",
        r#"{"to":"completed"}"#,
        "2026-05-20T11:00:00Z",
    );
    let summaries = pipelines_from_db(&conn);
    assert_eq!(summaries.len(), 2);
    let demo = summaries.iter().find(|s| s.spec_name == "demo").expect("demo");
    let other = summaries.iter().find(|s| s.spec_name == "other-spec").expect("other");
    assert_eq!(demo.status, "implementing");
    assert_eq!(other.status, "completed");
}

// ---------------------------------------------------------------------------
// active_pipelines_from_db tests
// ---------------------------------------------------------------------------

#[test]
fn active_pipelines_wave_and_task_counts() {
    let conn = setup();
    seed_demo(&conn);

    // Use a far-future now_secs so the dispatch-failure TTL never triggers.
    let now_secs = u64::MAX / 2;
    let pipelines = active_pipelines_from_db(&conn, now_secs);
    assert_eq!(pipelines.len(), 1);
    let p = &pipelines[0];

    assert_eq!(p.spec_name, "demo");
    assert_eq!(p.status, "implementing");
    // wave 1 completed → current_wave = 2
    assert_eq!(p.current_wave, Some(2));
    // total_waves not carried by pipeline.scope in this seed (totalWaves key mismatch check below)
    // tasks: t1 dispatched+completed → tasks_completed = 1
    assert_eq!(p.tasks_completed, 1);
    assert_eq!(p.tasks_pending, 0);
    assert!(!p.has_dispatch_failure);
}

#[test]
fn active_pipelines_total_waves_from_scope() {
    let conn = setup();
    // Emit scope with camelCase key (totalWaves) that the fold checks.
    insert_event(
        &conn, "spec-tw", "pipeline.scope",
        r#"{"scope":"full","totalWaves":5}"#,
        "2026-05-20T10:00:00Z",
    );
    insert_event(
        &conn, "spec-tw", "pipeline.status",
        r#"{"to":"active"}"#,
        "2026-05-20T10:01:00Z",
    );
    let pipelines = active_pipelines_from_db(&conn, u64::MAX / 2);
    let p = pipelines.iter().find(|p| p.spec_name == "spec-tw").expect("spec-tw");
    assert_eq!(p.total_waves, Some(5));
}

#[test]
fn active_pipelines_completed_status_not_filtered_by_function() {
    // active_pipelines_from_db does NOT filter — the caller (dashboard_active_pipelines)
    // does. Verify "completed" status is still present in the raw output.
    let conn = setup();
    insert_event(
        &conn, "done-spec", "pipeline.status",
        r#"{"to":"completed"}"#,
        "2026-05-20T12:00:00Z",
    );
    let pipelines = active_pipelines_from_db(&conn, u64::MAX / 2);
    let p = pipelines.iter().find(|p| p.spec_name == "done-spec").expect("done-spec");
    assert_eq!(p.status, "completed");
}

#[test]
fn active_pipelines_stale_dispatch_failure_cleared() {
    let conn = setup();
    // Use a timestamp far in the past (2020-01-01) to guarantee staleness.
    insert_event(
        &conn, "spec-fail", "pipeline.dispatch_failure",
        r#"{"reason":"timeout","at":"2020-01-01T00:00:00.000Z"}"#,
        "2020-01-01T00:00:00Z",
    );
    insert_event(
        &conn, "spec-fail", "pipeline.status",
        r#"{"to":"paused"}"#,
        "2020-01-01T00:00:01Z",
    );
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let pipelines = active_pipelines_from_db(&conn, now_secs);
    let p = pipelines.iter().find(|p| p.spec_name == "spec-fail").expect("spec-fail");
    assert!(!p.has_dispatch_failure, "stale failure should be cleared");
    assert!(p.failure_age_ms.is_none());
}

#[test]
fn active_pipelines_fresh_dispatch_failure_preserved() {
    let conn = setup();
    // Emit a failure with a very recent timestamp (2099 is always in the future
    // relative to now_secs computed below, so age = 0).
    insert_event(
        &conn, "spec-fresh", "pipeline.dispatch_failure",
        r#"{"reason":"budget exceeded","at":"2099-01-01T00:00:00.000Z"}"#,
        "2099-01-01T00:00:00Z",
    );
    insert_event(
        &conn, "spec-fresh", "pipeline.status",
        r#"{"to":"implementing"}"#,
        "2099-01-01T00:00:01Z",
    );
    // now_secs = 0 forces age = 0 (saturating_sub), which is < TTL → keep.
    let pipelines = active_pipelines_from_db(&conn, 0);
    let p = pipelines.iter().find(|p| p.spec_name == "spec-fresh").expect("spec-fresh");
    assert!(p.has_dispatch_failure, "fresh failure should be preserved");
}
