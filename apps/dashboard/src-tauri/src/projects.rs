//! Project-registry helpers (B6 Wave 1).
//!
//! The dashboard maintains a user-curated list of folders, each of which may
//! or may not be a Mustard project. This module exposes the single Tauri
//! command needed to inspect an arbitrary folder:
//!
//! - [`detect_project_mustard`] — does `<path>/mustard.json` exist? if so,
//!   read it (the project-root config, the workspace anchor) and surface the
//!   `version` stamp written by `mustard-cli init`. (`.claude/CLAUDE.md` is no
//!   longer the install signal: the orchestrator redesign stopped planting it,
//!   so `mustard.json` — which every install writes — is the marker, matching
//!   `discovery::discover`.)
//!
//! In-place refresh is no longer a dashboard command. Template and plugin
//! content now ships through the `mustard` plugin marketplace, and re-seeding
//! the local harness (settings, version stamp, plugin-enable) is `mustard init`
//! - idempotent, a CLI/plugin concern the dashboard does not drive.
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
    /// `true` when `<path>/mustard.json` exists.
    pub installed: bool,
    /// The `version` field from `<path>/mustard.json`, when readable.
    /// `None` when the file is missing, malformed, or has no `version` key.
    pub version: Option<String>,
}

/// Inspect `path` and return whether Mustard is installed there + its version.
///
/// Detection rule mirrors `discovery::discover`: a folder counts as installed
/// when its project-root `mustard.json` exists (the workspace anchor every
/// install writes; the orchestrator redesign stopped planting
/// `.claude/CLAUDE.md`, so that file signals nothing). The version is
/// best-effort — a malformed `mustard.json` yields `version: None` rather
/// than an error, so the UI can still show "installed, version unknown".
#[tauri::command]
pub async fn detect_project_mustard(path: String) -> Result<ProjectDetection, String> {
    let base = Path::new(&path);
    let installed = base.join("mustard.json").is_file();
    if !installed {
        return Ok(ProjectDetection { installed: false, version: None });
    }

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
