//! Prose ratchet for the pull-request WRITE path.
//!
//! The `/mustard:pr` and `/git` doors used to instruct the model to run
//! `rtk gh pr create/edit/ready` directly — `github` fixed in text no test
//! covered. Those writes now go through the provider port (`mustard-rt run
//! pr-open` / `pr-edit` / `pr-ready`), and this test is what keeps them there:
//! it reads the two door files and fails on ANY line that names a direct
//! `gh pr create` / `gh pr edit` / `gh pr ready` invocation.
//!
//! READS (`gh pr view`, `gh pr list`, `gh pr diff`) are deliberately NOT
//! flagged — they migrate behind the same port in their own unit, and the door
//! prose still names them as fallbacks until then.
//!
//! Deterministic: reads two committed files, no network, no env vars.

use std::fs;
use std::path::{Path, PathBuf};

/// The write invocations the doors must never name directly again. Any
/// spelling that reaches the provider CLI (`gh pr edit`, `rtk gh pr edit`,
/// inside a chain or a code fence) contains one of these substrings.
const FORBIDDEN: &[&str] = &["gh pr create", "gh pr edit", "gh pr ready"];

/// The two door files the ratchet guards.
const DOOR_FILES: &[&str] = &["plugin/commands/pr.md", "plugin/commands/git.md"];

/// The repo root, resolved from this crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_pr_doors_never_instruct_a_direct_gh_pr_write() {
    let root = repo_root();
    let mut offenders = Vec::new();
    for rel in DOOR_FILES {
        let path = root.join(rel);
        assert!(path.is_file(), "door file missing at {}", path.display());
        let text = fs::read(&path)
            .map_or_else(|_| String::new(), |b| String::from_utf8_lossy(&b).into_owned());
        for (idx, line) in text.lines().enumerate() {
            for needle in FORBIDDEN {
                if line.contains(needle) {
                    offenders.push(format!("{rel}:{}: names `{needle}`", idx + 1));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "door prose instructs a direct provider-CLI PR write - the WRITE path \
         is the port (`mustard-rt run pr-open` / `pr-edit` / `pr-ready`); \
         rewrite the line to name the command, never the CLI:\n{}",
        offenders.join("\n")
    );
}
