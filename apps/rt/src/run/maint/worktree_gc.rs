//! `mustard-rt run worktree-gc` — garbage-collect orphan Claude agent worktrees.
//!
//! ## Why
//!
//! Every `Task` invocation with `isolation: "worktree"` carves out a fresh git
//! worktree under `<repo>/.claude/worktrees/agent-<id>/`. When the task ends
//! cleanly the orchestrator removes it; when it crashes (process killed,
//! network drop, panic), the worktree lingers. They mirror the source tree,
//! so they balloon `docs-stale-check`, `security-scan`, and any other
//! filesystem walker — and the `locked` marker keeps `git worktree prune`
//! from reaping them automatically.
//!
//! This subcommand enumerates `.claude/worktrees/agent-*`, computes each
//! one's age, and removes those older than `--age-days N`. Dry-run by
//! default; `--apply` is required to mutate the filesystem.
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
//! - `worktree.gc.run { removed_count, kept_count, dry_run }` — the GC summary.
//! - `pipeline.economy.operation.invoked { operation: "worktree-gc", duration_ms }`
//!   — the universal `/economia` operation marker (W12 contract).

use crate::shared::context::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
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

/// Enumerate `<repo>/.claude/worktrees/agent-*` directories. Returns an empty
/// vec when the parent path is missing (fail-open).
///
/// `worktrees/` has no typed accessor on `ClaudePaths` (it's a legacy direct
/// child of `.claude/`); routing via `claude_dir()` keeps the boundary owned
/// by the canonical handle without expanding W4 scope.
fn list_agent_worktrees(repo: &Path) -> Vec<PathBuf> {
    let Ok(paths) = ClaudePaths::for_project(repo) else {
        return Vec::new();
    };
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
                .is_some_and(|n| n.starts_with("agent-"))
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

    for wt in list_agent_worktrees(repo) {
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

/// Emit `worktree.gc.run` + `pipeline.economy.operation.invoked` to the
/// project's harness event store. Fail-open at every step.
fn emit_telemetry(
    repo: &Path,
    removed_count: usize,
    kept_count: usize,
    dry_run: bool,
    duration_ms: u128,
) {
    let dir = repo.display().to_string();
    let spec = current_spec(&dir);
    let session = session_id();
    let ts = now_iso8601();

    // Cast a small unsigned count into i64 — clippy::cast_possible_wrap can't
    // hit on a `usize` derived from at most ~10^4 worktrees per repo.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let removed_i = removed_count as i64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let kept_i = kept_count as i64;
    // `duration_ms` is unbounded `u128`; cap at i64::MAX before casting so we
    // never produce a negative JSON number on an overflow.
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);

    let gc_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.clone(),
        session_id: session.clone(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("worktree-gc".to_string()),
            actor_type: None,
        },
        event: "worktree.gc.run".to_string(),
        payload: json!({
            "removed_count": removed_i,
            "kept_count": kept_i,
            "dry_run": dry_run,
        }),
        spec: spec.clone(),
    };

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

    // W5: `worktree.gc.run` is non-pipeline (NDJSON); `pipeline.economy.*`
    // is pipeline (SQLite). The router classifies each correctly so we no
    // longer need the open-store-then-append shape.
    let _ = crate::shared::events::route::emit(&dir, &gc_event);
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

    let removed_count = report.removed.len();
    let kept_count = report.kept.len();
    let dry_run = report.dry_run;

    // Print BEFORE telemetry so the byte-stable JSON ordering is independent
    // of how long the store append takes (or whether it succeeds at all).
    let body: Value = serde_json::to_value(&report)
        .unwrap_or_else(|_| json!({"removed":[],"kept":[],"errors":[]}));
    println!(
        "{}",
        serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into())
    );

    emit_telemetry(&repo, removed_count, kept_count, dry_run, duration_ms);
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

    /// Create a fake agent worktree at `<repo>/.claude/worktrees/agent-<id>`
    /// alongside the matching `.git/worktrees/<basename>/HEAD` marker. The
    /// marker is the file `age_signal` reads first; backdating it controls the
    /// computed age without needing a real `git worktree add`.
    fn fake_worktree(repo: &Path, id: &str, age_days: u64) -> PathBuf {
        let basename = format!("agent-{id}");
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
        assert!(list_agent_worktrees(dir.path()).is_empty());
    }

    #[test]
    fn list_skips_non_agent_prefixed_dirs() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".claude").join("worktrees");
        fs::create_dir_all(root.join("agent-good")).unwrap();
        fs::create_dir_all(root.join("not-agent")).unwrap();
        let found = list_agent_worktrees(dir.path());
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("agent-good"));
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
        assert!(report.removed[0].ends_with("agent-old"));
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
        let root = dir.path().join(".claude").join("worktrees").join("agent-new");
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
