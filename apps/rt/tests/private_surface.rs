//! `mustard-rt run upsert` — the footprint is invisible to the host repository,
//! unconditionally, and NO argv can ask for anything else.
//!
//! Driven through the real binary rather than the library, because the claim is
//! about the published CLI: a mode resolved in-process proves nothing about a
//! `dispatch` arm that was never wired, and the resolution lives inside that arm.
//!
//! The negative control is not a shared install any more — there is no argv that
//! produces one. It is the pair of assertions that the install really HAPPENED:
//! the seeds landed and the exclude file grew. Without them, a build that wrote
//! nothing at all would satisfy every "the visible file is absent" assertion
//! here, which is precisely how three defects in this unit shipped green.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn ac6_upsert_is_private_unconditionally_and_offers_no_switch() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);

    // --- run 1: a BARE run installs privately -------------------------------
    // There is no flag to ask for privacy, because there is nothing else to ask
    // for. The mode is not a choice the caller can express.
    let first = upsert(root, &[]);
    assert_eq!(first["private"], Value::Bool(true), "the run must declare itself private: {first}");
    assert!(
        root.join(".claude/settings.local.json").is_file(),
        "the harness settings belong on the untracked local layer",
    );
    assert!(
        !root.join(".claude/settings.json").exists(),
        "a private install must never create the file the host repository versions",
    );
    assert!(
        first["excluded"].as_array().is_some_and(|a| !a.is_empty()),
        "the footprint must reach the clone-local exclude file: {first}",
    );

    // --- run 2: NO flag, and the mode survives ------------------------------
    // The whole point of reading the exclude file instead of storing a setting:
    // a plain `run upsert` — the bootstrap door the plugin calls on its own —
    // must not quietly convert a private install back into a visible one.
    let second = upsert(root, &[]);
    assert_eq!(
        second["private"],
        Value::Bool(true),
        "a second run with no flag must still install privately: {second}",
    );
    assert!(
        !root.join(".claude/settings.json").exists(),
        "the autodetected run must not seed the shared settings file either",
    );
    assert_eq!(
        second["excluded"].as_array().map(Vec::len),
        None,
        "the exclude append converged, so the key is absent entirely: {second}",
    );

    // --- negative control A: the install really happened ---------------------
    // Every assertion above is of the form "the visible file is absent", and a
    // build that wrote NOTHING satisfies all of them. These two say the work was
    // actually done, so absence means refused rather than skipped.
    assert!(
        root.join("mustard.json").is_file(),
        "the install must really have run — the project config is its own evidence",
    );
    assert!(
        root.join(".claude/mustard/orchestrator.md").is_file(),
        "…and so is the injectable it seeds",
    );

    // --- negative control B: no argv reaches the other outcome ---------------
    // The mode is unconditional, so the surface must offer no way to ask for a
    // visible install — not the flag that used to exist, and not its opposite.
    for argv in [["--private"], ["--shared"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .arg("run")
            .arg("upsert")
            .args(argv)
            .current_dir(root)
            .env("CLAUDE_PROJECT_DIR", root)
            .output()
            .expect("run upsert");
        assert!(
            !out.status.success(),
            "`run upsert {}` must be rejected — the install mode is not a choice",
            argv[0],
        );
    }
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// Run `mustard-rt run upsert [args]` against `root` and parse its report.
///
/// `CLAUDE_PROJECT_DIR` is the resolution the `run` face documents for a project
/// with no anchor yet — the fresh-install path this test starts from. Exit 0 is
/// asserted because the `run` face never signals through the exit code: a
/// non-zero here would mean the process died before its own fail-open path.
///
/// `CLAUDE_CONFIG_DIR` points at a directory inside the fixture so the run's
/// last step — the plugin refresh — finds no registry and reports a skip. Left
/// unset, an `upsert` in a test would update the machine's REAL plugin install,
/// which is a side effect no test may have.
fn upsert(root: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .arg("upsert")
        .args(args)
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env("CLAUDE_CONFIG_DIR", root.join("claude-config"))
        .output()
        .expect("run the built binary");
    assert!(
        out.status.success(),
        "`run upsert {args:?}` exited non-zero: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("the run face must print one JSON report ({e}): {stdout}"))
}

/// A fresh repository with one commit — the state a host repo is in when the
/// operator installs. `core.autocrlf` is pinned off for the reason the core's
/// `private_install.rs` pins it: line-ending translation would turn a machine
/// preference into a failure of assertions that are about ignore rules.
fn init_repo(root: &Path) {
    git(root, &["init"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "core.autocrlf", "false"]);
    std::fs::write(root.join("README.md"), "host repo\n").expect("write README");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
}

/// Run a git command in `root`, asserting success — test scaffolding only.
fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}
