//! Project-registry helpers (B6 Wave 1).
//!
//! The dashboard maintains a user-curated list of folders, each of which may
//! or may not be a Mustard project. This module exposes the single Tauri
//! command needed to inspect an arbitrary folder:
//!
//! - [`detect_project_mustard`] — does `<path>/.claude/CLAUDE.md` exist? if so,
//!   read `<path>/.claude/mustard.json` and surface the `version` stamp written
//!   by `mustard-cli init` (see `apps/cli/src/commands/init.rs:333`).
//!
//! Install / update are NOT defined here. The native commands
//! `mustard_install` / `mustard_update` (see `lib.rs`) already wrap
//! `mustard_cli::init` / `update` without a sidecar process and are reused
//! verbatim from the TS side.
//!
//! `find_mustard_root()` is intentionally NOT used — the user-selected `path`
//! is the target, not the dashboard's own scaffold root.

use mustard_core::io::fs;
use serde::Serialize;
use std::path::Path;

/// Result of inspecting a folder for a Mustard installation.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectDetection {
    /// `true` when `<path>/.claude/CLAUDE.md` exists.
    pub installed: bool,
    /// The `version` field from `<path>/.claude/mustard.json`, when readable.
    /// `None` when the file is missing, malformed, or has no `version` key.
    pub version: Option<String>,
}

/// Inspect `path` and return whether Mustard is installed there + its version.
///
/// Detection rule mirrors `discovery::discover`: a folder counts as installed
/// when its `.claude/CLAUDE.md` exists. The version is best-effort — a missing
/// or malformed `.claude/mustard.json` yields `version: None` rather than an
/// error, so the UI can still show "installed, version unknown".
#[tauri::command]
pub async fn detect_project_mustard(path: String) -> Result<ProjectDetection, String> {
    let base = Path::new(&path);
    let claude_dir = base.join(".claude");
    let installed = claude_dir.join("CLAUDE.md").is_file();
    if !installed {
        return Ok(ProjectDetection { installed: false, version: None });
    }

    let version = read_mustard_json_version(&claude_dir);
    Ok(ProjectDetection { installed: true, version })
}

/// Best-effort uninstall of Mustard at `path` (B6 Wave 1).
///
/// Removes `<path>/.claude/` and `<path>/mustard.json`. NotFound is treated as
/// success — uninstalling something that isn't there is a no-op, not an error.
/// Other I/O failures (permissions, etc.) are surfaced as a string error so the
/// UI can show a meaningful message.
///
/// `find_mustard_root()` is intentionally NOT used — the user-selected `path`
/// is the target, not the dashboard's own scaffold root.
#[tauri::command]
pub async fn uninstall_mustard(path: String) -> Result<(), String> {
    let base = Path::new(&path);

    // fs::remove_dir_all is fail-open (success when path is absent).
    fs::remove_dir_all(base.join(".claude"))
        .map_err(|e| format!("Failed to remove .claude/: {e}"))?;

    // fs::remove_file returns Error::NotFound when absent — treat that as success.
    match fs::remove_file(base.join("mustard.json")) {
        Ok(()) | Err(mustard_core::platform::error::Error::NotFound(_)) => {}
        Err(e) => return Err(format!("Failed to remove mustard.json: {e}")),
    }

    Ok(())
}

/// Best-effort read of `.claude/mustard.json`'s `version` field. Any I/O or
/// parse failure collapses to `None` — callers treat that as "unknown".
fn read_mustard_json_version(claude_dir: &Path) -> Option<String> {
    let path = claude_dir.join("mustard.json");
    let content = fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("version")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}
