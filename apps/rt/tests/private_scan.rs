//! A private `scan --full` must leave a host repository's own instruction file
//! exactly as it found it.
//!
//! ## Why this file asks git instead of reading the code
//!
//! The claim is about what a REPOSITORY sees. Whether the Guards we write are
//! invisible depends on the clone-local exclude file, and whether the client's
//! own file survives depends on our never opening it — neither can be proven by
//! inspecting a constant. So the fixture seeds a real repository, commits a
//! subproject `CLAUDE.md` the way a consulting client already would, runs the
//! real pass, and asks git.
//!
//! The mode is never set here either: it is autodetected off the exclude file,
//! so the fixture makes the clone private the same way `run upsert --private`
//! does — by writing the footprint rules — and the pass reads them back.
//!
//! Both directions run in ONE test on purpose. A private-only assertion would
//! stay green if the pass had simply stopped writing Guards at all; the shared
//! half is the negative control that makes the private half mean something.

use std::path::Path;
use std::process::Command;

use mustard_core::domain::scan::Project;
use mustard_rt::commands::scan_claude::run_pass;

/// The client's own subproject instruction file, versioned before Mustard ever
/// arrived. Distinctive bytes, so "untouched" is a byte comparison, not a hope.
const HOST_MD: &str = "# Their subproject\n\n## Guards\n\n- The client's own rule.\n";

#[test]
fn ac5_private_scan_writes_local_guards_and_never_touches_claude_md() {
    // --- private: the Guards land beside their file, never in it -----------
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);
    let sub = root.join("apps").join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir sub");
    std::fs::write(sub.join("CLAUDE.md"), HOST_MD).expect("write host md");
    git(root, &["add", "apps/sub/CLAUDE.md"]);
    git(root, &["commit", "-m", "their instruction file"]);

    // Make the clone private exactly as a private install does — by writing the
    // footprint into the clone-local exclude file. Nothing else declares the
    // mode: there is no knob to set.
    let outcome = mustard_core::ensure_excluded(root, &mustard_core::footprint_paths());
    assert_eq!(outcome.unavailable, None, "the fixture needs a real repository: {outcome:?}");

    let result = run_pass(root, &[project("sub", "apps/sub")], true);

    // 1. Our Guards exist — on the local layer, with the import line intact.
    let local = sub.join("CLAUDE.local.md");
    let written = std::fs::read_to_string(&local).expect("Guards must land on the local layer");
    assert!(
        written.starts_with("@.claude/scan-map.md"),
        "the import resolves relative to its own file, so it rides the local layer unchanged: {written}",
    );
    assert!(written.contains("## Guards"), "no Guards section written: {written}");
    assert!(
        result.regenerated.iter().any(|p| p.ends_with("CLAUDE.local.md")),
        "the local layer must be reported: {:?}",
        result.regenerated,
    );

    // 2. The client's file was neither rewritten nor even reported.
    assert_eq!(
        std::fs::read_to_string(sub.join("CLAUDE.md")).ok().as_deref(),
        Some(HOST_MD),
        "a private pass must not touch the file the host repository versions",
    );
    assert!(
        !result.regenerated.iter().any(|p| p.ends_with("CLAUDE.md")),
        "no CLAUDE.md may be reported by a private pass: {:?}",
        result.regenerated,
    );

    // 3. The point of all of it: git has nothing to say about either file.
    let status = git_status(root);
    assert!(
        !status.contains("CLAUDE.md"),
        "the client's own file must not be reported modified: {status}",
    );
    assert!(
        !status.contains("CLAUDE.local.md"),
        "our Guards must be invisible to this clone's git: {status}",
    );

    // --- shared: the negative control ---------------------------------------
    // Same fixture, same pass, no exclude rules. If this half were missing, a
    // pass that had stopped writing Guards entirely would satisfy every
    // assertion above.
    let plain = tempfile::tempdir().expect("temp dir");
    let plain_root = plain.path();
    init_repo(plain_root);
    let plain_sub = plain_root.join("apps").join("sub");
    std::fs::create_dir_all(&plain_sub).expect("mkdir sub");
    std::fs::write(plain_sub.join("CLAUDE.md"), HOST_MD).expect("write host md");

    let shared = run_pass(plain_root, &[project("sub", "apps/sub")], true);

    assert!(
        !plain_sub.join("CLAUDE.local.md").exists(),
        "the local layer belongs to the private mode only",
    );
    let updated =
        std::fs::read_to_string(plain_sub.join("CLAUDE.md")).expect("the shared file is written");
    assert!(
        updated.starts_with("@.claude/scan-map.md"),
        "a shared pass still writes into CLAUDE.md, as it always did: {updated}",
    );
    assert!(
        shared.regenerated.iter().any(|p| p.ends_with("CLAUDE.md")),
        "the shared file must be reported: {:?}",
        shared.regenerated,
    );
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// One scan unit at `dir`, with the minimum a `--full` pass reads.
fn project(name: &str, dir: &str) -> Project {
    Project {
        name: name.into(),
        dir: dir.into(),
        kind: "rust".into(),
        code_files: 1,
        frameworks: Vec::new(),
        dependencies: Vec::new(),
        scripts: Vec::new(),
        detected_stacks: Vec::new(),
        own_git_root: false,
    }
}

/// A fresh repository with one commit — the state a host repo is in when the
/// operator installs.
///
/// `core.autocrlf` is pinned off for the reason the core's `private_install.rs`
/// pins it: line-ending translation would report a freshly committed file as
/// modified on a machine with the global setting on, turning an environment
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

/// The repository's dirt, trimmed. `--untracked-files=all` because the default
/// collapses a wholly untracked directory into ONE line, which would let an
/// unhidden file hide behind its parent's name.
///
/// The assertions it feeds are all NEGATIVE ("this name does not appear"), so a
/// measurement that did not happen would satisfy them silently. It is asserted
/// here instead: no status, no test.
fn git_status(root: &Path) -> String {
    let out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .expect("git status must run — the assertions below are negative");
    assert!(out.status.success(), "git status failed in {}", root.display());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
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
