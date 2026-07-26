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
//! name carries a declared `{base}_` is a WORK UNIT's, and cleanup of those is
//! `git-settle`'s job EXCLUSIVELY. Everything else is collectable.
//!
//! Widening what a destructive sweep can see demands the other half of the
//! platform's own contract, so it is here too: a worktree that still HOLDS WORK
//! (uncommitted or untracked changes, `.claude/` excepted) is never removed,
//! whatever its age. What removal can cost is then a checkout with nothing in
//! it — its branch survives `git worktree remove` untouched.
//!
//! This subcommand enumerates those worktrees, computes each one's age, and
//! removes those older than `--age-days N`. Dry-run by default; `--apply` is
//! required to mutate the filesystem.
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

use crate::commands::work_unit_open::{dirty_paths, is_unit_worktree_name};
use crate::shared::context::{current_spec, session_id};
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
    /// Human-readable reason: `"below threshold"`, `"unknown age"`,
    /// or `"dry-run"` (when `--apply` is not set).
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
fn list_collectable_worktrees(repo: &Path) -> Vec<PathBuf> {
    let Ok(paths) = ClaudePaths::for_project(repo) else {
        return Vec::new();
    };
    let bases: Vec<String> =
        mustard_core::ProjectConfig::load(repo).git.integration_bases().into_iter().collect();
    let root = paths.claude_dir().join("worktrees");
    let Ok(read) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !is_unit_worktree_name(n, &bases))
        })
        .collect();
    out.sort();
    out
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

    for wt in list_collectable_worktrees(repo) {
        let path = wt.display().to_string();
        let Some(mtime) = age_signal(repo, &wt) else {
            report.kept.push(KeptEntry {
                path,
                age_days: None,
                reason: "unknown age".into(),
            });
            continue;
        };
        let Some(age) = age_days_since(mtime) else {
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

        // Holds work → never removed, at any age: the exception the platform's
        // own periodic sweep makes, and what keeps collecting by "not a work
        // unit" safe rather than merely wider. Asked ONLY of an entry already
        // over the threshold — the probe that runs at every SessionStart must
        // not spawn a `git status` per worktree just to report an age.
        if !dirty_paths(&wt).is_empty() {
            report.kept.push(KeptEntry {
                path,
                age_days: Some(age),
                reason: "holds uncommitted work".into(),
            });
            continue;
        }

        if !apply {
            report.kept.push(KeptEntry {
                path,
                age_days: Some(age),
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

/// Threshold for the `SessionStart` advisory warning: more than this many
/// orphan worktrees older than the default `age_days` triggers a single
/// `eprintln!` (telemetry-only; never blocks).
const SESSION_WARN_THRESHOLD: usize = 3;

/// Default `--age-days` value used by the CLI and the SessionStart probe.
pub const DEFAULT_AGE_DAYS: u32 = 7;

/// Idempotent `SessionStart` probe: count worktrees older than
/// [`DEFAULT_AGE_DAYS`] and emit a stderr warning when the count exceeds
/// [`SESSION_WARN_THRESHOLD`]. Never mutates the filesystem.
///
/// Fail-open: a missing `.claude/worktrees/` directory or any IO failure
/// degrades to a silent no-op — the warning is advisory and must not break a
/// session boot.
pub fn session_start_probe(repo: &Path) {
    let report = gc(repo, DEFAULT_AGE_DAYS, /* apply = */ false);
    // `dry-run` kept entries that exceed the threshold are the orphan set.
    let orphan_count = report
        .kept
        .iter()
        .filter(|k| k.reason == "dry-run")
        .count();
    if orphan_count > SESSION_WARN_THRESHOLD {
        eprintln!(
            "[worktree-gc] {orphan_count} orphan worktrees older than {DEFAULT_AGE_DAYS}d in {} — \
             run `mustard-rt run worktree-gc --apply` to clean up.",
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

    /// Create a fake agent worktree at `<repo>/.claude/worktrees/<slug>`
    /// alongside the matching `.git/worktrees/<basename>/HEAD` marker. The
    /// marker is the file `age_signal` reads first; backdating it controls the
    /// computed age without needing a real `git worktree add`.
    ///
    /// The name is a SLUG, the shape `WorktreeCreate` actually hands over — the
    /// old `agent-<id>` fixture matched a prefix the platform never emits, so
    /// it was the only thing keeping the old filter looking alive.
    fn fake_worktree(repo: &Path, id: &str, age_days: u64) -> PathBuf {
        let basename = format!("recursing-{id}-063389");
        let wt = repo.join(".claude").join("worktrees").join(&basename);
        fs::create_dir_all(wt.join("src")).unwrap();
        // A token file inside so std::fs::remove_dir_all has something to do.
        fs::write(wt.join("src").join("touch.txt"), "x").unwrap();

        let admin = repo.join(".git").join("worktrees").join(&basename);
        fs::create_dir_all(&admin).unwrap();
        let head = admin.join("HEAD");
        fs::write(&head, "ref: refs/heads/worktree-agent-x\n").unwrap();

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
        let wt = fake_worktree(dir.path(), "old", 30);
        let report = gc(dir.path(), 7, /* apply = */ false);
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
        let young = fake_worktree(dir.path(), "young", 1);
        let edge = fake_worktree(dir.path(), "edge", 7);
        let old = fake_worktree(dir.path(), "old", 30);

        let report = gc(dir.path(), 7, /* apply = */ true);

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
    fn session_start_probe_does_not_remove_files() {
        let dir = tempdir().unwrap();
        let old = fake_worktree(dir.path(), "old", 30);
        session_start_probe(dir.path());
        assert!(old.exists(), "probe is read-only");
    }

    #[test]
    fn age_signal_prefers_admin_head_over_dir_mtime() {
        let dir = tempdir().unwrap();
        let wt = fake_worktree(dir.path(), "x", 30);
        let signal = age_signal(dir.path(), &wt).expect("HEAD marker is present");
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
