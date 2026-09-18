//! A `scan --full` pass leaves every instruction file of the host repository
//! exactly as it found it, private install or not.
//!
//! ## Why this file asks git instead of reading the code
//!
//! The claim is about what a REPOSITORY sees. The fixture seeds a real
//! repository, commits a subproject `CLAUDE.md` the way a consulting client
//! already would, runs the real pass, and asks git.
//!
//! The mode is never set here: it is autodetected off the exclude file, so the
//! fixture makes the clone private the same way `run upsert --private` does —
//! by writing the footprint rules — and the pass reads them back.
//!
//! Both directions run in ONE test on purpose: the map the pass writes must be
//! invisible to git in a private clone, and in neither clone may the pass write
//! a `CLAUDE.md` or a `CLAUDE.local.md`.

use std::path::Path;
use std::process::Command;

use mustard_core::domain::scan::Project;
use mustard_rt::commands::scan_claude::run_pass;

/// The client's own subproject instruction file, versioned before Mustard ever
/// arrived. Distinctive bytes, so "untouched" is a byte comparison, not a hope.
const HOST_MD: &str = "# Their subproject\n\n## Guards\n\n- The client's own rule.\n";

#[test]
fn a_scan_pass_writes_only_the_map_and_never_an_instruction_file() {
    // --- private: the map is written and git sees nothing ------------------
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    init_repo(root);
    let sub = root.join("apps").join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir sub");
    std::fs::write(sub.join("CLAUDE.md"), HOST_MD).expect("write host md");
    git(root, &["add", "apps/sub/CLAUDE.md"]);
    git(root, &["commit", "-m", "their instruction file"]);

    let outcome = mustard_core::ensure_excluded(root, &mustard_core::footprint_rules());
    assert_eq!(outcome.unavailable, None, "the fixture needs a real repository: {outcome:?}");

    let result = run_pass(root, &[project("sub", "apps/sub")], true);

    assert!(sub.join(".claude").join("scan-map.md").is_file(), "the map is written");
    assert!(!sub.join("CLAUDE.local.md").exists(), "no local layer is written");
    assert_eq!(
        std::fs::read_to_string(sub.join("CLAUDE.md")).ok().as_deref(),
        Some(HOST_MD),
        "the file the host repository versions is never touched",
    );
    assert!(
        result.regenerated.iter().all(|p| p.ends_with("scan-map.md")),
        "only maps are reported: {:?}",
        result.regenerated,
    );
    assert_eq!(git_status(root), "", "git has nothing to say after a private pass");

    // --- shared: the same pass, no exclude rules ----------------------------
    let plain = tempfile::tempdir().expect("temp dir");
    let plain_root = plain.path();
    init_repo(plain_root);
    let plain_sub = plain_root.join("apps").join("sub");
    std::fs::create_dir_all(&plain_sub).expect("mkdir sub");
    std::fs::write(plain_sub.join("CLAUDE.md"), HOST_MD).expect("write host md");

    let shared = run_pass(plain_root, &[project("sub", "apps/sub")], true);

    assert!(plain_sub.join(".claude").join("scan-map.md").is_file(), "the map is written");
    assert!(!plain_sub.join("CLAUDE.local.md").exists());
    assert_eq!(std::fs::read_to_string(plain_sub.join("CLAUDE.md")).ok().as_deref(), Some(HOST_MD));
    assert!(shared.regenerated.iter().all(|p| p.ends_with("scan-map.md")), "{:?}", shared.regenerated);
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
/// modified on a machine with the global setting on.
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
/// collapses a wholly untracked directory into ONE line.
fn git_status(root: &Path) -> String {
    let out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .expect("git status must run");
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
