//! `mustard-rt run worktree-gc` — garbage-collect orphan Claude agent worktrees.
//!
//! ## Why
//!
//! Every `Task` invocation with `isolation: "worktree"` carves out a fresh git
//! worktree under `<repo>/.claude/worktrees/<name>/`. When the task ends
//! cleanly the orchestrator removes it; when it crashes (process killed,
//! network drop, panic), the worktree lingers. They mirror the source tree,
//! so they balloon `docs-stale-check`, `security-scan`, and any other
//! filesystem walker — and the `locked` marker keeps `git worktree prune`
//! from reaping them automatically.
//!
//! ## Which worktrees, and which are off limits
//!
//! `<name>` is a SLUG the harness chooses — user-given (`feature-auth`), a
//! `pr-<number>`, or auto-generated (`bright-running-fox`,
//! `recursing-benz-063389`). There is no `agent-` prefix: `WorktreeCreate`
//! documents `name` as a plain identifier, and this collector used to filter on
//! a prefix the platform never emits, which made it inert against every real
//! orphan. It now collects by the ONE criterion the rest of the crate uses —
//! [`crate::commands::work_unit_open::is_unit_worktree_name`]: a worktree whose
//! name is a unit's — `{kind}/{slug}`, or a declared `{base}_` for one still in
//! the older shape — is a WORK UNIT's, and cleanup of those is
//! `git-settle`'s job EXCLUSIVELY. Everything else is collectable.
//!
//! Widening what a destructive sweep can see demands the other half of the
//! platform's own contract, so it is here too: a worktree that still HOLDS WORK
//! (uncommitted or untracked changes, `.claude/` INCLUDED — see below) is never
//! removed, whatever its age. What removal can cost is then a checkout with
//! nothing in it — its branch survives `git worktree remove` untouched.
//!
//! ## Only a positive observation of emptiness deletes
//!
//! That refusal is worth exactly what the probe behind it is worth, and the
//! probe belongs to the CALLER. The cut decision's probe
//! ([`crate::commands::work_unit_open::dirty_paths`]) answers a different
//! question — "may I cut a worktree here?" — with the opposite failure posture:
//! a failed `git status` reads as CLEAN (a refusal may only stand on a positive
//! observation) and every path under `.claude/` is dropped as redirected
//! harness state. Both are right there and catastrophic here. This collector's
//! candidates LIVE under `.claude/worktrees/`, so that carve-out hides a
//! candidate's OWN contents; and `git status` run inside a directory that is
//! not its own checkout answers about the ENCLOSING repository, so a directory
//! holding unsaved files was reported clean and `--apply` deleted them.
//! Reproduced twice, files lost both times.
//!
//! So the collector measures with its own [`contents`] probe, whose posture is
//! the one ownership already takes one decision earlier: what could not be
//! measured is never an authorisation. It asks git only where git can speak for
//! the candidate ITSELF, walks a plain directory's own contents otherwise, and
//! keeps whatever it could not settle.
//!
//! That probe runs BEFORE the age fallback, never after. Holding work is a
//! permanent refusal and being young is a temporary one, so the reason the
//! report states has to be the strongest of the two rather than whichever was
//! cheapest to compute: a directory full of unsaved files reported as `below
//! threshold` tells an operator to come back in a week for a deletion that must
//! never happen. And ordering it the other way made the guarantee itself
//! conditional — the guard only ran when a BEST-EFFORT mtime happened to read
//! old, which is a signal that answers differently per platform and filesystem.
//! What gets REMOVED is unchanged either way: proven empty, plus an owner
//! measured gone or an age past the threshold.
//!
//! ## Where it looks
//!
//! `<repo>/.claude/worktrees/` is where the platform puts an agent's worktree,
//! and for a long time it was the only tree walked. It is not the only place
//! the harness cuts one: the removal-proof pass
//! ([`crate::commands::review::work_removed`]) cuts its scratch checkout into
//! the OS temp directory, deliberately outside the project. An interrupted pass
//! therefore leaked a worktree NOTHING could ever reap — observed in the field.
//! So the sweep also reaches the temp directory, for the harness's own
//! `mustard-removal-{slug}-{pid}` name and only for it, and only when
//! `<repo>/.git/worktrees/<name>` proves the candidate is registered to THIS
//! repository (the temp directory is shared by every project on the machine,
//! and a directory name is not ownership).
//!
//! ## Ownership beats age
//!
//! That same scratch name carries the process id of whoever cut it. Asking
//! whether that process still exists answers "orphan or busy" EXACTLY, so a
//! worktree abandoned a minute ago is collected a minute later instead of in a
//! week — the age threshold only ever existed because nothing else told the
//! collector. Ownership is read from the harness's own prefix and never
//! anywhere else: a platform slug like `recursing-benz-063389` also ends in
//! digits, and reading those as a process id would authorise removing the
//! worktree of a live agent.
//!
//! Age stays as the FALLBACK, for every worktree whose owner cannot be read —
//! no ownership, or a liveness probe that could not answer. Unmeasured
//! ownership never authorises removal.
//!
//! This subcommand enumerates those worktrees, resolves each one's owner and
//! age, and removes the orphaned ones plus those older than `--age-days N`.
//! Dry-run by default; `--apply` is required to mutate the filesystem. The
//! `SessionStart` probe passes `--apply`: reporting an orphan every session and
//! never collecting it is how the leak above survived.
//!
//! ## Age signal
//!
//! `git worktree list` does not include the worktree's creation timestamp,
//! so we infer age from `<repo>/.git/worktrees/<basename>/HEAD` mtime (set
//! when `git worktree add` writes the initial ref) and fall back to the
//! worktree directory's own mtime. Either way it is best-effort — a
//! resolution failure marks the entry as unknown-age and the GC leaves it
//! alone (fail-open).
//!
//! ## Output
//!
//! Byte-stable pretty JSON:
//!
//! ```json
//! {
//!   "removed": ["<path>", ...],
//!   "kept":    [{"path": "<path>", "age_days": 3, "reason": "below threshold"}, ...],
//!   "errors":  [{"path": "<path>", "error": "<message>"}, ...]
//! }
//! ```
//!
//! ## Telemetry
//!
//! Emits two harness events per invocation (fail-open):
//!
//! - `pipeline.economy.operation.invoked { operation: "worktree-gc", duration_ms }`
//!   — the universal `/economia` operation marker (W12 contract).

use crate::commands::git_settle::git_out;
use crate::commands::review::work_removed::scratch_prefix;
use crate::commands::work_unit_open::is_unit_worktree_name;
use crate::shared::context::{current_spec, session_id};
use crate::shared::proc::process_liveness;
use mustard_core::time::now_iso8601;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Options + report types
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run worktree-gc`.
pub struct WorktreeGcOpts {
    /// Repo root override. Defaults to the current working directory.
    pub repo: Option<PathBuf>,
    /// Worktrees older than this many days are eligible for removal.
    pub age_days: u32,
    /// When `true`, removal proceeds; when `false` (the default), the report
    /// names every eligible worktree without touching the filesystem.
    pub apply: bool,
}

/// One kept-worktree entry in the JSON report.
#[derive(Serialize)]
struct KeptEntry {
    path: String,
    /// Whole days since the age signal — `None` when the signal could not be
    /// resolved (treated as "keep" under fail-open).
    age_days: Option<u64>,
    /// Human-readable reason, in the order [`gc`] asks: `"owner alive"`,
    /// `"holds uncommitted work"`, `"could not be proven empty"`,
    /// `"unknown age"`, `"below threshold"`, or `"dry-run"` (when `--apply`
    /// is not set).
    reason: String,
}

/// One error-entry in the JSON report — a worktree we tried to remove but
/// could not (lock held, IO error, etc.).
#[derive(Serialize)]
struct ErrorEntry {
    path: String,
    error: String,
}

/// The full machine-readable report.
#[derive(Serialize)]
struct GcReport {
    removed: Vec<String>,
    kept: Vec<KeptEntry>,
    errors: Vec<ErrorEntry>,
    age_days: u32,
    /// `true` when `--apply` was NOT set (report-only mode).
    dry_run: bool,
}

// ---------------------------------------------------------------------------
// Worktree discovery + age resolution
// ---------------------------------------------------------------------------

/// Enumerate the collectable directories under `<repo>/.claude/worktrees/` —
/// every one whose name is NOT a work unit's (see the module header). Returns
/// an empty vec when the parent path is missing (fail-open).
///
/// `worktrees/` has no typed accessor on `ClaudePaths` (it's a legacy direct
/// child of `.claude/`); routing via `claude_dir()` keeps the boundary owned
/// by the canonical handle without expanding W4 scope.
fn list_agent_worktrees(repo: &Path) -> Vec<PathBuf> {
    let Ok(paths) = ClaudePaths::for_project(repo) else {
        return Vec::new();
    };
    let config = mustard_core::ProjectConfig::load(repo);
    // ROOTED: a unit still in the `{base}_{slug}` shape whose base the flow no
    // longer declares is resolved by the branches the repository really has,
    // and this collector DELETES — reading such a worktree as nobody's unit is
    // how it would reap one.
    let flow = crate::shared::work_kind::BaseFlow::of_at(&config.git, repo);
    let root = paths.claude_dir().join("worktrees");
    let Ok(read) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    read.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            // A unit's worktree is never collected here — that is the exit
            // ritual's job exclusively. Since a unit is named `{kind}/{slug}`
            // its worktree sits one level DOWN, and the entry seen at this level
            // is the bare kind directory holding it: a container, not a
            // worktree, and collecting it would delete every unit inside.
            //
            // Told apart by STRUCTURE, not by the name. It used to ask whether
            // the name was one of three known kind tokens, and that stopped
            // working the moment the vocabulary opened: with any token a
            // possible kind, a plainly-named worktree (`my-unit`) reads as a
            // container and the collector collects NOTHING. Every git worktree
            // carries a `.git` entry and a mere holding directory does not, so
            // one `exists` answers the question for any vocabulary, invented
            // tokens included.
            if holds_unit_worktrees(p) {
                return false;
            }
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !is_unit_worktree_name(n, &flow))
        })
        .collect()
}

/// `true` when `dir` is a kind CONTAINER — the bare `feature/` or `spike/`
/// directory a `{kind}/{slug}` worktree sits inside — rather than a worktree.
///
/// Asked of the CHILDREN, which is the only phrasing that survives an open
/// vocabulary. The predicate used to be "is the name one of three known kind
/// tokens", and that broke the day any token could be a kind: a plainly-named
/// worktree (`agent-good`, `pr-1234`) then read as a container and the
/// collector collected nothing at all.
///
/// A container's children are worktrees, and every git worktree carries a
/// `.git` entry; a worktree's own children are source directories, which do
/// not. So one level of looking answers it for `feature/` and for `spike/`
/// alike, with no list to keep current.
///
/// Unreadable ⇒ `false`, and that is the safe direction here: the entry then
/// merely enters the candidate list, where the removal proof still has to clear
/// it before anything is deleted.
fn holds_unit_worktrees(dir: &Path) -> bool {
    let Ok(read) = std::fs::read_dir(dir) else {
        return false;
    };
    read.flatten().any(|child| {
        child.file_type().is_ok_and(|t| t.is_dir()) && child.path().join(".git").exists()
    })
}

/// The removal-proof scratch checkouts of THIS repository still sitting in the
/// OS temp directory — the worktrees the harness cuts outside the project, and
/// so the ones no sweep of `.claude/worktrees/` has ever been able to see.
///
/// Two conditions, both required, because the temp directory belongs to every
/// project and every tool on the machine:
///
/// 1. the name starts with this repo's own [`scratch_prefix`], and
/// 2. `<repo>/.git/worktrees/<name>` exists — git's own record that this
///    checkout is registered to this repository. A directory that merely
///    happens to be named like ours (another clone with the same folder name)
///    fails it, and `git worktree remove` would have refused it anyway.
fn list_abandoned_scratch_worktrees(repo: &Path) -> Vec<PathBuf> {
    let prefix = scratch_prefix(repo);
    let Ok(read) = std::fs::read_dir(std::env::temp_dir()) else {
        return Vec::new();
    };
    read.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|name| {
                name.starts_with(&prefix)
                    && repo.join(".git").join("worktrees").join(name).is_dir()
            })
        })
        .collect()
}

/// Every worktree this sweep may consider, from both places the harness cuts
/// one, sorted so the report is byte-stable.
fn list_collectable_worktrees(repo: &Path) -> Vec<PathBuf> {
    let mut out = list_agent_worktrees(repo);
    out.extend(list_abandoned_scratch_worktrees(repo));
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Ownership
// ---------------------------------------------------------------------------

/// What the collector could learn about who owns a worktree.
enum Ownership {
    /// The name names a process id, and that process is still running: someone
    /// is working in there right now.
    Alive,
    /// The name names a process id, and no such process exists. An orphan —
    /// now, not in seven days.
    Gone,
    /// The name carries no process id, or the liveness probe could not answer
    /// at all. Nothing was measured, so nothing is authorised: the age
    /// threshold decides instead.
    Unknown,
}

/// Read the owner of `worktree` out of its own name.
///
/// ONLY the harness's own `mustard-removal-{slug}-{pid}` names carry an owner,
/// and the id is taken from what follows `prefix` in full — never from "the
/// digits at the end". Platform slugs end in digits too
/// (`recursing-benz-063389`), and reading one of those as a process id would
/// hand a live agent's worktree to the sweep.
fn ownership(worktree: &Path, prefix: &str) -> Ownership {
    let Some(pid) = worktree
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|name| name.strip_prefix(prefix))
        .and_then(|tail| tail.parse::<u32>().ok())
    else {
        return Ownership::Unknown;
    };
    match process_liveness(pid) {
        Some(true) => Ownership::Alive,
        Some(false) => Ownership::Gone,
        None => Ownership::Unknown,
    }
}

// ---------------------------------------------------------------------------
// Contents — the collector's OWN work probe
// ---------------------------------------------------------------------------

/// What the collector could ESTABLISH about what a candidate holds.
///
/// Deliberately not a `bool`: a caller that DELETES has to tell "I measured
/// nothing in there" from "I could not measure", and only the first authorises
/// removal. It is the posture [`Ownership::Unknown`] already takes for the
/// owner probe, one decision earlier.
enum Contents {
    /// Positively observed to hold nothing: git answered for THIS checkout and
    /// reported no change, or the directory itself was walked and no file was
    /// found. The only verdict that authorises removal.
    ProvenEmpty,
    /// Something was positively observed inside — uncommitted, untracked, or
    /// simply a file sitting in a directory git does not track at all.
    HoldsWork,
    /// Nothing could be established. Never an authorisation.
    Unproven,
}

/// `git status --porcelain` for `worktree` — but ONLY when git answers about
/// `worktree` ITSELF.
///
/// A directory that is not its own checkout resolves to the ENCLOSING
/// repository, and `git -C <plain-dir> status` then reports the MAIN
/// checkout's dirt: an answer about a tree the collector is not deciding on,
/// which made a candidate's protection depend on whether an unrelated tree
/// happened to be dirty. Comparing `rev-parse --show-toplevel` against the
/// candidate is what separates a registered worktree (git can speak for it)
/// from a plain directory (it cannot).
///
/// `None` means "not measured" — never "clean".
fn own_checkout_status(worktree: &Path) -> Option<String> {
    let top = git_out(worktree, &["rev-parse", "--show-toplevel"])?;
    let here = std::fs::canonicalize(worktree).ok()?;
    let there = std::fs::canonicalize(Path::new(&top)).ok()?;
    if here != there {
        return None;
    }
    git_out(worktree, &["status", "--porcelain"])
}

/// Whether git's own bookkeeping claims this path is a checkout: an
/// administrative record under `<repo>/.git/worktrees/<name>`, or a `.git`
/// pointer inside the candidate. Either one says the answer that mattered was
/// git's — so [`own_checkout_status`] not having produced one is a FAILURE TO
/// MEASURE, not an empty tree.
fn claimed_as_checkout(repo: &Path, worktree: &Path) -> bool {
    if worktree.join(".git").exists() {
        return true;
    }
    worktree
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| repo.join(".git").join("worktrees").join(name).is_dir())
}

/// How deep the direct walk descends before it stops asking: a candidate with
/// structure this deep is not an empty directory by any reading.
const MAX_WALK_DEPTH: u32 = 24;

/// Whether `dir` holds ANY file at all, git's own bookkeeping (`.git`) aside.
///
/// The judgement a plain directory can still take when git cannot speak for
/// it: one file is one thing to lose. A symlink counts as a file (its
/// `file_type` is neither dir nor regular), which errs towards keeping.
///
/// `None` when the directory could not be read — read by the caller as
/// unproven, NEVER as empty.
fn holds_any_file(dir: &Path, depth: u32) -> Option<bool> {
    if depth == 0 {
        return Some(true);
    }
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if entry.file_name().to_string_lossy() == ".git" {
            continue;
        }
        if entry.file_type().ok()?.is_dir() {
            if holds_any_file(&entry.path(), depth - 1)? {
                return Some(true);
            }
        } else {
            return Some(true);
        }
    }
    Some(false)
}

/// What `worktree` holds, measured with the posture a caller that DELETES
/// needs: everything unmeasured is [`Contents::Unproven`], and unproven keeps.
///
/// NOT [`crate::commands::work_unit_open::dirty_paths`] — see the module
/// header. That probe serves the cut decision, reads a failed measurement as
/// clean, and drops every path under `.claude/`; this one sees the candidate's
/// own contents, `.claude/` included, and refuses what it could not settle.
fn contents(repo: &Path, worktree: &Path) -> Contents {
    if let Some(status) = own_checkout_status(worktree) {
        // git spoke for THIS checkout, so its answer is the whole answer — no
        // carve-out: a candidate's own `.claude/` is its contents, not the
        // harness state the redirect points elsewhere.
        return if status.lines().any(|line| !line.trim().is_empty()) {
            Contents::HoldsWork
        } else {
            Contents::ProvenEmpty
        };
    }
    // git could not speak for the candidate, so the directory answers for
    // itself. A file found is a positive observation and settles it; finding
    // none only settles it when nothing claimed the path was a checkout git
    // was supposed to answer for.
    match holds_any_file(worktree, MAX_WALK_DEPTH) {
        Some(true) => Contents::HoldsWork,
        None => Contents::Unproven,
        Some(false) if claimed_as_checkout(repo, worktree) => Contents::Unproven,
        Some(false) => Contents::ProvenEmpty,
    }
}

/// Best-effort age signal for `worktree` (full path to the agent worktree dir).
///
/// 1. `<repo>/.git/worktrees/<basename>/HEAD` mtime — `git worktree add`
///    writes this file at creation time and rarely touches it afterwards.
/// 2. The worktree directory's own mtime — touched whenever a file inside it
///    changes, so it skews newer but at least gives a lower bound on age.
///
/// Returns `None` when both signals fail.
fn age_signal(repo: &Path, worktree: &Path) -> Option<SystemTime> {
    let basename = worktree.file_name()?.to_str()?;
    let head = repo
        .join(".git")
        .join("worktrees")
        .join(basename)
        .join("HEAD");
    if let Ok(meta) = std::fs::metadata(&head) {
        if let Ok(modified) = meta.modified() {
            return Some(modified);
        }
    }
    std::fs::metadata(worktree)
        .ok()
        .and_then(|m| m.modified().ok())
}

/// Convert an mtime into "whole days since now". `None` when the mtime is in
/// the future (clock skew) or unreadable — the caller treats this as unknown.
fn age_days_since(mtime: SystemTime) -> Option<u64> {
    let now = SystemTime::now();
    let delta = now.duration_since(mtime).ok()?;
    Some(delta.as_secs() / 86_400)
}

// ---------------------------------------------------------------------------
// Removal
// ---------------------------------------------------------------------------

/// Remove one worktree: first ask git (`git worktree remove --force`) so the
/// administrative state under `.git/worktrees/<name>/` is cleaned up too, then
/// `remove_dir_all` if anything is left on disk. Either step may fail when the
/// worktree is `locked` — the caller surfaces the error in `errors[]`.
fn remove_worktree(repo: &Path, worktree: &Path) -> Result<(), String> {
    // `git worktree remove --force <path>` handles a locked worktree only when
    // we first unlock it; do both, ignoring the unlock failure (it has no
    // effect on unlocked entries).
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("worktree")
        .arg("unlock")
        .arg(worktree)
        .output();

    let remove_out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(worktree)
        .output();
    let git_ok = matches!(remove_out, Ok(ref o) if o.status.success());

    if worktree.exists() {
        if let Err(e) = std::fs::remove_dir_all(worktree) {
            return Err(format!("remove_dir_all failed: {e}"));
        }
    }

    if !git_ok {
        // Best-effort cleanup of the administrative entry left behind when
        // `git worktree remove` failed but `remove_dir_all` succeeded.
        let _ = Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("worktree")
            .arg("prune")
            .output();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Core GC routine (testable; takes a repo path, returns a report)
// ---------------------------------------------------------------------------

/// Run the GC against `repo` and return the resulting report. Pure function:
/// no telemetry side effects, no stdout — the CLI wrapper handles both.
fn gc(repo: &Path, age_days: u32, apply: bool) -> GcReport {
    let mut report = GcReport {
        removed: Vec::new(),
        kept: Vec::new(),
        errors: Vec::new(),
        age_days,
        dry_run: !apply,
    };

    let threshold = u64::from(age_days);
    let prefix = scratch_prefix(repo);

    for wt in list_collectable_worktrees(repo) {
        let path = wt.display().to_string();
        // Two `fs::metadata` calls, no process spawn — cheap enough to resolve
        // for every entry so the report can state an age even when the verdict
        // did not come from one.
        let age = age_signal(repo, &wt).and_then(age_days_since);

        let owner = ownership(&wt, &prefix);

        // A live owner is working in there. No age and no reading of the
        // contents can override that, and measuring a tree nobody may touch
        // would only spawn a `git status` to reach a foregone answer.
        if matches!(owner, Ownership::Alive) {
            report.kept.push(KeptEntry {
                path,
                age_days: age,
                reason: "owner alive".into(),
            });
            continue;
        }

        // What the candidate HOLDS is measured BEFORE the age fallback, and
        // that order is the point: holding work is a PERMANENT refusal, being
        // young is a temporary one, and the reason a destructive sweep reports
        // has to be the strongest one — not whichever probe happened to be
        // cheapest. Asking age first said `below threshold` about a directory
        // full of unsaved files, which tells an operator to come back in a week
        // for a deletion that must never happen; and it made the safety guard
        // itself reachable only when a BEST-EFFORT mtime happened to read old
        // ([`age_signal`] is explicitly fail-open), so the one guarantee that
        // survives a `--apply` sweep hung off a signal that answers differently
        // per platform and per filesystem.
        //
        // It costs what it measures: `contents` spawns git only where git can
        // speak for the candidate, and only for worktrees that are not a work
        // unit's — a set that is EMPTY in a healthy repository, and otherwise is
        // exactly the set this sweep is about to delete.
        //
        // What is removed does not change by an inch: a candidate still goes
        // only when it is proven empty AND (its owner is measured gone OR it is
        // past the threshold). Only the refusal that gets REPORTED changes,
        // and only for an entry both gates would have kept.
        match contents(repo, &wt) {
            Contents::ProvenEmpty => {}
            Contents::HoldsWork => {
                report.kept.push(KeptEntry {
                    path,
                    age_days: age,
                    reason: "holds uncommitted work".into(),
                });
                continue;
            }
            Contents::Unproven => {
                report.kept.push(KeptEntry {
                    path,
                    age_days: age,
                    reason: "could not be proven empty".into(),
                });
                continue;
            }
        }

        // Proven empty. Removing it still needs a reason to act NOW: an owner
        // MEASURED gone, or an age past the threshold. Nothing measured about
        // the owner → the age fallback decides, exactly as before.
        if matches!(owner, Ownership::Unknown) {
            let Some(age) = age else {
                report.kept.push(KeptEntry {
                    path,
                    age_days: None,
                    reason: "unknown age".into(),
                });
                continue;
            };
            if age <= threshold {
                report.kept.push(KeptEntry {
                    path,
                    age_days: Some(age),
                    reason: "below threshold".into(),
                });
                continue;
            }
        }

        if !apply {
            report.kept.push(KeptEntry {
                path,
                age_days: age,
                reason: "dry-run".into(),
            });
            continue;
        }

        match remove_worktree(repo, &wt) {
            Ok(()) => report.removed.push(path),
            Err(e) => report.errors.push(ErrorEntry { path, error: e }),
        }
    }

    report
}

// ---------------------------------------------------------------------------
// Telemetry
// ---------------------------------------------------------------------------

/// Emit `pipeline.economy.operation.invoked` to the project's harness event
/// store. Fail-open at every step.
fn emit_telemetry(repo: &Path, duration_ms: u128) {
    let dir = repo.display().to_string();
    let spec = current_spec(&dir);
    let session = session_id();
    let ts = now_iso8601();

    // `duration_ms` is unbounded `u128`; cap at i64::MAX before casting so we
    // never produce a negative JSON number on an overflow.
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);

    let econ_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts,
        session_id: session,
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("worktree-gc".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "worktree-gc",
            "duration_ms": duration_capped,
        }),
        spec,
    };

    let _ = crate::shared::events::route::emit(&dir, &econ_event);
}

// ---------------------------------------------------------------------------
// SessionStart helper
// ---------------------------------------------------------------------------

/// Default `--age-days` value used by the CLI and the SessionStart probe — the
/// FALLBACK for a worktree whose owner cannot be read at all, not the rule.
pub const DEFAULT_AGE_DAYS: u32 = 7;

/// Idempotent `SessionStart` collection: remove the worktrees that are proven
/// orphaned (owner gone) or stale beyond [`DEFAULT_AGE_DAYS`], and say on
/// stderr how many went.
///
/// It COLLECTS rather than reports, which is the whole point. Running the same
/// sweep in simulation at every session start and printing a count above a
/// threshold is how a leaked worktree survived indefinitely: the fact was on
/// screen every single session and nothing ever acted on it. Acting is safe
/// here for a reason that predates this — [`gc`] already refuses a work unit's
/// worktree (that is `git-settle`'s alone) and refuses any worktree holding
/// uncommitted or untracked work, whatever its owner or age.
///
/// Fail-open: a missing `.claude/worktrees/` directory or any IO failure
/// degrades to a silent no-op — a session boot must never break on this.
pub fn session_start_probe(repo: &Path) {
    let report = gc(repo, DEFAULT_AGE_DAYS, /* apply = */ true);
    if !report.removed.is_empty() {
        eprintln!(
            "[worktree-gc] collected {} orphan worktree(s) in {}",
            report.removed.len(),
            repo.display()
        );
    }
}

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

/// Dispatch `mustard-rt run worktree-gc [--repo <p>] [--age-days N] [--apply]`.
pub fn run(opts: WorktreeGcOpts) {
    let repo = opts.repo.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    });

    let started = std::time::Instant::now();
    let report = gc(&repo, opts.age_days, opts.apply);
    let duration_ms = started.elapsed().as_millis();

    // Print BEFORE telemetry so the byte-stable JSON ordering is independent
    // of how long the store append takes (or whether it succeeds at all).
    let body: Value = serde_json::to_value(&report)
        .unwrap_or_else(|_| json!({"removed":[],"kept":[],"errors":[]}));
    println!(
        "{}",
        serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into())
    );

    emit_telemetry(&repo, duration_ms);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    /// Best-effort mtime backdating: open the file with write access and call
    /// `set_modified`. Mirrors the helper in `session_cleanup`'s test module.
    fn backdate(path: &Path, when: SystemTime) -> std::io::Result<()> {
        let file = std::fs::OpenOptions::new().write(true).open(path)?;
        file.set_modified(when)
    }

    /// Cut an agent worktree at `<repo>/.claude/worktrees/<slug>` of a REAL
    /// repository and backdate `.git/worktrees/<basename>/HEAD` — the file
    /// `age_signal` reads first — so the computed age is whatever the test
    /// needs.
    ///
    /// A real `git worktree add` rather than a directory with a token file in
    /// it, because the collector now judges a candidate by what it can
    /// ESTABLISH about its contents: git can speak for a registered worktree
    /// and answers "clean", while a plain directory holding a file holds
    /// something and is never removed. The old fixture — a bare directory with
    /// `src/touch.txt` inside — asserted precisely the behaviour that deleted
    /// real files.
    ///
    /// The name is a SLUG, the shape `WorktreeCreate` actually hands over — the
    /// old `agent-<id>` fixture matched a prefix the platform never emits, so
    /// it was the only thing keeping the old filter looking alive.
    fn fake_worktree(repo: &Path, id: &str, age_days: u64) -> PathBuf {
        let basename = format!("recursing-{id}-063389");
        let wt = repo.join(".claude").join("worktrees").join(&basename);
        Command::new("git")
            .args(["worktree", "add", "--detach", &wt.to_string_lossy(), "HEAD"])
            .current_dir(repo)
            .output()
            .expect("git");

        let head = repo.join(".git").join("worktrees").join(&basename).join("HEAD");
        let when = SystemTime::now() - Duration::from_secs(age_days * 86_400 + 60);
        let _ = backdate(&head, when);

        wt
    }

    #[test]
    fn list_returns_empty_when_dir_missing() {
        let dir = tempdir().unwrap();
        assert!(list_collectable_worktrees(dir.path()).is_empty());
    }

    #[test]
    fn list_collects_slug_names_and_skips_work_units() {
        // The criterion is the declared `{base}_`, not a name shape: every slug
        // the harness can emit is collectable, and a work unit's worktree never
        // is (git-settle owns those). The old `agent-` filter had it backwards
        // — it matched only a prefix the platform never produces.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("mustard.json"), r#"{"git":{"flow":{"*":"dev"}}}"#).unwrap();
        let root = dir.path().join(".claude").join("worktrees");
        for name in ["recursing-benz-063389", "bright-running-fox", "pr-1234", "agent-good"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        fs::create_dir_all(root.join("dev_my-unit")).unwrap();

        let found: Vec<String> = list_collectable_worktrees(dir.path())
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
            .collect();
        assert_eq!(
            found,
            vec!["agent-good", "bright-running-fox", "pr-1234", "recursing-benz-063389"],
            "every non-unit name collected, sorted"
        );
        assert!(!found.iter().any(|n| n == "dev_my-unit"), "a work unit is never collected");
    }

    #[test]
    fn a_worktree_holding_work_is_never_removed() {
        // The safety half of collecting by "not a work unit": a checkout with
        // uncommitted or untracked work survives regardless of age. Driven
        // through a REAL git worktree, because `dirty_paths` asks git.
        let dir = tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        for args in [
            vec!["init", "."],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
            vec!["checkout", "-b", "dev"],
        ] {
            Command::new("git").args(&args).current_dir(&repo).output().expect("git");
        }
        fs::write(repo.join("mustard.json"), r#"{"git":{"flow":{"*":"dev"}}}"#).unwrap();
        fs::write(repo.join(".gitignore"), ".claude/\n").unwrap();
        fs::write(repo.join("a.txt"), "seed").unwrap();
        for args in [vec!["add", "-A"], vec!["commit", "-m", "seed"]] {
            Command::new("git").args(&args).current_dir(&repo).output().expect("git");
        }
        Command::new("git")
            .args(["worktree", "add", ".claude/worktrees/bright-running-fox"])
            .current_dir(&repo)
            .output()
            .expect("git");
        let wt = repo.join(".claude").join("worktrees").join("bright-running-fox");
        fs::write(wt.join("unsaved.txt"), "never committed").unwrap();
        // Age it well past any threshold.
        let admin = repo.join(".git").join("worktrees").join("bright-running-fox").join("HEAD");
        let _ = backdate(&admin, SystemTime::now() - Duration::from_secs(90 * 86_400));

        let report = gc(&repo, 7, /* apply = */ true);
        assert!(wt.exists(), "a worktree holding work survives the sweep");
        assert!(report.removed.is_empty(), "{:?}", report.removed);
        assert!(
            report.kept.iter().any(|k| k.reason == "holds uncommitted work"),
            "and the report says why",
        );

        // The other direction, same fixture minus the work: once clean, the
        // same over-threshold worktree IS collected — so the guard above is the
        // work, not the age.
        std::fs::remove_file(wt.join("unsaved.txt")).unwrap();
        let report = gc(&repo, 7, /* apply = */ true);
        assert!(!wt.exists(), "a clean orphan past the threshold is removed");
        assert_eq!(report.removed.len(), 1, "{:?}", report.kept.len());
    }

    #[test]
    fn dry_run_does_not_remove_anything() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-dry-run");
        let wt = fake_worktree(&repo, "old", 30);
        let report = gc(&repo, 7, /* apply = */ false);
        assert!(wt.exists(), "dry-run must not touch the filesystem");
        assert!(report.removed.is_empty());
        // The eligible worktree shows up in `kept[]` with reason "dry-run".
        assert!(report
            .kept
            .iter()
            .any(|k| k.reason == "dry-run" && k.age_days.unwrap_or(0) >= 30));
    }

    #[test]
    fn apply_removes_only_above_threshold() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-threshold");
        let young = fake_worktree(&repo, "young", 1);
        let edge = fake_worktree(&repo, "edge", 7);
        let old = fake_worktree(&repo, "old", 30);

        let report = gc(&repo, 7, /* apply = */ true);

        // The 1-day and 7-day worktrees survive; only the 30-day one goes.
        assert!(young.exists(), "1d worktree must survive");
        assert!(edge.exists(), "7d worktree must survive (threshold inclusive)");
        assert!(!old.exists(), "30d worktree must be removed");

        assert_eq!(report.removed.len(), 1);
        assert!(report.removed[0].ends_with("recursing-old-063389"), "{}", report.removed[0]);
        // The two survivors land in `kept[]` with reason "below threshold".
        let below: Vec<&KeptEntry> = report
            .kept
            .iter()
            .filter(|k| k.reason == "below threshold")
            .collect();
        assert_eq!(below.len(), 2);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn session_start_probe_is_noop_when_dir_missing() {
        let dir = tempdir().unwrap();
        // No .claude/worktrees at all — must not panic / exit / mutate.
        session_start_probe(dir.path());
    }

    #[test]
    fn session_start_probe_collects_instead_of_reporting() {
        // The behaviour this wave exists for: the probe that used to run in
        // simulation now acts. A stale, unowned worktree goes; a young one
        // stays, so what changed is the ACTION, not the rule.
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-probe-acts");
        let old = fake_worktree(&repo, "old", 30);
        let young = fake_worktree(&repo, "young", 1);
        session_start_probe(&repo);
        assert!(!old.exists(), "a stale orphan is collected, not merely counted");
        assert!(young.exists(), "and the age fallback still spares a young one");
    }

    // -----------------------------------------------------------------------
    // Ownership — the scratch worktrees the removal pass cuts into temp
    // -----------------------------------------------------------------------

    /// A real repo with one commit, at `<tmp>/<name>` so its scratch prefix is
    /// unique to this test (the temp directory is shared with every other test
    /// and every other project on the machine).
    fn seeded_repo(dir: &Path, name: &str) -> PathBuf {
        let repo = dir.join(name);
        fs::create_dir_all(&repo).unwrap();
        for args in [
            vec!["init", "-b", "dev"],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "t"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            Command::new("git").args(&args).current_dir(&repo).output().expect("git");
        }
        fs::write(repo.join("mustard.json"), r#"{"git":{"flow":{"*":"dev"}}}"#).unwrap();
        fs::write(repo.join("a.txt"), "seed").unwrap();
        for args in [vec!["add", "-A"], vec!["commit", "-m", "seed"]] {
            Command::new("git").args(&args).current_dir(&repo).output().expect("git");
        }
        repo
    }

    /// Cut a scratch worktree of `repo` where the removal pass cuts one — the
    /// OS temp directory, named `mustard-removal-{slug}-{pid}` — and hand back
    /// its path. Mirrors `work_removed::scratch_path` through the shared
    /// prefix, so the fixture cannot drift from the real name.
    fn scratch_worktree(repo: &Path, pid: u32) -> PathBuf {
        let tree = std::env::temp_dir().join(format!("{}{pid}", scratch_prefix(repo)));
        let _ = std::fs::remove_dir_all(&tree);
        Command::new("git")
            .args(["worktree", "add", "--detach", &tree.to_string_lossy(), "HEAD"])
            .current_dir(repo)
            .output()
            .expect("git");
        tree
    }

    /// A process id that is certainly NOT running: spawn something trivial,
    /// take its id, and wait for it to exit.
    fn dead_pid() -> u32 {
        let mut child = Command::new("git")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("git");
        let pid = child.id();
        let _ = child.wait();
        pid
    }

    /// AC-4 — a worktree whose owner is gone is collected NOW, minutes old,
    /// nowhere near the seven-day threshold. Two-sided: the identical worktree
    /// owned by a process that IS alive survives, so what collected the first
    /// one was ownership and not merely the widened reach.
    #[test]
    fn an_orphan_worktree_is_collected_without_waiting_for_age() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-orphan-age");
        let orphan = scratch_worktree(&repo, dead_pid());
        assert!(orphan.exists(), "fixture: the scratch worktree was cut");

        let report = gc(&repo, DEFAULT_AGE_DAYS, /* apply = */ true);
        assert!(
            !orphan.exists(),
            "an orphan minutes old is collected: {:?}",
            report.kept.iter().map(|k| &k.reason).collect::<Vec<_>>()
        );
        assert_eq!(report.removed.len(), 1, "{:?}", report.removed);

        let busy = scratch_worktree(&repo, std::process::id());
        let report = gc(&repo, DEFAULT_AGE_DAYS, /* apply = */ true);
        assert!(busy.exists(), "a worktree whose owner is alive is never touched");
        assert!(
            report.kept.iter().any(|k| k.reason == "owner alive"),
            "and the report names the owner, not the age: {:?}",
            report.kept.iter().map(|k| &k.reason).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(&busy);
    }

    /// AC-5 — the collector that now ACTS still refuses a worktree holding
    /// work, even when its owner is provably gone. The guard that made acting
    /// safe is not weakened by acting.
    #[test]
    fn the_acting_collector_still_refuses_a_worktree_holding_work() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-orphan-dirty");
        let orphan = scratch_worktree(&repo, dead_pid());
        fs::write(orphan.join("unsaved.txt"), "never committed").unwrap();

        session_start_probe(&repo);
        assert!(orphan.exists(), "untracked work survives an orphaned owner");

        let report = gc(&repo, DEFAULT_AGE_DAYS, /* apply = */ true);
        assert!(report.removed.is_empty(), "{:?}", report.removed);
        assert!(
            report.kept.iter().any(|k| k.reason == "holds uncommitted work"),
            "and the report says why: {:?}",
            report.kept.iter().map(|k| &k.reason).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(&orphan);
    }

    /// Plant the `.git/worktrees/<name>/HEAD` marker [`age_signal`] reads
    /// first, backdated to `when`.
    ///
    /// BEST-EFFORT, and nothing in a test may hang off it: `set_modified` is a
    /// filesystem favour, not a guarantee, and the collector's own doc calls the
    /// whole age signal fail-open. A test that needs a verdict must get it from
    /// something measured — which is why [`contents`] is asked before this ever
    /// decides anything.
    fn age_marker(repo: &Path, name: &str, when: SystemTime) {
        let admin = repo.join(".git").join("worktrees").join(name);
        fs::create_dir_all(&admin).unwrap();
        let head = admin.join("HEAD");
        fs::write(&head, "ref: refs/heads/detached\n").unwrap();
        let _ = backdate(&head, when);
    }

    /// AC-12 — the collector removes on a POSITIVE observation of emptiness and
    /// on nothing else. Both halves, in the two shapes the field has:
    ///
    /// (a) a candidate whose emptiness could not be ESTABLISHED — git's own
    ///     bookkeeping claims the path is a checkout, and git cannot speak for
    ///     it — is kept, not removed;
    /// (b) a plain directory under `.claude/worktrees/`, never registered as a
    ///     worktree, holding a file under `.claude/`, is seen as HOLDING WORK.
    ///
    /// (b) is the reproduction that found the defect. `git status` run inside a
    /// directory that is not its own checkout answers about the ENCLOSING
    /// repository, and the cut decision's probe then drops everything under
    /// `.claude/` as redirected state — so the candidate's own contents were
    /// invisible, "clean" was read from a tree nobody was deciding on, and
    /// `--apply` deleted unsaved files. The first assertion below pins that
    /// blindness in place, so this test fails the day the collector goes back
    /// to asking it.
    ///
    /// Both verdicts here are reached WITHOUT consulting a clock, and that is
    /// deliberate. While the age fallback was asked first, this proof was
    /// reachable only where a best-effort mtime happened to read old: it passed
    /// on Windows and reported `below threshold` — "too young to consider" — on
    /// the Linux and macOS runners, where it proved nothing at all about what
    /// the collector can establish. The fixture states its own preconditions
    /// below so no future arrangement can make it quietly vacuous again.
    #[test]
    fn the_collector_refuses_what_it_could_not_prove_empty() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-unproven");
        let stale = SystemTime::now() - Duration::from_secs(30 * 86_400);

        // (b) A directory where the collector looks, holding an unsaved file
        // under `.claude/`. No `git worktree add`, no `.git` of its own.
        let holding = repo.join(".claude").join("worktrees").join("pasta-antiga");
        let precious = holding.join(".claude").join("precioso.txt");
        fs::create_dir_all(holding.join(".claude")).unwrap();
        fs::write(&precious, "trabalho que ninguem salvou").unwrap();
        age_marker(&repo, "pasta-antiga", stale);

        // (a) A path git's bookkeeping calls a checkout, with no checkout there
        // to measure: nothing about THIS directory can be established. The
        // `.git` pointer is what an abandoned worktree really leaves behind, and
        // it is planted here so the claim lives INSIDE the candidate rather than
        // only in a `.git/worktrees/` record — which is a registration git is
        // free to reap, and which a fixture must not depend on for its meaning.
        let ghost = repo.join(".claude").join("worktrees").join("pasta-fantasma");
        fs::create_dir_all(&ghost).unwrap();
        age_marker(&repo, "pasta-fantasma", stale);
        let admin = repo.join(".git").join("worktrees").join("pasta-fantasma");
        fs::write(ghost.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();

        // The mechanism that lost the files: the CUT decision's probe reports
        // both candidates clean, because it answers about the enclosing repo
        // and drops everything under `.claude/`.
        for candidate in [&holding, &ghost] {
            assert!(
                crate::commands::work_unit_open::dirty_paths(candidate).is_empty(),
                "fixture: the cut probe is blind here — that is the defect ({})",
                candidate.display(),
            );
            assert!(
                list_collectable_worktrees(&repo).contains(candidate),
                "fixture: the sweep must actually see the candidate ({})",
                candidate.display(),
            );
        }

        // The two preconditions (a) rests on, stated rather than assumed: git
        // cannot speak for the ghost, and something nonetheless claims the path
        // is a checkout. Together they are what makes "could not be proven
        // empty" the only honest answer about it.
        assert!(
            own_checkout_status(&ghost).is_none(),
            "fixture: git must not be able to answer for the ghost itself",
        );
        assert!(
            claimed_as_checkout(&repo, &ghost),
            "fixture: something must claim the ghost is a checkout",
        );

        let report = gc(&repo, DEFAULT_AGE_DAYS, /* apply = */ true);
        let reason_for = |p: &Path| {
            report
                .kept
                .iter()
                .find(|k| k.path == p.display().to_string())
                .map_or_else(String::new, |k| k.reason.clone())
        };
        // The age travels with every reason in the dump: a verdict that turns
        // out to have come from the clock has to say so in its own failure,
        // instead of leaving the next reader to guess across three platforms.
        let dump = || {
            report
                .kept
                .iter()
                .map(|k| (&k.path, &k.reason, k.age_days))
                .collect::<Vec<_>>()
        };

        assert!(report.removed.is_empty(), "nothing was proven empty: {:?}", report.removed);
        assert!(precious.exists(), "the unsaved file survives an --apply sweep");
        assert_eq!(
            fs::read_to_string(&precious).unwrap_or_default(),
            "trabalho que ninguem salvou",
            "and survives with its contents intact",
        );
        assert_eq!(
            reason_for(&holding),
            "holds uncommitted work",
            "a directory holding files under `.claude/` holds work — and says so \
             whatever its age reads: {:?}",
            dump(),
        );

        assert!(ghost.exists(), "what could not be measured is never removed");
        assert_eq!(
            reason_for(&ghost),
            "could not be proven empty",
            "and the report says it was never established, not that it was clean: {:?}",
            dump(),
        );
    }

    /// The refusal is not a function of the clock: a candidate the age signal
    /// reads as BRAND NEW, holding an unsaved file, is refused for what it
    /// HOLDS and says so.
    ///
    /// This is the shape AC-12 could not pin on its own. AC-12 ages its fixture,
    /// so on any machine where the backdating takes it reaches [`contents`]
    /// whatever the order is — which is how "ask the age first" survived review
    /// and shipped, green on Windows and reporting `below threshold` on the
    /// Linux and macOS runners. Here the age is genuinely 0, so the only way to
    /// reach "holds uncommitted work" is to measure the contents BEFORE falling
    /// back to age. Put the age check first again and this goes red everywhere.
    #[test]
    fn a_young_candidate_holding_work_is_refused_for_what_it_holds() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-young-dirty");
        // No age marker at all: created now, so `age_signal` falls back to the
        // directory's own mtime and the entry is 0 days old.
        let fresh = repo.join(".claude").join("worktrees").join("pasta-nova");
        fs::create_dir_all(&fresh).unwrap();
        fs::write(fresh.join("unsaved.txt"), "trabalho que ninguem salvou").unwrap();

        let report = gc(&repo, DEFAULT_AGE_DAYS, /* apply = */ true);
        let kept = report
            .kept
            .iter()
            .find(|k| k.path == fresh.display().to_string())
            .expect("the young candidate is in the report");

        assert_eq!(kept.age_days, Some(0), "fixture: the candidate really is brand new");
        assert_eq!(
            kept.reason, "holds uncommitted work",
            "a young directory holding work is refused for the work, not spared for its age",
        );
        assert!(fresh.exists(), "and it survives an --apply sweep");
        assert!(report.removed.is_empty(), "{:?}", report.removed);
    }

    /// AC-6 — the worktree an interrupted removal pass leaves behind lives in
    /// the OS temp directory, outside the only tree the collector used to walk.
    /// It is now within reach AND collected by the session-start sweep.
    #[test]
    fn an_abandoned_removal_worktree_is_within_reach_and_collected() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-abandoned-proof");
        let abandoned = scratch_worktree(&repo, dead_pid());

        assert!(
            list_collectable_worktrees(&repo).contains(&abandoned),
            "reach: the sweep sees a worktree outside .claude/worktrees/"
        );
        session_start_probe(&repo);
        assert!(!abandoned.exists(), "and collects it");
    }

    /// The temp directory belongs to the whole machine, so a look-alike that
    /// this repository never registered is not ours to remove.
    #[test]
    fn a_scratch_name_this_repo_never_registered_is_out_of_reach() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-lookalike");
        let impostor = std::env::temp_dir().join(format!("{}{}", scratch_prefix(&repo), 4_242));
        fs::create_dir_all(&impostor).unwrap();

        let found = list_collectable_worktrees(&repo);
        assert!(
            !found.contains(&impostor),
            "no `.git/worktrees/<name>` record, so not this repo's to collect"
        );
        let _ = std::fs::remove_dir_all(&impostor);
    }

    /// Ownership is read from the harness's own prefix and from nothing else —
    /// a platform slug ending in digits is NOT a process id.
    #[test]
    fn a_slugs_trailing_digits_are_never_read_as_an_owner() {
        let prefix = scratch_prefix(Path::new("/x/proj"));
        assert_eq!(prefix, "mustard-removal-proj-");
        assert!(matches!(
            ownership(Path::new("/x/.claude/worktrees/recursing-benz-063389"), &prefix),
            Ownership::Unknown
        ));
        assert!(matches!(
            ownership(Path::new("/tmp/mustard-removal-proj-notanumber"), &prefix),
            Ownership::Unknown
        ));
        assert!(matches!(
            ownership(
                &std::env::temp_dir().join(format!("{prefix}{}", std::process::id())),
                &prefix
            ),
            Ownership::Alive
        ));
    }

    #[test]
    fn age_signal_prefers_admin_head_over_dir_mtime() {
        let dir = tempdir().unwrap();
        let repo = seeded_repo(dir.path(), "gc-age-signal");
        let wt = fake_worktree(&repo, "x", 30);
        let signal = age_signal(&repo, &wt).expect("HEAD marker is present");
        let days = age_days_since(signal).unwrap_or(0);
        assert!(days >= 29, "expected ~30d, got {days}");
    }

    #[test]
    fn unknown_age_keeps_worktree() {
        // No admin HEAD, no dir mtime override — the dir was just created so
        // age_days_since returns 0, which is below any positive threshold.
        let dir = tempdir().unwrap();
        let root = dir.path().join(".claude").join("worktrees").join("bright-running-fox");
        fs::create_dir_all(&root).unwrap();
        let report = gc(dir.path(), 7, true);
        assert!(report.removed.is_empty(), "fresh dir must not be removed");
        assert_eq!(report.kept.len(), 1);
    }

    #[test]
    fn report_json_shape_is_stable() {
        let dir = tempdir().unwrap();
        let report = gc(dir.path(), 7, false);
        let value = serde_json::to_value(&report).unwrap();
        // The three named arrays the AC checks for must be present and ARRAY-typed.
        assert!(value.get("removed").is_some_and(Value::is_array));
        assert!(value.get("kept").is_some_and(Value::is_array));
        assert!(value.get("errors").is_some_and(Value::is_array));
    }
}
