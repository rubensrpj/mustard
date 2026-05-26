//! `mustard-rt run unhook` — kill-switch for the harness.
//!
//! Disables the Claude Code hook layer by renaming `.claude/settings.json` to
//! `settings.json.disabled-<timestamp>` and wiping volatile harness state
//! (`.agent-state/`, `.cluster-cache.json`, `.worktrees/`). Restore with
//! `mustard-rt run rehook`.
//!
//! Scope selection:
//!
//! - `this` (default) — only `<repo>/.claude/settings.json`.
//! - `monorepo` — `<repo>/.claude/` + every `apps/*/.claude/` and
//!   `packages/*/.claude/` it finds.
//! - `all` — `monorepo` plus the user-global `~/.claude/settings.json`. Gated
//!   by `--confirm`: without it, the global target is skipped and surfaced as
//!   `state: "skipped"` in the report.
//!
//! Fail-open: a missing `settings.json` lands in the report as
//! `state: "missing"` instead of erroring; per-entry IO failures are captured
//! in `state: "error"` so the rest of the sweep still completes.

use crate::util::now_iso8601;
use mustard_core::ClaudePaths;
use serde::Serialize;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Options + report types
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run unhook`.
pub struct UnhookOpts {
    /// Repo root override. Defaults to the current working directory.
    pub repo: Option<PathBuf>,
    /// Scope: `this`, `monorepo`, or `all`.
    pub scope: String,
    /// Required for `--scope all` (touches user-global settings).
    pub confirm: bool,
}

/// One per `.claude/` directory the sweep touched.
#[derive(Serialize)]
pub struct DisabledEntry {
    /// The settings.json path we targeted.
    pub settings_path: String,
    /// The disabled path (`Some` only when state == "disabled").
    pub moved_to: Option<String>,
    /// `disabled` | `missing` | `skipped` | `error`.
    pub state: String,
    /// Volatile state paths we wiped (recursive contents, then the dir/file).
    pub cleared: Vec<String>,
    /// Populated when `state == "error"` or a cleanup hit an unrecoverable IO error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The full machine-readable report.
#[derive(Serialize)]
pub struct UnhookReport {
    pub scope: String,
    pub timestamp: String,
    pub entries: Vec<DisabledEntry>,
    /// One-liner the user can paste to revert.
    pub revert_with: String,
}

// ---------------------------------------------------------------------------
// Scope resolution
// ---------------------------------------------------------------------------

/// Collect every `.claude/` directory in scope.
///
/// Order is deterministic: repo root first, then `apps/*` (sorted), then
/// `packages/*` (sorted), then the user-global directory (when `--scope all`
/// and `--confirm` line up). Used by both `unhook` and `rehook`.
pub(crate) fn collect_claude_dirs(
    repo: &Path,
    scope: &str,
    confirm: bool,
) -> Vec<(PathBuf, ScopeKind)> {
    let mut dirs: Vec<(PathBuf, ScopeKind)> = Vec::new();

    // Every input — repo root, each subproject under `apps/*` / `packages/*`,
    // and the user-global home — flows through `ClaudePaths::for_project` so
    // the I1 `.claude/.claude/` guard fires at the boundary. An input that
    // already nests is dropped from the sweep (the only failure mode of
    // `for_project`), which is the right call: we never want to disable a
    // settings file we built off a contaminated path.
    if let Ok(paths) = ClaudePaths::for_project(repo) {
        dirs.push((paths.claude_dir(), ScopeKind::Repo));
    }

    if matches!(scope, "monorepo" | "all") {
        for parent in ["apps", "packages"] {
            let base = repo.join(parent);
            let Ok(read) = std::fs::read_dir(&base) else { continue };
            let mut subs: Vec<PathBuf> = read
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|e| {
                    let sub_root = e.path();
                    ClaudePaths::for_project(&sub_root)
                        .ok()
                        .map(|p| p.claude_dir())
                })
                .filter(|p| p.is_dir())
                .collect();
            subs.sort();
            for p in subs {
                dirs.push((p, ScopeKind::Subproject));
            }
        }
    }

    if scope == "all" {
        if let Some(home) = home_dir() {
            // `$HOME` is not a Mustard workspace anchor (no `mustard.json`),
            // but `ClaudePaths::for_project` only rejects re-nested `.claude/`
            // paths — a flat home directory is accepted and the guard still
            // fires if `home` happens to terminate in `.claude`.
            if let Ok(home_paths) = ClaudePaths::for_project(&home) {
                let global = home_paths.claude_dir();
                if confirm {
                    dirs.push((global, ScopeKind::Global));
                } else {
                    dirs.push((global, ScopeKind::GlobalSkipped));
                }
            }
        }
    }

    dirs
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    Repo,
    Subproject,
    Global,
    /// `--scope all` was passed without `--confirm`; surface as `skipped`.
    GlobalSkipped,
}

/// Best-effort home directory: `HOME` then `USERPROFILE`.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// Per-directory disable
// ---------------------------------------------------------------------------

/// Disable one `.claude/` directory's settings + wipe its volatile state.
fn disable_one(claude_dir: &Path, kind: ScopeKind, timestamp_tag: &str) -> DisabledEntry {
    let settings = claude_dir.join("settings.json");
    let settings_path = settings.display().to_string();

    if kind == ScopeKind::GlobalSkipped {
        return DisabledEntry {
            settings_path,
            moved_to: None,
            state: "skipped".into(),
            cleared: Vec::new(),
            error: Some("user-global target requires --confirm".into()),
        };
    }

    if !settings.exists() {
        // No settings.json — still wipe any stray volatile state below.
        let cleared = wipe_volatile_state(claude_dir);
        return DisabledEntry {
            settings_path,
            moved_to: None,
            state: "missing".into(),
            cleared,
            error: None,
        };
    }

    let disabled = claude_dir.join(format!("settings.json.disabled-{timestamp_tag}"));
    if let Err(e) = std::fs::rename(&settings, &disabled) {
        return DisabledEntry {
            settings_path,
            moved_to: None,
            state: "error".into(),
            cleared: Vec::new(),
            error: Some(format!("rename failed: {e}")),
        };
    }

    let cleared = wipe_volatile_state(claude_dir);
    DisabledEntry {
        settings_path,
        moved_to: Some(disabled.display().to_string()),
        state: "disabled".into(),
        cleared,
        error: None,
    }
}

/// Wipe the three volatile-state paths the user named: `.agent-state/`,
/// `.cluster-cache.json`, `.worktrees/`. Returns the paths actually removed.
fn wipe_volatile_state(claude_dir: &Path) -> Vec<String> {
    let mut cleared: Vec<String> = Vec::new();

    let agent_state = claude_dir.join(".agent-state");
    if agent_state.is_dir() && std::fs::remove_dir_all(&agent_state).is_ok() {
        cleared.push(agent_state.display().to_string());
    }

    let cluster_cache = claude_dir.join(".cluster-cache.json");
    if cluster_cache.is_file() && std::fs::remove_file(&cluster_cache).is_ok() {
        cleared.push(cluster_cache.display().to_string());
    }

    let worktrees = claude_dir.join(".worktrees");
    if worktrees.is_dir() && std::fs::remove_dir_all(&worktrees).is_ok() {
        cleared.push(worktrees.display().to_string());
    }

    cleared
}

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

/// Filename-safe variant of [`now_iso8601`] — `:` is illegal on NTFS, so
/// `2026-05-24T12:34:56.789Z` becomes `2026-05-24T12-34-56`.
pub(crate) fn filename_safe_timestamp() -> String {
    let iso = now_iso8601();
    iso.split('.')
        .next()
        .unwrap_or(&iso)
        .replace(':', "-")
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Dispatch `mustard-rt run unhook [--repo <p>] [--scope this|monorepo|all] [--confirm]`.
pub fn run(opts: UnhookOpts) {
    let repo = opts
        .repo
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let scope = opts.scope.as_str();

    if !matches!(scope, "this" | "monorepo" | "all") {
        eprintln!("unhook: --scope must be one of: this, monorepo, all (got '{scope}')");
        std::process::exit(1);
    }

    let timestamp_tag = filename_safe_timestamp();
    let dirs = collect_claude_dirs(&repo, scope, opts.confirm);

    let entries: Vec<DisabledEntry> = dirs
        .iter()
        .map(|(dir, kind)| disable_one(dir, *kind, &timestamp_tag))
        .collect();

    let report = UnhookReport {
        scope: scope.to_string(),
        timestamp: now_iso8601(),
        entries,
        revert_with: format!("mustard-rt run rehook --scope {scope}"),
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn filename_safe_timestamp_has_no_colons() {
        let ts = filename_safe_timestamp();
        assert!(!ts.contains(':'), "got {ts}");
        assert!(ts.contains('T'), "got {ts}");
    }

    #[test]
    fn scope_this_returns_only_repo_root() {
        let dir = tempdir().unwrap();
        let dirs = collect_claude_dirs(dir.path(), "this", false);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].0, dir.path().join(".claude"));
        assert_eq!(dirs[0].1, ScopeKind::Repo);
    }

    #[test]
    fn scope_monorepo_includes_apps_and_packages() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("apps/cli/.claude")).unwrap();
        std::fs::create_dir_all(dir.path().join("apps/rt/.claude")).unwrap();
        std::fs::create_dir_all(dir.path().join("packages/core/.claude")).unwrap();

        let dirs = collect_claude_dirs(dir.path(), "monorepo", false);
        let names: Vec<String> = dirs
            .iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        assert!(names.iter().any(|n| n.ends_with("cli\\.claude") || n.ends_with("cli/.claude")));
        assert!(names.iter().any(|n| n.ends_with("rt\\.claude") || n.ends_with("rt/.claude")));
        assert!(names.iter().any(|n| n.ends_with("core\\.claude") || n.ends_with("core/.claude")));
    }

    #[test]
    fn disable_renames_settings_and_clears_state() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        write(&claude.join("settings.json"), r#"{"hooks":{}}"#);
        write(&claude.join(".agent-state/counter.json"), "1");
        write(&claude.join(".cluster-cache.json"), "[]");

        let entry = disable_one(&claude, ScopeKind::Repo, "2026-05-24T12-00-00");

        assert_eq!(entry.state, "disabled");
        assert!(entry.moved_to.unwrap().ends_with(".disabled-2026-05-24T12-00-00"));
        assert!(!claude.join("settings.json").exists());
        assert!(!claude.join(".agent-state").exists());
        assert!(!claude.join(".cluster-cache.json").exists());
        assert_eq!(entry.cleared.len(), 2);
    }

    #[test]
    fn disable_missing_settings_reports_missing() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let entry = disable_one(&claude, ScopeKind::Repo, "ts");
        assert_eq!(entry.state, "missing");
        assert!(entry.moved_to.is_none());
    }

    #[test]
    fn global_without_confirm_is_skipped() {
        let dir = tempdir().unwrap();
        let entry = disable_one(&dir.path().join(".claude"), ScopeKind::GlobalSkipped, "ts");
        assert_eq!(entry.state, "skipped");
        assert!(entry.error.unwrap().contains("--confirm"));
    }
}
