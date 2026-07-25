//! `mustard-rt run wave-reclaim` — the way BACK: fold a finished wave's commit
//! from its isolated agent checkout onto the work-unit branch.
//!
//! [`crate::commands::work_unit_open`] is the way IN (it cuts the agent
//! worktree from the work unit's HEAD); this is the way OUT. The documented
//! subagent lifecycle ends with the checkout still on disk — a worktree that
//! finished WITH changes is kept, and the periodic sweep never touches one that
//! still holds work. Nothing merges it anywhere. That is right for the shape the
//! platform docs describe (independent copies that each open their own pull
//! request) and wrong for this pipeline, whose waves converge on ONE work-unit
//! branch: wave 3 must see what waves 1 and 2 produced.
//!
//! ## The fold
//!
//! Because the cut descends from the unit's HEAD, the common case is a
//! fast-forward. Two waves of the same round diverge from a shared point, so the
//! second one needs a real merge — `git merge` picks whichever applies. The
//! merge runs in the MAIN checkout, on the unit branch, one checkout at a time
//! (`wave-done` calls this per wave, in completion order).
//!
//! ## Posture: fail CLOSED
//!
//! This is a verdict about integrity, exactly like [`crate::commands::git_settle`]'s
//! merge check — not telemetry. Anything that would strand work returns
//! `{ok:false, reason, files:[…]}` and `wave-done` refuses to emit the
//! completion: a conflict (the unmerged paths are named), an agent checkout
//! carrying UNCOMMITTED work (it would be destroyed by the prune), a detached or
//! non-unit HEAD (the fold would land on an integration base), or several agent
//! checkouts that cannot be attributed to this wave. Never swallow, never force,
//! never `-X ours`.
//!
//! Nothing is destroyed on failure: a conflicted merge is aborted so the main
//! checkout is left as it was, and the agent checkout is preserved byte for byte
//! for the operator to inspect. The prune runs ONLY after a proven fold — the
//! same "prove it merged, only then prune" order `git-settle` uses, verified
//! here by re-asking git whether the agent branch still holds anything the unit
//! lacks.
//!
//! ## The clean no-op
//!
//! With isolation off, no agent checkout exists and this answers
//! `{ok:true, action:"nothing-to-reclaim"}` without touching the repository, so
//! the shared-tree pipeline keeps working byte for byte.

use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::ClaudePaths;
use serde_json::{json, Value};

use crate::commands::git_settle::{git_ok, git_out, main_checkout_root, parse_worktrees};
use crate::commands::pipeline::dispatch_plan::wave_declared_files;
use crate::commands::work_unit_open::{dirty_paths, AGENT_NAME_PREFIX};

/// Options for `mustard-rt run wave-reclaim`.
pub struct WaveReclaimOpts {
    /// Any directory inside the repo (worktrees welcome — the command resolves
    /// the main checkout itself). Defaults to the current dir.
    pub root: PathBuf,
    /// Parent spec slug under `.claude/spec/`.
    pub spec: String,
    /// Wave number (1-based).
    pub wave: u64,
}

/// One agent checkout that still holds work the unit branch lacks.
struct Candidate {
    /// Absolute path of the worktree, forward-slash normalised.
    path: String,
    /// The branch it has checked out.
    branch: String,
    /// How many commits it carries that the unit branch does not.
    commits: usize,
}

/// Run `git` in `dir`: stdout on success, stderr on failure. The merge step is
/// the one place whose FAILURE TEXT the operator must see, so it cannot go
/// through the boolean [`git_ok`].
fn git_run(dir: &Path, args: &[&str]) -> Result<String, String> {
    match Command::new("git").args(args).current_dir(dir).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// The paths git left UNMERGED after a failed merge, sorted. Empty when the
/// merge failed for a reason other than a content conflict (a dirty tree that
/// would be overwritten, a missing ref) — the caller reports the git message in
/// that case instead of an empty file list.
fn conflicting_paths(main: &Path) -> Vec<String> {
    let mut paths: Vec<String> = git_out(main, &["diff", "--name-only", "--diff-filter=U"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().replace('\\', "/"))
        .filter(|l| !l.is_empty())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// Every registered agent checkout (`.claude/worktrees/agent-*`) that carries at
/// least one commit `unit` does not, sorted by branch.
///
/// [`parse_worktrees`] already keeps only the harness-owned entries; the
/// `agent-` prefix on the DIRECTORY name is what separates a subagent checkout
/// from a work unit's own worktree — the same signal `worktree-gc` collects by,
/// and the one the branch name cannot carry (the harness may prefix it).
fn candidates(main: &Path, unit: &str) -> Vec<Candidate> {
    let entries = git_out(main, &["worktree", "list", "--porcelain"])
        .map(|s| parse_worktrees(&s))
        .unwrap_or_default();
    let mut out = Vec::new();
    for entry in entries {
        let is_agent = Path::new(&entry.path)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(AGENT_NAME_PREFIX));
        if !is_agent {
            continue;
        }
        let range = format!("^{unit}");
        let commits = git_out(main, &["rev-list", "--count", &entry.branch, &range])
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if commits > 0 {
            out.push(Candidate { path: entry.path, branch: entry.branch, commits });
        }
    }
    out
}

/// Narrow several simultaneous candidates down to this wave's, by the files its
/// sub-spec DECLARES: a candidate qualifies when its commits touch at least one
/// declared path. Waves dispatched in the same round have disjoint `## Files`
/// (that is what `wave-overlap-check` audits), so the declaration is the only
/// wave↔checkout link this repository actually persists — the harness names a
/// subagent worktree `agent-<id>` and records the id nowhere.
///
/// Returns every candidate when the wave declares no files: an empty filter must
/// never look like "no match" (the caller then fails closed on the ambiguity
/// rather than silently leaving work behind).
fn attributed_to_wave(main: &Path, opts: &WaveReclaimOpts, unit: &str, pool: Vec<Candidate>) -> Vec<Candidate> {
    let Ok(spec_dir) = ClaudePaths::for_project(main).and_then(|p| p.for_spec(&opts.spec)) else {
        return pool;
    };
    let wave = u32::try_from(opts.wave).unwrap_or(u32::MAX);
    // Role `""` never matches a directory exactly, so the resolver falls back to
    // the first `wave-{N}-*` dir — which is what the caller knows here.
    let declared = wave_declared_files(spec_dir.dir(), wave, "");
    if declared.is_empty() {
        return pool;
    }
    let matched: Vec<Candidate> = pool
        .into_iter()
        .filter(|c| {
            let range = format!("{unit}...{}", c.branch);
            let touched = git_out(main, &["diff", "--name-only", &range]).unwrap_or_default();
            touched
                .lines()
                .map(|l| l.trim().replace('\\', "/"))
                .any(|p| declared.iter().any(|d| d == &p))
        })
        .collect();
    matched
}

/// A worktree path as the report shows it: relative to the main checkout, so the
/// JSON stays byte-stable across machines (the crate's determinism Guard).
fn relative_to(main: &Path, path: &str) -> String {
    let main = main.to_string_lossy().replace('\\', "/");
    path.strip_prefix(&format!("{main}/")).unwrap_or(path).to_string()
}

/// The reclaim pass — the testable core of [`run`]. Never panics.
pub(crate) fn reclaim_at(opts: &WaveReclaimOpts) -> Value {
    let nothing = json!({ "ok": true, "action": "nothing-to-reclaim", "wave": opts.wave });
    // No repository → no worktrees → nothing to fold. The shared-tree pipeline
    // (and every unit test that drives `wave-done` outside a repo) must keep
    // working exactly as before.
    let Some(main) = main_checkout_root(&opts.root) else {
        return nothing;
    };

    // Ask for the checkouts BEFORE anything else can refuse: with isolation off
    // there are none, and the whole command is a no-op that never inspects the
    // branch it is standing on.
    let head = git_out(&main, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let head = head.trim().to_string();
    let any_agent = !candidates(&main, if head.is_empty() { "HEAD" } else { &head }).is_empty();
    if !any_agent {
        return nothing;
    }

    // From here on work EXISTS somewhere, so every unresolved precondition is a
    // refusal — never a silent skip.
    if head.is_empty() || head == "HEAD" {
        return json!({
            "ok": false,
            "reason": "detached-head",
            "wave": opts.wave,
            "files": [],
            "hint": "the main checkout is not on a branch — there is nowhere to fold the wave's work",
        });
    }
    let config = mustard_core::ProjectConfig::load(&main);
    let bases: Vec<String> = config.git.integration_bases().into_iter().collect();
    if !bases.iter().any(|b| head.starts_with(&format!("{b}_"))) {
        return json!({
            "ok": false,
            "reason": "not-a-work-unit",
            "wave": opts.wave,
            "unit": head,
            "bases": bases,
            "files": [],
            "hint": "the main checkout sits on an integration base — a wave's work is folded onto its work unit, never straight onto a base",
        });
    }

    let pool = candidates(&main, &head);
    let mut matched = attributed_to_wave(&main, opts, &head, pool);
    if matched.len() > 1 {
        matched.sort_by(|a, b| a.branch.cmp(&b.branch));
        return json!({
            "ok": false,
            "reason": "ambiguous-agent-checkout",
            "wave": opts.wave,
            "unit": head,
            "files": [],
            "candidates": matched.iter().map(|c| c.branch.clone()).collect::<Vec<_>>(),
            "hint": "more than one agent checkout holds work this wave could claim — fold them by hand and re-run",
        });
    }
    let Some(candidate) = matched.into_iter().next() else {
        // Agent checkouts exist, but none of them touches a file this wave
        // declares: their work belongs to another wave, which reclaims it on its
        // own `wave-done`.
        return nothing;
    };
    let worktree = Path::new(&candidate.path).to_path_buf();
    let shown = relative_to(&main, &candidate.path);

    // Uncommitted work in the agent checkout would NOT travel with the fold and
    // WOULD be destroyed by the prune. Refuse before touching anything.
    let dirty = dirty_paths(&worktree);
    if !dirty.is_empty() {
        let mut files = dirty;
        files.sort();
        return json!({
            "ok": false,
            "reason": "agent-checkout-dirty",
            "wave": opts.wave,
            "unit": head,
            "branch": candidate.branch,
            "worktree": shown,
            "files": files,
            "hint": "commit these paths inside the agent checkout, then re-run — they would not travel with the fold",
        });
    }

    // The fold. `merge` fast-forwards when the unit is an ancestor (the common
    // case, since the cut descends from the unit's HEAD) and builds a real merge
    // commit otherwise (two waves of the same round).
    let fast_forward =
        git_ok(&main, &["merge-base", "--is-ancestor", &head, &candidate.branch]);
    if let Err(error) = git_run(&main, &["merge", "--no-edit", &candidate.branch]) {
        let files = conflicting_paths(&main);
        // Leave the main checkout exactly as it was — a half-merged tree with
        // conflict markers is the "stranded work" this command exists to prevent.
        git_ok(&main, &["merge", "--abort"]);
        let reason = if files.is_empty() { "merge-refused" } else { "merge-conflict" };
        return json!({
            "ok": false,
            "reason": reason,
            "wave": opts.wave,
            "unit": head,
            "branch": candidate.branch,
            "worktree": shown,
            "files": files,
            "error": error,
            "hint": "the agent checkout is untouched — resolve the conflict there, or fold it by hand",
        });
    }

    // Prove it merged, only then prune: re-ask git whether the branch still
    // holds anything the unit lacks.
    let leftover = git_out(&main, &["rev-list", "--count", &candidate.branch, &format!("^{head}")])
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(1);
    if leftover > 0 {
        return json!({
            "ok": false,
            "reason": "fold-not-proven",
            "wave": opts.wave,
            "unit": head,
            "branch": candidate.branch,
            "worktree": shown,
            "files": [],
            "hint": "git reported the merge as done but the branch still carries commits the unit lacks — nothing was pruned",
        });
    }

    // Pruning is bookkeeping, not integrity: the work is already on the unit
    // branch, so a worktree the OS still holds open reports honestly instead of
    // blocking the wave (the same per-step reporting `git-settle` uses).
    let worktree_removed = git_ok(&main, &["worktree", "remove", &candidate.path]);
    let branch_deleted = worktree_removed && git_ok(&main, &["branch", "-D", &candidate.branch]);

    json!({
        "ok": true,
        "action": "reclaimed",
        "wave": opts.wave,
        "unit": head,
        "branch": candidate.branch,
        "worktree": shown,
        "commits": candidate.commits,
        "fastForward": fast_forward,
        "worktreeRemoved": worktree_removed,
        "branchDeleted": branch_deleted,
    })
}

/// Run `wave-reclaim`, print the single-line JSON report, and exit 1 when the
/// fold was refused — a blocked reclaim is a verdict the caller must not miss.
pub fn run(opts: WaveReclaimOpts) {
    let result = reclaim_at(&opts);
    let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
    println!("{}", serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()));
    if !ok {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repo on the work unit `dev_unit`, with ONE agent checkout cut from the
    /// unit's HEAD (exactly what `work_unit_open`'s hook produces) carrying one
    /// commit. `.claude/` is gitignored, so the worktree never reads as dirt.
    pub(super) fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&main).expect("mkdir");
        git(&main, &["init", "."]);
        git(&main, &["config", "user.email", "t@t"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-b", "dev"]);
        std::fs::write(main.join("mustard.json"), r#"{"git":{"flow":{"*":"dev"}}}"#).expect("cfg");
        std::fs::write(main.join(".gitignore"), ".claude/\n").expect("ignore");
        std::fs::write(main.join("a.txt"), "base\n").expect("seed");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "seed"]);
        git(&main, &["checkout", "-b", "dev_unit"]);
        git(&main, &["worktree", "add", ".claude/worktrees/agent-w1", "-b", "agent-w1"]);
        let wt = main.join(".claude").join("worktrees").join("agent-w1");
        (dir, main, wt)
    }

    fn opts(main: &Path) -> WaveReclaimOpts {
        WaveReclaimOpts { root: main.to_path_buf(), spec: "demo".into(), wave: 1 }
    }

    /// AC-5: a wave that finished in its own checkout has its commit folded onto
    /// the work-unit branch, so the NEXT wave starts from a tree containing it —
    /// and the checkout is pruned only once the fold is proven.
    #[test]
    fn wave_reclaim_folds_commit_onto_unit_branch() {
        let (_dir, main, wt) = fixture();
        std::fs::write(wt.join("wave.txt"), "wave work\n").expect("wave file");
        git(&wt, &["add", "-A"]);
        git(&wt, &["commit", "-m", "wave 1 work"]);
        let wave_sha = git_out(&wt, &["rev-parse", "HEAD"]).expect("wave head");

        let v = reclaim_at(&opts(&main));
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["action"], json!("reclaimed"), "{v}");
        assert_eq!(v["unit"], json!("dev_unit"), "{v}");
        assert_eq!(v["commits"], json!(1), "{v}");
        assert_eq!(v["fastForward"], json!(true), "cut from the unit's HEAD: {v}");
        assert_eq!(
            v["worktree"],
            json!(".claude/worktrees/agent-w1"),
            "path reported relative to the main checkout: {v}"
        );

        // The work really is on the unit branch, in the main checkout's tree.
        assert_eq!(git_out(&main, &["rev-parse", "HEAD"]).as_deref(), Some(wave_sha.as_str()));
        assert!(main.join("wave.txt").is_file(), "the next wave sees the file");
        // …and only then was the checkout pruned.
        assert_eq!(v["worktreeRemoved"], json!(true), "{v}");
        assert!(!wt.exists(), "agent checkout pruned after a proven fold");
        assert!(
            git_out(&main, &["branch", "--list", "agent-w1"]).unwrap_or_default().is_empty(),
            "the merged agent branch is deleted too"
        );

        // Idempotent: nothing left to reclaim.
        let again = reclaim_at(&opts(&main));
        assert_eq!(again["action"], json!("nothing-to-reclaim"), "{again}");
    }

    /// The clean no-op that keeps the shared-tree pipeline byte-identical while
    /// isolation is still off: no agent checkout at all, and no repository at
    /// all, both answer `nothing-to-reclaim` without inspecting anything.
    #[test]
    fn reclaim_without_an_agent_checkout_is_a_no_op() {
        let (_dir, main, wt) = fixture();
        git(&main, &["worktree", "remove", wt.to_string_lossy().as_ref()]);
        let v = reclaim_at(&opts(&main));
        assert_eq!(v, json!({ "ok": true, "action": "nothing-to-reclaim", "wave": 1 }), "{v}");

        let empty = tempdir().expect("tempdir");
        let v = reclaim_at(&WaveReclaimOpts { root: empty.path().to_path_buf(), ..opts(&main) });
        assert_eq!(v["action"], json!("nothing-to-reclaim"), "no repo, no fold: {v}");
    }

    /// Uncommitted work inside the agent checkout would not travel with the fold
    /// and would be destroyed by the prune — refuse before touching anything.
    #[test]
    fn reclaim_refuses_a_dirty_agent_checkout() {
        let (_dir, main, wt) = fixture();
        std::fs::write(wt.join("wave.txt"), "committed\n").expect("wave file");
        git(&wt, &["add", "-A"]);
        git(&wt, &["commit", "-m", "wave 1 work"]);
        std::fs::write(wt.join("stray.txt"), "never committed\n").expect("stray");

        let v = reclaim_at(&opts(&main));
        assert_eq!(v["ok"], json!(false), "{v}");
        assert_eq!(v["reason"], json!("agent-checkout-dirty"), "{v}");
        assert_eq!(v["files"], json!(["stray.txt"]), "{v}");
        assert!(wt.exists(), "nothing destroyed");
        assert!(!main.join("wave.txt").exists(), "and nothing folded");
    }

    /// A checkout whose work belongs to an integration base has nowhere safe to
    /// go: the fold must never land straight on `dev`.
    #[test]
    fn reclaim_refuses_when_the_main_checkout_sits_on_a_base() {
        let (_dir, main, wt) = fixture();
        std::fs::write(wt.join("wave.txt"), "wave work\n").expect("wave file");
        git(&wt, &["add", "-A"]);
        git(&wt, &["commit", "-m", "wave 1 work"]);
        git(&main, &["checkout", "dev"]);

        let v = reclaim_at(&opts(&main));
        assert_eq!(v["reason"], json!("not-a-work-unit"), "{v}");
        assert_eq!(v["unit"], json!("dev"), "{v}");
        assert!(wt.exists(), "nothing destroyed");
    }
}
