//! Integration tests for `pipeline_state_ingest` + `pipeline_state_for_spec` round-trip.
//!
//! AC-11 / AC-12 coverage:
//! - Happy path: write a sample JSON, run the ingest, query the event store,
//!   fold via `pipeline_state_for_spec`, assert the resulting `PipelineStateView`
//!   matches what the original JSON described.
//! - Missing-fields tolerance: a JSON with sparse fields ingests without panic.
//! - Malformed JSON: a bad file is pushed into `errors` and siblings are not aborted.
//!
//! Since `mustard-rt` is binary-only (no `lib.rs`), these tests call the binary
//! as a subprocess (via `CARGO_BIN_EXE_mustard-rt`) and query the SQLite store
//! through `mustard_core` directly for fold assertions.

use mustard_core::io::event_store::EventSink;
use mustard_core::io::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, SCHEMA_VERSION,
    EVENT_PIPELINE_SCOPE, EVENT_PIPELINE_STATUS, EVENT_PIPELINE_TASK_COMPLETE,
    EVENT_PIPELINE_TASK_DISPATCH, EVENT_PIPELINE_WAVE_COMPLETE,
};
use serde_json::{Value, json};
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Create a temp dir whose layout mimics a project root:
/// `{tmp}/.claude/.pipeline-states/` exists.
fn project_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join(".claude").join(".pipeline-states"))
        .expect("create .pipeline-states");
    dir
}

/// Open (or create) the harness SQLite store inside the temp project dir.
fn store_for(dir: &Path) -> SqliteEventStore {
    SqliteEventStore::for_project(dir).expect("open store")
}

/// Write a `.pipeline-states/{name}.json` file with the given JSON value.
fn write_state(dir: &Path, name: &str, value: &Value) {
    let path = dir
        .join(".claude")
        .join(".pipeline-states")
        .join(format!("{name}.json"));
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).expect("write state");
}

/// Run `mustard-rt run pipeline-state-ingest [--delete]` against `dir` and
/// return the parsed JSON output. Panics on binary execution failure.
fn run_ingest(dir: &Path, delete: bool) -> Value {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let mut cmd = std::process::Command::new(bin);
    cmd.args(["run", "pipeline-state-ingest"]);
    if delete {
        cmd.arg("--delete");
    }
    cmd.env("CLAUDE_PROJECT_DIR", dir.to_string_lossy().as_ref());
    let out = cmd.output().expect("run mustard-rt");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| json!({ "parse_error": e.to_string(), "raw": stdout.as_ref() }))
}

/// Minimal helper to build a `HarnessEvent` for direct store inserts.
fn pipeline_ev(event: &str, spec: &str, payload: Value, ts: &str) -> HarnessEvent {
    HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.to_string(),
        session_id: "test".to_string(),
        wave: 0,
        actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
        event: event.to_string(),
        payload,
        spec: Some(spec.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Test 1 — happy path: full pipeline-state JSON → events → store round-trip
// ---------------------------------------------------------------------------

#[test]
fn happy_path_round_trip() {
    let tmp = project_dir();
    let dir = tmp.path();

    let state = json!({
        "specName": "test-spec-happy",
        "status": "active",
        "scope": "full",
        "lang": "en",
        "model": "opus",
        "isWavePlan": true,
        "totalWaves": 3,
        "tasks": [
            {
                "name": "Wave 1: implement store",
                "agent": "general-purpose",
                "wave": 1,
                "status": "completed",
                "files": ["apps/rt/src/run/emit_pipeline.rs"]
            },
            {
                "name": "Wave 2: projections",
                "agent": "general-purpose",
                "wave": 2,
                "status": "pending",
                "files": []
            }
        ],
        "completedWaves": [1],
        "updatedAt": "2026-05-19T10:00:00.000Z",
        "createdAt": "2026-05-19T09:00:00.000Z"
    });
    write_state(dir, "test-spec-happy", &state);

    let result = run_ingest(dir, false);
    assert_eq!(result["ingested"], json!(1), "expected 1 ingested: {result}");
    assert_eq!(result["deleted"], json!(0), "no deletes without --delete");
    let errors = result["errors"].as_array().expect("errors array");
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");

    // Open the store and verify events were written for the spec.
    let store = store_for(dir);
    let events = store.query(Some("test-spec-happy")).expect("query");
    assert!(!events.is_empty(), "expected events in store for test-spec-happy");

    // Verify scope event present.
    let scope_ev = events.iter().find(|e| e.event == EVENT_PIPELINE_SCOPE);
    assert!(scope_ev.is_some(), "pipeline.scope event must be present");
    let scope_payload = &scope_ev.unwrap().payload;
    assert_eq!(scope_payload["scope"], json!("full"), "scope must be full");
    assert_eq!(scope_payload["lang"], json!("en"), "lang must be en");
    assert_eq!(scope_payload["model"], json!("opus"), "model must be opus");
    assert_eq!(scope_payload["isWavePlan"], json!(true), "isWavePlan must be true");
    assert_eq!(scope_payload["totalWaves"], json!(3), "totalWaves must be 3");

    // Verify status event.
    let status_ev = events.iter().find(|e| e.event == EVENT_PIPELINE_STATUS);
    assert!(status_ev.is_some(), "pipeline.status event must be present");
    assert_eq!(status_ev.unwrap().payload["to"], json!("active"));

    // Verify task dispatch events (one per task).
    let dispatch_evs: Vec<_> = events.iter().filter(|e| e.event == EVENT_PIPELINE_TASK_DISPATCH).collect();
    assert_eq!(dispatch_evs.len(), 2, "must have 2 task dispatch events");

    // Verify task complete event (only for completed task).
    let complete_evs: Vec<_> = events.iter().filter(|e| e.event == EVENT_PIPELINE_TASK_COMPLETE).collect();
    assert_eq!(complete_evs.len(), 1, "must have 1 task complete event");
    assert_eq!(complete_evs[0].payload["name"], json!("Wave 1: implement store"));

    // Verify wave complete event.
    let wave_evs: Vec<_> = events.iter().filter(|e| e.event == EVENT_PIPELINE_WAVE_COMPLETE).collect();
    assert_eq!(wave_evs.len(), 1, "must have 1 wave complete event");
    assert_eq!(wave_evs[0].payload["wave"], json!(1));

    // Verify timestamps are preserved from updatedAt (not "now").
    for ev in &events {
        assert_eq!(
            ev.ts, "2026-05-19T10:00:00.000Z",
            "event timestamp must match updatedAt: event={}", ev.event
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2 — missing-fields tolerance: sparse JSON ingests without panic
// ---------------------------------------------------------------------------

#[test]
fn sparse_json_ingest_tolerates_missing_fields() {
    let tmp = project_dir();
    let dir = tmp.path();

    // Minimal: only specName + status, no tasks, no completedWaves.
    let state = json!({
        "specName": "test-spec-sparse",
        "status": "approved"
    });
    write_state(dir, "test-spec-sparse", &state);

    let result = run_ingest(dir, false);
    assert_eq!(result["ingested"], json!(1), "sparse JSON must ingest: {result}");
    let errors = result["errors"].as_array().expect("errors array");
    assert!(errors.is_empty(), "no errors expected for sparse JSON: {errors:?}");

    let store = store_for(dir);
    let events = store.query(Some("test-spec-sparse")).expect("query");
    // At minimum, pipeline.status must have been emitted.
    let status_ev = events.iter().find(|e| e.event == EVENT_PIPELINE_STATUS);
    assert!(status_ev.is_some(), "pipeline.status must be emitted for sparse spec");
    assert_eq!(status_ev.unwrap().payload["to"], json!("approved"));

    // No tasks, no waves.
    assert!(!events.iter().any(|e| e.event == EVENT_PIPELINE_TASK_DISPATCH));
    assert!(!events.iter().any(|e| e.event == EVENT_PIPELINE_WAVE_COMPLETE));
}

// ---------------------------------------------------------------------------
// Test 3 — malformed JSON: bad file pushed to errors; siblings unaffected
// ---------------------------------------------------------------------------

#[test]
fn malformed_json_skipped_without_aborting_siblings() {
    let tmp = project_dir();
    let dir = tmp.path();

    // Write a valid sibling first (sorts before "zzzz-bad").
    let valid = json!({
        "specName": "test-spec-sibling",
        "status": "active"
    });
    write_state(dir, "aaaa-sibling", &valid);

    // Write a malformed file that sorts after it.
    let bad_path = dir
        .join(".claude")
        .join(".pipeline-states")
        .join("zzzz-bad.json");
    std::fs::write(&bad_path, "{ this is not json !!").expect("write bad file");

    let result = run_ingest(dir, false);

    // The valid sibling must be ingested.
    assert_eq!(result["ingested"], json!(1), "valid sibling must be ingested: {result}");

    // The malformed file must appear in errors.
    let errors = result["errors"].as_array().expect("errors array");
    assert!(!errors.is_empty(), "malformed file must produce an error");
    let bad_error = errors.iter().find(|e| {
        e.get("file")
            .and_then(Value::as_str)
            .map(|f| f.contains("zzzz-bad"))
            .unwrap_or(false)
    });
    assert!(bad_error.is_some(), "error for zzzz-bad.json must be present: {errors:?}");
}

// ---------------------------------------------------------------------------
// Test 4 — metrics.json files are excluded from ingest
// ---------------------------------------------------------------------------

#[test]
fn metrics_json_files_are_excluded() {
    let tmp = project_dir();
    let dir = tmp.path();

    // Write only a metrics file — no regular state files.
    let metrics = json!({ "specName": "should-not-ingest" });
    let metrics_path = dir
        .join(".claude")
        .join(".pipeline-states")
        .join("2026-01-01-test.metrics.json");
    std::fs::write(&metrics_path, serde_json::to_string(&metrics).unwrap())
        .expect("write metrics");

    let result = run_ingest(dir, false);
    assert_eq!(result["ingested"], json!(0), "metrics.json must be excluded: {result}");
    let errors = result["errors"].as_array().expect("errors array");
    assert!(errors.is_empty(), "no errors expected when only metrics files present");
}

// ---------------------------------------------------------------------------
// Test 5 — --delete flag removes successfully-ingested files
// ---------------------------------------------------------------------------

#[test]
fn delete_flag_removes_ingested_file() {
    let tmp = project_dir();
    let dir = tmp.path();

    let state = json!({
        "specName": "test-spec-delete",
        "status": "active"
    });
    write_state(dir, "test-spec-delete", &state);

    let path = dir
        .join(".claude")
        .join(".pipeline-states")
        .join("test-spec-delete.json");
    assert!(path.exists(), "file must exist before ingest");

    let result = run_ingest(dir, true);
    assert_eq!(result["ingested"], json!(1), "must ingest: {result}");
    assert_eq!(result["deleted"], json!(1), "must delete: {result}");
    assert!(!path.exists(), "file must be removed after --delete ingest");
}

// ---------------------------------------------------------------------------
// Test 6 — pipeline.scope event not emitted when scope fields absent
// ---------------------------------------------------------------------------

#[test]
fn scope_event_absent_when_no_scope_fields() {
    let tmp = project_dir();
    let dir = tmp.path();

    // No scope, lang, model, isWavePlan, or totalWaves.
    let state = json!({
        "specName": "test-spec-no-scope",
        "status": "active"
    });
    write_state(dir, "test-spec-no-scope", &state);

    let result = run_ingest(dir, false);
    assert_eq!(result["ingested"], json!(1), "{result}");

    let store = store_for(dir);
    let events = store.query(Some("test-spec-no-scope")).expect("query");
    // pipeline.scope should NOT be emitted when none of the fields are present.
    let scope_ev = events.iter().find(|e| e.event == EVENT_PIPELINE_SCOPE);
    assert!(scope_ev.is_none(), "pipeline.scope must not be emitted when no scope fields present");
}

// ---------------------------------------------------------------------------
// Test 7 — empty .pipeline-states dir → ingested:0 errors:[]
// ---------------------------------------------------------------------------

#[test]
fn empty_pipeline_states_dir_returns_zero() {
    let tmp = project_dir();
    let dir = tmp.path();
    // Directory exists but is empty — no JSON files.
    let result = run_ingest(dir, false);
    assert_eq!(result["ingested"], json!(0), "{result}");
    assert_eq!(result["deleted"], json!(0), "{result}");
    let errors = result["errors"].as_array().expect("errors array");
    assert!(errors.is_empty(), "no errors for empty dir: {errors:?}");
}
