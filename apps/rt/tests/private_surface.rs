//! `mustard-rt run upsert --private` — the one door that chooses the footprint,
//! and the autodetection that means it is only ever typed once.
//!
//! Driven through the real binary rather than the library, because the claim is
//! about the published CLI: a flag that parses in-process proves nothing about a
//! variant whose `dispatch` arm was never wired, and the mode is resolved inside
//! that arm. Each run is a fresh process, which is also the only honest way to
//! prove the SECOND half — a same-process call could be answered by the cache the
//! first one warmed, so "autodetected" would really mean "remembered".

use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn ac6_upsert_accepts_private_flag_and_mode_is_autodetected() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);

    // --- run 1: the flag is accepted, and it installs privately -------------
    let first = upsert(root, &["--private"]);
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

    // --- the negative control ------------------------------------------------
    // Without the marks in its exclude file the same argv installs SHARED. If
    // this were missing, a build that hard-coded `private` would pass every
    // assertion above.
    let plain = tempfile::tempdir().expect("temp dir");
    let plain_root = plain.path();
    init_repo(plain_root);

    let shared = upsert(plain_root, &[]);
    assert_eq!(
        shared.get("private"),
        None,
        "a shared report carries no private key at all: {shared}",
    );
    assert!(
        plain_root.join(".claude/settings.json").is_file(),
        "a shared install seeds the file it always seeded",
    );
    assert!(
        !plain_root.join(".claude/settings.local.json").exists(),
        "the local layer belongs to the private mode only",
    );
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
fn upsert(root: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .arg("upsert")
        .args(args)
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
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
