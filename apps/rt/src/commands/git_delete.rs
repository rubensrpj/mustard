//! `mustard-rt run git-delete` — the CANCEL path of an ABANDONED work unit.
//!
//! [`crate::commands::git_settle`] retires a unit that WAS delivered, and its
//! central invariant is a hard merge gate: 100% merged or nothing is touched.
//! This command answers the opposite question — "the work was given up on, take
//! it away" — so it deliberately does NOT live in that module: a cancel path
//! sharing a file with a merge gate is one edit away from becoming the way
//! around it. What the two DO share is the pruning vocabulary, imported from
//! there rather than written a second time.
//!
//! ## The unit is deleted WHOLE
//!
//! A work unit is its branch plus everything the work produced — the spec, the
//! waves, the ceremony, the code and the notebook all live ON that branch. So
//! there is nothing to clean up item by item: removing the worktree, the local
//! branch, the remote branch and the open pull request removes the unit entire.
//! That is also why the worktree comes off with `--force`: an abandoned unit is
//! abandoned precisely because it still holds uncommitted work, and refusing to
//! delete a unit *because* it was never finished would refuse every case the
//! command exists for.
//!
//! ## Only from an integration base
//!
//! Deleting the branch you are standing on is not an operation — git refuses it,
//! and the ritual would leave the session on a ref that no longer exists.
//! "Which unit do I give up on" is a question asked from the BASE, exactly like
//! `pr-list`, so a work branch REFUSES and names the base to switch to, touching
//! nothing. Two more refusals guard the same edge from the other side: a unit
//! that names an integration base is never deleted (the `BG07` rule of the
//! destructive-ops law, restated where this command can enforce it), and a unit
//! no ref carries anywhere is reported as `no-such-unit` rather than answered
//! with a cheerful "deleted" over a typo.
//!
//! Fail-open around the provider (an absent `gh` is an honest `ghError` field
//! and exit 0) and around the remote delete, which is best-effort in the same
//! way `git-settle`'s is. The three refusals above never degrade: they are the
//! whole guard.

use std::path::Path;

use serde_json::{json, Value};

use crate::commands::git_settle::{git_ok, git_out, main_checkout_root, parse_worktrees, show};
use crate::commands::review::pr_door::{gh_json, gh_out};

/// The number of the OPEN pull request whose head is `branch`, if the provider
/// answers at all.
///
/// `Ok(None)` is a MEASURED absence — `gh pr list` prints `[]` and exits 0 when
/// nothing matches — while `Err` is "the provider was not reached". Keeping the
/// two apart is what lets the report say `prClosed: false` for a unit that never
/// had a pull request without claiming the same for one whose provider was
/// simply offline.
fn open_pr(root: &Path, branch: &str) -> Result<Option<u64>, String> {
    let rows = gh_json(
        root,
        &["pr", "list", "--head", branch, "--state", "open", "--limit", "1", "--json", "number"],
    )?;
    Ok(rows
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("number"))
        .and_then(Value::as_u64))
}

/// Close the unit's open pull request. Returns the PR number, whether it was
/// closed, and the provider's own reason when it did not answer.
fn close_open_pr(root: &Path, branch: &str) -> (Option<u64>, bool, Option<String>) {
    match open_pr(root, branch) {
        Err(e) => (None, false, Some(e)),
        Ok(None) => (None, false, None),
        Ok(Some(number)) => match gh_out(root, &["pr", "close", &number.to_string()]) {
            Ok(_) => (Some(number), true, None),
            Err(e) => (Some(number), false, Some(e)),
        },
    }
}

/// The delete pass — the testable core of [`run`]. `unit` is the work branch to
/// remove; the checkout `start` stands on must be an integration base. Never
/// panics, and every refusal touches nothing.
#[must_use]
pub(crate) fn delete_at(start: &Path, unit: &str) -> Value {
    let Some(main) = main_checkout_root(start) else {
        return json!({
            "ok": false,
            "reason": "not-a-git-repo",
            "path": show(start),
            "exists": start.exists(),
            "hint": "git resolved no repository at that path — check `--root` before suspecting the unit",
        });
    };
    let cfg = mustard_core::ProjectConfig::load(&main);
    let flow = crate::shared::work_kind::BaseFlow::of_at(&cfg.git, &main);
    let bases: Vec<String> = flow.bases().to_vec();

    // The branch of the INVOCATION, not of the main checkout: called from
    // inside the unit's own worktree the two disagree, and it is the caller's
    // floor that decides whether this is a base-side gesture.
    let branch = git_out(start, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    if !bases.iter().any(|b| b == &branch) {
        let target =
            flow.base_of(&branch).into_known().unwrap_or_else(|| cfg.git.primary_base());
        return json!({
            "ok": false,
            "reason": "not-on-integration-base",
            "branch": branch,
            "unit": unit,
            "bases": bases,
            "hint": format!(
                "`git delete` retires a unit from the OUTSIDE — switch to `{target}` \
                 (`git checkout {target}`) and run it again; nothing was touched"
            ),
        });
    }

    let unit = unit.trim();
    if unit.is_empty() {
        return json!({
            "ok": false,
            "reason": "no-unit",
            "branch": branch,
            "hint": "name the work branch to delete: `mustard-rt run git-delete --unit dev_my-unit`",
        });
    }
    if bases.iter().any(|b| b == unit) {
        return json!({
            "ok": false,
            "reason": "is-integration-base",
            "branch": branch,
            "unit": unit,
            "bases": bases,
            "hint": "an integration base is never a work unit — nothing was touched",
        });
    }

    // A unit no ref carries is a typo, not a job already done. Answering
    // "deleted" here would teach the operator that the branch they meant is
    // gone while it sits untouched under the name they mistyped.
    let local = git_ok(&main, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{unit}")]);
    let remote_ref =
        git_ok(&main, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{unit}")]);
    if !local && !remote_ref {
        return json!({
            "ok": false,
            "reason": "no-such-unit",
            "branch": branch,
            "unit": unit,
            "hint": "no local or remote ref carries that branch — check the name with `git branch -a`",
        });
    }

    // The provider first: a pull request left open over a branch that no longer
    // exists is the one leftover the git side cannot clear afterwards.
    let (pr, pr_closed, gh_error) = close_open_pr(&main, unit);

    // Then the git side, each step on its own field. The only real coupling is
    // that git refuses to delete a branch some worktree still checks out, so the
    // local delete waits for the floor to be clear. The remote delete does not
    // wait for anything — a worktree the OS still locks must never strand the
    // server branch.
    let entries = git_out(&main, &["worktree", "list", "--porcelain"])
        .map(|s| parse_worktrees(&s))
        .unwrap_or_default();
    let (worktree_removed, floor_clear) = match entries.iter().find(|e| e.branch == unit) {
        Some(e) => {
            let removed = git_ok(&main, &["worktree", "remove", "--force", &e.path]);
            (removed, removed)
        }
        None => (false, true),
    };
    // `-D`, never `-d`: an abandoned unit is unmerged BY DEFINITION, and `-d`
    // would refuse exactly the branches this command exists to remove.
    let branch_deleted = local && floor_clear && git_ok(&main, &["branch", "-D", unit]);
    let remote_deleted = git_ok(&main, &["push", "origin", "--delete", unit]);

    let local_clear = !local || branch_deleted;
    let mut report = json!({
        "ok": local_clear,
        "action": if local_clear { "deleted" } else { "partial" },
        "branch": branch,
        "unit": unit,
        "base": flow.base_of(unit).into_known(),
        "worktreeRemoved": worktree_removed,
        "branchDeleted": branch_deleted,
        "remoteDeleted": remote_deleted,
        "pr": pr,
        "prClosed": pr_closed,
    });
    if let Some(e) = gh_error {
        report["ghError"] = json!(e);
    }
    if !local_clear {
        report["hint"] = json!(
            "the local branch is still there — a worktree still has it checked out; \
             remove that checkout and run `git delete` again"
        );
    }
    report
}

/// Run `git-delete` from `root` and print the JSON report.
pub fn run(root: &Path, unit: &str) {
    println!("{}", serde_json::to_string_pretty(&delete_at(root, unit)).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// `dev` (primary) + `main` declared, sitting on `dev`, with one work unit
    /// `dev_abandoned` already cut. No remote and no provider — the gate is
    /// answered from local state alone, which is exactly what it must do.
    fn repo() -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("cfg");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);
        git(root, &["branch", "dev_abandoned"]);
        dir
    }

    fn branch_exists(root: &Path, branch: &str) -> bool {
        git_ok(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
    }

    /// AC-6 — invoked from a work branch, `git delete` REFUSES, names the base
    /// to switch to and touches nothing.
    #[test]
    fn git_delete_refuses_off_an_integration_base_and_touches_nothing() {
        let dir = repo();
        let root = dir.path();
        git(root, &["checkout", "dev_abandoned"]);

        let refused = delete_at(root, "dev_abandoned");
        assert_eq!(refused["ok"], json!(false), "a work branch must be refused");
        assert_eq!(refused["reason"], json!("not-on-integration-base"));
        assert_eq!(refused["branch"], json!("dev_abandoned"));
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("dev"), "the refusal must name the base: {hint}");
        assert!(branch_exists(root, "dev_abandoned"), "the refusal touched the unit");

        // A branch with no declared prefix still gets a base named, so the
        // message is never a dead end.
        git(root, &["checkout", "-b", "loose-branch"]);
        let loose = delete_at(root, "dev_abandoned");
        assert_eq!(loose["reason"], json!("not-on-integration-base"));
        assert!(loose["hint"].as_str().unwrap_or_default().contains("dev"), "primary base named");
        assert!(branch_exists(root, "dev_abandoned"));
    }

    /// From the base the unit goes whole: the local branch is deleted, and the
    /// remote/provider halves are reported rather than demanded (this repo has
    /// neither).
    #[test]
    fn git_delete_removes_the_unit_from_an_integration_base() {
        let dir = repo();
        let root = dir.path();

        let done = delete_at(root, "dev_abandoned");
        assert_eq!(done["ok"], json!(true), "report: {done}");
        assert_eq!(done["action"], json!("deleted"));
        assert_eq!(done["unit"], json!("dev_abandoned"));
        assert_eq!(done["base"], json!("dev"));
        assert_eq!(done["branchDeleted"], json!(true));
        assert!(!branch_exists(root, "dev_abandoned"), "the unit's branch survived");
        // No remote in this repo: the best-effort half reports false and the
        // command still succeeds — a missing origin never strands the local
        // cleanup.
        assert_eq!(done["remoteDeleted"], json!(false));
        assert_eq!(done["prClosed"], json!(false));
    }

    /// The two refusals that guard the same edge from the other side: an
    /// integration base is never a unit, and a name no ref carries is a typo.
    #[test]
    fn git_delete_refuses_a_base_and_reports_an_unknown_unit() {
        let dir = repo();
        let root = dir.path();

        let base = delete_at(root, "main");
        assert_eq!(base["ok"], json!(false));
        assert_eq!(base["reason"], json!("is-integration-base"));

        let typo = delete_at(root, "dev_never-existed");
        assert_eq!(typo["ok"], json!(false));
        assert_eq!(typo["reason"], json!("no-such-unit"), "a typo is never answered with success");
        assert!(branch_exists(root, "dev_abandoned"), "the real unit was not touched");

        let blank = delete_at(root, "   ");
        assert_eq!(blank["reason"], json!("no-unit"));
    }
}
