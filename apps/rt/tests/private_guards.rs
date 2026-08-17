//! A private install's Guards must reach the AGENT, not merely reach the disk.
//!
//! ## Why this file exists next to `private_scan.rs`
//!
//! `private_scan.rs` proves the writer moved: under a private install the
//! subproject Guards land in `CLAUDE.local.md` and the host's `CLAUDE.md` is
//! never touched. That is a claim about a FILE. It stays green even if every
//! consumer of that file kept reading the old name — which is exactly the state
//! the review found: the `## GUARDS` block inlined into every dispatched prompt
//! read `<sub>/CLAUDE.md` unconditionally, so a private install shipped a
//! harness whose central artifact was inert.
//!
//! So this file asserts EFFECTIVENESS instead of presence: it renders a real
//! dispatch prompt through the published CLI and requires the rules to be in it.
//!
//! ## Why the real binary
//!
//! The mode is autodetected off the clone-local exclude file and memoised
//! per-process. A same-process render could be answered by a cache another
//! assertion warmed, so "resolved" would really mean "remembered". Each render
//! here is a fresh process reading a fresh repository.
//!
//! ## The three halves, and why none of them is optional
//!
//! 1. **private** — Guards on the local layer must reach the prompt, and the
//!    client's own file must NOT (the resolver PREFERS the local layer; it does
//!    not merge the two, so this pins which file was actually opened).
//! 2. **shared** — the negative control. Without it, a build that had simply
//!    stopped injecting Guards altogether would satisfy half of (1) and a build
//!    that always injected both files would satisfy the other half.
//! 3. **private, before the first scan** — the fallback. A clone that went
//!    private before it ever had a local layer must still read the shared file
//!    rather than go silent; without this half the fix could be "in private mode
//!    read only `CLAUDE.local.md`", which is silence dressed as correctness.

use std::path::Path;
use std::process::Command;

/// The client's own subproject instruction file, versioned before Mustard ever
/// arrived. Its rule token appears nowhere else, so "this file was read" is a
/// substring test rather than a hope.
const HOST_MD: &str = "# Their subproject\n\n## Guards\n\n- THEIR-OWN-RULE: the client's rule, in the file their git tracks.\n";

/// What a private `scan --full` + guards enrich leaves on the local layer: a
/// complete instruction file, with Mustard's Guards in it.
const OURS_LOCAL_MD: &str = "@.claude/scan-map.md\n\n# Sub\n\n## Guards\n\n- OURS-PRIVATE-RULE: the harness rule, on the untracked layer.\n";

#[test]
fn ac10_private_dispatch_prompt_carries_the_guards() {
    // --- 1. private: the local layer is what the agent is handed -------------
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    let sub = seed_project(root);
    std::fs::write(sub.join("CLAUDE.md"), HOST_MD).expect("write host md");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", "their instruction file"]);

    // Make the clone private exactly as a private install does — by writing the
    // footprint into the clone-local exclude file. Nothing else declares the
    // mode: there is no knob to set.
    let outcome = mustard_core::ensure_excluded(root, &mustard_core::footprint_rules());
    assert_eq!(outcome.unavailable, None, "the fixture needs a real repository: {outcome:?}");

    std::fs::write(sub.join("CLAUDE.local.md"), OURS_LOCAL_MD).expect("write local md");

    let private = render(root, "apps/sub");
    assert!(
        private.contains("OURS-PRIVATE-RULE"),
        "a private install's Guards must reach the dispatched agent, not just the disk:\n{private}",
    );
    assert!(
        !private.contains("THEIR-OWN-RULE"),
        "the resolver PREFERS the local layer — reading the client's file instead would mean the \
         Guards the enrich authored are still inert:\n{private}",
    );

    // --- 2. shared: the negative control ------------------------------------
    // Same render, same role, no exclude rules. If this half were missing, a
    // build that had stopped injecting Guards entirely would pass half of the
    // block above and a build that concatenated both files would pass the other.
    let plain = tempfile::tempdir().expect("temp dir");
    let plain_root = plain.path();
    let plain_sub = seed_project(plain_root);
    std::fs::write(plain_sub.join("CLAUDE.md"), HOST_MD).expect("write host md");

    let shared = render(plain_root, "apps/sub");
    assert!(
        shared.contains("THEIR-OWN-RULE"),
        "a shared install still reads `CLAUDE.md`, as it always did:\n{shared}",
    );

    // --- 3. private, no local layer yet: the fallback ------------------------
    // A clone can be made private before its first `scan --full`. The resolver
    // falls back to the shared file so the harness is never silent — "no Guards
    // at all" is not the private mode's contract.
    let early = tempfile::tempdir().expect("temp dir");
    let early_root = early.path();
    let early_sub = seed_project(early_root);
    std::fs::write(early_sub.join("CLAUDE.md"), HOST_MD).expect("write host md");
    let outcome = mustard_core::ensure_excluded(early_root, &mustard_core::footprint_rules());
    assert_eq!(outcome.unavailable, None, "the fixture needs a real repository: {outcome:?}");

    let fallback = render(early_root, "apps/sub");
    assert!(
        fallback.contains("THEIR-OWN-RULE"),
        "with no local layer written yet, a private install must fall back to the shared file \
         instead of dispatching an agent with no rules at all:\n{fallback}",
    );
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// Render a real dispatch prompt through the published CLI and return it.
///
/// Spec-less on purpose: the `## GUARDS` block is the only section under test,
/// and a spec directory would drag in wave scaffolding that has nothing to say
/// about which instruction file was opened. `--task-text` keeps the prompt
/// self-contained the way the `/scan` enrich dispatch does.
fn render(root: &Path, subproject: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args([
            "run",
            "agent-prompt-render",
            "--role",
            "implementer",
            "--subproject",
            subproject,
            "--task-text",
            "irrelevant to this criterion",
        ])
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_WORKSPACE_ROOT")
        .output()
        .expect("run the built binary");
    assert!(
        out.status.success(),
        "`run agent-prompt-render` exited non-zero: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let rendered = String::from_utf8_lossy(&out.stdout).to_string();
    // The assertions below are substring tests, two of them NEGATIVE — an empty
    // render would satisfy those silently. It is asserted here instead.
    assert!(
        rendered.contains("## TASK"),
        "no prompt was rendered at all, so nothing below measures anything: {rendered:?}",
    );
    rendered
}

/// A fresh repository with one commit, the Mustard anchor, and one subproject —
/// the state a host repo is in when the operator installs.
///
/// `core.autocrlf` is pinned off for the reason the core's `private_install.rs`
/// pins it: line-ending translation would turn a machine preference into a
/// failure of assertions that are about ignore rules. Returns the subproject dir.
fn seed_project(root: &Path) -> std::path::PathBuf {
    git(root, &["init"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "core.autocrlf", "false"]);
    std::fs::write(root.join("README.md"), "host repo\n").expect("write README");
    // The workspace anchor, so the `run` face resolves THIS root rather than
    // walking out of the temp directory.
    std::fs::write(root.join("mustard.json"), "{}\n").expect("write anchor");
    std::fs::create_dir_all(root.join(".claude")).expect("mkdir .claude");
    let sub = root.join("apps").join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir sub");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
    sub
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
