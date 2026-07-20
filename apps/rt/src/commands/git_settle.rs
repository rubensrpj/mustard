//! `mustard-rt run git-settle` — the EXIT RITUAL of a delivered work unit,
//! answering "the PR merged; now what?" with the user's exact contract:
//!
//! 1. **Runs from the WORK BRANCH** — invoked bare while sitting on an
//!    integration base (`dev`/`main`) it REFUSES (`on-integration-base`):
//!    settle is how a unit leaves the stage, not a base-side sweeper. From a
//!    base it only runs with an explicit `--unit <branch>` (the finish step
//!    of the dance below).
//! 2. **100% merged or nothing**: the unit's branch must verifiably be on its
//!    base — true ancestry first, `gh pr list --state merged` as the
//!    squash-merge fallback (this repo squash-merges, which breaks pure
//!    ancestry). Not merged → `{ok:false, reason:"not-merged"}` and NOTHING
//!    is touched.
//! 3. **Back to an up-to-date base**: every local base advances — the one the
//!    MAIN checkout sits on via `merge --ff-only` (clean tree only; the
//!    harness-owned `.claude/worktrees/` dir is exempt from the dirty check),
//!    every other via `git fetch origin <base>:<base>`, which fast-forwards a
//!    non-checked-out ref and refuses anything unsafe.
//! 4. **Only then prune**: the unit's worktree is removed and its local
//!    branch deleted (`-D` — merge is already proven), remote delete
//!    attempted fail-open (GitHub auto-delete usually got there first). When
//!    the process runs INSIDE the unit's worktree it cannot remove its own
//!    floor: it verifies + updates and answers `action:"exit-and-rerun"` —
//!    leave the worktree (`ExitWorktree`), then finish with
//!    `git-settle --unit <branch>` from the main checkout. An IN-PLACE unit
//!    (cut by the work-branch gate on the MAIN checkout — no worktree) has no
//!    floor to leave and no `ExitWorktree` to run: settle itself performs the
//!    exit — check out the base (the ff advance above is the "pull"), then
//!    delete the branch. A checkout git refuses degrades to `"partial"`.
//!
//! Output: one JSON report (sorted arrays, no timestamps). Fail-open
//! everywhere: absent `gh`, no remote, or a locked path degrades to an
//! honest field, exit 0 — but the MERGE VERIFICATION itself is a hard gate,
//! never fail-open (guarding a verdict: missing evidence blocks).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

/// Run `git` in `dir`, returning stdout on success.
pub(crate) fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(dir).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run `git` in `dir`, success as a bool.
pub(crate) fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Resolve the MAIN checkout root from anywhere inside the repo — including
/// from inside a linked worktree (`--git-common-dir` names the shared `.git`).
pub(crate) fn main_checkout_root(from: &Path) -> Option<PathBuf> {
    let common = git_out(from, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let common = PathBuf::from(common);
    if common.file_name().and_then(|n| n.to_str()) == Some(".git") {
        common.parent().map(Path::to_path_buf)
    } else {
        git_out(from, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
    }
}

/// The base a work branch integrates into, read from its `{base}_` prefix
/// (tolerating the harness's `worktree-` prefix). `None` when the prefix names
/// no known base — such a branch is never settled.
fn base_of_branch(branch: &str, bases: &[String]) -> Option<String> {
    let name = branch.strip_prefix("worktree-").unwrap_or(branch);
    let (prefix, _) = name.split_once('_')?;
    bases.iter().find(|b| b.as_str() == prefix).cloned()
}

/// One `.claude/worktrees/` entry from `git worktree list --porcelain`.
#[derive(Debug, PartialEq)]
pub(crate) struct WorktreeEntry {
    pub(crate) path: String,
    pub(crate) branch: String,
}

/// Parse `git worktree list --porcelain` into the harness-owned entries only
/// (paths under `.claude/worktrees/`, forward-slash normalized). Detached or
/// branchless entries are ignored.
pub(crate) fn parse_worktrees(porcelain: &str) -> Vec<WorktreeEntry> {
    let mut out = Vec::new();
    let mut path: Option<String> = None;
    for line in porcelain.lines().chain(std::iter::once("")) {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.trim().replace('\\', "/"));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if let Some(p) = path.clone() {
                if p.contains("/.claude/worktrees/") {
                    out.push(WorktreeEntry { path: p, branch: b.trim().to_string() });
                }
            }
        } else if line.is_empty() {
            path = None;
        }
    }
    out.sort_by(|a, b| a.branch.cmp(&b.branch));
    out
}

/// Whether `branch` already landed on `origin/<base>`: true ancestry first;
/// squash-merge fallback via `gh pr list --state merged`. This is the 100%
/// gate — no evidence means NOT merged (conservative, never fail-open).
fn is_merged(main: &Path, branch: &str, base: &str) -> bool {
    if git_ok(main, &["merge-base", "--is-ancestor", branch, &format!("origin/{base}")]) {
        return true;
    }
    Command::new("gh")
        .args(["pr", "list", "--head", branch, "--state", "merged", "--limit", "1", "--json", "number", "--jq", ".[0].number"])
        .current_dir(main)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// Advance every local base: the MAIN checkout's own branch via ff-only merge
/// (clean tree only, `.claude/worktrees/` exempt), every other base via the
/// ff-safe `fetch origin <base>:<base>`. Returns (report-of-current,
/// reports-of-others) — pure bookkeeping, all reversible.
fn update_bases(main: &Path, bases: &[String]) -> (Value, Vec<Value>) {
    let current = git_out(main, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let current_report = if bases.iter().any(|b| b == &current) {
        let clean = git_out(main, &["status", "--porcelain"])
            .map(|s| {
                !s.lines().any(|l| {
                    l.len() > 3
                        && !l[3..].trim_start().replace('\\', "/").starts_with(".claude/worktrees/")
                })
            })
            .unwrap_or(false);
        if !clean {
            json!({ "branch": current, "updated": false, "reason": "dirty-tree" })
        } else if git_ok(main, &["merge", "--ff-only", &format!("origin/{current}")]) {
            json!({ "branch": current, "updated": true })
        } else {
            json!({ "branch": current, "updated": false, "reason": "non-ff-or-no-remote" })
        }
    } else {
        json!({ "branch": current, "updated": false, "reason": "not-on-a-base" })
    };
    let others: Vec<Value> = bases
        .iter()
        .filter(|b| *b != &current)
        .map(|b| {
            let updated = git_ok(main, &["fetch", "origin", &format!("{b}:{b}")]);
            json!({ "branch": b, "updated": updated })
        })
        .collect();
    (current_report, others)
}

/// The settle pass — the testable core of [`run`]. `unit` = the work branch to
/// settle; `None` reads it from the invocation directory's HEAD (and REFUSES
/// when that is an integration base). Never panics.
pub(crate) fn settle_at(start: &Path, unit: Option<&str>) -> Value {
    let Some(main) = main_checkout_root(start) else {
        return json!({ "ok": false, "reason": "not-a-git-repo" });
    };
    let bases: Vec<String> =
        mustard_core::ProjectConfig::load(&main).git.integration_bases().into_iter().collect();

    // The user's contract: bare settle NEVER runs from a base — it is the
    // unit's exit ritual. `--unit` is the finish step, allowed anywhere.
    let inv_branch = git_out(start, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let unit_branch = match unit {
        Some(u) => u.trim().to_string(),
        None => {
            if bases.iter().any(|b| b == &inv_branch) {
                return json!({
                    "ok": false,
                    "reason": "on-integration-base",
                    "branch": inv_branch,
                    "hint": "pr close é o ritual de saída da unidade: rode a partir do BRANCH DE TRABALHO; numa base, só com --unit <branch>",
                });
            }
            inv_branch.clone()
        }
    };
    let Some(base) = base_of_branch(&unit_branch, &bases) else {
        return json!({ "ok": false, "reason": "no-base-prefix", "branch": unit_branch });
    };

    // One fetch for every base — fail-open (offline still verifies against
    // whatever origin/<base> is already known locally; the merge gate below
    // stays conservative either way).
    let mut fetch_args: Vec<&str> = vec!["fetch", "origin"];
    fetch_args.extend(bases.iter().map(String::as_str));
    let fetched = git_ok(&main, &fetch_args);

    // THE gate: 100% merged or nothing happens.
    if !is_merged(&main, &unit_branch, &base) {
        return json!({
            "ok": false,
            "reason": "not-merged",
            "branch": unit_branch,
            "base": base,
            "fetched": fetched,
            "hint": "nada foi tocado — mergeie o PR primeiro (squash é detectado via gh)",
        });
    }

    // Where the unit lives decides the exit. Read the worktree table BEFORE
    // advancing the bases: an IN-PLACE unit (no worktree — the work-branch
    // gate cut the branch on the MAIN checkout itself) has no ExitWorktree to
    // hand the session back to its base, so settle performs the exit here:
    // check out the base FIRST, so `update_bases` fast-forwards it (the
    // "pull") and the branch becomes deletable below. A checkout git refuses
    // (overlapping local changes) degrades honestly — nothing is ever forced.
    let entries = git_out(&main, &["worktree", "list", "--porcelain"])
        .map(|s| parse_worktrees(&s))
        .unwrap_or_default();
    let unit_entry = entries.iter().find(|e| e.branch == unit_branch);
    let main_head = git_out(&main, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let in_place = unit_entry.is_none() && main_head == unit_branch;
    let in_place_exited = in_place && git_ok(&main, &["checkout", &base]);

    // Merged confirmed → bring every local base up to date.
    let (base_report, other_bases) = update_bases(&main, &bases);

    // Prune the unit. Inside its own worktree the process cannot remove its
    // floor — verify+update happened; hand back the finish step.
    let cwd = std::env::current_dir().unwrap_or_default().to_string_lossy().replace('\\', "/");
    // Prune in three steps, each tried and reported on its OWN field. The only
    // real coupling: git refuses to delete a branch a worktree still checks out,
    // so the LOCAL delete waits for the floor to be clear (removed by us, or
    // already absent). The REMOTE delete does not depend on the worktree outcome
    // and runs either way — a worktree the OS still locks must never strand the
    // server branch. `worktreeRemoved` stays true only when WE removed it (an
    // already-absent worktree reports false, matching the prior happy path).
    let (action, worktree_removed, branch_deleted, remote_deleted) =
        if unit_entry.is_some_and(|e| cwd.starts_with(&e.path)) {
            // Inside our own worktree we cannot remove our floor; verify+update
            // already ran, so hand back the finish step and touch nothing else.
            ("exit-and-rerun", false, false, false)
        } else {
            let (worktree_removed, floor_clear) = match unit_entry {
                Some(e) => {
                    let removed = git_ok(&main, &["worktree", "remove", &e.path]);
                    (removed, removed)
                }
                // In-place: the "floor" is the unit branch checked out on the
                // MAIN checkout — clear only once the exit above landed.
                None if in_place => (false, in_place_exited),
                None => (false, true), // never removed by us, but already free to delete
            };
            let branch_deleted = floor_clear && git_ok(&main, &["branch", "-D", &unit_branch]);
            let remote_deleted = git_ok(&main, &["push", "origin", "--delete", &unit_branch]);
            // Floor clear → unit fully off the local stage: "settled". A leftover
            // worktree still blocking local cleanup → "partial"; the per-field
            // booleans tell the true story (remote may be gone while the worktree
            // and local branch remain).
            let action = if floor_clear { "settled" } else { "partial" };
            (action, worktree_removed, branch_deleted, remote_deleted)
        };

    // Other merged harness worktrees — informative only (settle acts on ONE
    // unit; the user decides about the rest, each via its own settle).
    let also_mergeable: Vec<String> = entries
        .iter()
        .filter(|e| e.branch != unit_branch)
        .filter(|e| base_of_branch(&e.branch, &bases).is_some_and(|b| is_merged(&main, &e.branch, &b)))
        .map(|e| e.branch.clone())
        .collect();

    json!({
        "ok": true,
        "unit": {
            "branch": unit_branch,
            "base": base,
            "merged": true,
            "inPlace": in_place,
            "action": action,
            "worktreeRemoved": worktree_removed,
            "branchDeleted": branch_deleted,
            "remoteDeleted": remote_deleted,
        },
        "baseCheckout": base_report,
        "otherBases": other_bases,
        "alsoMergeable": also_mergeable,
        "fetched": fetched,
    })
}

/// Run `git-settle` from `root` and print the JSON report.
pub fn run(root: &Path, unit: Option<&str>) {
    let result = settle_at(root, unit);
    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix() {
        let bases = vec!["dev".to_string(), "main".to_string()];
        assert_eq!(base_of_branch("dev_fix-thing", &bases).as_deref(), Some("dev"));
        assert_eq!(base_of_branch("worktree-dev_fix-thing", &bases).as_deref(), Some("dev"));
        assert_eq!(base_of_branch("main_hotfix", &bases).as_deref(), Some("main"));
        assert_eq!(base_of_branch("feature_x", &bases), None, "unknown prefix never settles");
        assert_eq!(base_of_branch("nounderscore", &bases), None);
    }

    #[test]
    fn parse_worktrees_keeps_only_harness_owned_entries_sorted() {
        let porcelain = "worktree C:/repo\nHEAD abc\nbranch refs/heads/dev\n\n\
                         worktree C:/repo/.claude/worktrees/dev_b\nHEAD def\nbranch refs/heads/dev_b\n\n\
                         worktree C:/repo/.claude/worktrees/dev_a\nHEAD ghi\nbranch refs/heads/worktree-dev_a\n\n\
                         worktree C:/elsewhere/wt\nHEAD jkl\nbranch refs/heads/dev_c\n";
        let got = parse_worktrees(porcelain);
        assert_eq!(
            got,
            vec![
                WorktreeEntry { path: "C:/repo/.claude/worktrees/dev_b".into(), branch: "dev_b".into() },
                WorktreeEntry { path: "C:/repo/.claude/worktrees/dev_a".into(), branch: "worktree-dev_a".into() },
            ],
            "main checkout and foreign paths excluded; sorted by branch"
        );
    }

    /// Build the two-unit fixture: a bare origin, a main checkout on `dev`
    /// (with `.claude/` gitignored so worktrees never read as dirt), one unit
    /// MERGED into origin/dev (`dev_done`), one open (`dev_open`), and the
    /// local dev rewound one merge so settle has something to fast-forward.
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let bare = dir.path().join("origin.git");
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&bare).expect("mkdir bare");
        std::fs::create_dir_all(&main).expect("mkdir main");
        git(&bare, &["init", "--bare", "."]);
        git(&main, &["init", "."]);
        git(&main, &["config", "user.email", "t@t"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-b", "dev"]);
        std::fs::write(main.join("mustard.json"), r#"{"git":{"flow":{"*":"dev"}}}"#).expect("cfg");
        std::fs::write(main.join(".gitignore"), ".claude/\n").expect("ignore");
        std::fs::write(main.join("a.txt"), "a").expect("seed");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "seed"]);
        git(&main, &["remote", "add", "origin", bare.to_string_lossy().as_ref()]);
        git(&main, &["push", "-u", "origin", "dev"]);

        git(&main, &["worktree", "add", ".claude/worktrees/dev_done", "-b", "dev_done"]);
        let wt1 = main.join(".claude").join("worktrees").join("dev_done");
        std::fs::write(wt1.join("done.txt"), "x").expect("wt file");
        git(&wt1, &["add", "-A"]);
        git(&wt1, &["commit", "-m", "done work"]);
        git(&main, &["merge", "--no-ff", "dev_done", "-m", "merge dev_done"]);
        git(&main, &["push", "origin", "dev"]);
        git(&main, &["reset", "--hard", "HEAD~1"]);

        git(&main, &["worktree", "add", ".claude/worktrees/dev_open", "-b", "dev_open"]);
        let wt2 = main.join(".claude").join("worktrees").join("dev_open");
        std::fs::write(wt2.join("open.txt"), "y").expect("wt file");
        git(&wt2, &["add", "-A"]);
        git(&wt2, &["commit", "-m", "open work"]);

        (dir, main)
    }

    /// The user's contract, end to end: bare settle on a base REFUSES; settle
    /// of an UNMERGED unit refuses touching nothing; `--unit` of the merged
    /// one prunes worktree + branch and fast-forwards the base.
    #[test]
    fn contract_refuses_on_base_blocks_unmerged_and_settles_merged_unit() {
        let (_dir, main) = fixture();

        // (1) Bare settle from the base → refused (settle is the unit's exit
        // ritual, never a base-side command).
        let v = settle_at(&main, None);
        assert_eq!(v["ok"], json!(false), "{v}");
        assert_eq!(v["reason"], json!("on-integration-base"));

        // (2) The open unit, from ITS worktree — not merged → hard stop,
        // nothing touched.
        let wt2 = main.join(".claude").join("worktrees").join("dev_open");
        let v = settle_at(&wt2, None);
        assert_eq!(v["reason"], json!("not-merged"), "{v}");
        assert!(wt2.exists(), "unmerged worktree untouched");

        // (3) The merged unit via --unit from the base (the finish step) →
        // worktree pruned, branch gone, base fast-forwarded to origin.
        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["unit"]["action"], json!("settled"), "{v}");
        assert_eq!(v["unit"]["worktreeRemoved"], json!(true));
        assert_eq!(v["unit"]["branchDeleted"], json!(true));
        assert_eq!(v["baseCheckout"]["updated"], json!(true), "base ff'd: {v}");
        assert!(!main.join(".claude").join("worktrees").join("dev_done").exists());
        assert!(
            git_out(&main, &["branch", "--list", "dev_done"]).unwrap_or_default().is_empty(),
            "merged local branch deleted"
        );
        let local = git_out(&main, &["rev-parse", "dev"]).expect("local");
        let remote = git_out(&main, &["rev-parse", "origin/dev"]).expect("remote");
        assert_eq!(local, remote, "base fast-forwarded");
    }

    /// Partial failure: a worktree git refuses to remove (here LOCKED, a stand-in
    /// for the OS still holding the folder open) must not strand the remote branch.
    /// Each step tries and reports on its own field — the remote delete runs even
    /// though worktree-remove and the (still-checked-out) local delete both fail.
    #[test]
    fn partial_failure_still_deletes_remote_and_reports_each_step() {
        let (_dir, main) = fixture();
        git(&main, &["push", "origin", "dev_done"]);
        let wt = main.join(".claude").join("worktrees").join("dev_done");
        git(&main, &["worktree", "lock", wt.to_string_lossy().as_ref()]);

        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["unit"]["action"], json!("partial"), "{v}");
        assert_eq!(v["unit"]["worktreeRemoved"], json!(false), "{v}");
        assert_eq!(v["unit"]["branchDeleted"], json!(false), "{v}");
        assert_eq!(v["unit"]["remoteDeleted"], json!(true), "{v}"); // the fix
        assert!(wt.exists(), "leftover worktree preserved (removal really failed)");
        assert!(
            !git_out(&main, &["branch", "--list", "dev_done"]).unwrap_or_default().is_empty(),
            "local branch kept (worktree still holds it checked out)"
        );
        assert!(
            git_out(&main, &["ls-remote", "--heads", "origin", "dev_done"]).unwrap_or_default().is_empty(),
            "remote branch deleted independently of the worktree"
        );
    }

    /// The IN-PLACE unit (cut by the work-branch gate on the main checkout —
    /// no worktree): `--unit` invoked while the checkout still SITS on the
    /// unit branch must perform the whole exit — check out the base,
    /// fast-forward it, delete the local branch. Before the fix this answered
    /// baseCheckout `not-on-a-base` and `branchDeleted:false` (git refuses to
    /// delete a checked-out branch), stranding the session on the dead branch.
    #[test]
    fn in_place_unit_settles_by_checking_out_base_and_deleting_branch() {
        let (_dir, main) = fixture();
        // Re-align local dev with origin (the shared fixture rewound it), then
        // cut the unit ON the main checkout, merge it into origin/dev, rewind
        // local dev again, and leave the checkout sitting on the unit branch —
        // the exact post-merge state of an in-place unit.
        git(&main, &["merge", "--ff-only", "origin/dev"]);
        git(&main, &["checkout", "-b", "dev_inplace"]);
        std::fs::write(main.join("inplace.txt"), "z").expect("file");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "in-place work"]);
        git(&main, &["checkout", "dev"]);
        git(&main, &["merge", "--no-ff", "dev_inplace", "-m", "merge dev_inplace"]);
        git(&main, &["push", "origin", "dev"]);
        git(&main, &["reset", "--hard", "HEAD~1"]);
        git(&main, &["checkout", "dev_inplace"]);

        let v = settle_at(&main, Some("dev_inplace"));
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["unit"]["inPlace"], json!(true), "{v}");
        assert_eq!(v["unit"]["action"], json!("settled"), "{v}");
        assert_eq!(v["unit"]["branchDeleted"], json!(true), "{v}");
        assert_eq!(
            git_out(&main, &["rev-parse", "--abbrev-ref", "HEAD"]).as_deref(),
            Some("dev"),
            "main checkout handed back to the base"
        );
        assert_eq!(v["baseCheckout"]["branch"], json!("dev"), "{v}");
        assert_eq!(v["baseCheckout"]["updated"], json!(true), "base pulled (ff): {v}");
        assert!(
            git_out(&main, &["branch", "--list", "dev_inplace"]).unwrap_or_default().is_empty(),
            "in-place unit branch deleted after the exit"
        );
        let local = git_out(&main, &["rev-parse", "dev"]).expect("local");
        let remote = git_out(&main, &["rev-parse", "origin/dev"]).expect("remote");
        assert_eq!(local, remote, "base fast-forwarded to origin");
    }
}
