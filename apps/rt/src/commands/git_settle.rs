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
    let bases: Vec<String> =
        mustard_core::ProjectConfig::load(&cfg_root).git.integration_bases().into_iter().collect();

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
    let submodules = git_out(&main, &["submodule", "status"])
        .map(|s| parse_submodule_paths(&s))
        .unwrap_or_default();
    for rel in submodules {
        if let Some(entry) = repo_settlement(&main.join(&rel), &rel, &unit_branch) {
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
        "repos": repos,
        "complete": complete,
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
