//! Integration test: `emit-pipeline` accepts the new canonical state-model
//! kinds and double-writes legacy aliases (Wave 2 of
//! `spec-lifecycle-unification`).
//!
//! - Emitting `pipeline.stage` directly writes exactly one row (no alias).
//! - Emitting the legacy `pipeline.phase` writes TWO rows (the legacy event,
//!   tagged `legacy_alias=true`, plus the equivalent `pipeline.stage`), both
//!   sharing the same timestamp + session id (AC-W2-6).

use mustard_core::store::sqlite_store::SqliteEventStore;
use serde_json::Value;
use std::path::Path;
use tempfile::TempDir;

fn project_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join(".claude").join(".harness")).expect("harness dir");
    dir
}

fn emit(project: &Path, kind: &str, spec: &str, payload: &str) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    std::process::Command::new(bin)
        .args([
            "run",
            "emit-pipeline",
            "--kind",
            kind,
            "--spec",
            spec,
            "--payload",
            payload,
        ])
        .current_dir(project)
        .env("CLAUDE_PROJECT_DIR", project.to_string_lossy().as_ref())
        .output()
        .expect("run mustard-rt")
}

#[test]
fn emit_new_stage_kind_writes_single_row() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "test-stage";

    let out = emit(project, "pipeline.stage", spec, r#"{"stage":"execute"}"#);
    assert!(
        out.status.success(),
        "emit must exit 0. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let store = SqliteEventStore::for_project(project).expect("open store");
    let events = store.query(Some(spec)).expect("query");

    let stage_rows: Vec<_> = events.iter().filter(|e| e.event == "pipeline.stage").collect();
    assert_eq!(stage_rows.len(), 1, "exactly one pipeline.stage row (no duplication)");
    assert_eq!(stage_rows[0].payload["stage"], Value::String("execute".into()));

    // A directly-emitted new kind must NOT produce a legacy alias.
    let outcome_rows = events.iter().filter(|e| e.event == "pipeline.outcome").count();
    assert_eq!(outcome_rows, 0, "no spurious alias for a direct new-kind emit");
}

#[test]
fn emit_legacy_phase_writes_legacy_and_new_rows_same_timestamp() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "test-phase";

    let out = emit(project, "pipeline.phase", spec, r#"{"phase":"execute","to":"execute"}"#);
    assert!(
        out.status.success(),
        "emit must exit 0. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let store = SqliteEventStore::for_project(project).expect("open store");
    let events = store.query(Some(spec)).expect("query");

    let phase_rows: Vec<_> = events.iter().filter(|e| e.event == "pipeline.phase").collect();
    let stage_rows: Vec<_> = events.iter().filter(|e| e.event == "pipeline.stage").collect();

    // Two equivalent rows: the legacy event + the new-kind alias.
    assert_eq!(phase_rows.len(), 1, "one legacy pipeline.phase row");
    assert_eq!(stage_rows.len(), 1, "one aliased pipeline.stage row");

    // The legacy event is tagged for audit.
    assert_eq!(
        phase_rows[0].payload["legacy_alias"],
        Value::Bool(true),
        "legacy event must carry legacy_alias=true"
    );
    // The alias forwards the transition target.
    assert_eq!(stage_rows[0].payload["stage"], Value::String("execute".into()));

    // Both rows share the same timestamp + session id (one transition).
    assert_eq!(
        phase_rows[0].ts, stage_rows[0].ts,
        "legacy + alias must share the same timestamp"
    );
    assert_eq!(
        phase_rows[0].session_id, stage_rows[0].session_id,
        "legacy + alias must share the same session id"
    );
}

#[test]
fn emit_legacy_status_terminal_aliases_to_outcome() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "test-status";

    // Seed the spec.md so the status-header sync (existing behavior) has a
    // file to find — irrelevant to the alias, but exercises the full path.
    let spec_dir = project.join(".claude").join("spec").join(spec);
    std::fs::create_dir_all(&spec_dir).expect("spec dir");
    std::fs::write(spec_dir.join("spec.md"), "# t\n### Status: implementing\n").expect("write");

    let out = emit(project, "pipeline.status", spec, r#"{"to":"completed"}"#);
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));

    let store = SqliteEventStore::for_project(project).expect("open store");
    let events = store.query(Some(spec)).expect("query");

    let status_rows: Vec<_> = events.iter().filter(|e| e.event == "pipeline.status").collect();
    let outcome_rows: Vec<_> = events.iter().filter(|e| e.event == "pipeline.outcome").collect();
    assert_eq!(status_rows.len(), 1);
    assert_eq!(outcome_rows.len(), 1, "terminal status aliases to pipeline.outcome");
    assert_eq!(outcome_rows[0].payload["outcome"], Value::String("completed".into()));
    assert_eq!(status_rows[0].payload["legacy_alias"], Value::Bool(true));
    assert_eq!(status_rows[0].ts, outcome_rows[0].ts);
}
