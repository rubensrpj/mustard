//! Integration test: `complete-spec` emits a `qa.result` event via
//! `qa_run::run_for_spec` before marking the spec `completed`.
//!
//! Approach: seed a tempdir with a minimal `spec.md` containing one always-
//! passing AC, invoke `mustard-rt run complete-spec <spec>` as a subprocess
//! (the crate has no `lib.rs`), then query the SQLite event store and assert
//! that a `qa.result` event with `overall = "pass"` was emitted.

use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde_json::Value;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temp project dir with the standard harness subdirectory layout.
fn project_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join(".claude").join(".harness"))
        .expect("create .harness");
    dir
}

/// Seed a minimal `spec.md` for `spec_name` under
/// `.claude/spec/<spec_name>/spec.md` (flat layout, wave-2 of
/// `2026-05-21-flatten-spec-layout-and-multi-collab`).
///
/// The AC section contains one item whose command is `node -e "process.exit(0)"`,
/// which always exits 0 (pass) on any platform that has Node.js installed.
fn seed_spec(project: &Path, spec_name: &str) {
    let spec_dir = project
        .join(".claude")
        .join("spec")
        .join(spec_name);
    std::fs::create_dir_all(&spec_dir).expect("create spec dir");
    let content = format!(
        "# {spec_name}\n\
         ### Status: implementing\n\
         \n\
         ## Acceptance Criteria\n\
         \n\
         - [ ] AC-1: always passes — Command: `node -e \"process.exit(0)\"`\n"
    );
    std::fs::write(spec_dir.join("spec.md"), content).expect("write spec.md");
}

/// Run `mustard-rt run complete-spec <spec_name>` against `project_dir`.
///
/// Sets `CLAUDE_PROJECT_DIR` so the binary resolves the DB from the temp dir,
/// and changes its working directory to `project_dir` so `run_for_spec` picks
/// up the spec file via `find_spec_file`.
fn run_complete_spec(project_dir: &Path, spec_name: &str) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    std::process::Command::new(bin)
        .args(["run", "complete-spec", spec_name])
        .current_dir(project_dir)
        .env("CLAUDE_PROJECT_DIR", project_dir.to_string_lossy().as_ref())
        .output()
        .expect("run mustard-rt")
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[test]
fn complete_spec_emits_qa_result_event() {
    let tmp = project_dir();
    let project = tmp.path();
    let spec_name = "test-spec-qa-emit";

    seed_spec(project, spec_name);
    let out = run_complete_spec(project, spec_name);

    // The command must exit 0 regardless of QA outcome (fail-open contract).
    assert!(
        out.status.success(),
        "complete-spec must exit 0 (fail-open). stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // W5: qa.result lives in per-spec NDJSON, not SQLite.
    let events_dir = project
        .join(".claude")
        .join("spec")
        .join(spec_name)
        .join(".events");
    let events = read_harness_events_from_ndjson_dir(&events_dir);

    let qa_events: Vec<&mustard_core::domain::model::event::HarnessEvent> = events
        .iter()
        .filter(|e| e.event == "qa.result")
        .collect();

    assert!(
        !qa_events.is_empty(),
        "expected at least one qa.result event for spec={spec_name}; \
         got events: {:?}\nstdout: {}\nstderr: {}",
        events.iter().map(|e| &e.event).collect::<Vec<_>>(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    // The overall should be "pass" (the single AC always exits 0).
    let first = qa_events[0];
    let overall = first
        .payload
        .get("overall")
        .and_then(Value::as_str)
        .unwrap_or("missing");

    assert_eq!(
        overall, "pass",
        "qa.result overall should be 'pass'; payload: {}",
        first.payload
    );
}
