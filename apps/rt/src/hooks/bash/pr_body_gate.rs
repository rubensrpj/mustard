//! `pr_body_gate` — the PR-body advisory.
//!
//! ## Scope (ONE behavior)
//!
//! A `PreToolUse(Bash)` stage that warns when a `gh pr create` is issued
//! WITHOUT `--body-file`, for a unit that has a spec.
//!
//! ## Why it exists
//!
//! `/mustard:pr open` instructs the caller to write `<spec>/pr-body.md` and
//! publish with `--body-file`. That instruction is PROSE: it is read by a model
//! and it holds only for as long as the model follows it. Every other rule in
//! this project that survived contact with reality has a mechanism behind it —
//! the QA↔integration coupling next door is exactly the same shape, raised at
//! exactly the same moment.
//!
//! What it protects is not cosmetic. `--fill` writes the commit list into the
//! description, which hands the reviewer a diff and asks them to reconstruct
//! reasoning the unit ALREADY recorded: `spec.md`'s Context, the Decisions with
//! their reasons, the criteria with the commands that verify them. The body
//! file is how that record reaches the person reading the diff, and the review
//! is the one moment it is worth anything.
//!
//! ## Advisory, never blocking
//!
//! It answers `Warn`, never `Deny`. A PR opened with a thin body is recoverable
//! by one `gh pr edit --body-file` and the SAME PR is updated until it merges,
//! so a veto would cost more than the defect: an operator who genuinely wants a
//! bare PR (a base→base promotion carries no unit and no spec) must not be
//! stopped by a rule about units.
//!
//! ## Fail-open invariant
//!
//! A non-`create` PR command, a command that already carries `--body-file` or
//! an explicit `--body`, or no spec bound to the session all answer `None`
//! (pass through). The stage speaks only on a positive observation: opening a
//! PR, for a known unit, with nothing carrying its record.

use std::path::Path;
use std::time::SystemTime;

use mustard_core::domain::model::contract::Verdict;

use super::pr_detect::classify_pr;

/// Warn when the unit's PR body is OLDER than the work being pushed.
///
/// The second half of the same rule, and the half that actually bites. A body
/// written once at `open` is correct for exactly one commit: every `push` after
/// it re-targets the SAME pull request, so the description on the provider
/// silently drifts behind the diff it describes. A reviewer reading a stale
/// body is worse served than one reading none, because a wrong explanation is
/// believed.
///
/// Staleness is measured, not guessed: the body file's mtime against the mtime
/// of `.git/HEAD`, which every commit rewrites. Both are metadata reads, taken
/// only after a cheap string match — no process is spawned in the Bash chain.
///
/// Fail-open at every step: not a push, no spec, no body file yet (the `create`
/// half already speaks for that case), or either timestamp unreadable, all
/// answer `None`.
pub(super) fn pr_body_stale_gate(command: &str, cwd: &str) -> Option<Verdict> {
    if !is_push(command) {
        return None;
    }
    let spec = crate::shared::context::current_spec(cwd)?;
    let root = Path::new(cwd);
    let body = root.join(".claude/spec").join(&spec).join("pr-body.md");
    let body_at = modified_at(&body)?;
    // `.git` is a FILE in a submodule or a linked worktree, so the literal path
    // is not universal — but a missing HEAD simply means "cannot measure", and
    // an advisory that cannot measure says nothing.
    let head_at = modified_at(&root.join(".git/HEAD"))?;
    if body_at >= head_at {
        return None;
    }
    Some(Verdict::Warn {
        message: format!(
            "[pr-body] Pushing `{spec}` with a PR body older than the work. Every push re-targets \
             the SAME pull request, so the description on the provider now describes a smaller \
             diff than the one under review — and a reviewer believes a wrong explanation more \
             readily than a missing one. Rewrite `.claude/spec/{spec}/pr-body.md` from what the \
             unit recorded, then push it to the PR too: `gh pr edit <n> --body-file \
             .claude/spec/{spec}/pr-body.md`. Updating the file alone changes nothing for the \
             person reading the PR."
        ),
    })
}

/// Whether `command` pushes. Deliberately literal and `rtk`-tolerant: the
/// rewrite stage runs first, so what reaches here is usually `rtk git push`.
fn is_push(command: &str) -> bool {
    command.contains("git push")
}

/// `path`'s mtime, or `None` when it cannot be read — absence and IO error are
/// the same answer here, because both mean the comparison did not happen.
fn modified_at(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Warn when a PR is opened for a spec without a body file.
///
/// `None` = pass through. Reuses [`classify_pr`] — the same conservative,
/// `rtk`-wrapper-tolerant `gh pr` classifier the DORA telemetry and the QA
/// coupling use — so the three advisories can never disagree about what
/// counts as opening a PR.
pub(super) fn pr_body_gate(command: &str, cwd: &str) -> Option<Verdict> {
    if classify_pr(command)? != "pr.opened" {
        return None;
    }
    // An author who passed a body of any shape made the call deliberately.
    // `--fill` is NOT one of those: it is the default that produces the commit
    // list this stage exists to notice.
    if command.contains("--body-file") || command.contains("--body ") || command.contains("--body=")
    {
        return None;
    }
    let spec = crate::shared::context::current_spec(cwd)?;
    Some(Verdict::Warn {
        message: format!(
            "[pr-body] Opening a PR for `{spec}` with no `--body-file`. The unit already recorded \
             what a reviewer needs — Context and Decisions with their reasons in `spec.md`, the \
             criteria with the commands that verify them — and `--fill` replaces all of it with \
             the commit list. Write `.claude/spec/{spec}/pr-body.md` (sections and rules: \
             `/mustard:pr open`) and reopen with `--body-file`, or fix it after the fact with \
             `gh pr edit --body-file`; the same PR is updated until it merges."
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stage is silent on everything that is not "opening a PR for a unit
    /// with no body" — the fail-open half, which is what keeps an advisory from
    /// becoming noise nobody reads.
    #[test]
    fn silent_unless_a_bodyless_create_is_observed() {
        let cwd = ".";
        for quiet in [
            "cargo build",                                   // not a PR command
            "rtk gh pr merge 12",                            // merging, not opening
            "rtk gh pr create --body-file .claude/x.md",     // body supplied
            "rtk gh pr create --body \"already written\"",   // body supplied inline
            "rtk gh pr create --body=written",               // …and its = spelling
        ] {
            assert!(
                pr_body_gate(quiet, cwd).is_none(),
                "must pass through: {quiet}",
            );
        }
    }

    /// The staleness half is silent on everything it cannot MEASURE, and that
    /// is the property worth pinning: an advisory that guesses is an advisory
    /// operators learn to ignore.
    #[test]
    fn staleness_is_silent_when_it_cannot_measure() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cwd = dir.path().to_string_lossy().into_owned();
        // Not a push at all.
        assert!(pr_body_stale_gate("cargo build", &cwd).is_none());
        // A push, but nothing to compare against: no spec bound, no body file,
        // no `.git/HEAD`. Each alone is enough to stay quiet.
        assert!(pr_body_stale_gate("rtk git push", &cwd).is_none());
    }

    /// A body newer than the last commit is exactly the state the rule asks
    /// for, so it must produce no noise — otherwise the operator who DID the
    /// right thing is the one who gets warned.
    #[test]
    fn a_body_newer_than_head_is_quiet() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git")).expect("git dir");
        std::fs::write(root.join(".git/HEAD"), b"ref: refs/heads/x\n").expect("head");
        std::fs::create_dir_all(root.join(".claude/spec/demo")).expect("spec dir");
        // Written after HEAD, which is the ordering the rule wants.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(root.join(".claude/spec/demo/pr-body.md"), b"# body\n").expect("body");

        assert!(
            pr_body_stale_gate("rtk git push", &root.to_string_lossy()).is_none(),
            "a body written after the last commit must not be reported stale",
        );
    }

    /// `--fill` is the exact case: it looks like an author's choice and is in
    /// fact the default that writes a commit list where the reasoning belongs.
    /// Asserted through the `rtk` wrapper too, because that is the spelling the
    /// rewrite stage produces before this one ever sees the command.
    #[test]
    fn fill_is_not_a_body() {
        for spelling in ["gh pr create --fill", "rtk gh pr create --base dev --fill"] {
            assert_eq!(
                classify_pr(spelling),
                Some("pr.opened"),
                "the classifier must see {spelling} as opening a PR",
            );
            // Without a spec bound to the session the stage still passes
            // through — the warning is about a UNIT's record, and a PR with no
            // unit (a base→base promotion) has none to carry.
            assert!(pr_body_gate(spelling, ".").is_none());
        }
    }
}
