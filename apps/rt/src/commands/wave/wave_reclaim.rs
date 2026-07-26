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
//! merge runs on the unit branch, one checkout at a time (`wave-done` calls this
//! per wave, in completion order).
//!
//! ## Which unit, and in which tree
//!
//! The unit is read from the INVOKING tree, through the very same
//! [`crate::commands::work_unit_open::current_unit_branch`] the way IN uses —
//! never from whatever the main checkout happens to have out. `orchestrator.md`
//! puts the session inside `.claude/worktrees/{base}_{slug}` for the whole of
//! EXECUTE, so the main checkout is somewhere else entirely: reading it would
//! refuse every fold when it sits on an integration base, and would silently
//! land a wave's work on a BYSTANDER work unit when it sits on another one.
//! The merge then runs in that same invoking tree, which is by construction the
//! tree holding the branch we just read.
//!
//! ## Posture: fail CLOSED
//!
//! This is a verdict about integrity, exactly like [`crate::commands::git_settle`]'s
//! merge check — not telemetry. Anything that would strand work returns
//! `{ok:false, reason, files:[…]}` and `wave-done` refuses to emit the
//! completion: a conflict (the unmerged paths are named), an agent checkout
//! carrying UNCOMMITTED work (it would be destroyed by the prune), a detached or
//! non-unit HEAD (the fold would land on an integration base), several agent
//! checkouts this wave could claim, or agent checkouts NONE of which can be
//! attributed to it. Never swallow, never force, never `-X ours`.
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
//! the shared-tree pipeline keeps working byte for byte. That is the ONLY
//! `ok:true` no-op: once a checkout carrying unmerged work exists, silence would
//! strand it.

use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::ClaudePaths;
use serde_json::{json, Value};

use crate::commands::agent::render::sections::{normalise_path, same_file};
use crate::commands::git_settle::{git_ok, git_out, main_checkout_root, parse_worktrees};
use crate::commands::pipeline::dispatch_plan::wave_declared_files;
use crate::commands::work_unit_open::{current_unit_branch, dirty_paths, is_unit_worktree_name};

/// Options for `mustard-rt run wave-reclaim`.
pub struct WaveReclaimOpts {
    /// The INVOKING tree: any directory inside the repo (worktrees welcome —
    /// the command resolves the main checkout itself). Load-bearing, not just a
    /// locator: the work unit is the branch THIS tree has checked out, and the
    /// fold happens here. Defaults to the current dir.
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
    /// The ref the fold names: the branch it has checked out, or — when the
    /// checkout is DETACHED — its HEAD sha, which is the only handle such a
    /// checkout offers. Either way it is what `merge`/`rev-list` are given.
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
fn conflicting_paths(tree: &Path) -> Vec<String> {
    let mut paths: Vec<String> = git_out(tree, &["diff", "--name-only", "--diff-filter=U"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().replace('\\', "/"))
        .filter(|l| !l.is_empty())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// Every registered agent checkout that carries at least one commit `unit` does
/// not, sorted by the ref that names it.
///
/// [`parse_worktrees`] already keeps only the harness-owned entries
/// (`.claude/worktrees/…`); what separates a SUBAGENT's checkout from a work
/// unit's own worktree is the DIRECTORY name carrying a declared `{base}_`
/// prefix — [`is_unit_worktree_name`], the very question the way IN asks before
/// cutting. Asking it here of the same `mustard.json#git.flow` is what keeps
/// the two halves of the isolation from disagreeing.
///
/// It replaced an `agent-` prefix test that matched NOTHING: `WorktreeCreate`
/// hands over a slug (`recursing-benz-063389`, `feature-auth`, `pr-1234`), never
/// a prefixed name, so with isolation genuinely on this sweep found no
/// candidate and the command answered `nothing-to-reclaim` while the wave's
/// commit stayed in its checkout — a fail-open on the one metric this spec is
/// measured by.
///
/// A DETACHED checkout is included on its HEAD sha (see [`parse_worktrees`]):
/// it can hold commits just the same, and silence over it would strand them.
fn candidates(main: &Path, unit: &str, bases: &[String]) -> Vec<Candidate> {
    let entries = git_out(main, &["worktree", "list", "--porcelain"])
        .map(|s| parse_worktrees(&s))
        .unwrap_or_default();
    let mut out = Vec::new();
    for entry in entries {
        let is_unit_checkout = Path::new(&entry.path)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| is_unit_worktree_name(n, bases));
        if is_unit_checkout {
            continue;
        }
        // Detached → no branch name; its HEAD is the only ref there is.
        let git_ref = if entry.branch.is_empty() { entry.head.clone() } else { entry.branch.clone() };
        if git_ref.is_empty() {
            continue; // nothing nameable — never guess at a ref
        }
        let range = format!("^{unit}");
        let commits = git_out(main, &["rev-list", "--count", &git_ref, &range])
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if commits > 0 {
            out.push(Candidate { path: entry.path, branch: git_ref, commits });
        }
    }
    out.sort_by(|a, b| a.branch.cmp(&b.branch));
    out
}

/// Narrow several simultaneous candidates down to this wave's, by the files its
/// sub-spec DECLARES: a candidate qualifies when its commits touch at least one
/// declared path. Waves dispatched in the same round have disjoint `## Files`
/// (that is what `wave-overlap-check` audits), so the declaration is the only
/// wave↔checkout link this repository actually persists — the harness names a
/// subagent worktree with an opaque slug and records the id nowhere.
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
                .any(|p| declared.iter().any(|d| declared_covers(d, p)))
        })
        .collect();
    matched
}

/// Whether a DECLARED `## Files` entry covers a path the candidate's commits
/// touched.
///
/// String equality would drop the attribution on three spellings that name the
/// same work, and a dropped attribution now fails the wave closed:
///
/// - a declaration relative to the SUBPROJECT versus git's repo-relative
///   answer — resolved by [`same_file`], the crate's one segment-anchored
///   suffix relation (`## Files` entries reach the census through it too);
/// - a declared DIRECTORY (`apps/rt/src`) standing for the files under it —
///   the containment below, anchored on `/` in both directions so `rc/a.rs`
///   never covers `notsrc/a.rs`;
/// - a case-differing spelling on Windows, where the hand-written declaration
///   and git's recorded path name one file. Folded here rather than inside
///   [`same_file`], whose census callers deliberately keep case significant.
fn declared_covers(declared: &str, touched: &str) -> bool {
    let d = normalise_path(&declared.to_lowercase());
    let t = normalise_path(&touched.to_lowercase());
    if d.is_empty() || t.is_empty() {
        return false;
    }
    same_file(&d, &t) || t.starts_with(&format!("{d}/")) || t.contains(&format!("/{d}/"))
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

    // The tree the command was INVOKED in — during EXECUTE the session sits in
    // the work unit's own worktree, so this is where the unit branch is checked
    // out and where the merge has to happen. `--show-toplevel` normalises any
    // directory inside it to the worktree root.
    let unit_tree = git_out(&opts.root, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .unwrap_or_else(|| opts.root.clone());

    let config = mustard_core::ProjectConfig::load(&main);
    let bases: Vec<String> = config.git.integration_bases().into_iter().collect();
    // The SAME resolver the way in uses, asked of the SAME tree — the two halves
    // of the isolation cannot be allowed to disagree about which unit this is.
    let unit = current_unit_branch(&unit_tree, &bases);
    // What that tree is standing on, unit or not — only the refusal messages
    // need it, to tell a detached HEAD from a branch that is simply not a unit.
    let raw_head = git_out(&unit_tree, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let raw_head = raw_head.trim().to_string();

    // Ask for the checkouts BEFORE anything else can refuse: with isolation off
    // there are none, and the whole command is a no-op that never inspects the
    // branch it is standing on. A non-unit invoking tree still has a commit to
    // measure "carries work the unit lacks" against — its own HEAD.
    let exclude = match unit.clone() {
        Some(u) => u,
        None if !raw_head.is_empty() && raw_head != "HEAD" => raw_head.clone(),
        None => git_out(&unit_tree, &["rev-parse", "HEAD"]).unwrap_or_else(|| "HEAD".to_string()),
    };
    let pool = candidates(&main, &exclude, &bases);
    if pool.is_empty() {
        return nothing;
    }

    // From here on work EXISTS somewhere, so every unresolved precondition is a
    // refusal — never a silent skip.
    let Some(head) = unit else {
        if raw_head.is_empty() || raw_head == "HEAD" {
            return json!({
                "ok": false,
                "reason": "detached-head",
                "wave": opts.wave,
                "files": [],
                "hint": "the invoking tree is not on a branch — there is nowhere to fold the wave's work",
            });
        }
        return json!({
            "ok": false,
            "reason": "not-a-work-unit",
            "wave": opts.wave,
            "unit": raw_head,
            "bases": bases,
            "files": [],
            "hint": "the invoking tree sits on an integration base — a wave's work is folded onto its work unit, never straight onto a base",
        });
    };

    let mut all: Vec<String> = pool.iter().map(|c| c.branch.clone()).collect();
    all.sort();
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
        // Agent checkouts exist and carry commits the unit lacks, but nothing
        // links any of them to this wave. That is UNATTRIBUTED work, not absent
        // work: answering `nothing-to-reclaim` here would let `wave-done` report
        // the wave complete while a commit sits in a checkout nobody folds. Fail
        // closed, exactly like the several-matches case — the only difference is
        // how many candidates the operator has to look at.
        return json!({
            "ok": false,
            "reason": "unattributed-agent-checkout",
            "wave": opts.wave,
            "unit": head,
            "files": [],
            "candidates": all,
            "hint": "agent checkouts hold work no path this wave declares can claim — declare the paths in the wave's ## Files, or fold them by hand and re-run",
        });
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
    // In the INVOKING tree: that is the one with `head` checked out. Merging in
    // the main checkout would either fail (the branch is busy elsewhere) or, far
    // worse, advance whatever branch the main checkout happens to hold.
    if let Err(error) = git_run(&unit_tree, &["merge", "--no-edit", &candidate.branch]) {
        let files = conflicting_paths(&unit_tree);
        // Leave the unit's tree exactly as it was — a half-merged tree with
        // conflict markers is the "stranded work" this command exists to prevent.
        git_ok(&unit_tree, &["merge", "--abort"]);
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

    /// The name the harness really gives a subagent's isolated worktree: an
    /// opaque slug. Taken verbatim from this repository's own
    /// `.claude/worktrees/`; `WorktreeCreate#name` documents exactly this shape
    /// (user-given, `pr-<n>`, or auto-generated) and never a prefixed one.
    /// Every fixture below drives the SLUG, because an `agent-`-shaped fixture
    /// is the only reason the old prefix test ever looked alive.
    pub(super) const AGENT_SLUG: &str = "recursing-benz-063389";

    /// A repo on the work unit `dev_unit`, with ONE agent checkout named
    /// `name`, cut from the unit's HEAD (exactly what `work_unit_open`'s hook
    /// produces). `.claude/` is gitignored, so the worktree never reads as dirt.
    pub(super) fn fixture_named(name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
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
        git(&main, &["worktree", "add", &format!(".claude/worktrees/{name}"), "-b", name]);
        let wt = main.join(".claude").join("worktrees").join(name);
        (dir, main, wt)
    }

    /// [`fixture_named`] with the real slug shape.
    pub(super) fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        fixture_named(AGENT_SLUG)
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
            json!(format!(".claude/worktrees/{AGENT_SLUG}")),
            "path reported relative to the main checkout: {v}"
        );

        // The work really is on the unit branch, in the main checkout's tree.
        assert_eq!(git_out(&main, &["rev-parse", "HEAD"]).as_deref(), Some(wave_sha.as_str()));
        assert!(main.join("wave.txt").is_file(), "the next wave sees the file");
        // …and only then was the checkout pruned.
        assert_eq!(v["worktreeRemoved"], json!(true), "{v}");
        assert!(!wt.exists(), "agent checkout pruned after a proven fold");
        assert!(
            git_out(&main, &["branch", "--list", AGENT_SLUG]).unwrap_or_default().is_empty(),
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

    /// Commit `wave.txt` inside the agent checkout and answer its sha.
    fn commit_wave_work(wt: &Path, file: &str) -> String {
        let path = wt.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, "wave work\n").expect("wave file");
        git(wt, &["add", "-A"]);
        git(wt, &["commit", "-m", "wave 1 work"]);
        git_out(wt, &["rev-parse", "HEAD"]).expect("wave head")
    }

    /// Declare `## Files` for `wave-1-rt` of the `demo` spec.
    fn declare_files(main: &Path, files: &[&str]) {
        let dir = main.join(".claude/spec/demo/wave-1-rt");
        std::fs::create_dir_all(&dir).expect("wave dir");
        let mut body = String::from("## Files\n\n");
        for f in files {
            body.push_str(&format!("- {f}\n"));
        }
        std::fs::write(dir.join("spec.md"), body).expect("wave spec");
    }

    /// Move the main checkout off the unit and check the unit out in its OWN
    /// worktree — the shape `orchestrator.md` puts the session in during
    /// EXECUTE. Answers the unit worktree's path.
    fn move_unit_into_its_own_worktree(main: &Path, main_goes_to: &[&str]) -> PathBuf {
        git(main, main_goes_to);
        git(main, &["worktree", "add", ".claude/worktrees/dev_unit", "dev_unit"]);
        main.join(".claude").join("worktrees").join("dev_unit")
    }

    /// The unit comes from the INVOKING tree, not from whatever the main
    /// checkout happens to have out: with the session inside the unit's
    /// worktree and the main checkout on ANOTHER unit, the fold lands on the
    /// unit — never on the bystander branch.
    #[test]
    fn reclaim_folds_onto_the_invoking_trees_unit_not_the_main_checkouts() {
        let (_dir, main, wt) = fixture();
        let wave_sha = commit_wave_work(&wt, "wave.txt");
        let unit_tree = move_unit_into_its_own_worktree(&main, &["checkout", "-b", "dev_other"]);
        let bystander = git_out(&main, &["rev-parse", "dev_other"]).expect("dev_other");

        let v = reclaim_at(&WaveReclaimOpts { root: unit_tree.clone(), ..opts(&main) });
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["unit"], json!("dev_unit"), "the unit is the invoking tree's: {v}");
        assert_eq!(
            git_out(&main, &["rev-parse", "dev_unit"]).as_deref(),
            Some(wave_sha.as_str()),
            "the wave's commit is on the unit branch: {v}"
        );
        assert!(unit_tree.join("wave.txt").is_file(), "and in the tree the session sits in");
        assert_eq!(
            git_out(&main, &["rev-parse", "dev_other"]).as_deref(),
            Some(bystander.as_str()),
            "the branch the main checkout happened to hold is untouched"
        );
        assert!(!main.join("wave.txt").exists(), "nothing landed in the main checkout");
    }

    /// The documented EXECUTE flow with the main checkout left on an
    /// integration base: the fold still happens, because the unit is read from
    /// the tree the command was invoked in.
    #[test]
    fn reclaim_folds_from_a_worktree_while_the_main_checkout_sits_on_a_base() {
        let (_dir, main, wt) = fixture();
        let wave_sha = commit_wave_work(&wt, "wave.txt");
        let unit_tree = move_unit_into_its_own_worktree(&main, &["checkout", "dev"]);

        let v = reclaim_at(&WaveReclaimOpts { root: unit_tree.clone(), ..opts(&main) });
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["action"], json!("reclaimed"), "{v}");
        assert_eq!(v["unit"], json!("dev_unit"), "{v}");
        assert_eq!(
            git_out(&main, &["rev-parse", "dev_unit"]).as_deref(),
            Some(wave_sha.as_str()),
            "{v}"
        );
        assert!(unit_tree.join("wave.txt").is_file(), "the next wave sees the file");
    }

    /// Agent checkouts that exist but cannot be attributed to this wave are
    /// STRANDED work, not a no-op: they fail closed. The genuine no-op — no
    /// agent checkout at all — stays a clean pass.
    #[test]
    fn reclaim_fails_closed_when_no_checkout_can_be_attributed() {
        let (_dir, main, wt) = fixture();
        declare_files(&main, &["declared.txt"]);
        commit_wave_work(&wt, "undeclared.txt");

        let v = reclaim_at(&opts(&main));
        assert_eq!(v["ok"], json!(false), "stranded work is never a success: {v}");
        assert_eq!(v["reason"], json!("unattributed-agent-checkout"), "{v}");
        assert_eq!(v["candidates"], json!([AGENT_SLUG]), "{v}");
        assert!(wt.exists(), "nothing destroyed");
        assert!(!main.join("undeclared.txt").exists(), "and nothing folded");

        // The other direction: with the checkout gone there is no work at all,
        // and the shared-tree pipeline keeps its byte-identical clean pass.
        git(&main, &["worktree", "remove", "--force", wt.to_string_lossy().as_ref()]);
        let v = reclaim_at(&opts(&main));
        assert_eq!(v, json!({ "ok": true, "action": "nothing-to-reclaim", "wave": 1 }), "{v}");
    }

    /// Attribution compares paths by segment-anchored containment, not by
    /// string equality: a declared DIRECTORY covers the files under it. The
    /// anchor is what keeps `notsrc/a.rs` out of a `rc/a.rs` declaration.
    #[test]
    fn reclaim_attributes_a_declared_directory_prefix() {
        let (_dir, main, wt) = fixture();
        declare_files(&main, &["src"]);
        commit_wave_work(&wt, "src/a.txt");

        let v = reclaim_at(&opts(&main));
        assert_eq!(v["ok"], json!(true), "a declared directory covers its files: {v}");
        assert_eq!(v["action"], json!("reclaimed"), "{v}");
        assert!(main.join("src/a.txt").is_file(), "{v}");
    }

    /// …and the anchor holds in the negative direction: a declaration that is
    /// only a CHARACTER-wise suffix of the touched path attributes nothing, so
    /// the wave fails closed instead of folding another wave's checkout.
    #[test]
    fn reclaim_does_not_attribute_an_unanchored_suffix() {
        let (_dir, main, wt) = fixture();
        declare_files(&main, &["rc/a.txt"]);
        commit_wave_work(&wt, "notsrc/a.txt");

        let v = reclaim_at(&opts(&main));
        assert_eq!(v["reason"], json!("unattributed-agent-checkout"), "{v}");
        assert!(!main.join("notsrc").exists(), "nothing folded: {v}");
    }

    /// The sweep's criterion, both directions.
    ///
    /// POSITIVE: the slug the harness really emits is swept. This is the case
    /// the `agent-` prefix test could not see — with isolation genuinely on it
    /// found nothing, answered `nothing-to-reclaim`, and `wave-done` reported
    /// the wave COMPLETE with its commit stranded in the checkout.
    #[test]
    fn reclaim_sweeps_a_slug_named_checkout() {
        for name in ["recursing-benz-063389", "bright-running-fox", "pr-1234"] {
            let (_dir, main, wt) = fixture_named(name);
            let wave_sha = commit_wave_work(&wt, "wave.txt");

            let v = reclaim_at(&opts(&main));
            assert_eq!(v["ok"], json!(true), "{name}: {v}");
            assert_eq!(v["action"], json!("reclaimed"), "{name}: {v}");
            assert_eq!(v["branch"], json!(name), "{name}: {v}");
            assert_eq!(
                git_out(&main, &["rev-parse", "dev_unit"]).as_deref(),
                Some(wave_sha.as_str()),
                "{name}: the wave's commit is on the unit branch",
            );
        }
    }

    /// NEGATIVE: a worktree whose name carries a declared `{base}_` is a WORK
    /// UNIT's own checkout, never a wave's — even when it holds commits the
    /// invoking tree lacks. Folding one would merge a sibling unit's work.
    #[test]
    fn reclaim_never_sweeps_a_unit_worktree() {
        let (_dir, main, wt) = fixture();
        // Retire the agent checkout so the unit worktree is the ONLY thing on
        // disk that could be mistaken for a candidate.
        git(&main, &["worktree", "remove", "--force", wt.to_string_lossy().as_ref()]);
        // A SECOND work unit, in its own worktree, carrying its own commit.
        git(&main, &["worktree", "add", ".claude/worktrees/dev_sibling", "-b", "dev_sibling"]);
        let sibling = main.join(".claude").join("worktrees").join("dev_sibling");
        let sibling_sha = commit_wave_work(&sibling, "sibling.txt");

        let v = reclaim_at(&opts(&main));
        assert_eq!(
            v,
            json!({ "ok": true, "action": "nothing-to-reclaim", "wave": 1 }),
            "a unit worktree is not a wave's checkout: {v}"
        );
        assert_eq!(
            git_out(&main, &["rev-parse", "dev_sibling"]).as_deref(),
            Some(sibling_sha.as_str()),
            "the sibling unit is untouched",
        );
        assert!(!main.join("sibling.txt").exists(), "and nothing was folded from it");
    }

    /// A DETACHED agent checkout carrying commits is work like any other. The
    /// worktree parser used to drop every branchless entry, so this checkout
    /// was invisible to the sweep and the command answered `ok:true` over it.
    #[test]
    fn reclaim_sees_a_detached_agent_checkout() {
        let (_dir, main, wt) = fixture();
        let wave_sha = commit_wave_work(&wt, "wave.txt");
        // Detach the checkout at that same commit, exactly where it stands.
        git(&wt, &["checkout", "--detach", &wave_sha]);
        assert_eq!(
            git_out(&wt, &["rev-parse", "--abbrev-ref", "HEAD"]).as_deref(),
            Some("HEAD"),
            "the fixture really is detached",
        );
        // Drop the branch so the commit lives ONLY in the detached checkout —
        // otherwise the branch would be swept and the detachment prove nothing.
        git(&main, &["branch", "-D", AGENT_SLUG]);

        let v = reclaim_at(&opts(&main));
        assert_ne!(
            v["action"],
            json!("nothing-to-reclaim"),
            "a detached checkout holding unreclaimed commits is never silence: {v}"
        );
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["branch"], json!(wave_sha), "named by its HEAD — it has no branch: {v}");
        assert_eq!(
            git_out(&main, &["rev-parse", "dev_unit"]).as_deref(),
            Some(wave_sha.as_str()),
            "and the commit really landed on the unit: {v}",
        );
    }

    /// A checkout whose work belongs to an integration base has nowhere safe to
    /// go: the fold must never land straight on `dev`. Invoked AT the main
    /// checkout, which is therefore also the invoking tree.
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
