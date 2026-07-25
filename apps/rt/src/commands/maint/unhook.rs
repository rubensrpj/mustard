//! `mustard-rt run unhook` — kill-switch for the harness.
//!
//! Disables the Claude Code hook layer by writing `"disableAllHooks": true`
//! into `.claude/settings.json` and wiping volatile harness state
//! (`.agent-state/`, `.cluster-cache.json`, `.worktrees/`). Restore with
//! `mustard-rt run rehook`.
//!
//! ## Why a surgical write and not a rename
//!
//! Mustard's hooks ship in `plugin/hooks/hooks.json`; `settings.json` carries
//! no `hooks` block at all. Renaming the file therefore left every hook firing
//! while removing what the file *does* carry — the `permissions.deny` safety
//! net (recursive deletes, force pushes, hard resets, key/credential reads),
//! `permissions.allow`, `statusLine` and the telemetry `env` block. Asking to
//! stop being observed must not silently drop the safety net. `disableAllHooks`
//! is the single lever the platform exposes that also reaches plugin hooks, so
//! setting it is both the switch that actually works and the one that keeps the
//! rest of the file intact.
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
//! in `state: "error"` so the rest of the sweep still completes. A
//! `settings.json` that cannot be read or parsed is reported as `error` and
//! left byte-for-byte untouched — a blind overwrite would destroy the very
//! safety net this command now exists to preserve.

use mustard_core::io::fs as mfs;
use mustard_core::time::now_iso8601;
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::Value;
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
pub(crate) struct DisabledEntry {
    /// The settings.json path we targeted.
    pub settings_path: String,
    /// Where the settings file was moved to. Always `None` since the switch
    /// stopped renaming: the file stays where it is. Kept so the report shape
    /// does not change under consumers that already read it.
    pub moved_to: Option<String>,
    /// `true` once `"disableAllHooks": true` is persisted in `settings_path`.
    pub hooks_disabled: bool,
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
pub(crate) struct UnhookReport {
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
// settings.json surgery (shared with `rehook`)
// ---------------------------------------------------------------------------

/// The one settings key that silences plugin hooks as well as the ones declared
/// in `settings.json` itself. Mustard registers its hooks through
/// `plugin/hooks/hooks.json`, and the platform exposes no way to disable a
/// single plugin's hooks — this flag is the whole kill-switch.
pub(crate) const DISABLE_ALL_HOOKS_KEY: &str = "disableAllHooks";

/// Read `settings.json` as its top-level JSON object.
///
/// `Err` carries a human-readable reason destined for the report's `error`
/// field. Callers must not write anything back on `Err`: the file holds the
/// developer's `permissions.deny` rules, so a file we could not understand is
/// left exactly as found.
pub(crate) fn read_settings_object(
    settings: &Path,
) -> Result<serde_json::Map<String, Value>, String> {
    let raw = mfs::read_to_string(settings).map_err(|e| format!("read failed: {e}"))?;
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("settings.json is not a JSON object".to_string()),
        Err(e) => Err(format!("parse failed: {e}")),
    }
}

/// Write `obj` back over `settings.json` as pretty JSON with a trailing
/// newline, through the atomic seam (temp file + rename) so an interrupted
/// write cannot leave the developer with a truncated settings file.
pub(crate) fn write_settings_object(
    settings: &Path,
    obj: serde_json::Map<String, Value>,
) -> Result<(), String> {
    let mut serialized = serde_json::to_string_pretty(&Value::Object(obj))
        .map_err(|e| format!("serialize failed: {e}"))?;
    serialized.push('\n');
    mfs::write_atomic(settings, serialized.as_bytes()).map_err(|e| format!("write failed: {e}"))
}

/// Set `disableAllHooks: true` in `settings`, preserving every other key —
/// `permissions.allow`, `permissions.deny`, `statusLine` and the `env` block
/// all survive verbatim.
fn set_disable_all_hooks(settings: &Path) -> Result<(), String> {
    let mut obj = read_settings_object(settings)?;
    obj.insert(DISABLE_ALL_HOOKS_KEY.to_string(), Value::Bool(true));
    write_settings_object(settings, obj)
}

// ---------------------------------------------------------------------------
// Per-directory disable
// ---------------------------------------------------------------------------

/// Disable one `.claude/` directory's hooks + wipe its volatile state.
fn disable_one(claude_dir: &Path, kind: ScopeKind) -> DisabledEntry {
    let settings = claude_dir.join("settings.json");
    let settings_path = settings.display().to_string();

    if kind == ScopeKind::GlobalSkipped {
        return DisabledEntry {
            settings_path,
            moved_to: None,
            hooks_disabled: false,
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
            hooks_disabled: false,
            state: "missing".into(),
            cleared,
            error: None,
        };
    }

    if let Err(e) = set_disable_all_hooks(&settings) {
        return DisabledEntry {
            settings_path,
            moved_to: None,
            hooks_disabled: false,
            state: "error".into(),
            cleared: Vec::new(),
            error: Some(e),
        };
    }

    let cleared = wipe_volatile_state(claude_dir);
    DisabledEntry {
        settings_path,
        moved_to: None,
        hooks_disabled: true,
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

    let dirs = collect_claude_dirs(&repo, scope, opts.confirm);

    let entries: Vec<DisabledEntry> = dirs
        .iter()
        .map(|(dir, kind)| disable_one(dir, *kind))
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

    // Mustard 2.0 ships as a Claude Code plugin; the native toggle disables
    // that one plugin, which is the narrower and usually preferable action.
    // `disableAllHooks` is broader — it silences EVERY hook the harness would
    // fire, Mustard's and anyone else's. Guidance on stderr keeps stdout pure
    // JSON.
    eprintln!();
    eprintln!("Mustard now ships as a Claude Code plugin. To turn only Mustard OFF: claude plugin disable mustard");
    eprintln!("The `disableAllHooks: true` written above silences EVERY hook, not just Mustard's; it also wiped volatile state (.agent-state/, .cluster-cache.json, .worktrees/). Your permissions and statusLine were left intact.");
    eprintln!("Re-enable with: claude plugin enable mustard   (or clear the flag: mustard-rt run rehook).");
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

    /// `unhook` no longer mints these names, but `rehook` still restores a
    /// `settings.json.disabled-<ts>` snapshot left by an older build, so the
    /// shape of that stamp remains load-bearing for the legacy path.
    #[test]
    fn filename_safe_timestamp_has_no_colons() {
        let ts = mustard_core::time::filename_safe_now();
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
    fn disable_sets_the_flag_in_place_and_clears_state() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        write(&claude.join("settings.json"), r#"{"hooks":{}}"#);
        write(&claude.join(".agent-state/counter.json"), "1");
        write(&claude.join(".cluster-cache.json"), "[]");

        let entry = disable_one(&claude, ScopeKind::Repo);

        assert_eq!(entry.state, "disabled");
        assert!(entry.hooks_disabled);
        assert!(entry.moved_to.is_none(), "the file is no longer moved");
        // The settings file survives, carrying the flag.
        let settings = claude.join("settings.json");
        assert!(settings.exists(), "settings.json must stay in place");
        let obj = read_settings_object(&settings).unwrap();
        assert_eq!(obj.get(DISABLE_ALL_HOOKS_KEY), Some(&Value::Bool(true)));
        // Volatile state is still wiped.
        assert!(!claude.join(".agent-state").exists());
        assert!(!claude.join(".cluster-cache.json").exists());
        assert_eq!(entry.cleared.len(), 2);
    }

    #[test]
    fn disable_missing_settings_reports_missing() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        let entry = disable_one(&claude, ScopeKind::Repo);
        assert_eq!(entry.state, "missing");
        assert!(entry.moved_to.is_none());
        assert!(!entry.hooks_disabled);
    }

    /// A settings.json we cannot parse is reported, never rewritten — it is the
    /// file that carries `permissions.deny`.
    #[test]
    fn disable_leaves_unparseable_settings_untouched() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        let settings = claude.join("settings.json");
        write(&settings, "{ not json at all");

        let entry = disable_one(&claude, ScopeKind::Repo);

        assert_eq!(entry.state, "error");
        assert!(!entry.hooks_disabled);
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            "{ not json at all",
            "the original bytes must survive"
        );
    }

    #[test]
    fn global_without_confirm_is_skipped() {
        let dir = tempdir().unwrap();
        let entry = disable_one(&dir.path().join(".claude"), ScopeKind::GlobalSkipped);
        assert_eq!(entry.state, "skipped");
        assert!(!entry.hooks_disabled);
        assert!(entry.error.unwrap().contains("--confirm"));
    }
}
