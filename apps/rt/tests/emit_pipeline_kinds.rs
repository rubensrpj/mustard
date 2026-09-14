//! Integration test: the kinds `emit-pipeline` takes at the command line.
//!
//! - The legacy `pipeline.phase` only writes to the old log: TWO rows (the
//!   legacy event, tagged `legacy_alias=true`, plus the equivalent
//!   `pipeline.stage`), both sharing the same timestamp and session id.
//! - A kind that creates or advances a spec (`pipeline.stage`,
//!   `pipeline.status` and the others) is refused at the command line and
//!   sends to `open`: nothing is written, and no `meta.json` is born.
//!
//! Every run fixes `CLAUDE_PROJECT_DIR` on the temporary project and drops the
//! session variables, so nothing lands in the real project.

use mustard_core::domain::model::event::HarnessEvent;
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
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .output()
        .expect("run mustard-rt")
}

/// Read every NDJSON event under `<project>/.claude/spec/<spec>/.events/`
/// and return only the rows whose `spec` payload field matches `spec`.
fn events_for_spec(project: &Path, spec: &str) -> Vec<HarnessEvent> {
    mustard_core::view::projection::read_workspace_events(project)
        .into_iter()
        .filter(|e| e.spec.as_deref() == Some(spec))
        .collect()
}

/// The refusal a kind that creates or advances a spec gets at the command
/// line: exit 1, the short reason, no event and no `meta.json`.
fn assert_refused(project: &Path, out: &std::process::Output, spec: &str) {
    assert_eq!(out.status.code(), Some(1), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    let report: Value = serde_json::from_slice(&out.stdout).expect("a JSON refusal");
    assert_eq!(report["reason"], Value::String("pipeline-door-retired".into()), "{report}");
    assert!(events_for_spec(project, spec).is_empty(), "nothing is written");
    let meta = project.join(".claude").join("spec").join(spec).join("meta.json");
    assert!(!meta.exists(), "no meta.json is born");
}

#[test]
fn a_stage_move_is_refused_at_the_command_line() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "test-stage";
    let out = emit(project, "pipeline.stage", spec, r#"{"stage":"execute"}"#);
    assert_refused(project, &out, spec);
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

    let events = events_for_spec(project, spec);

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
    // The legacy phase only logs: it creates no `meta.json`.
    assert!(!project.join(".claude").join("spec").join(spec).join("meta.json").exists());
}

#[test]
fn a_status_move_is_refused_at_the_command_line() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "test-status";

    let spec_dir = project.join(".claude").join("spec").join(spec);
    std::fs::create_dir_all(&spec_dir).expect("spec dir");
    std::fs::write(spec_dir.join("spec.md"), "# t\n### Status: implementing\n").expect("write");

    let out = emit(project, "pipeline.status", spec, r#"{"to":"completed"}"#);
    assert_refused(project, &out, spec);
    let md = std::fs::read_to_string(spec_dir.join("spec.md")).expect("spec.md");
    assert_eq!(md, "# t\n### Status: implementing\n", "the old document stays");
}
