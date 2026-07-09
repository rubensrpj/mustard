//! `mustard-rt run git-settle` — post-merge housekeeping for delivered work
//! units, answering "the PR merged; now what?" deterministically:
//!
//! 1. **Fetch** every integration base (`mustard.json#git.flow`).
//! 2. **Fast-forward the base you are on** — only when the MAIN checkout sits
//!    on a base with a CLEAN tree (a dirty tree or a diverged base is reported,
//!    never touched).
//! 3. **Prune settled work units**: every `.claude/worktrees/` worktree whose
//!    branch already landed on its base — ancestor check first, `gh pr list
//!    --state merged` as the squash-merge fallback (this repo squash-merges,
//!    which breaks pure ancestry) — has its worktree removed and its local
//!    branch deleted; the remote branch delete is attempted fail-open (GitHub
//!    auto-delete usually got there first).
//!
//! A dirty worktree, the worktree this process runs inside, or an unmerged
//! branch is SKIPPED with its reason — settle never forces, never destroys
//! unmerged work (the /git iron law: only reversible operations).
//!
//! Output: one JSON report (sorted arrays, no timestamps). Fail-open
//! everywhere: a missing remote, absent `gh`, or a locked path degrades to a
//! skip entry, exit 0.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

/// Run `git` in `dir`, returning stdout on success.
fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(dir).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run `git` in `dir`, success as a bool.
fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Resolve the MAIN checkout root from anywhere inside the repo — including
/// from inside a linked worktree (`--git-common-dir` names the shared `.git`).
fn main_checkout_root(from: &Path) -> Option<PathBuf> {
    let common = git_out(from, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let common = PathBuf::from(common);
    if common.file_name().and_then(|n| n.to_str()) == Some(".git") {
        common.parent().map(Path::to_path_buf)
    } else {
        // Bare/odd layout — fall back to the toplevel of `from`.
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
struct WorktreeEntry {
    path: String,
    branch: String,
}

/// Parse `git worktree list --porcelain` into the harness-owned entries only
/// (paths under `.claude/worktrees/`, forward-slash normalized). Detached or
/// branchless entries are ignored.
fn parse_worktrees(porcelain: &str) -> Vec<WorktreeEntry> {
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
/// squash-merge fallback via `gh pr list --state merged` (fail-open — no `gh`,
/// no network, no PR → false, the branch is simply not settled yet).
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

/// The settle pass against an explicit repo root — the testable core of
/// [`run`]. Never panics; every failure lands as a `skipped` reason.
pub(crate) fn settle_at(start: &Path) -> Value {
    let Some(main) = main_checkout_root(start) else {
        return json!({ "ok": false, "reason": "not-a-git-repo" });
    };
    // BTreeSet → Vec keeps the deterministic (sorted) order downstream.
    let bases: Vec<String> =
        mustard_core::ProjectConfig::load(&main).git.integration_bases().into_iter().collect();

    // One fetch for every base — fail-open (offline settle still prunes by
    // whatever origin/<base> is already known locally).
    let mut fetch_args: Vec<&str> = vec!["fetch", "origin"];
    fetch_args.extend(bases.iter().map(String::as_str));
    let fetched = git_ok(&main, &fetch_args);

    // --- Prune settled worktrees -------------------------------------------
    let cwd = std::env::current_dir().unwrap_or_default().to_string_lossy().replace('\\', "/");
    let mut settled: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let entries = git_out(&main, &["worktree", "list", "--porcelain"])
        .map(|s| parse_worktrees(&s))
        .unwrap_or_default();
    for e in entries {
        let Some(base) = base_of_branch(&e.branch, &bases) else {
            skipped.push(json!({ "branch": e.branch, "reason": "no-base-prefix" }));
            continue;
        };
        if cwd.starts_with(&e.path) {
            skipped.push(json!({ "branch": e.branch, "reason": "current-worktree" }));
            continue;
        }
        let dirty = git_out(Path::new(&e.path), &["status", "--porcelain"])
            .map(|s| !s.is_empty())
            .unwrap_or(true);
        if dirty {
            skipped.push(json!({ "branch": e.branch, "reason": "dirty" }));
            continue;
        }
        if !is_merged(&main, &e.branch, &base) {
            skipped.push(json!({ "branch": e.branch, "reason": "not-merged" }));
            continue;
        }
        if !git_ok(&main, &["worktree", "remove", &e.path]) {
            skipped.push(json!({ "branch": e.branch, "reason": "worktree-remove-failed" }));
            continue;
        }
        // Merged is confirmed above, so -D is safe (squash merges make -d
        // refuse even for delivered work).
        let branch_deleted = git_ok(&main, &["branch", "-D", &e.branch]);
        // GitHub's auto-delete usually beat us to it — attempt, never insist.
        let remote_deleted = git_ok(&main, &["push", "origin", "--delete", &e.branch]);
        settled.push(json!({
            "branch": e.branch,
            "base": base,
            "worktree": e.path,
            "branchDeleted": branch_deleted,
            "remoteDeleted": remote_deleted,
        }));
    }

    // --- Fast-forward the base under our feet ------------------------------
    let current = git_out(&main, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let base_report = if bases.iter().any(|b| b == &current) {
        // Dirty check for the BASE ignores the harness-owned worktrees dir:
        // in a repo that does not gitignore it, the linked worktrees would
        // read as untracked and permanently veto the fast-forward — but that
        // dir is exactly what settle manages, never user work at risk.
        let clean = git_out(&main, &["status", "--porcelain"])
            .map(|s| {
                !s.lines().any(|l| {
                    l.len() > 3
                        && !l[3..].trim_start().replace('\\', "/").starts_with(".claude/worktrees/")
                })
            })
            .unwrap_or(false);
        if !clean {
            json!({ "branch": current, "updated": false, "reason": "dirty-tree" })
        } else if git_ok(&main, &["merge", "--ff-only", &format!("origin/{current}")]) {
            json!({ "branch": current, "updated": true })
        } else {
            json!({ "branch": current, "updated": false, "reason": "non-ff-or-no-remote" })
        }
    } else {
        json!({ "branch": current, "updated": false, "reason": "not-on-a-base" })
    };

    // Update the OTHER local bases without checking them out: `git fetch
    // origin <base>:<base>` fast-forwards a non-checked-out local ref and
    // REFUSES anything that is not a clean ff — so a base you are not sitting
    // on never goes stale, and nothing risky can happen to it. The base under
    // HEAD is excluded here (its ff went through `merge --ff-only` above; git
    // refuses the refspec form for the checked-out branch anyway).
    let other_bases: Vec<Value> = bases
        .iter()
        .filter(|b| *b != &current)
        .map(|b| {
            let updated = git_ok(&main, &["fetch", "origin", &format!("{b}:{b}")]);
            json!({ "branch": b, "updated": updated })
        })
        .collect();

    json!({
        "ok": true,
        "fetched": fetched,
        "base": base_report,
        "otherBases": other_bases,
        "settled": settled,
        "skipped": skipped,
    })
}

/// Run `git-settle` from `root` and print the JSON report.
pub fn run(root: &Path) {
    let result = settle_at(root);
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

    /// End-to-end on a real repo: a merged work unit is pruned (worktree +
    /// local branch), an unmerged one is skipped, and the base fast-forwards
    /// to origin. Mirrors the house pattern of real-git tempdir fixtures.
    #[test]
    fn settles_merged_worktree_skips_unmerged_and_ffs_base() {
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
        // Ignore the harness dir — a linked worktree under `.claude/worktrees/`
        // must not read as an untracked (dirty) path in the MAIN tree, exactly
        // like real projects exclude harness runtime paths.
        std::fs::write(main.join(".gitignore"), ".claude/\n").expect("ignore");
        std::fs::write(main.join("a.txt"), "a").expect("seed");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "seed"]);
        git(&main, &["remote", "add", "origin", bare.to_string_lossy().as_ref()]);
        git(&main, &["push", "-u", "origin", "dev"]);

        // Work unit 1 — merged into dev and pushed to origin.
        git(&main, &["worktree", "add", ".claude/worktrees/dev_done", "-b", "dev_done"]);
        let wt1 = main.join(".claude").join("worktrees").join("dev_done");
        std::fs::write(wt1.join("done.txt"), "x").expect("wt file");
        git(&wt1, &["add", "-A"]);
        git(&wt1, &["commit", "-m", "done work"]);
        git(&main, &["merge", "--no-ff", "dev_done", "-m", "merge dev_done"]);
        git(&main, &["push", "origin", "dev"]);
        // Rewind the LOCAL base one merge so settle has something to ff.
        git(&main, &["reset", "--hard", "HEAD~1"]);

        // Work unit 2 — never merged; must be skipped untouched.
        git(&main, &["worktree", "add", ".claude/worktrees/dev_open", "-b", "dev_open"]);
        let wt2 = main.join(".claude").join("worktrees").join("dev_open");
        std::fs::write(wt2.join("open.txt"), "y").expect("wt file");
        git(&wt2, &["add", "-A"]);
        git(&wt2, &["commit", "-m", "open work"]);

        let report = settle_at(&main);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(report["base"]["updated"], json!(true), "base ff'd to origin: {report}");
        let settled: Vec<&str> =
            report["settled"].as_array().expect("settled").iter().filter_map(|s| s["branch"].as_str()).collect();
        assert_eq!(settled, vec!["dev_done"], "{report}");
        let skipped: Vec<(&str, &str)> = report["skipped"]
            .as_array()
            .expect("skipped")
            .iter()
            .filter_map(|s| Some((s["branch"].as_str()?, s["reason"].as_str()?)))
            .collect();
        assert!(skipped.contains(&("dev_open", "not-merged")), "{report}");
        assert!(!wt1.exists(), "merged worktree pruned");
        assert!(wt2.exists(), "unmerged worktree untouched");
        assert!(
            git_out(&main, &["branch", "--list", "dev_done"]).unwrap_or_default().is_empty(),
            "merged local branch deleted"
        );
        // Local dev now equals origin/dev (the merge came back via ff).
        let local = git_out(&main, &["rev-parse", "dev"]).expect("local");
        let remote = git_out(&main, &["rev-parse", "origin/dev"]).expect("remote");
        assert_eq!(local, remote, "base fast-forwarded");
    }
}
