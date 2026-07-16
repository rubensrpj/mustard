//! Worktree redirect (integration): when `mustard-rt` runs from inside a LINKED
//! git worktree (`.claude/worktrees/{slug}`), the canonical workspace resolver
//! must resolve every `.claude/` path — specs, events, active-spec markers,
//! telemetry, state — to the MAIN checkout, never the worktree's own tree. The
//! worktree carries code only. A normal checkout must be byte-for-byte unchanged.
//!
//! The redirect lives at ONE central point — core's `workspace_root`
//! (`resolve_with_override` is its unit-testable seam). Both the `run` face
//! (`context::workspace_root_strict`) and every hook (`dispatch::build_ctx`)
//! derive their project dir from it, so asserting it here at the public API
//! proves the fix for both faces at once.
//!
//! These fixtures build REAL git repos in tempdirs (like the `git_settle` /
//! `work_branch_gate` suites) and no-op on a git-less host. They live in the rt
//! crate — not core — because core's own test binary is currently unbuildable on
//! this branch (an unrelated stale `retrieval` test in `domain/config.rs`), so
//! `cargo test -p mustard-rt` is the vehicle that actually runs them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::io::workspace::resolve_with_override;
use mustard_core::ClaudePaths;

/// Run a git command in `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

/// `true` when a `git` binary is available — otherwise the fixtures no-op rather
/// than fail spuriously on a git-less host.
fn has_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Canonicalise for a stable comparison, falling back to the path as-given.
fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Build a real repo on branch `dev` with the Mustard anchor COMMITTED, so a
/// linked-worktree checkout reproduces `mustard.json` + `.claude/`.
fn seed_repo(main: &Path) {
    std::fs::create_dir_all(main.join(".claude")).unwrap();
    std::fs::write(main.join("mustard.json"), b"{}").unwrap();
    std::fs::write(main.join(".claude").join(".keep"), b"").unwrap();
    git(main, &["init"]);
    git(main, &["config", "user.email", "t@t"]);
    git(main, &["config", "user.name", "t"]);
    git(main, &["checkout", "-b", "dev"]);
    git(main, &["add", "-A"]);
    git(main, &["commit", "-m", "seed"]);
}

/// A LINKED worktree under `.claude/worktrees/{slug}` resolves ALL `.claude/`
/// state to the MAIN checkout — the exact bug this change fixes.
#[test]
fn linked_worktree_resolves_state_to_main_checkout() {
    if !has_git() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("repo");
    seed_repo(&main);

    // The harness cuts the worktree at <main>/.claude/worktrees/<slug>.
    git(&main, &["worktree", "add", ".claude/worktrees/dev_x", "-b", "dev_x"]);
    let wt = main.join(".claude").join("worktrees").join("dev_x");
    // The worktree IS its own anchor (mustard.json + .claude/ are checked out),
    // so WITHOUT the redirect the ancestor walk would stop at the worktree root.
    assert!(
        wt.join("mustard.json").is_file() && wt.join(".claude").is_dir(),
        "worktree checkout reproduces the Mustard anchor",
    );

    // Resolving from inside the worktree redirects to the MAIN checkout root…
    let resolved = resolve_with_override(&wt, None).expect("resolves");
    assert_eq!(
        canon(&resolved),
        canon(&main),
        "a linked worktree resolves to its main checkout, not itself",
    );

    // …so the `.claude` root every ClaudePaths accessor hangs off (spec dir,
    // events, `.session` markers, telemetry) is the MAIN checkout's, distinct
    // from the worktree's own .claude. Both dirs exist on disk, so canonicalising
    // reconciles the git forward-slash / verbatim-prefix path forms.
    let cp = ClaudePaths::for_project(&resolved).expect("valid anchor");
    let main_claude = canon(&main.join(".claude"));
    let wt_claude = canon(&wt.join(".claude"));
    assert_eq!(canon(&cp.claude_dir()), main_claude, "claude root is the main checkout's");
    assert_ne!(main_claude, wt_claude, "main and worktree .claude are distinct dirs");
    // Every derived state path (spec dir shown) is rooted at that main .claude.
    assert!(
        cp.spec_dir().starts_with(cp.claude_dir()),
        "spec dir is rooted at the resolved (main) .claude",
    );
}

/// Regression: a NORMAL checkout (git-dir == git-common-dir) resolves to the
/// repo root itself — byte-for-byte identical to the pre-change behaviour.
#[test]
fn main_checkout_resolution_is_unchanged() {
    if !has_git() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("repo");
    seed_repo(&main);

    let resolved = resolve_with_override(&main, None).expect("resolves");
    assert_eq!(
        canon(&resolved),
        canon(&main),
        "the main checkout resolves to itself, unchanged",
    );
}
