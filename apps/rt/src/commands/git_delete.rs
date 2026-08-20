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
//! ## Never from INSIDE a unit
//!
//! Deleting the branch you are standing on is not an operation — git refuses it,
//! and the ritual would leave the session on a ref that no longer exists.
//! "Which unit do I give up on" is a question asked from the BASE, exactly like
//! `pr-list`, so a checkout that IS somebody's unit REFUSES and names the base
//! to switch to, touching nothing.
//!
//! Both refusals are measured, never read off a declared list. The old test was
//! membership in `git.flow`'s bases, which refused every real base the install
//! never wrote down — and the installer writes no flow at all. What replaces it
//! is the pair of facts the repository can actually answer: whether the branch
//! is somebody's WORK UNIT ([`crate::shared::work_kind::BaseFlow::base_of`]) and
//! whether [`mustard_core::protected_branches`] names it.
//!
//! Two more refusals guard the same edge from the other side: a name that is
//! nobody's work unit — a bare base, a hand-cut branch, anything
//! `protected_branches` measures — is never deleted (the `BG07` rule of the
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
    // The DECLARED set, echoed in the refusals below purely as context. Not
    // `preselected_bases`, whose `{main, master}` fallback would report two
    // branches a project without a flow may not have — and every project the
    // current installer touches is one.
    let bases: Vec<String> = cfg.git.declared_bases().into_iter().collect();

    // The branch of the INVOCATION, not of the main checkout: called from
    // inside the unit's own worktree the two disagree, and it is the caller's
    // floor that decides whether this is a base-side gesture.
    let branch = git_out(start, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let protected = mustard_core::protected_branches(&main, &cfg.git);
    let standing_on = flow.base_of(&branch);
    // Standing on a branch this project holds NO unit record for is standing on
    // a base, whatever the name looks like — a reviewer measured `release/…`
    // being refused here for the shape of its name alone.
    if flow.has_unit_record(&branch) && !protected.contains(&branch) {
        // The unit's OWN record answers where to go back to; `origin/HEAD` is
        // the last resort, so nothing here spells a branch name of its own.
        let target = standing_on
            .known()
            .map(str::to_string)
            .or_else(|| mustard_core::default_branch(&main));
        let hint = match &target {
            Some(base) => format!(
                "`git delete` retires a unit from the OUTSIDE — switch to `{base}` \
                 (`git checkout {base}`) and run it again; nothing was touched"
            ),
            None => "`git delete` retires a unit from the OUTSIDE — switch to the branch this \
                     unit integrates into and run it again; nothing was touched"
                .to_string(),
        };
        return json!({
            "ok": false,
            "reason": "not-on-integration-base",
            "branch": branch,
            "unit": unit,
            "bases": bases,
            "hint": hint,
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
    // A unit no ref carries is a typo, not a job already done. Answering
    // "deleted" here would teach the operator that the branch they meant is
    // gone while it sits untouched under the name they mistyped.
    //
    // This is asked BEFORE the work-unit test on purpose: a name nothing carries
    // has nothing to protect, and "that name is nobody's unit" would send the
    // operator hunting for a rule when the real answer is that they mistyped.
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

    // What this command may remove is a WORK UNIT, and the test is the project's
    // OWN RECORD of it — never the name's shape, never a declared list. Both of
    // those were tried and both destroyed a real branch. The kind vocabulary is
    // open by design, so `release/2026-Q3` splits into a kind and a slug exactly
    // like `fix/aba` and read as somebody's unit; and with no `git.flow` written
    // — the shape `mustard init` produces today — the declared set degrades to
    // the hardcoded `{main, master}`, so it protected two literals and nothing
    // else. Measured against that shape: `git-delete --unit release/2026-Q3`
    // answered `remoteDeleted: true` and the release line was gone FROM THE
    // REMOTE. Absence of evidence refuses here — this is the one door where
    // being wrong cannot be undone.
    //
    // Two places the record can live, and this door must ask BOTH: the working
    // tree (projects that leave `.claude/spec/` untracked) and the unit's own
    // branch (projects that commit it — which is where the flow authors it, so
    // from the base the directory is simply not on disk). `git delete` retires a
    // unit from OUTSIDE it, so reading only the working tree refuses every
    // legitimate delete — measured, before the second leg existed.
    // Three places, because a unit is retired from OUTSIDE it and its record can
    // be in any of them: the working tree (projects that leave `.claude/spec/`
    // untracked), the unit's LOCAL branch, and — when the local branch is
    // already gone but the remote one is not — the unit's REMOTE branch. The
    // `no-such-unit` check above deliberately accepts a remote-only unit, so
    // stopping at the local ref here refused exactly the unit that still had a
    // remote branch and an open PR to retire.
    let has_record = flow.has_unit_record(unit)
        || flow.slug_of(unit).is_some_and(|slug| {
            let path = format!(".claude/spec/{slug}");
            git_out(&main, &["cat-file", "-e", &format!("{unit}:{path}")]).is_some()
                || git_out(&main, &["cat-file", "-e", &format!("origin/{unit}:{path}")]).is_some()
        });
    if !has_record || protected.contains(unit) {
        return json!({
            "ok": false,
            "reason": "not-a-work-unit",
            "branch": branch,
            "unit": unit,
            "bases": bases,
            "protected": protected.iter().collect::<Vec<_>>(),
            "hint": "`git delete` retires a WORK UNIT, and this project holds no unit record \
                     for that name (or it is protected) — nothing was touched",
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
        // What makes `dev_abandoned` a UNIT is not the shape of its name — it is
        // the record this project holds for it. The fixture used to create only
        // the branch, which is why every door here could be satisfied by a name
        // that merely looked like a unit.
        std::fs::create_dir_all(root.join(".claude").join("spec").join("abandoned"))
            .expect("unit record");
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

        // A branch that is NOBODY's unit is a base as far as this question
        // goes. It used to be refused for the sole reason that `git.flow` does
        // not list it — and the installer writes no flow, so that refusal fired
        // on every branch a real project integrates through.
        git(root, &["checkout", "dev"]);
        git(root, &["checkout", "-b", "loose-branch"]);
        let loose = delete_at(root, "dev_abandoned");
        assert_eq!(loose["ok"], json!(true), "an undeclared base is still outside the unit: {loose}");
        assert!(!branch_exists(root, "dev_abandoned"), "the unit was retired from it");
    }

    /// AC-3 — a declared integration base whose NAME carries a slash
    /// (`release/2026-Q3`) parses into a first segment that reads as a kind and
    /// a second that reads as a slug, exactly like `feature/aba` does. So the
    /// project's own release line answered "somebody's work unit", and both
    /// doors acted on that answer: this one OFFERED to delete it, and `pr list`
    /// REFUSED to run from it.
    ///
    /// Behaviour, not source text: every assertion below is a command's own
    /// report plus what the repository looks like afterwards.
    #[test]
    fn a_slashed_integration_base_is_never_deleted_and_never_refused() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"release/2026-Q3"}}}"#,
        )
        .expect("cfg");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);
        git(root, &["branch", "release/2026-Q3"]);
        git(root, &["branch", "feature/na-linha"]);
        // The unit has a RECORD; the release line has none. That asymmetry — not
        // the shape of either name — is what the two doors below must read.
        std::fs::create_dir_all(root.join(".claude").join("spec").join("na-linha"))
            .expect("unit record");

        // Standing ON the slashed base, the PR door does not refuse. `gh` is
        // absent here, which is reported as `ghError` — never as a refusal.
        git(root, &["checkout", "release/2026-Q3"]);
        let listed = crate::commands::review::pr_door::list_at(root);
        assert!(
            listed.ok,
            "`pr list` refused from the project's own base: {:?} / {:?}",
            listed.reason, listed.hint
        );
        assert_eq!(listed.branch, "release/2026-Q3");

        // …and the base itself is not deletable, from there or anywhere: a base
        // is nobody's unit, whatever the shape of its name.
        let refused = delete_at(root, "release/2026-Q3");
        assert_eq!(
            refused["ok"],
            json!(false),
            "the declared release line was accepted for deletion: {refused}"
        );
        assert_eq!(refused["reason"], json!("not-a-work-unit"));
        assert!(
            branch_exists(root, "release/2026-Q3"),
            "the project's release line was deleted"
        );

        // A REAL unit still goes, from that same slashed base — the door works
        // while standing on it, which is the other half of the criterion.
        let done = delete_at(root, "feature/na-linha");
        assert_eq!(done["ok"], json!(true), "report: {done}");
        assert_eq!(done["action"], json!("deleted"));
        assert!(!branch_exists(root, "feature/na-linha"), "the unit's branch survived");

        // --- and now the shape the INSTALLER writes: no `git.flow` at all ----
        //
        // The section above declares the release line, and a guard reading the
        // declared set passes it. That is exactly how this defect survived two
        // fix rounds: `mustard init` writes no flow, so the declared set falls
        // back to the hardcoded `{main, master}` and protects two literals and
        // nothing else. Measured against this shape before the record test
        // existed, `git delete` answered `remoteDeleted: true` and the release
        // line was gone.
        let bare = tempdir().expect("tempdir");
        let bare_root = bare.path();
        git(bare_root, &["init", "."]);
        git(bare_root, &["config", "user.email", "t@t"]);
        git(bare_root, &["config", "user.name", "t"]);
        git(bare_root, &["checkout", "-b", "dev"]);
        std::fs::write(bare_root.join("mustard.json"), r#"{"git":{"provider":"github"}}"#)
            .expect("cfg");
        git(bare_root, &["add", "-A"]);
        git(bare_root, &["commit", "-m", "seed"]);
        git(bare_root, &["branch", "release/2026-Q3"]);
        git(bare_root, &["branch", "hml_prod"]);
        git(bare_root, &["branch", "feature/real"]);
        std::fs::create_dir_all(bare_root.join(".claude").join("spec").join("real"))
            .expect("unit record");

        for base in ["release/2026-Q3", "hml_prod"] {
            let refused = delete_at(bare_root, base);
            assert_eq!(
                refused["reason"],
                json!("not-a-work-unit"),
                "a project's own base was accepted for deletion with no flow declared: \
                 {refused}",
            );
            assert!(branch_exists(bare_root, base), "`{base}` was deleted");
        }

        // …and the unit is still deletable there — the fix may not be "refuse
        // everything", which is the other way to make the assertions above pass.
        let done = delete_at(bare_root, "feature/real");
        assert_eq!(done["ok"], json!(true), "the real unit was refused: {done}");
        assert!(!branch_exists(bare_root, "feature/real"), "the unit's branch survived");
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

    /// The two refusals that guard the same edge from the other side: a name
    /// that is nobody's work unit is never deleted, and a name no ref carries
    /// is a typo.
    #[test]
    fn git_delete_refuses_a_base_and_reports_an_unknown_unit() {
        let dir = repo();
        let root = dir.path();
        // Both names must EXIST for these assertions to be about the reading
        // rather than about absence: a name no ref carries is answered
        // `no-such-unit` first, and that is a different claim from "this is a
        // base". The fixture never created them, so the two cases below were
        // passing for the wrong reason.
        git(root, &["branch", "main"]);
        git(root, &["branch", "squad-b-integration"]);

        let base = delete_at(root, "main");
        assert_eq!(base["ok"], json!(false));
        assert_eq!(base["reason"], json!("not-a-work-unit"));

        // And the refusal does NOT come from a declared list: a base this
        // project's `git.flow` never mentions is refused on the same reading.
        let undeclared = delete_at(root, "squad-b-integration");
        assert_eq!(undeclared["reason"], json!("not-a-work-unit"), "{undeclared}");

        let typo = delete_at(root, "dev_never-existed");
        assert_eq!(typo["ok"], json!(false));
        assert_eq!(typo["reason"], json!("no-such-unit"), "a typo is never answered with success");
        assert!(branch_exists(root, "dev_abandoned"), "the real unit was not touched");

        let blank = delete_at(root, "   ");
        assert_eq!(blank["reason"], json!("no-unit"));
    }
}
