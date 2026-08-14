//! The seeded `.claude/.gitignore` hides every path the RUNTIME writes —
//! proven against real git, not by reading the template.
//!
//! ## Why this file exists
//!
//! The Mustard runtime writes files into `.claude/` while it works: the feature
//! digest, a spec's QA sidecars, rendered dispatch prompts, compaction memory,
//! the knowledge store. None of it is code and none of it is user state — it is
//! regenerable output. But the seeded ignore list did not cover it, so the more
//! the harness worked, the dirtier its own tree became, and the exit ritual
//! (`git-settle`) kept tripping over files the harness itself had just written.
//! In the field the sole dirt blocking a `pr close` was
//! `.claude/feature-digest.json`, with a spec's `qa-report.json` and
//! `qa-report.html` queued to do it again on the next close.
//!
//! Asserting the template CONTAINS a line would prove nothing about matching:
//! a `.gitignore` pattern is anchored (or not) by where it sits and whether it
//! carries a slash, and the seed sits inside `.claude/`, one level below the
//! paths people quote. So this seeds the real template into a real repository,
//! writes the real artefacts and asks git.
//!
//! ## Where the paths come from — and why not from this file's author
//!
//! The first version of this test chose its own four-item list, and passed
//! while SEVEN other runtime paths stayed untracked in a freshly seeded project
//! (found in review, 2026-08-11: the same omission class as the round before,
//! different entries). A list the test picks can only ever prove the template
//! covers what the test already thought of.
//!
//! So the paths come from two sources outside this file, and neither is a
//! matter of taste:
//!
//! 1. [`WRITER_PATHS`] — each entry cites the writer in the code that produces
//!    it (`file.rs:line`). Delete the writer and the citation goes stale; add a
//!    writer and this list is where the new path is declared.
//! 2. [`field_proven_samples`] — every `.claude/`-scoped rule in THIS
//!    repository's own root `.gitignore`, turned into a concrete sample path.
//!    That file is the accumulated field record of the project that runs this
//!    harness hardest: every entry in it was added because something really
//!    appeared in a diff here. The seed must not be a strict subset of it, and
//!    the day a new rule is added there, this test asks for it in the template
//!    too — which is exactly the ratchet the review found missing.
//!
//! ## The negative control is half the proof
//!
//! An ignore list that hid a spec's own content would be worse than the defect
//! it fixes: a spec belongs to its unit and stays versioned. The same test
//! therefore also writes `spec.md` and requires git to still SEE it — the
//! sidecars are runtime output, the spec is not.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The runtime's own writes, each beside the code that performs it. Relative to
/// `.claude/`; every one of these is produced while a unit is in flight and must
/// never reach a diff.
const WRITER_PATHS: &[&str] = &[
    // `run feature` writes the full digest here on every query.
    // apps/rt/src/commands/feature.rs:842
    "feature-digest.json",
    // The MACHINE renders of a spec's QA — regenerable from the run.
    "spec/demo/qa-report.json",
    "spec/demo/qa-report.html",
    // Sanctioned scratch evidence: the write gate lets a diagnosis land here on
    // a protected base, so it has to be ignored by construction.
    // apps/rt/src/hooks/write/work_branch_gate.rs (`.claude/scratch/` carve-out)
    "scratch/probe.sh",
    // Compaction memory, pruned after 24h by the session cleanup observer.
    // apps/rt/src/hooks/session/session_cleanup_observer.rs:183
    ".compact-state/2026-08-11-session.json",
    // Rendered dispatch prompts — the spec-scoped one and the spec-less one.
    // apps/rt/src/commands/agent/render/prompt_ref.rs:109 and :121
    "spec/demo/.dispatch/wave-2-rt.first.prompt.md",
    ".dispatch/explore-0000000000000000.prompt.md",
    // The relevance gate's approved-name list, read back on the next inject.
    // apps/rt/src/commands/agent/context_inject.rs:281
    "spec/demo/.memory-approved",
    // The epic summary the wave fold writes into the knowledge store.
    // apps/rt/src/commands/wave/epic_fold.rs:167
    "knowledge/epic-demo.md",
];

/// A spec's OWN content — versioned, part of the unit. The negative control.
///
/// `qa/report.md` sits here and not in [`WRITER_PATHS`] on purpose: it is the
/// unit's record of WHICH criteria passed, the peer of `review/verdict.md`, and
/// the close commits it (this repository versions 31 of them). Ignoring it
/// would hide from every new project exactly what this one keeps — the review
/// finding that sent this file back. Only its machine renders are regenerable.
const SPEC_CONTENT: &[&str] = &["spec/demo/spec.md", "spec/demo/qa/report.md"];

#[test]
fn the_seeded_ignore_hides_every_path_the_runtime_writes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    seed_repo(root);

    // 1. The writers in the code.
    for rel in WRITER_PATHS {
        write_under_claude(root, rel, "generated");
    }
    assert_eq!(
        git_status(root),
        "",
        "the runtime wrote {} paths and git must see none of them — an untracked \
         one here is exactly what blocks the exit ritual",
        WRITER_PATHS.len(),
    );

    // 2. The field record: every `.claude/` rule this repository has had to add.
    let field = field_proven_samples();
    for rel in &field {
        write_under_claude(root, rel, "generated");
    }
    assert_eq!(
        git_status(root),
        "",
        "the seeded template is a strict SUBSET of this repository's own ignore \
         list ({} rules derived): a project installed from the template dirties \
         itself where this one stays clean",
        field.len(),
    );

    // 3. Negative control: the unit's own record is NOT swallowed by the same rules.
    for rel in SPEC_CONTENT {
        write_under_claude(root, rel, "# unit record");
    }
    let status = git_status(root);
    for rel in SPEC_CONTENT {
        assert!(
            status.contains(rel),
            "{rel} belongs to its unit and stays versioned — only the sidecars \
             are runtime output, but git reported: {status:?}",
        );
    }
    assert!(
        !status.contains("qa-report") && !status.contains("feature-digest"),
        "…and adding it must not un-hide the artefacts: {status:?}",
    );
}

/// Every `.claude/`-scoped rule of this repository's root `.gitignore`, as a
/// concrete path relative to `.claude/`.
///
/// `*` stands for one whole segment (`spec/*/`) or part of one (`wave-*`); any
/// concrete value matches the pattern, so the sample picks one. A rule ending in
/// `/` names a directory, which git only reports through a file inside it.
///
/// Rules that name a path elsewhere in the tree (`packages/cli/templates/…`)
/// are NOT derived: they cover fixtures of this repository, not what a project's
/// runtime writes under its own `.claude/`.
fn field_proven_samples() -> Vec<String> {
    let source = repo_root().join(".gitignore");
    let body = std::fs::read_to_string(&source).unwrap_or_else(|e| {
        // A derivation whose source is missing did not happen. Failing here is
        // the point: reading it as "nothing to check" would hand back the
        // hand-picked list this test exists to stop trusting.
        panic!("the derivation source is unreadable: {} ({e})", source.display())
    });
    let mut samples: Vec<String> = Vec::new();
    for line in body.lines() {
        let rule = line.trim();
        if rule.is_empty() || rule.starts_with('#') || rule.starts_with('!') {
            continue;
        }
        let Some(under_claude) = rule
            .strip_prefix("**/.claude/")
            .or_else(|| rule.strip_prefix(".claude/"))
        else {
            continue;
        };
        let mut sample = under_claude.replace('*', "x");
        if sample.ends_with('/') {
            sample.push_str("probe.txt");
        }
        if !samples.contains(&sample) {
            samples.push(sample);
        }
    }
    assert!(
        samples.len() > 10,
        "the derivation produced almost nothing ({samples:?}) — the source's \
         shape changed and this test would now prove very little",
    );
    samples
}

/// This repository's root — two levels up from `packages/core`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
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
