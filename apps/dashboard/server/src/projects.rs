//! Project-registry helpers (B6 Wave 1).
//!
//! The dashboard maintains a user-curated list of folders, each of which may
//! or may not be a Mustard project. This module owns both halves of that:
//!
//! - [`detect_project_mustard`] — does `<path>/mustard.json` exist? if so,
//!   read it (the project-root config, the workspace anchor) and surface the
//!   `version` stamp written by `mustard-cli init`. (`.claude/CLAUDE.md` is no
//!   longer the install signal: the orchestrator redesign stopped planting it,
//!   so `mustard.json` — which every install writes — is the marker, matching
//!   `discovery::discover`.)
//! - [`list_registered`] / [`register`] / [`unregister`] — the registry
//!   itself, persisted at [`registry_path`].
//!
//! The registry used to live in the desktop app's `projects.json`, written by a
//! browser-side key/value store plugin. It is server state now,
//! under `~/.claude/`: the dashboard covers EVERY Mustard project on the
//! machine, so the list is a fact about the machine. Kept in browser storage,
//! opening the dashboard from a second browser — or from a phone over
//! Tailscale — would show an empty list, and neither view would be the truth.
//!
//! Registration is distinct from DISCOVERY (`discovery::discover`, which walks
//! the disk looking for `mustard.json`): the registry records a choice, the
//! scan finds candidates.
//!
//! In-place refresh is no longer a dashboard command. Template and plugin
//! content now ships through the `mustard` plugin marketplace, and re-seeding
//! the local harness (settings, version stamp, plugin-enable) is `mustard init`
//! - idempotent, a CLI/plugin concern the dashboard does not drive.
//!
//! `find_mustard_root()` is intentionally NOT used — the user-selected `path`
//! is the target, not the dashboard's own scaffold root.

use mustard_core::ProjectConfig;
use mustard_core::io::fs;
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
pub fn detect_project_mustard(path: String) -> Result<ProjectDetection, String> {
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
pub fn uninstall_mustard(path: String) -> Result<(), String> {
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

/// One entry in the machine-level project registry.
///
/// Re-exported from `mustard_core`, not declared here. The format used to have
/// its only implementation in this crate, which the CLI cannot reach — so
/// `mustard init` had no way to record the project it had just created, and the
/// dashboard opened on an empty list (field, 2026-08-28). Moving the type and
/// its IO to the core gave the format ONE owner and two callers, instead of two
/// owners drifting apart.
pub use mustard_core::dashboard_registry::ProjectEntry;

use mustard_core::dashboard_registry as registry;

/// The registered projects, oldest first.
pub fn list_registered() -> Result<Vec<ProjectEntry>, String> {
    Ok(registry::read())
}

/// Register `path`, returning the whole list so the caller re-renders from one
/// answer. Registering an already-registered path is a no-op, not an error —
/// the operator's intent ("track this folder") is already satisfied.
pub fn register(path: String) -> Result<Vec<ProjectEntry>, String> {
    registry::register(std::path::Path::new(&path))?;
    Ok(registry::read())
}

/// Drop `path` from the registry, returning the remaining list. Removing an
/// absent path is a no-op. This does NOT touch the project on disk — that is
/// [`uninstall_mustard`].
pub fn unregister(path: String) -> Result<Vec<ProjectEntry>, String> {
    let mut entries = registry::read();
    let before = entries.len();
    entries.retain(|e| e.path != path);
    if entries.len() != before {
        registry::write(&entries)?;
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use mustard_core::dashboard_registry::basename;

    #[test]
    fn basename_takes_the_trailing_segment_on_both_separators() {
        assert_eq!(basename("/home/u/projects/mustard"), "mustard");
        assert_eq!(basename("/home/u/projects/mustard/"), "mustard");
        assert_eq!(basename(r"C:\repos\mustard"), "mustard");
        assert_eq!(basename("mustard"), "mustard");
    }
}
