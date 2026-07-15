//! Project-registry helpers (B6 Wave 1).
//!
//! The dashboard maintains a user-curated list of folders, each of which may
//! or may not be a Mustard project. This module exposes the single Tauri
//! command needed to inspect an arbitrary folder:
//!
//! - [`detect_project_mustard`] — does `<path>/.claude/CLAUDE.md` exist? if so,
//!   read `<path>/mustard.json` (the project-root config) and surface the
//!   `version` stamp written by `mustard-cli init`.
//!
//! Update is NOT defined here. The native `mustard_update` command (see
//! `lib.rs`) wraps `mustard_cli::update` without a sidecar process and is
//! reused verbatim from the TS side.
//!
//! `find_mustard_root()` is intentionally NOT used — the user-selected `path`
//! is the target, not the dashboard's own scaffold root.

use mustard_core::io::fs;
use mustard_core::ProjectConfig;
use serde::Serialize;
use std::path::Path;

/// Result of inspecting a folder for a Mustard installation.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectDetection {
    /// `true` when `<path>/.claude/CLAUDE.md` exists.
    pub installed: bool,
    /// The `version` field from `<path>/mustard.json`, when readable.
    /// `None` when the file is missing, malformed, or has no `version` key.
    pub version: Option<String>,
}

/// Inspect `path` and return whether Mustard is installed there + its version.
///
/// Detection rule mirrors `discovery::discover`: a folder counts as installed
/// when its `.claude/CLAUDE.md` exists. The version is best-effort — a missing
/// or malformed `mustard.json` yields `version: None` rather than an
/// error, so the UI can still show "installed, version unknown".
#[tauri::command]
pub async fn detect_project_mustard(path: String) -> Result<ProjectDetection, String> {
    let base = Path::new(&path);
    let claude_dir = base.join(".claude");
    let installed = claude_dir.join("CLAUDE.md").is_file();
    if !installed {
        return Ok(ProjectDetection { installed: false, version: None });
    }

    // Version lives in the project-root mustard.json (the workspace anchor),
    // not under `.claude/`. `installed` is still keyed on `.claude/CLAUDE.md`.
    let version = ProjectConfig::load(base).version;
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
