//! AC-8 — a private install leaves a host repository with NOTHING to report,
//! and the client's own instruction file byte-identical.
//!
//! ## Why this file asks git instead of reading a rule list
//!
//! Every claim here is about what a REPOSITORY sees. Whether a rule hides a path
//! depends on where the rule file sits, on whether the pattern carries a slash,
//! and on whether the path is already in the index — none of which can be read
//! off a constant. So this seeds a real repository in a temporary directory, runs
//! the real install, writes what the harness really writes, and asks git. Same
//! shape as `seeded_ignore.rs`, for the same reason.
//!
//! The paths are not this file's invention either. The seeds come from the
//! install's own report — the engine says what it wrote — and the footprint the
//! negative control looks for is read back from
//! [`mustard_core::footprint_pathspecs`], one projection of the single
//! declaration wave 1 introduced.
//! A list retyped here could only ever prove the rules cover what the test author
//! already thought of, which is exactly how the previous version of this class of
//! test passed while seven runtime paths stayed visible.
//!
//! ## What the fixture is, and why each part of it is there
//!
//! A host repository the operator does NOT own:
//!
//! - it versions its own `CLAUDE.md` inside a subproject, committed with CRLF
//!   terminators. This is the case a clone-local exclude rule cannot cover (git
//!   applies ignore rules only to untracked paths) and the reason the private
//!   mode writes `CLAUDE.local.md` BESIDE that file instead of into it;
//! - that subproject also carries its own `.claude/`, because a full scan creates
//!   one per subproject. A rule containing a slash is anchored to the repository
//!   root, so `.claude/settings.json` never reaches a subproject's copy — without
//!   a fixture at depth this criterion cannot see that class of defect at all;
//! - a `.claude/` from an earlier install, plus the `.claude.backup.<stamp>/`
//!   copy that `mustard init`'s "backup and overwrite" choice leaves behind. The
//!   backup is seeded rather than produced because that choice is INTERACTIVE:
//!   with stdin not a terminal — a test, CI, the Tauri backend — `init` takes the
//!   merge branch and never writes one. Placing the directory the branch leaves
//!   is what puts it under the assertion below, which is the only thing that
//!   verifies it: a backup sits outside `.claude/`, so nothing that covers the
//!   harness's own directory reaches it.
//!
//! ## The negative control is half the proof
//!
//! A test that reported a clean tree because nothing had been written at all
//! would pass this criterion while the feature was entirely broken. So the same
//! fixture also receives a SHARED install, and git is required to SEE that
//! footprint — every entry of it that really landed on disk.

use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::{footprint_pathspecs, upsert_project, InstallMode, UpsertReport};

/// The subproject whose own `CLAUDE.md` the host repository versions. Nested one
/// level down so the fixture really exercises depth, which is where a
/// root-anchored rule stops.
const SUBPROJECT: &str = "packages/api";

/// The client's own Guards, committed with CRLF terminators. A private install
/// must return these bytes unchanged — a silently normalised line ending is a
/// diff in the client's repository just as surely as a new file is.
const HOST_GUARDS: &str = "# API\r\n\r\n## Guards\r\n\r\n- the client's own rule\r\n";

/// A `.claude.backup.<stamp>/` left by an earlier interactive install.
const BACKUP_DIR: &str = ".claude.backup.20260817-101500";

// ---------------------------------------------------------------------------
// AC-8
// ---------------------------------------------------------------------------

#[test]
fn ac8_host_repo_stays_clean_and_untouched() {
    let work = tempfile::tempdir().expect("temp dir");

    // --- THE CRITERION -----------------------------------------------------
    let private = host_repo(work.path(), "private-host");
    let host_md = private.join(SUBPROJECT).join("CLAUDE.md");
    let before = std::fs::read(&host_md).expect("the client's file is readable");

    let report = upsert_project(&private, Some("9.9.9"), InstallMode::Private).expect("upsert");
    assert!(report.private, "the fixture must really have installed privately: {report:?}");
    assert_eq!(report.exclude_unavailable, None, "git was available: {report:?}");
    write_harness_output(&private, InstallMode::Private);

    assert_eq!(
        git_status(&private),
        "",
        "a private install plus a full scan plus a spec directory must leave the \
         host repository with nothing to stage, nothing to diff and nothing to push",
    );
    assert_eq!(
        std::fs::read(&host_md).ok().as_deref(),
        Some(before.as_slice()),
        "the client's own {SUBPROJECT}/CLAUDE.md must come back byte-identical — \
         content and line terminators alike",
    );

    // The other side of "leaves no trace": the rules must hide OUR footprint and
    // nothing else. This assertion cannot be folded into the one above — an
    // over-broad rule makes an EMPTY status more likely, not less, so a clean
    // tree is exactly what a client-swallowing install also produces. What
    // separates them is whether files the operator writes FOR the client, after
    // the install, are still visible to that client's own `git add -A`.
    write(&private.join("CLAUDE.md"), b"# the client's own root rules\n");
    write(&private.join(SUBPROJECT).join("mustard.json"), b"{}\n");
    write(&private.join("services/billing/CLAUDE.md"), b"# their other subproject\n");
    let visible = git_status(&private);
    let sub_config = format!("{SUBPROJECT}/mustard.json");
    for theirs in ["CLAUDE.md", sub_config.as_str(), "services/billing/CLAUDE.md"] {
        assert!(
            visible.contains(theirs),
            "the install hid {theirs}, which it never wrote: {visible:?}",
        );
    }

    // --- THE NEGATIVE CONTROL ----------------------------------------------
    // The same fixture, installed SHARED. Without this a green run above would
    // be indistinguishable from an install that wrote nothing at all.
    let shared = host_repo(work.path(), "shared-host");
    let report = upsert_project(&shared, Some("9.9.9"), InstallMode::Shared).expect("upsert");
    write_harness_output(&shared, InstallMode::Shared);
    let status = git_status(&shared);

    let seeds = footprint_on_disk(&shared, &report);
    assert!(
        seeds.len() >= 4,
        "the control measured almost nothing ({seeds:?}) — a shared install writes \
         four seeds, so the comparison below would be proving very little",
    );
    for path in &seeds {
        assert!(
            status.contains(path.as_str()),
            "a shared install is versionable, so git must report {path}: {status:?}",
        );
    }
    for path in [
        format!("{SUBPROJECT}/.claude/scan-map.md"),
        format!("{SUBPROJECT}/CLAUDE.md"),
        ".claude/spec/demo/spec.md".to_string(),
        ".claude/spec/demo/qa/report.md".to_string(),
        format!("{BACKUP_DIR}/settings.json"),
    ] {
        assert!(
            status.contains(path.as_str()),
            "…and so is what the harness writes while it works: {path} is missing \
             from {status:?}",
        );
    }
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// A host repository as the operator finds it: the client's own code and Guards
/// committed, plus the residue of an earlier, non-private Mustard install.
///
/// `core.autocrlf` is pinned off for the same reason `seeded_ignore.rs` pins it:
/// line-ending translation would report the freshly committed CRLF file as
/// modified on a machine with the global setting on, turning an environment
/// preference into a failure of assertions that are about ignore rules.
fn host_repo(work: &Path, name: &str) -> PathBuf {
    let root = work.join(name);
    write(&root.join("README.md"), b"host repo\n");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "core.autocrlf", "false"]);

    // The client's own code and their own Guards — versioned before Mustard ever
    // arrived, and at a subproject path, where a root-anchored rule cannot reach.
    write(&root.join(SUBPROJECT).join("src/lib.rs"), b"pub fn theirs() {}\n");
    write(&root.join(SUBPROJECT).join("CLAUDE.md"), HOST_GUARDS.as_bytes());
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-m", "the client's own repository"]);
    assert_eq!(git_status(&root), "", "the fixture starts clean");

    // The state that makes `mustard init` take its backup-and-overwrite branch:
    // a `.claude/` from an earlier shared install, and the timestamped copy that
    // branch leaves beside it.
    write(&root.join(".claude/settings.json"), b"{\n  \"userKey\": true\n}\n");
    write(&root.join(BACKUP_DIR).join("settings.json"), b"{}\n");
    write(&root.join(BACKUP_DIR).join("mustard/orchestrator.md"), b"# Orchestrator Rules\n");
    root
}

/// Everything the harness puts in the project while it WORKS — as opposed to
/// what the install seeds, which `upsert_project` has already written by the time
/// this runs.
///
/// A full scan writes one census per subproject plus that subproject's Guards,
/// and a unit in flight writes its spec directory. Under
/// [`InstallMode::Private`] the Guards go to the untracked local layer beside the
/// client's file; under [`InstallMode::Shared`] they go INTO it, which is the
/// visible trace the private mode exists to avoid — and, here, the negative
/// control's most direct evidence.
fn write_harness_output(root: &Path, mode: InstallMode) {
    let sub = root.join(SUBPROJECT);
    write(&sub.join(".claude/scan-map.md"), b"Type: cargo - 12 files\n");
    let guards = "\n## Guards\n\n- reuse the shared client\n";
    if mode.is_private() {
        write(&sub.join("CLAUDE.local.md"), guards.as_bytes());
    } else {
        let mut merged = HOST_GUARDS.to_string();
        merged.push_str(guards);
        write(&sub.join("CLAUDE.md"), merged.as_bytes());
    }
    write(&root.join(".claude/spec/demo/spec.md"), b"# demo\n");
    write(&root.join(".claude/spec/demo/qa/report.md"), b"# QA\n");
}

/// The footprint entries that really exist on disk under `root`, read back from
/// [`footprint_pathspecs`] rather than retyped here.
///
/// The PATHSPEC projection, not the rule one: a rule is a pattern (`/mustard.json`
/// is anchored, `**/.claude/` is a shape) while a pathspec names the file the
/// install actually produces, which is what "exists on disk" can be asked about.
///
/// An entry the install did not produce is skipped, and that is not a judgement
/// call: it is not evidence of anything. `report` is consulted only to fail
/// loudly if the two ever disagree about a file the engine claims to have
/// written.
fn footprint_on_disk(root: &Path, report: &UpsertReport) -> Vec<String> {
    let claimed: Vec<&String> = report
        .created
        .iter()
        .chain(&report.updated)
        .chain(&report.preserved)
        .collect();
    let mut found = Vec::new();
    for entry in footprint_pathspecs() {
        if !root.join(&entry).exists() {
            assert!(
                !claimed.iter().any(|c| **c == entry),
                "the install reported writing {entry}, but it is not on disk",
            );
            continue;
        }
        found.push(entry);
    }
    found
}

// ---------------------------------------------------------------------------
// git + IO scaffolding
// ---------------------------------------------------------------------------

/// The repository's dirt, trimmed. `--untracked-files=all` because the default
/// collapses a wholly untracked directory into ONE line, which would let an
/// unhidden artefact hide behind its parent's name.
fn git_status(root: &Path) -> String {
    let out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        // No git, or a repository git refuses to read: the measurement did not
        // happen, so return a sentinel that fails every assertion here rather
        // than an empty string that would satisfy the first one.
        _ => "<git status unavailable>".to_string(),
    }
}

/// Run a git command in `root`, asserting success — test scaffolding only.
fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed in {}", root.display());
}

/// Write `body` at `path`, creating the parent directories. Bytes, not a string:
/// the CRLF fixture must reach disk exactly as written on every platform.
fn write(path: &Path, body: &[u8]) {
    if let Some(parent) = path.parent() {
        assert!(std::fs::create_dir_all(parent).is_ok(), "create {}", parent.display());
    }
    assert!(std::fs::write(path, body).is_ok(), "write {}", path.display());
}
