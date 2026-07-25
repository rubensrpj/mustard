//! `mustard-rt run rehook` — restore the harness after [`crate::commands::maint::unhook`].
//!
//! For each `.claude/` directory in scope (`this` / `monorepo` / `all` — same
//! resolution as `unhook`), two shapes are restored:
//!
//! 1. **Current** — a live `settings.json` carrying `"disableAllHooks": true`.
//!    The key is removed and every other key is left verbatim. This is the
//!    exact inverse of what `unhook` writes today.
//! 2. **Legacy** — no `settings.json` at all, because an older build renamed it
//!    to `settings.json.disabled-<timestamp>`. The newest such snapshot is
//!    renamed back, so a project unhooked before this change is still
//!    recoverable by the current binary.
//!
//! Volatile state directories cleared by `unhook` are intentionally **not**
//! recreated — the runtime regenerates them on the next run.
//!
//! Fail-open: missing `.claude/`, no disabled snapshot, or a `settings.json`
//! that never carried the flag each surface as their own `state` string rather
//! than erroring.

use crate::commands::maint::unhook::{
    collect_claude_dirs, read_settings_object, write_settings_object, ScopeKind,
    DISABLE_ALL_HOOKS_KEY,
};
use mustard_core::time::now_iso8601;
use serde::Serialize;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Options + report types
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run rehook`.
pub struct RehookOpts {
    pub repo: Option<PathBuf>,
    pub scope: String,
    pub confirm: bool,
}

#[derive(Serialize)]
pub(crate) struct RestoredEntry {
    pub settings_path: String,
    /// Snapshot the settings file was renamed back from. Only the legacy path
    /// sets it — clearing `disableAllHooks` in place restores from nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
    /// `restored` | `already-active` | `no-snapshot` | `missing` | `skipped` | `error`.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct RehookReport {
    pub scope: String,
    pub timestamp: String,
    pub entries: Vec<RestoredEntry>,
}

// ---------------------------------------------------------------------------
// Per-directory restore
// ---------------------------------------------------------------------------

fn restore_one(claude_dir: &Path, kind: ScopeKind) -> RestoredEntry {
    let settings = claude_dir.join("settings.json");
    let settings_path = settings.display().to_string();

    if kind == ScopeKind::GlobalSkipped {
        return RestoredEntry {
            settings_path,
            restored_from: None,
            state: "skipped".into(),
            error: Some("user-global target requires --confirm".into()),
        };
    }

    if !claude_dir.is_dir() {
        return RestoredEntry {
            settings_path,
            restored_from: None,
            state: "missing".into(),
            error: None,
        };
    }

    // A live `settings.json` is the shape the current kill-switch leaves
    // behind: the file was never moved, only flagged. Clearing the flag IS the
    // restore. Checked before the snapshot sweep because a project that has
    // been unhooked by both builds must be restored from its live file, not
    // from an older snapshot.
    if settings.exists() {
        return match clear_disable_all_hooks(&settings) {
            Ok(true) => RestoredEntry {
                settings_path,
                restored_from: None,
                state: "restored".into(),
                error: None,
            },
            Ok(false) => RestoredEntry {
                settings_path,
                restored_from: None,
                state: "already-active".into(),
                error: None,
            },
            Err(e) => RestoredEntry {
                settings_path,
                restored_from: None,
                state: "error".into(),
                error: Some(e),
            },
        };
    }

    let Some(snapshot) = newest_disabled_snapshot(claude_dir) else {
        return RestoredEntry {
            settings_path,
            restored_from: None,
            state: "no-snapshot".into(),
            error: None,
        };
    };

    if let Err(e) = std::fs::rename(&snapshot, &settings) {
        return RestoredEntry {
            settings_path,
            restored_from: Some(snapshot.display().to_string()),
            state: "error".into(),
            error: Some(format!("rename failed: {e}")),
        };
    }

    // Re-assert PATH-independent hook commands on the restored snapshot. A
    // snapshot taken before this fix (or hand-edited) still carries the bare
    // `rtk mustard-rt on <Event>` tokens; rewrite them to the absolute path of
    // *this* `mustard-rt` (the binary running rehook). Fail-open — a rewrite
    // failure never downgrades the restore, which already succeeded.
    if let Some(exe) = mustard_core::resolve_mustard_rt() {
        let _ = mustard_core::rewrite_settings_hooks(claude_dir, &exe);
    }

    RestoredEntry {
        settings_path,
        restored_from: Some(snapshot.display().to_string()),
        state: "restored".into(),
        error: None,
    }
}

/// Remove the `disableAllHooks` key from a live `settings.json`, preserving
/// every other key. Returns `true` when the key was there (a real restore),
/// `false` when it was absent (the project was never unhooked, or a user
/// already cleared it) — in which case nothing is written.
///
/// `Err` on an unreadable / unparseable file: the report says so instead of
/// claiming a restore it could not verify, and the file is left as found.
fn clear_disable_all_hooks(settings: &Path) -> Result<bool, String> {
    let mut obj = read_settings_object(settings)?;
    if obj.remove(DISABLE_ALL_HOOKS_KEY).is_none() {
        return Ok(false);
    }
    write_settings_object(settings, obj)?;
    Ok(true)
}

/// Find the most-recent `settings.json.disabled*` snapshot in `claude_dir`.
/// Both the timestamped form (`settings.json.disabled-<ts>`) and the bare
/// `settings.json.disabled` from the session-start git status are accepted.
///
/// Ordering key: `(mtime, filename)` — the filename tiebreaker matters because
/// two snapshots created in the same second land with identical mtimes on
/// Windows; the ISO-8601 timestamp baked into the filename then provides
/// deterministic recency.
fn newest_disabled_snapshot(claude_dir: &Path) -> Option<PathBuf> {
    let read = std::fs::read_dir(claude_dir).ok()?;
    let mut best: Option<(std::time::SystemTime, String, PathBuf)> = None;

    for entry in read.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.starts_with("settings.json.disabled") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        let name_owned = name.to_string();
        let is_better = best
            .as_ref()
            .is_none_or(|(t, n, _)| (mtime, &name_owned) > (*t, n));
        if is_better {
            best = Some((mtime, name_owned, path));
        }
    }

    best.map(|(_, _, p)| p)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Dispatch `mustard-rt run rehook [--repo <p>] [--scope this|monorepo|all] [--confirm]`.
pub fn run(opts: RehookOpts) {
    let repo = opts
        .repo
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let scope = opts.scope.as_str();

    if !matches!(scope, "this" | "monorepo" | "all") {
        eprintln!("rehook: --scope must be one of: this, monorepo, all (got '{scope}')");
        std::process::exit(1);
    }

    let dirs = collect_claude_dirs(&repo, scope, opts.confirm);

    let entries: Vec<RestoredEntry> = dirs
        .iter()
        .map(|(dir, kind)| restore_one(dir, *kind))
        .collect();

    let report = RehookReport {
        scope: scope.to_string(),
        timestamp: now_iso8601(),
        entries,
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
    );

    // Mustard 2.0 ships as a Claude Code plugin; the native toggle is the
    // narrower on-switch. Clearing `disableAllHooks` un-silences every hook.
    eprintln!();
    eprintln!("Mustard now ships as a Claude Code plugin. To turn only Mustard back ON: claude plugin enable mustard");
    eprintln!("The restore above cleared `disableAllHooks` from settings.json (or, for a project unhooked by an older build, renamed its settings.json.disabled-* snapshot back).");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn restore_picks_newest_disabled_snapshot() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();

        std::fs::write(claude.join("settings.json.disabled-2026-05-20T00-00-00"), "OLD").unwrap();
        // Sleep is not allowed here; touch the second file last so its mtime is newer
        // on every reasonable filesystem.
        std::fs::write(claude.join("settings.json.disabled-2026-05-24T12-00-00"), "NEW").unwrap();

        let entry = restore_one(&claude, ScopeKind::Repo);
        assert_eq!(entry.state, "restored");
        let restored = std::fs::read_to_string(claude.join("settings.json")).unwrap();
        assert_eq!(restored, "NEW");
    }

    #[test]
    fn restore_with_live_settings_is_already_active() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("settings.json"), "{}").unwrap();
        std::fs::write(claude.join("settings.json.disabled-ts"), "OLD").unwrap();

        let entry = restore_one(&claude, ScopeKind::Repo);
        assert_eq!(entry.state, "already-active");
    }

    /// The inverse of the current kill-switch: the file was never moved, so the
    /// restore is the removal of one key. A stale snapshot from an older build
    /// must not win over the live file.
    #[test]
    fn restore_clears_the_flag_from_a_live_settings_file() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("settings.json"),
            r#"{"disableAllHooks":true,"cleanupPeriodDays":30}"#,
        )
        .unwrap();
        std::fs::write(claude.join("settings.json.disabled-ts"), "STALE").unwrap();

        let entry = restore_one(&claude, ScopeKind::Repo);

        assert_eq!(entry.state, "restored");
        assert!(entry.restored_from.is_none(), "nothing was renamed back");
        let raw = std::fs::read_to_string(claude.join("settings.json")).unwrap();
        assert!(!raw.contains("disableAllHooks"), "flag cleared: {raw}");
        assert!(raw.contains("cleanupPeriodDays"), "siblings survive: {raw}");
    }

    /// A settings.json we cannot parse is reported, never rewritten.
    #[test]
    fn restore_leaves_unparseable_settings_untouched() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("settings.json"), "{ not json").unwrap();

        let entry = restore_one(&claude, ScopeKind::Repo);

        assert_eq!(entry.state, "error");
        assert_eq!(
            std::fs::read_to_string(claude.join("settings.json")).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn restore_without_snapshot_is_no_snapshot() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let entry = restore_one(&claude, ScopeKind::Repo);
        assert_eq!(entry.state, "no-snapshot");
    }

    #[test]
    fn restore_missing_claude_dir_is_missing() {
        let dir = tempdir().unwrap();
        let entry = restore_one(&dir.path().join(".claude"), ScopeKind::Repo);
        assert_eq!(entry.state, "missing");
    }

    #[test]
    fn restore_accepts_bare_disabled_suffix() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        // Mirrors the `.disabled` file pattern seen in the repo before this
        // command existed (no timestamp suffix).
        std::fs::write(claude.join("settings.json.disabled"), "BARE").unwrap();

        let entry = restore_one(&claude, ScopeKind::Repo);
        assert_eq!(entry.state, "restored");
        assert_eq!(
            std::fs::read_to_string(claude.join("settings.json")).unwrap(),
            "BARE"
        );
    }
}
