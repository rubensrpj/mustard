//! Integration test: `emit-pipeline` accepts the three W5 `hygiene.*` event
//! kinds (`spec-lifecycle-unification` Wave 5).
//!
//! These are first-class new kinds (no legacy alias), so each `emit-pipeline
//! --kind hygiene.*` writes exactly one row and exits 0. An unknown kind still
//! exits 1 (the validation contract is unchanged).

use mustard_core::projection::read_harness_events_from_ndjson_dir;
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
        .args(["run", "emit-pipeline", "--kind", kind, "--spec", spec, "--payload", payload])
        .current_dir(project)
        .env("CLAUDE_PROJECT_DIR", project.to_string_lossy().as_ref())
        .output()
        .expect("run mustard-rt")
}

#[test]
fn hygiene_kinds_are_accepted_and_write_single_rows() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec = "hygiene-kinds";

    for kind in ["hygiene.detected", "hygiene.autoclose", "hygiene.skipped"] {
        let out = emit(project, kind, spec, r#"{"spec":"hygiene-kinds"}"#);
        assert!(
            out.status.success(),
            "emit-pipeline --kind {kind} must exit 0. stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // W5: hygiene.* events route to per-spec NDJSON, not SQLite.
    let events_dir = project.join(".claude").join("spec").join(spec).join("events");
    let events = read_harness_events_from_ndjson_dir(&events_dir);
    for kind in ["hygiene.detected", "hygiene.autoclose", "hygiene.skipped"] {
        let n = events.iter().filter(|e| e.event == kind).count();
        assert_eq!(n, 1, "exactly one {kind} row (no alias fan-out)");
    }
}

#[test]
fn unknown_kind_still_rejected() {
    let tmp = project_dir();
    let out = emit(tmp.path(), "hygiene.bogus", "x", "null");
    assert!(
        !out.status.success(),
        "an unknown kind must still exit non-zero (validation contract)"
    );
}
