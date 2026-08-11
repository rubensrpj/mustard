//! The seeded `.claude/.gitignore` hides every artefact the harness writes —
//! proven against real git, not by reading the template.
//!
//! ## Why this file exists
//!
//! The Mustard runtime writes files into `.claude/` while it works: the feature
//! digest, and a spec's QA sidecars. None of them is code and none of them is
//! user state — they are regenerable output. But the seeded ignore list did not
//! cover them, so the more the harness worked, the dirtier its own tree became,
//! and the exit ritual (`git-settle`) kept tripping over files the harness
//! itself had just written. In the field the sole dirt blocking a `pr close`
//! was `.claude/feature-digest.json`, with `.claude/spec/<slug>/qa/` and
//! `qa-report.json` queued to do it again on the next close.
//!
//! Asserting the template CONTAINS a line would prove nothing about matching:
//! a `.gitignore` pattern is anchored (or not) by where it sits and whether it
//! carries a slash, and the seed sits inside `.claude/`, one level below the
//! paths people quote. So this seeds the real template into a real repository,
//! writes the real artefacts and asks git.
//!
//! ## The negative control is half the proof
//!
//! An ignore list that hid a spec's own content would be worse than the defect
//! it fixes: a spec belongs to its unit and stays versioned. The same test
//! therefore also writes `spec.md` and requires git to still SEE it — the
//! sidecars are runtime output, the spec is not.

use std::path::Path;
use std::process::Command;

/// The harness artefacts, relative to `.claude/`. Every one of these is written
/// by the runtime while a unit is in flight and must never reach a diff.
const ARTEFACTS: &[&str] = &[
    // `run feature` writes the full digest here on every query.
    "feature-digest.json",
    // The QA renderer writes all three side by side for one spec.
    "spec/demo/qa-report.json",
    "spec/demo/qa-report.html",
    "spec/demo/qa/report.md",
    // Sanctioned scratch evidence: the write gate lets a diagnosis land here on
    // a protected base, so it has to be ignored by construction.
    "scratch/probe.sh",
];

/// A spec's OWN content — versioned, part of the unit. The negative control.
const SPEC_CONTENT: &str = "spec/demo/spec.md";

#[test]
fn the_seeded_ignore_hides_every_artefact_the_harness_writes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    seed_repo(root);

    for rel in ARTEFACTS {
        write_under_claude(root, rel, "generated");
    }
    assert_eq!(
        git_status(root),
        "",
        "the harness wrote {} artefacts and git must see none of them — an \
         untracked artefact here is exactly what blocks the exit ritual",
        ARTEFACTS.len(),
    );

    // Negative control: the spec itself is NOT swallowed by the same rules.
    write_under_claude(root, SPEC_CONTENT, "# spec");
    let status = git_status(root);
    assert!(
        status.contains("spec.md"),
        "a spec belongs to its unit and stays versioned — only the sidecars are \
         runtime output, but git reported: {status:?}",
    );
    assert!(
        !status.contains("qa-report") && !status.contains("feature-digest"),
        "…and adding it must not un-hide the artefacts: {status:?}",
    );
}

/// A fresh repository carrying the seeded `.claude/.gitignore` as its single
/// COMMITTED file — the state every `mustard init` leaves behind. The seed must
/// be committed, not merely present: an untracked ignore file is itself dirt,
/// and would mask the very thing this test measures.
fn seed_repo(root: &Path) {
    git(root, &["init"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    // Line-ending translation would report the freshly committed seed as
    // modified on a machine with `core.autocrlf=true`, turning a global setting
    // into a test failure. The fixture pins it; the assertion is about ignore
    // rules, nothing else.
    git(root, &["config", "core.autocrlf", "false"]);

    write_under_claude(root, ".gitignore", mustard_core::CLAUDE_GITIGNORE);
    git(root, &["add", ".claude/.gitignore"]);
    git(root, &["commit", "-m", "seed"]);
    assert_eq!(git_status(root), "", "the seeded repository starts clean");
}

/// Write `body` at `.claude/<rel>`, creating the parent directories.
fn write_under_claude(root: &Path, rel: &str, body: &str) {
    let path = root.join(".claude").join(rel);
    if let Some(parent) = path.parent() {
        assert!(
            std::fs::create_dir_all(parent).is_ok(),
            "create {}",
            parent.display(),
        );
    }
    assert!(std::fs::write(&path, body).is_ok(), "write {}", path.display());
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

/// The repository's dirt, trimmed. `--untracked-files=all` because the default
/// collapses a wholly untracked directory into ONE line, which would let an
/// unignored artefact hide behind its parent's name.
fn git_status(root: &Path) -> String {
    let out = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output();
    let stdout = match out {
        Ok(o) if o.status.success() => o.stdout,
        // No git, or a repository git refuses to read: the measurement did not
        // happen, so return a sentinel that fails every assertion here rather
        // than an empty string that would pass the first one.
        _ => b"<git status unavailable>".to_vec(),
    };
    String::from_utf8_lossy(&stdout).trim().to_string()
}
