//! `mustard-rt run git-settle` — the EXIT RITUAL of a delivered work unit,
//! answering "the PR merged; now what?" with the user's exact contract:
//!
//! 1. **Runs from the WORK BRANCH** — invoked bare while sitting on an
//!    integration base (`dev`/`main`) it REFUSES (`on-integration-base`):
//!    settle is how a unit leaves the stage, not a base-side sweeper. From a
//!    base it only runs with an explicit `--unit <branch>` (the finish step
//!    of the dance below).
//! 2. **100% merged or nothing**, measured per REF: EVERY ref that still carries
//!    the unit — the local head and each `<remote>/<branch>` — must be contained
//!    in `origin/<base>` now, or covered by the frozen head of a merged pull
//!    request. The provider half is not decoration: a portal that rewrites the
//!    commits when it merges leaves the unit's tip off the base although the work
//!    landed. Which of the two answers first is a property of the PROJECT's merge
//!    settings, measured per repository — this doc asserts nothing about it. What
//!    neither half may do any more is vouch for a ref it did not measure: a
//!    merged pull request whose branch was pushed to afterwards stops reading as
//!    prunable. Not accounted for → `{ok:false, reason:"not-merged"}` and NOTHING
//!    is touched.
//! 3. **Back to an up-to-date base**: every local base advances — the one the
//!    MAIN checkout sits on via `merge --ff-only` (clean tree only; the
//!    harness-owned `.claude/worktrees/` dir and a moved GITLINK are exempt from
//!    the dirty check, the latter because the advance itself manufactures it),
//!    every other via `git fetch origin <base>:<base>`, which fast-forwards a
//!    non-checked-out ref and refuses anything unsafe. Afterwards the submodules
//!    that advance left behind are re-seated on the new pointer — but only those
//!    standing DETACHED; one sitting on a branch is somebody's live work and is
//!    reported untouched (`submodulePointers`). A FINISHING pass
//!    (`settled`/`partial`) that leaves the unit's own base behind anyway answers
//!    `{ok:false, reason:"base-behind"}` instead of burying the fact mid-JSON.
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
//! `--report` is the READING face of the same ritual: it classifies every work
//! branch of every repo (`shared::branch_state`) and prints, touching nothing.
//! The separation is structural — the reading path builds its answer out of
//! plain state values that carry no way to act on the repository.
//!
//! Output: one JSON report (sorted arrays, no timestamps), including `repos` —
//! one entry per repository the unit lives in (the repo settle acted on, plus
//! every submodule that still carries the unit branch) and the global
//! `complete`. Settle ACTS on the repository it was pointed at; the report
//! tells the truth about all of them, because the exit ritual is per repo just
//! like `commit`/`push`/`pr`. Fail-open
//! everywhere: absent `gh`, no remote, or a locked path degrades to an
//! honest field, exit 0 — but the MERGE VERIFICATION itself is a hard gate,
//! never fail-open (guarding a verdict: missing evidence blocks).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::shared::branch_state::{
    self, base_of_branch, BranchEnumerator, GitReachability, PrEvidence, PrLookup, ProviderPrCli,
    StateClassifier,
};

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

/// A path as the report shows it: forward slashes, so one JSON shape reads the
/// same on every platform.
fn show(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The immediate SUPERPROJECT of `repo` — `None` outside a submodule. Asked of
/// git itself, never derived from a filesystem walk, so a `.git` FILE, a linked
/// worktree or a nested submodule resolve exactly the way git resolves them.
fn superproject_of(repo: &Path) -> Option<PathBuf> {
    let out = git_out(repo, &["rev-parse", "--show-superproject-working-tree"])?;
    let out = out.trim();
    (!out.is_empty()).then(|| PathBuf::from(out))
}

/// How far [`config_root`] climbs before giving up — nested submodules are rare
/// and shallow; the bound only guarantees the climb terminates.
const MAX_SUPERPROJECT_HOPS: usize = 8;

/// The root whose `mustard.json` declares the integration bases, paired with the
/// immediate superproject (`None` outside a submodule).
///
/// A submodule is an independent repository and carries no harness config of its
/// own: read there, `git.flow` is absent and the bases silently degrade to the
/// built-in `{main, master}` last resort — which then refuses a perfectly valid
/// `dev_` unit with `no-base-prefix`, blaming the branch name for a problem of
/// location. The bases of a submodule's unit live in the SUPERPROJECT, so climb
/// while the config is still missing.
fn config_root(repo: &Path) -> (PathBuf, Option<PathBuf>) {
    let superproject = superproject_of(repo);
    let mut root = repo.to_path_buf();
    let mut up = superproject.clone();
    for _ in 0..MAX_SUPERPROJECT_HOPS {
        if mustard_core::ProjectConfig::exists(&root) {
            break;
        }
        let Some(parent) = up else { break };
        up = superproject_of(&parent);
        root = parent;
    }
    (root, superproject)
}

/// One `.claude/worktrees/` entry from `git worktree list --porcelain`.
#[derive(Debug, PartialEq)]
pub(crate) struct WorktreeEntry {
    pub(crate) path: String,
    /// The branch the checkout has out — EMPTY when it is DETACHED.
    pub(crate) branch: String,
    /// The commit the checkout stands on. Always populated; it is the ONLY
    /// handle a detached entry has, and the reason such an entry is reported.
    pub(crate) head: String,
}

/// Parse `git worktree list --porcelain` into the harness-owned entries only
/// (paths under `.claude/worktrees/`, forward-slash normalized).
///
/// A DETACHED checkout is reported too, with an empty `branch` and its `head`
/// sha. Dropping it — which this parser used to do — hid a real checkout, one
/// that may hold commits nobody has integrated, from every sweep built on top
/// of this parser: the entry simply did not exist, so a sweep reported success
/// over work it had never looked at.
///
/// Callers keyed on a branch NAME are unaffected by construction: an empty
/// branch equals no work-unit branch, and [`base_of_branch`] refuses it.
pub(crate) fn parse_worktrees(porcelain: &str) -> Vec<WorktreeEntry> {
    let mut out = Vec::new();
    let mut path: Option<String> = None;
    let mut head = String::new();
    // Emit one entry per block, once its ref line is seen — `branch` for an
    // attached checkout, `detached` for one standing on a bare commit.
    let push = |path: &Option<String>, branch: &str, head: &str, out: &mut Vec<WorktreeEntry>| {
        if let Some(p) = path {
            if p.contains("/.claude/worktrees/") {
                out.push(WorktreeEntry {
                    path: p.clone(),
                    branch: branch.to_string(),
                    head: head.to_string(),
                });
            }
        }
    };
    for line in porcelain.lines().chain(std::iter::once("")) {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.trim().replace('\\', "/"));
            head = String::new();
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            head = h.trim().to_string();
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            push(&path, b.trim(), &head, &mut out);
        } else if line.trim() == "detached" {
            push(&path, "", &head, &mut out);
        } else if line.is_empty() {
            path = None;
            head = String::new();
        }
    }
    // Path breaks the tie: every detached entry sorts under the same empty
    // branch, and the crate's determinism Guard admits no arbitrary order.
    out.sort_by(|a, b| a.branch.cmp(&b.branch).then_with(|| a.path.cmp(&b.path)));
    out
}

/// The unit's identity ACROSS repositories: everything after the first `_` of a
/// work-branch name (the harness's `worktree-` prefix stripped first). Every
/// repo the unit touches cuts `{its own base}_{slug}` — `submodule-rules.md`
/// derives a submodule's branch exactly this way — so the slug travels while the
/// prefix does not.
fn unit_slug(branch: &str) -> Option<&str> {
    let name = branch.strip_prefix("worktree-").unwrap_or(branch);
    name.split_once('_').map(|(_, slug)| slug).filter(|s| !s.is_empty())
}

/// Paths of the INITIALIZED submodules in `git submodule status` output, sorted.
/// Each line is `<status><sha> <path>[ (<describe>)]`; the `-` status marks a
/// submodule with no checkout — there is no working tree to inspect there, so it
/// is skipped rather than reported as if it had been examined.
pub(crate) fn parse_submodule_paths(status: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in status.lines() {
        // Marker-AGNOSTIC on purpose. `git submodule status` marks a CLEAN
        // submodule with a leading SPACE, and `git_out` trims the whole stdout —
        // so the first line reaches here already stripped of its marker. Keying
        // on the marker made a clean single-submodule monorepo (this project's
        // own shape, right after `commit` stages the gitlink) report NO
        // submodule at all, which is precisely the blind spot this command was
        // changed to remove. Only `-` (uninitialized) is load-bearing, and it
        // survives trimming because it is not whitespace.
        let mut fields = line.split_whitespace();
        let Some(sha) = fields.next() else { continue };
        if sha.starts_with('-') {
            continue; // not initialized — no working tree to inspect
        }
        let Some(path) = fields.next() else { continue };
        if !path.is_empty() {
            out.push(path.replace('\\', "/"));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// What `repo` still holds of the unit — `None` when it carries no trace of it
/// (a repository the unit never touched is not part of the unit's report).
/// Purely observational: every command here reads, none writes.
///
/// "Carries" means the work branch is checked out, exists locally, or is still
/// alive on `origin` — the three states the field incident showed a submodule
/// sitting in while the parent's report already read `settled`.
fn repo_settlement(repo: &Path, label: &str, unit_branch: &str) -> Option<Value> {
    let slug = unit_slug(unit_branch);
    let same_unit = |branch: &str| branch == unit_branch || (slug.is_some() && unit_slug(branch) == slug);
    let refs_of = |namespace: &str| -> Vec<String> {
        git_out(repo, &["for-each-ref", "--format=%(refname:short)", namespace])
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_string)
            .collect()
    };

    let head = git_out(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let on_unit_branch = same_unit(&head);
    let mut branches: Vec<String> =
        refs_of("refs/heads").into_iter().filter(|b| same_unit(b)).collect();
    if on_unit_branch && !branches.iter().any(|b| b == &head) {
        branches.push(head.clone());
    }
    branches.sort();

    // Local refs decide FIRST, and they are free: a repo with no trace of the
    // unit — not on its branch, no branch, no tracking ref — is not part of it
    // and answers without touching the network. In a monorepo most submodules
    // are strangers to the unit being settled; they must not each cost a probe.
    let tracked: Vec<String> = refs_of("refs/remotes/origin")
        .into_iter()
        .filter_map(|r| r.strip_prefix("origin/").map(str::to_string))
        .filter(|b| same_unit(b))
        .collect();
    if !on_unit_branch && branches.is_empty() && tracked.is_empty() {
        return None;
    }

    // Only then ask the SERVER about each candidate: a tracking ref outlives the
    // branch it followed, so believing it would report a closed repo as open.
    let mut candidates: Vec<&str> = branches.iter().map(String::as_str).collect();
    candidates.extend(tracked.iter().map(String::as_str));
    candidates.sort_unstable();
    candidates.dedup();
    let mut remote_branches: Vec<String> = Vec::new();
    let mut probe_failed = false;
    for branch in candidates {
        match git_out(repo, &["ls-remote", "--heads", "origin", branch]) {
            Some(out) if !out.trim().is_empty() => remote_branches.push(branch.to_string()),
            Some(_) => {}
            None => probe_failed = true,
        }
    }
    if probe_failed {
        // Unreachable remote: keep what this repo last knew rather than let an
        // unanswered probe read as "already gone" — `remoteProbe` marks the list
        // unconfirmed.
        for name in &tracked {
            if !remote_branches.contains(name) {
                remote_branches.push(name.clone());
            }
        }
    }
    remote_branches.sort();

    if !on_unit_branch && branches.is_empty() && remote_branches.is_empty() {
        return None; // the tracking ref outlived the branch — nothing left here
    }
    let reason = if on_unit_branch {
        "on-unit-branch"
    } else if branches.is_empty() {
        "remote-branch-alive"
    } else {
        "branch-alive"
    };
    Some(json!({
        "repo": label,
        "settled": false,
        "reason": reason,
        "head": head,
        "branches": branches,
        "remoteBranches": remote_branches,
        "remoteProbe": if probe_failed { "unavailable" } else { "ok" },
    }))
}

/// Whether `branch` may leave the stage: EVERY ref that still carries it is
/// contained in `origin/<base>` now, or covered by the frozen head of a merged
/// pull request.
///
/// Delegates to the ONE per-ref predicate
/// ([`branch_state::all_refs_accounted`]). The hand-written second copy this
/// function used to be is exactly how the two sweeps diverged before: it asked
/// the provider whether a pull request had EVER merged, compared no sha, and so
/// answered "merged" forever after — including for a branch that had been pushed
/// to since. The provider fallback survives the rewrite, because a portal that
/// rewrites the commits when it merges is real; what it no longer does is
/// authorise a prune on its own.
///
/// This is the 100% gate — no evidence means NOT merged (conservative, never
/// fail-open).
fn is_merged(main: &Path, branch: &str, base: &str, provider: &str) -> bool {
    let git_read = |args: &[&str]| git_out(main, args);
    let bases = [base.to_string()];
    let swept = BranchEnumerator::sweep(&git_read, &bases);
    let Some(unit) = swept.units().iter().find(|u| u.branch == branch) else {
        // No ref carries it: nothing to prove and nothing to prune.
        return false;
    };
    // Against the SERVER's base — settle has just fetched it, and a local base
    // that lags would call an unintegrated ref settled.
    let contained = branch_state::refs_merged_into(&git_read, &format!("origin/{base}"));
    // The provider is asked ONLY when git did not already answer: a repository
    // whose refs all sit on the base must not pay a network round-trip per unit
    // to be told what `for-each-ref` just proved. (`also_mergeable` asks this
    // once per work branch.)
    let evidence = if unit.refnames().iter().all(|r| contained.contains(r)) {
        PrEvidence::unqueried()
    } else {
        ProviderPrCli::new(main, provider).evidence_of(branch)
    };
    let reach = GitReachability::new(&git_read);
    let verdicts = branch_state::ref_verdicts(unit, &contained, &evidence, &reach);
    branch_state::all_refs_accounted(&verdicts)
}

/// The PATH of one `git status --porcelain` line, tolerating the caller's trim.
///
/// The format is `XY <path>`, but [`git_out`] trims the WHOLE stdout, so the
/// first line arrives with its leading status char already gone whenever that
/// char is a space — the same trap [`parse_submodule_paths`] documents. Splitting
/// on the first space instead of slicing at a fixed offset survives it; slicing
/// at `[3..]` turned ` M sub` into `ub` and matched nothing.
fn status_path(line: &str) -> String {
    line.trim_start()
        .split_once(' ')
        .map(|(_, rest)| rest.trim_start().replace('\\', "/"))
        .unwrap_or_default()
}

/// Whether one `status --porcelain` line is dirt that must stop the ff-only
/// advance. Two exemptions, both measured rather than assumed:
///
/// - the harness-owned `.claude/worktrees/` tree, which the ritual itself
///   creates;
/// - a moved GITLINK. The base advance MANUFACTURES that line — a submodule
///   directory does not travel with the parent's branch — so counting it as dirt
///   left the parent's base permanently behind, which is the field incident this
///   exemption answers. Measured on real git 2.53: `merge --ff-only` accepts a
///   gitlink-only working tree, staged or not, and fast-forwards even when the
///   range moves the pointer itself.
fn blocks_fast_forward(line: &str, submodules: &[String]) -> bool {
    let path = status_path(line);
    if path.is_empty() || path.starts_with(".claude/worktrees/") {
        return false;
    }
    !submodules.iter().any(|sub| sub == &path)
}

/// Advance every local base: the MAIN checkout's own branch via ff-only merge
/// (clean tree only — see [`blocks_fast_forward`] for the two exemptions), every
/// other base via the ff-safe `fetch origin <base>:<base>`. Returns
/// (report-of-current, reports-of-others) — pure bookkeeping, all reversible.
fn update_bases(main: &Path, bases: &[String], submodules: &[String]) -> (Value, Vec<Value>) {
    let current = git_out(main, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let current_report = if bases.iter().any(|b| b == &current) {
        let clean = git_out(main, &["status", "--porcelain"])
            .map(|s| !s.lines().any(|l| blocks_fast_forward(l, submodules)))
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

/// After the base advanced, bring each submodule's checkout back onto the
/// gitlink the base now names — but ONLY where the submodule stands DETACHED.
///
/// Measured on real git 2.53: an unconditional `git submodule update` yanks a
/// submodule sitting on a live work branch into detached HEAD (the branch
/// survives; the checkout does not), which is the very symptom the exit ritual
/// is supposed to stop producing. A submodule on ANY branch is therefore
/// reported and left exactly where it is — its own `pr close` moves it.
///
/// One entry per submodule, in the sorted order [`parse_submodule_paths`]
/// returns, so the report stays byte-stable.
fn sync_submodule_pointers(main: &Path, submodules: &[String]) -> Vec<Value> {
    submodules
        .iter()
        .map(|rel| {
            let Some(head) = git_out(&main.join(rel), &["rev-parse", "--abbrev-ref", "HEAD"])
            else {
                // Unreadable is not detached: leave it alone and say so.
                return json!({ "path": rel, "action": "unreadable" });
            };
            if head != "HEAD" {
                return json!({ "path": rel, "action": "left-on-branch", "branch": head });
            }
            let updated = git_ok(main, &["submodule", "update", "--", rel]);
            json!({ "path": rel, "action": if updated { "updated" } else { "update-failed" } })
        })
        .collect()
}

/// Whether a settle pass may answer `ok`.
///
/// A FINISHING shape (`settled`/`partial`) that left the unit's own base behind
/// is NOT a success: the branch is gone and the tree still holds pre-merge file
/// versions. `updated:false` buried mid-JSON is what let the field incident read
/// as done — the caller reads `ok` first, so `ok` has to know.
///
/// `exit-and-rerun` is the deliberate exception: it is an INSTRUCTION, not a
/// failure. Its own `action` field says what to do next, and the base advances
/// on the rerun that follows — downgrading it would teach the caller to stop
/// exactly where it must continue.
fn pass_is_ok(action: &str, base_advanced: bool) -> bool {
    !matches!(action, "settled" | "partial") || base_advanced
}

/// The settle pass — the testable core of [`run`]. `unit` = the work branch to
/// settle; `None` reads it from the invocation directory's HEAD (and REFUSES
/// when that is an integration base). Never panics.
pub(crate) fn settle_at(start: &Path, unit: Option<&str>) -> Value {
    let Some(main) = main_checkout_root(start) else {
        // Echo the path that failed: the field incident behind this message was
        // a `--root` that did not exist, and a bare "not-a-git-repo" let the
        // submodule take the blame.
        return json!({
            "ok": false,
            "reason": "not-a-git-repo",
            "path": show(start),
            "exists": start.exists(),
            "hint": "git não resolveu repositório nesse caminho — confira o --root antes de suspeitar de submódulo",
        });
    };
    let (cfg_root, superproject) = config_root(&main);
    let cfg = mustard_core::ProjectConfig::load(&cfg_root);
    let bases: Vec<String> = cfg.git.integration_bases().into_iter().collect();
    // The merge gate asks the provider the project DECLARES — this command names
    // no CLI of its own, exactly like the reading face below.
    let provider = cfg.git.provider.clone();

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
        // Name WHAT was resolved, not just the branch: the prefix only looks
        // wrong from a root whose config was never read (a submodule reads its
        // superproject's `git.flow` — say where that is).
        //
        // Deliberate exception to the crate's byte-stable-stdout Guard: absolute
        // machine paths appear ONLY on a refusal (`ok: false`), never on the
        // success report that gets diffed or snapshotted. A refusal whose whole
        // job is to say WHERE the command looked cannot omit the path and still
        // do that job — the field incident was a `--root` that did not exist,
        // and the old message let the submodule take the blame for it.
        let hint = if superproject.is_some() {
            "prefixo não bate com base conhecida — este repo é submódulo: as bases vêm do git.flow em configRoot"
        } else {
            "prefixo não bate com base conhecida — confira git.flow no mustard.json de configRoot"
        };
        return json!({
            "ok": false,
            "reason": "no-base-prefix",
            "branch": unit_branch,
            "root": show(&main),
            "configRoot": show(&cfg_root),
            "superproject": superproject.as_deref().map(show),
            "bases": bases,
            "hint": hint,
        });
    };

    // One fetch for every base — fail-open (offline still verifies against
    // whatever origin/<base> is already known locally; the merge gate below
    // stays conservative either way).
    let mut fetch_args: Vec<&str> = vec!["fetch", "origin"];
    fetch_args.extend(bases.iter().map(String::as_str));
    let fetched = git_ok(&main, &fetch_args);

    // THE gate: 100% merged or nothing happens.
    if !is_merged(&main, &unit_branch, &base, &provider) {
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

    // The submodule table, read ONCE and used twice below: the base advance
    // treats a moved gitlink as non-blocking dirt, and the pointer sync only
    // touches the DETACHED ones.
    let submodules = git_out(&main, &["submodule", "status"])
        .map(|s| parse_submodule_paths(&s))
        .unwrap_or_default();

    // Merged confirmed → bring every local base up to date, then re-seat the
    // submodules the advance just left behind (detached ones only).
    let (base_report, other_bases) = update_bases(&main, &bases, &submodules);
    let submodule_sync = sync_submodule_pointers(&main, &submodules);

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

    // One entry per repository the unit lives in. Settle still ACTS only on the
    // repo it was pointed at — this is the report refusing to claim the unit is
    // gone while a submodule still sits on its work branch. The acting repo is
    // reported from the outcome just produced; each submodule from what it still
    // carries. An untouched submodule yields no entry: it is not part of the
    // unit.
    let repo_report = if action == "settled" {
        json!({ "repo": ".", "branch": unit_branch, "settled": true })
    } else {
        json!({ "repo": ".", "branch": unit_branch, "settled": false, "reason": action })
    };
    let mut repos = vec![repo_report];
    for rel in &submodules {
        if let Some(entry) = repo_settlement(&main.join(rel), rel, &unit_branch) {
            repos.push(entry);
        }
    }
    // Look UP as well as down. `submodule-rules.md` makes the SUBMODULE side
    // step 1 of the close ritual, so the first report of every multi-repo unit
    // is produced from inside a submodule — and a report that only enumerates
    // its own children would answer `complete: true` there while the parent
    // still holds the unit. That is the same half-settled "done" this command
    // was changed to stop printing, reached from the other end.
    if let Some(parent) = superproject.as_deref() {
        if let Some(entry) = repo_settlement(parent, "..", &unit_branch) {
            repos.push(entry);
        }
    }
    let complete = repos.iter().all(|r| r["settled"] == json!(true));

    // Other merged units — informative only (settle acts on ONE unit; the user
    // decides about the rest, each via its own settle).
    //
    // Enumerated from the REFS, not from the worktree table. Keyed on worktrees
    // this field lied by omission: the work-branch gate cuts a unit IN PLACE by
    // default — on the main checkout, no worktree — so a repository holding six
    // merged units answered `[]`. The enumerator sees local and remote refs
    // alike, so an in-place unit and one that lives only on the server are both
    // reported.
    let git_read = |args: &[&str]| git_out(&main, args);
    let enumerated = BranchEnumerator::sweep(&git_read, &bases);
    let ahead = branch_state::refs_ahead_of_base(&git_read, enumerated.units(), &bases);
    let also_mergeable: Vec<String> = enumerated
        .units()
        .iter()
        .filter(|u| u.branch != unit_branch)
        // A branch carrying no commit of its own delivered nothing: it is
        // reachable from its base because it was CUT from it, and `is_merged`
        // cannot see that difference. Listing it here would tell the user their
        // live work is ready to be pruned.
        .filter(|u| ahead.contains(&u.branch))
        .filter(|u| is_merged(&main, &u.branch, &u.base, &provider))
        .map(|u| u.branch.clone())
        .collect();

    // Did the UNIT'S OWN base advance? `baseCheckout` describes whatever branch
    // the main checkout happens to sit on, which is not always it — when settle
    // finishes from another branch, the unit's base is one of `otherBases`.
    let advanced = |report: &Value| report["updated"] == json!(true);
    let base_advanced = if base_report["branch"] == json!(base) {
        advanced(&base_report)
    } else {
        other_bases.iter().any(|b| b["branch"] == json!(base) && advanced(b))
    };
    let ok = pass_is_ok(action, base_advanced);

    let mut report = json!({
        "ok": ok,
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
        "repos": repos,
        "complete": complete,
        "baseCheckout": base_report,
        "otherBases": other_bases,
        "submodulePointers": submodule_sync,
        "alsoMergeable": also_mergeable,
        "fetched": fetched,
    });
    if !ok {
        report["reason"] = json!("base-behind");
    }
    report
}

/// The READING pass — every work branch of every repo of the project, each with
/// the ONE state it is in. Acts on nothing, by construction: it holds no
/// deleting step, and what it hands the printer is a list of plain state values
/// (`shared::branch_state::BranchState`) that carry no handle to a repository.
///
/// Per repository, like `repos` in [`settle_at`]: the acting side of this
/// command is per-repo, so the reading side has to be too — a monorepo answer
/// that folded the submodules into the parent would hide exactly the branches
/// the exit ritual keeps having to be told about.
pub(crate) fn report_at(start: &Path) -> Value {
    let Some(main) = main_checkout_root(start) else {
        return json!({
            "ok": false,
            "reason": "not-a-git-repo",
            "path": show(start),
            "exists": start.exists(),
            "hint": "git não resolveu repositório nesse caminho — confira o --root",
        });
    };
    let (cfg_root, _superproject) = config_root(&main);
    let cfg = mustard_core::ProjectConfig::load(&cfg_root);
    let bases: Vec<String> = cfg.git.integration_bases().into_iter().collect();
    let provider = cfg.git.provider.clone();

    let mut repos = vec![repo_inventory(&main, ".", &bases, &provider)];
    let submodules = git_out(&main, &["submodule", "status"])
        .map(|s| parse_submodule_paths(&s))
        .unwrap_or_default();
    for rel in submodules {
        repos.push(repo_inventory(&main.join(&rel), &rel, &bases, &provider));
    }
    json!({ "ok": true, "bases": bases, "repos": repos })
}

/// One repository's inventory: sweep its refs, measure ancestry locally, then
/// let the PR port confirm. The provider is whatever `mustard.json` declares —
/// this command names no CLI of its own.
///
/// Asked through the enumerator's FALLIBLE face, because this is a REPORTING
/// consumer: a sweep git never answered, printed as an empty inventory, is the
/// same lie as an unmeasured PR printed as "no PR" — and refusing that lie is
/// why this module exists. A repository whose refs would not read says so.
fn repo_inventory(repo: &Path, label: &str, bases: &[String], provider: &str) -> Value {
    let git_read = |args: &[&str]| git_out(repo, args);
    let Some(units) = BranchEnumerator::try_sweep(&git_read, bases) else {
        return json!({
            "repo": label,
            "ok": false,
            "reason": "refs-unreadable",
            "units": [],
            "awaitingPrune": [],
        });
    };
    let merged = branch_state::merged_refs(&git_read, bases);
    let ahead = branch_state::refs_ahead_of_base(&git_read, units.units(), bases);
    let lookup = ProviderPrCli::new(repo, provider);
    let reach = GitReachability::new(&git_read);
    let states = StateClassifier::new(&lookup, &reach).classify(units.units(), &merged, &ahead);
    branch_state::report_value(label, &states)
}

/// Run `git-settle` from `root` and print the JSON report. `report` selects the
/// reading pass, which settles nothing.
pub fn run(root: &Path, unit: Option<&str>, report: bool) {
    let result = if report { report_at(root) } else { settle_at(root, unit) };
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
                WorktreeEntry {
                    path: "C:/repo/.claude/worktrees/dev_b".into(),
                    branch: "dev_b".into(),
                    head: "def".into(),
                },
                WorktreeEntry {
                    path: "C:/repo/.claude/worktrees/dev_a".into(),
                    branch: "worktree-dev_a".into(),
                    head: "ghi".into(),
                },
            ],
            "main checkout and foreign paths excluded; sorted by branch"
        );
    }

    #[test]
    fn parse_worktrees_reports_a_detached_checkout_by_its_head() {
        // A DETACHED harness worktree carries no `branch refs/heads/…` line.
        // Skipping it (the old behaviour) made a checkout that may hold
        // unintegrated commits invisible to every sweep built on this parser.
        let porcelain = "worktree C:/repo\nHEAD abc\nbranch refs/heads/dev\n\n\
                         worktree C:/repo/.claude/worktrees/recursing-benz-063389\nHEAD deadbeef\ndetached\n\n\
                         worktree C:/repo/.claude/worktrees/dev_b\nHEAD def\nbranch refs/heads/dev_b\n";
        let got = parse_worktrees(porcelain);
        assert_eq!(
            got,
            vec![
                WorktreeEntry {
                    path: "C:/repo/.claude/worktrees/recursing-benz-063389".into(),
                    branch: String::new(),
                    head: "deadbeef".into(),
                },
                WorktreeEntry {
                    path: "C:/repo/.claude/worktrees/dev_b".into(),
                    branch: "dev_b".into(),
                    head: "def".into(),
                },
            ],
            "the detached entry is reported with an empty branch + its head sha"
        );
        // And it can never be mistaken for a work unit: no branch name, so the
        // base parser refuses it.
        assert_eq!(base_of_branch(&got[0].branch, &["dev".to_string()]), None);
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

    /// The monorepo fixture: the same parent as [`fixture`] plus a REAL git
    /// submodule at `sub` (added with `protocol.file.allow=always` — git refuses
    /// a local-path submodule otherwise). The unit `dev_done` is merged into
    /// origin/dev in the PARENT and still fully live in the SUBMODULE: work
    /// branch checked out, local and remote alive. That is the exact field state
    /// where the report used to answer `settled` with `alsoMergeable: []`.
    fn fixture_with_submodule() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().expect("tempdir");
        let bare = dir.path().join("origin.git");
        let sub_bare = dir.path().join("sub-origin.git");
        let seed = dir.path().join("sub-seed");
        let main = dir.path().join("repo");
        for p in [&bare, &sub_bare, &seed, &main] {
            std::fs::create_dir_all(p).expect("mkdir");
        }
        git(&bare, &["init", "--bare", "."]);
        git(&sub_bare, &["init", "--bare", "."]);
        // The submodule's own default branch — a clone must find something to
        // check out, and `dev` keeps parent and submodule on the same base name.
        git(&sub_bare, &["symbolic-ref", "HEAD", "refs/heads/dev"]);

        git(&seed, &["init", "."]);
        git(&seed, &["config", "user.email", "t@t"]);
        git(&seed, &["config", "user.name", "t"]);
        git(&seed, &["checkout", "-b", "dev"]);
        std::fs::write(seed.join("s.txt"), "s").expect("seed");
        git(&seed, &["add", "-A"]);
        git(&seed, &["commit", "-m", "sub seed"]);
        git(&seed, &["remote", "add", "origin", sub_bare.to_string_lossy().as_ref()]);
        git(&seed, &["push", "-u", "origin", "dev"]);

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
        git(&main, &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            sub_bare.to_string_lossy().as_ref(),
            "sub",
        ]);
        git(&main, &["commit", "-m", "add submodule"]);
        git(&main, &["push", "origin", "dev"]);

        // The unit in the PARENT: worked in its worktree, merged into origin/dev,
        // local dev rewound one merge so settle has something to fast-forward.
        git(&main, &["worktree", "add", ".claude/worktrees/dev_done", "-b", "dev_done"]);
        let wt = main.join(".claude").join("worktrees").join("dev_done");
        std::fs::write(wt.join("done.txt"), "x").expect("wt file");
        git(&wt, &["add", "-A"]);
        git(&wt, &["commit", "-m", "done work"]);
        git(&main, &["merge", "--no-ff", "dev_done", "-m", "merge dev_done"]);
        git(&main, &["push", "origin", "dev"]);
        git(&main, &["reset", "--hard", "HEAD~1"]);

        // The SAME unit in the SUBMODULE — never closed: branch checked out,
        // pushed, remote alive.
        let sub = main.join("sub");
        git(&sub, &["config", "user.email", "t@t"]);
        git(&sub, &["config", "user.name", "t"]);
        git(&sub, &["checkout", "-b", "dev_done"]);
        std::fs::write(sub.join("w.txt"), "w").expect("sub file");
        git(&sub, &["add", "-A"]);
        git(&sub, &["commit", "-m", "sub work"]);
        git(&sub, &["push", "-u", "origin", "dev_done"]);

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

    /// AC-3 — an IN-PLACE merged unit (cut on the main checkout, no worktree —
    /// the default shape the work-branch gate produces) must appear among the
    /// units still awaiting a prune.
    ///
    /// Keyed on the worktree table, this field answered `[]` in exactly that
    /// case: it enumerated checkouts, and an in-place unit has none. Measured in
    /// the field as six local and six remote branches invisible to the report.
    #[test]
    fn in_place_merged_unit_is_reported_pending() {
        let (_dir, main) = fixture();
        // An in-place unit: cut on the MAIN checkout, merged into origin/dev,
        // never given a worktree. Local dev is rewound afterwards so the shared
        // fixture's `dev_done` stays settleable.
        git(&main, &["merge", "--ff-only", "origin/dev"]);
        git(&main, &["checkout", "-b", "dev_inplace"]);
        std::fs::write(main.join("inplace.txt"), "z").expect("file");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "in-place work"]);
        git(&main, &["checkout", "dev"]);
        git(&main, &["merge", "--no-ff", "dev_inplace", "-m", "merge dev_inplace"]);
        git(&main, &["push", "origin", "dev"]);
        git(&main, &["reset", "--hard", "HEAD~1"]);
        assert!(
            !main.join(".claude").join("worktrees").join("dev_inplace").exists(),
            "the fixture must reproduce the IN-PLACE shape: no worktree for this unit",
        );

        // Settle a DIFFERENT unit; the in-place one is what the report must not
        // hide.
        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(
            v["alsoMergeable"],
            json!(["dev_inplace"]),
            "the merged in-place unit must be reported pending: {v}",
        );
    }

    /// The READING face at the COMMAND level: what `--report` actually prints.
    ///
    /// The other tests of this ritual classify through the shared module; this
    /// one goes through `report_at`, because that is the face the field
    /// measurement read — and what it read of a branch cut seconds earlier was
    /// `awaiting-prune-local`, the unit the user was editing announced as
    /// delivered. Both halves, so the assertions can fail: a unit that really
    /// landed is in the same report, listed.
    #[test]
    fn report_lists_delivered_units_and_never_a_freshly_cut_one() {
        let (_dir, main) = fixture();
        // A unit that really landed: commits of its own, merged into the base,
        // branch alive on both sides.
        git(&main, &["checkout", "-b", "dev_landed"]);
        std::fs::write(main.join("landed.txt"), "l").expect("file");
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-m", "landed work"]);
        git(&main, &["push", "origin", "dev_landed"]);
        git(&main, &["checkout", "dev"]);
        git(&main, &["merge", "--no-ff", "dev_landed", "-m", "merge dev_landed"]);
        // And a unit shaped the way the work-branch gate opens every one: a
        // branch at the base's tip with nothing committed on it yet.
        git(&main, &["branch", "dev_justcut"]);

        let v = report_at(&main);
        let repo = &v["repos"][0];
        assert_eq!(repo["ok"], json!(true), "the refs WERE read — an empty list would mean empty");
        assert_eq!(repo["awaitingPrune"], json!(["dev_landed"]), "{v}");

        let row = |branch: &str| -> Value {
            repo["units"]
                .as_array()
                .expect("units")
                .iter()
                .find(|u| u["branch"] == json!(branch))
                .cloned()
                .expect("the report must carry a row per swept unit")
        };
        let cut = row("dev_justcut");
        assert_eq!(cut["ancestry"], json!(true), "a fresh cut IS reachable from its base");
        assert_eq!(cut["ahead"], json!(false), "…and carries no commit of its own");
        assert_ne!(cut["state"], json!("awaiting-prune-local"), "cutting is not delivering: {cut}");
        assert_eq!(row("dev_landed")["state"], json!("awaiting-prune"));
    }

    /// AC-9 — the module's prose may not assert a merge method nobody measured.
    ///
    /// Both halves, so the assertion can fail: the doc no longer claims THIS
    /// repository squash-merges (measured false — the merges carry two parents
    /// each and `--merged` recognises every branch), while the provider fallback
    /// it justified is still in the code. Removing an unmeasured sentence must
    /// not quietly remove the mechanism: a portal that squashes is real, and the
    /// fallback is what covers it.
    #[test]
    fn settle_doc_states_no_unmeasured_merge_method() {
        let src = include_str!("git_settle.rs");
        // ALL the ritual's prose, not just its header: the header was cleaned
        // while the merge gate's own docstring — the prose that answers "why ask
        // the provider at all" and therefore the one this criterion is about —
        // went on naming a method nobody measured, out of the assertion's reach.
        // The production region only; this test's own doc necessarily quotes the
        // claim it forbids.
        let production = src.split("#[cfg(test)]").next().unwrap_or_default();
        let doc: String = production
            .lines()
            .map(str::trim_start)
            .filter(|l| l.starts_with("//!") || l.starts_with("///"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        assert!(doc.contains("exit ritual"), "the module header must be in range");
        assert!(
            doc.contains("100% gate"),
            "…and so must the merge gate's own docstring — asserting over the header alone is \
             how the claim survived the first time",
        );

        // Needles assembled at runtime so writing the test does not plant the
        // forbidden spelling in the file under assertion.
        for claim in [
            ["squash", "-merges"].concat(),
            ["this repo squash", ""].concat(),
            ["squash", "-merge fallback"].concat(),
        ] {
            assert!(
                !doc.contains(&claim),
                "the doc asserts an unmeasured merge method of this repository: {claim}",
            );
        }

        // The mechanism survives the sentence: the merge gate still asks the
        // provider when containment says no. It asks through the ONE adapter,
        // which is why the argv itself is no longer spelled here — both halves,
        // so the assertion can fail if the mechanism is dropped rather than
        // moved.
        assert!(
            src.contains("ProviderPrCli::new(main, provider).evidence_of(branch)"),
            "the merge gate must still consult the provider — a portal that rewrites the commits \
             when it merges is real",
        );
        // Needle assembled at runtime so writing the test does not plant the
        // spelling in the file under assertion.
        let spawn_cli = ["Command::new(\"", "gh\")"].concat();
        assert!(
            !src.contains(&spawn_cli),
            "the ritual must not launch the provider CLI a SECOND time — the hand-written copy \
             is exactly how the two sweeps diverged",
        );
        assert!(
            include_str!("../shared/branch_state.rs").contains("Command::new(GITHUB_CLI)"),
            "…and the ONE home of that query must still hold it, or this test asserts nothing",
        );
    }

    #[test]
    fn parse_submodule_paths_keeps_initialized_entries_sorted() {
        let status = " 1111111 packages/one (heads/dev)\n\
                      +2222222 apps/two (v1.0-2-g2222222)\n\
                      -3333333 not/initialized\n\
                      U4444444 conflicted/three\n";
        assert_eq!(
            parse_submodule_paths(status),
            vec!["apps/two", "conflicted/three", "packages/one"],
            "the `-` entry has no checkout to inspect and is skipped"
        );
        assert!(parse_submodule_paths("").is_empty(), "a repo with no submodules yields none");
    }

    /// The shape the ONLY caller can actually deliver. `git_out` trims the whole
    /// stdout, so a CLEAN submodule — marked by a leading SPACE — arrives with
    /// that marker already gone from the FIRST line. The previous test fed a
    /// leading space the parser never sees in production and stayed green while
    /// a clean single-submodule monorepo reported no submodule at all.
    #[test]
    fn parse_submodule_paths_survives_the_trim_its_caller_applies() {
        let raw = " 1111111 sub (heads/dev)\n";
        assert_eq!(
            parse_submodule_paths(raw.trim()),
            vec!["sub"],
            "a clean lone submodule must survive the caller's trim",
        );
        // Two clean entries: the first loses its marker, the second keeps it.
        let two = " aaaaaaa first (heads/dev)\n aaaaaaa second (heads/dev)\n";
        assert_eq!(parse_submodule_paths(two.trim()), vec!["first", "second"]);
        // The `-` marker is not whitespace, so it still survives and still means
        // "no checkout to inspect".
        assert!(parse_submodule_paths("-3333333 not/initialized".trim()).is_empty());
    }

    /// A submodule carries no `mustard.json` of its own, so the bases of a unit
    /// settled there live in the SUPERPROJECT. Before the fix the config lookup
    /// stopped at the submodule root, fell back to the built-in `{main, master}`
    /// and refused this `dev_` unit with `no-base-prefix` — the branch blamed for
    /// a problem of location.
    #[test]
    fn settle_resolves_bases_from_superproject() {
        let (_dir, main) = fixture_with_submodule();
        let sub = main.join("sub");
        assert!(!sub.join("mustard.json").exists(), "the submodule has no config of its own");

        let v = settle_at(&sub, Some("dev_done"));
        assert_eq!(v["base"], json!("dev"), "base read from the superproject's git.flow: {v}");
        assert_eq!(v["reason"], json!("not-merged"), "recognised the unit, then gated on merge: {v}");
    }

    /// The refusal must say what it RESOLVED — root, config root and the bases it
    /// knows — instead of naming only the branch, which reads as "your branch is
    /// wrong" when the real answer is "I read the wrong config".
    #[test]
    fn no_base_prefix_names_root_and_known_bases() {
        let (_dir, main) = fixture();
        let v = settle_at(&main, Some("feature_x"));
        assert_eq!(v["reason"], json!("no-base-prefix"), "{v}");
        assert_eq!(v["branch"], json!("feature_x"));
        assert_eq!(v["bases"], json!(["dev"]), "the bases it knows: {v}");
        let root = v["root"].as_str().unwrap_or_default();
        assert!(root.ends_with("/repo"), "names the root it resolved: {v}");
        assert_eq!(v["configRoot"], v["root"], "config came from the repo itself: {v}");
        assert_eq!(v["superproject"], json!(null), "not a submodule: {v}");
    }

    /// The `--root` that does not exist — the actual trigger of the field
    /// incident — must be visible in the answer, not hidden behind a bare
    /// "not-a-git-repo" that let the submodule take the blame.
    #[test]
    fn not_a_git_repo_echoes_the_path_it_tried() {
        let dir = tempdir().expect("tempdir");
        let missing = dir.path().join("nope");
        let v = settle_at(&missing, None);
        assert_eq!(v["reason"], json!("not-a-git-repo"), "{v}");
        assert!(
            v["path"].as_str().unwrap_or_default().ends_with("/nope"),
            "echoes the path it tried: {v}"
        );
        assert_eq!(v["exists"], json!(false), "and says the path is not even there: {v}");
    }

    /// The unit spans parent AND submodule: settling the parent must not answer
    /// "done" while the submodule still sits on the work branch with its local
    /// and remote branches alive. One entry per repository, plus the global
    /// `complete` that stays false until every repo is settled.
    #[test]
    fn settle_reports_every_repo_of_the_unit() {
        let (_dir, main) = fixture_with_submodule();

        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["unit"]["action"], json!("settled"), "the parent really settled: {v}");
        assert_eq!(v["complete"], json!(false), "a repo of the unit is still open: {v}");

        let repos = v["repos"].as_array().expect("repos array");
        assert_eq!(repos.len(), 2, "one entry per repository of the unit: {v}");
        assert_eq!(repos[0]["repo"], json!("."), "{v}");
        assert_eq!(repos[0]["settled"], json!(true), "{v}");
        assert_eq!(repos[1]["repo"], json!("sub"), "{v}");
        assert_eq!(repos[1]["settled"], json!(false), "{v}");
        assert_eq!(repos[1]["reason"], json!("on-unit-branch"), "{v}");
        assert_eq!(repos[1]["branches"], json!(["dev_done"]), "{v}");
        assert_eq!(repos[1]["remoteBranches"], json!(["dev_done"]), "remote still alive: {v}");
    }

    /// The state the field actually reaches: `commit` stages the moved gitlink
    /// in the parent (its own documented procedure), which makes the submodule
    /// CLEAN — and `git submodule status` marks clean entries with a leading
    /// SPACE that `git_out`'s trim removes from the first line. Keyed on that
    /// marker, the lone submodule of a monorepo disappeared from `repos` and the
    /// report answered `complete: true` over an open repository.
    #[test]
    fn clean_submodule_still_appears_in_the_report() {
        let (_dir, main) = fixture_with_submodule();
        // Stage the gitlink: the submodule now reports clean (leading space).
        git(&main, &["add", "--", "sub"]);
        let status = git_out(&main, &["submodule", "status"]).expect("status");
        assert!(
            !status.starts_with(['+', 'U']),
            "the fixture must reproduce the CLEAN shape, got: {status:?}",
        );

        let v = settle_at(&main, Some("dev_done"));
        let repos = v["repos"].as_array().expect("repos array");
        assert_eq!(repos.len(), 2, "the clean submodule must still be reported: {v}");
        assert_eq!(repos[1]["repo"], json!("sub"), "{v}");
        assert_eq!(repos[1]["settled"], json!(false), "{v}");
        assert_eq!(v["complete"], json!(false), "an open repo forbids `complete`: {v}");
    }

    /// `submodule-rules.md` makes the SUBMODULE side step 1 of the close ritual,
    /// so the first report of a multi-repo unit is produced from inside a
    /// submodule. Enumerating only its own children would answer `complete:
    /// true` there while the parent still holds the unit — the same half-settled
    /// "done", entered from the other end. The report must look UP too.
    #[test]
    fn settle_inside_a_submodule_reports_the_parent_too() {
        let (_dir, main) = fixture_with_submodule();
        let sub = main.join("sub");
        // Step 1 of the ritual runs AFTER the submodule's own PR merged, so put
        // the fixture in that state: the unit is on the submodule's base, and
        // the submodule is still standing on the unit branch. The parent has
        // merged nothing — it is the repository that must keep `complete` false.
        git(&sub, &["checkout", "dev"]);
        git(&sub, &["merge", "--ff-only", "dev_done"]);
        git(&sub, &["push", "origin", "dev"]);
        git(&sub, &["checkout", "dev_done"]);

        let v = settle_at(&sub, Some("dev_done"));
        assert_eq!(v["ok"], json!(true), "bases resolve from the superproject: {v}");

        let repos = v["repos"].as_array().expect("repos array");
        let parent = repos
            .iter()
            .find(|r| r["repo"] == json!(".."))
            .unwrap_or_else(|| panic!("the superproject must be reported: {v}"));
        assert_eq!(parent["settled"], json!(false), "the parent still holds the unit: {v}");
        assert_eq!(
            v["complete"],
            json!(false),
            "`complete` must not claim the unit is gone while the parent holds it: {v}",
        );
    }

    /// The pruning evidence is per REF: a unit whose LOCAL head landed while its
    /// remote ref moved on afterwards is NOT settled, and settle must touch
    /// nothing.
    ///
    /// This is the shape the old gate could not see. It asked whether the branch
    /// tip was an ancestor of the base, then whether a pull request had ever
    /// merged — neither question looks at the other refs, and the name-keyed
    /// merged set behind the report had the same hole. Both halves, so the
    /// assertions can fail: the local head really IS contained (the evidence the
    /// old measurement stopped at), and the moment the remote ref is back on
    /// what the merge accounts for, the very same command settles the unit.
    #[test]
    fn settle_refuses_when_a_ref_moved_after_merge() {
        let (_dir, main) = fixture();
        git(&main, &["push", "origin", "dev_done"]);
        let merged_tip = git_out(&main, &["rev-parse", "dev_done"]).expect("the merged tip");

        // A commit made where it cannot move the LOCAL head — a detached
        // sidecar — then pushed onto the unit's remote branch. That is the state
        // a second machine (or a reviewer's fixup) leaves behind.
        git(&main, &["worktree", "add", "--detach", ".claude/worktrees/sidecar", "dev_done"]);
        let side = main.join(".claude").join("worktrees").join("sidecar");
        std::fs::write(side.join("after.txt"), "after").expect("post-merge file");
        git(&side, &["add", "-A"]);
        git(&side, &["commit", "-m", "pushed after the merge"]);
        git(&side, &["push", "origin", "HEAD:dev_done"]);
        git(&main, &["fetch", "origin"]);
        git(&main, &["worktree", "remove", side.to_string_lossy().as_ref()]);

        let git_read = |args: &[&str]| git_out(&main, args);
        let contained = branch_state::refs_merged_into(&git_read, "origin/dev");
        assert!(
            contained.contains("refs/heads/dev_done"),
            "the local head really landed — a name-keyed set would have stopped here: {contained:?}",
        );
        assert!(
            !contained.contains("refs/remotes/origin/dev_done"),
            "…and the remote ref really carries commits the merge never saw: {contained:?}",
        );

        let wt = main.join(".claude").join("worktrees").join("dev_done");
        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(v["reason"], json!("not-merged"), "a ref beyond the merge is not settled: {v}");
        assert!(wt.exists(), "nothing touched: the worktree survives");
        assert!(
            !git_out(&main, &["branch", "--list", "dev_done"]).unwrap_or_default().is_empty(),
            "nothing touched: the local branch survives",
        );
        assert!(
            !git_out(&main, &["ls-remote", "--heads", "origin", "dev_done"])
                .unwrap_or_default()
                .is_empty(),
            "nothing touched: the remote branch — the one carrying the extra commits — survives",
        );

        // The other half: put the remote ref back on what the merge accounts
        // for, and the same command settles. Only the refs changed.
        git(&main, &["update-ref", "refs/remotes/origin/dev_done", &merged_tip]);
        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["unit"]["action"], json!("settled"), "{v}");
    }

    /// A moved GITLINK is not dirt that may stop the base advance — the advance
    /// itself manufactures it, because a submodule directory does not travel
    /// with the parent's branch.
    ///
    /// Both halves, so the exemption cannot become a blanket "never dirty": the
    /// line-level rule keeps refusing a real file change, and a repository whose
    /// only dirt is a real file still reports `dirty-tree`.
    #[test]
    fn gitlink_only_dirt() {
        // The line-level rule, on the shape `git_out`'s trim really delivers:
        // the leading SPACE status char is gone from the first line.
        let subs = vec!["sub".to_string()];
        assert!(!blocks_fast_forward("M sub", &subs), "trimmed gitlink line: {:?}", status_path("M sub"));
        assert!(!blocks_fast_forward(" M sub", &subs), "untrimmed gitlink line");
        assert!(!blocks_fast_forward("M  sub", &subs), "STAGED gitlink line");
        assert!(!blocks_fast_forward("?? .claude/worktrees/dev_x/f", &subs), "harness-owned tree");
        assert!(blocks_fast_forward("M a.txt", &subs), "a real file change is still dirt");
        assert!(blocks_fast_forward(" M other", &subs), "a path no submodule claims is still dirt");

        // And end to end: the submodule sits on its own work branch, so the
        // parent reports a moved gitlink — and the base still advances.
        let (_dir, main) = fixture_with_submodule();
        let status = git_out(&main, &["submodule", "status"]).expect("submodule status");
        assert!(!status.is_empty(), "the fixture must carry a submodule: {status:?}");
        let dirt = git_out(&main, &["status", "--porcelain"]).expect("status");
        assert!(dirt.contains("sub"), "the fixture must reproduce the gitlink dirt: {dirt:?}");

        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(
            v["baseCheckout"]["updated"],
            json!(true),
            "gitlink-only dirt must not block the ff-only advance: {v}",
        );
        let local = git_out(&main, &["rev-parse", "dev"]).expect("local base");
        let remote = git_out(&main, &["rev-parse", "origin/dev"]).expect("remote base");
        assert_eq!(local, remote, "the base really advanced");
        // The submodule is on a BRANCH, so the pointer sync reports it and
        // leaves it exactly where it is.
        assert_eq!(
            v["submodulePointers"],
            json!([{ "path": "sub", "action": "left-on-branch", "branch": "dev_done" }]),
            "{v}",
        );
        assert_eq!(
            git_out(&main.join("sub"), &["rev-parse", "--abbrev-ref", "HEAD"]).as_deref(),
            Some("dev_done"),
            "a submodule on a branch is never yanked into detached HEAD",
        );

        // The other half: a real file change still stops the advance.
        let (_dir2, main2) = fixture_with_submodule();
        std::fs::write(main2.join("a.txt"), "edited").expect("dirty file");
        let v = settle_at(&main2, Some("dev_done"));
        assert_eq!(v["baseCheckout"]["reason"], json!("dirty-tree"), "{v}");
        assert_eq!(v["baseCheckout"]["updated"], json!(false), "{v}");
    }

    /// A FINISHING pass that left the unit's own base behind must not answer
    /// `ok:true`. The field incident read as a success while `updated:false` sat
    /// buried mid-JSON and the tree still held pre-merge file versions.
    ///
    /// Both halves: the same fixture with a clean tree answers `ok:true`, so the
    /// downgrade is caused by the base and not by settling; and the rule itself
    /// exempts `exit-and-rerun`, which is an instruction rather than a failure.
    #[test]
    fn base_behind_downgrades_ok() {
        let (_dir, main) = fixture();
        // Dirt the exemptions do not cover, so the ff-only advance cannot run.
        std::fs::write(main.join("a.txt"), "edited").expect("dirty file");
        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(v["unit"]["action"], json!("settled"), "the prune itself finished: {v}");
        assert_eq!(v["baseCheckout"]["updated"], json!(false), "{v}");
        assert_eq!(v["ok"], json!(false), "a finishing pass over a base left behind: {v}");
        assert_eq!(v["reason"], json!("base-behind"), "{v}");

        // Clean tree, same shape: the base advances and the pass IS ok.
        let (_dir2, main2) = fixture();
        let v = settle_at(&main2, Some("dev_done"));
        assert_eq!(v["unit"]["action"], json!("settled"), "{v}");
        assert_eq!(v["ok"], json!(true), "{v}");
        assert_eq!(v["reason"], json!(null), "a successful pass carries no reason: {v}");

        // The rule, on the arm no fixture can reach: whether the process sits
        // inside the unit's own worktree is a property of the running process's
        // cwd, so the exemption is asserted where it is decided.
        assert!(pass_is_ok("exit-and-rerun", false), "an instruction is not a failure");
        assert!(!pass_is_ok("settled", false));
        assert!(!pass_is_ok("partial", false));
        assert!(pass_is_ok("settled", true));
    }

    /// The single-repo project keeps exactly one entry and a `complete` that
    /// mirrors the action — the new fields must not invent an unsettled repo
    /// where there is none.
    #[test]
    fn single_repo_unit_reports_itself_complete() {
        let (_dir, main) = fixture();
        let v = settle_at(&main, Some("dev_done"));
        assert_eq!(v["complete"], json!(true), "{v}");
        assert_eq!(v["repos"], json!([{ "repo": ".", "branch": "dev_done", "settled": true }]), "{v}");
    }
}
