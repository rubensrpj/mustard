// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration test for the `plan_approval_observer` hook module.
//!
//! Drives `mustard-rt on PostToolUse` as a subprocess with an `ExitPlanMode`
//! payload against a `.claude/` fixture holding a Full spec in PLAN, and
//! asserts the `<spec>/.approved-by-user` marker appears on approval — and
//! does NOT appear on the rejection shape. End-to-end equivalent of the
//! in-module unit tests in `apps/rt/src/hooks/observe/plan_approval_observer.rs`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Drive `mustard-rt on PostToolUse` with an ExitPlanMode payload rooted at
/// `cwd`. Asserts a clean (exit 0) fail-open dispatch.
fn fire_exit_plan_mode(cwd: &Path, session: &str, tool_response: serde_json::Value) {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let input = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "ExitPlanMode",
        "tool_input": { "plan": "# The plan" },
        "tool_response": tool_response,
        "session_id": session,
        "cwd": cwd.to_str().unwrap()
    });
    let mut child = Command::new(bin)
        .args(["on", "PostToolUse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mustard-rt");
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        let _ = write!(stdin, "{input}");
    }
    let status = child.wait().expect("wait");
    assert_eq!(
        status.code(),
        Some(0),
        "mustard-rt PostToolUse must exit 0 (fail-open)"
    );
}

/// Seed a Full spec in PLAN and bind the session to it.
fn seed_full_plan_spec(project: &Path, session: &str, spec: &str) {
    let spec_dir = project.join(".claude").join("spec").join(spec);
    fs::create_dir_all(&spec_dir).unwrap();
    fs::write(
        spec_dir.join("meta.json"),
        format!(r#"{{"scope":"full (wave plan)","stage":"Plan","outcome":"Active"}}"#),
    )
    .unwrap();
    let session_dir = project.join(".claude").join(".session").join(session);
    fs::create_dir_all(&session_dir).unwrap();
    fs::write(session_dir.join("active-spec"), spec).unwrap();
}

fn marker(project: &Path, spec: &str) -> std::path::PathBuf {
    project
        .join(".claude")
        .join("spec")
        .join(spec)
        .join(".approved-by-user")
}

#[test]
fn approved_exit_plan_mode_mints_the_marker_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    seed_full_plan_spec(project, "s-plan", "epic");

    fire_exit_plan_mode(
        project,
        "s-plan",
        serde_json::json!({ "plan": "# Approved plan body" }),
    );

    let m = marker(project, "epic");
    assert!(m.exists(), "the approval marker must be minted");
    let body = fs::read_to_string(&m).unwrap();
    assert!(body.contains("spec=epic"), "spec recorded: {body}");
    assert!(body.contains("via=ExitPlanMode"), "provenance recorded: {body}");
    assert!(body.contains("session=s-plan"), "session recorded: {body}");
}

#[test]
fn rejected_exit_plan_mode_mints_nothing_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    seed_full_plan_spec(project, "s-rej", "epic");

    // The observed rejection wire shape: a bare string tool_response.
    fire_exit_plan_mode(project, "s-rej", serde_json::json!("User rejected tool use"));

    assert!(
        !marker(project, "epic").exists(),
        "a rejection must never mint the marker"
    );
}
