//! The machine-level dashboard project registry — `~/.claude/dashboard-projects.json`.
//!
//! ## Why this lives in core
//!
//! The dashboard opens on a list of projects, and that list is a fact about the
//! MACHINE, not about any one project or browser. Two very different callers
//! need to agree on it: the dashboard server, which reads it to render the list
//! and writes it when someone adds a folder by hand, and `mustard init`, which
//! knows the one moment a new Mustard project comes into existence.
//!
//! Before this module the format had a single writer, inside the dashboard
//! server crate, and the CLI could not reach it. So installing Mustard into a
//! project told the dashboard nothing: the operator installed it, opened the
//! dashboard, and saw an empty list with no hint that anything was missing
//! (reported in the field, 2026-08-28). The alternative — teaching the CLI to
//! write the same JSON shape independently — would have put two writers on one
//! format, which is how a format drifts.
//!
//! ## Contract
//!
//! Fail-open on every read: a missing, unreadable or malformed registry is an
//! EMPTY list, never an error. An operator who has registered nothing and one
//! whose file we cannot parse both want the dashboard to open. Writes are
//! atomic and idempotent — registering a path that is already there refreshes
//! nothing and duplicates nothing.
//!
//! ## Not a discovery scan
//!
//! Registration records a CHOICE; discovery walks the disk looking for
//! `mustard.json` and finds candidates. This module is only the former.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::io::fs;
use crate::ClaudePaths;

/// One entry in the machine-level project registry.
///
/// Keys are snake_case to match every other value the dashboard returns; the
/// desktop build's `projects.json` used `addedAt` because a browser-side store
/// wrote it, and that spelling died with the store that produced it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProjectEntry {
    /// Absolute filesystem path. Doubles as the entry's identity.
    pub path: String,
    /// Display label — defaults to the trailing segment of `path`.
    pub name: String,
    /// ISO-8601 timestamp the entry was added (UTC).
    pub added_at: String,
}

/// Where the registry is persisted: `~/.claude/dashboard-projects.json`.
///
/// Composed through [`ClaudePaths`] so the `.claude` segment cannot drift from
/// the rest of the harness. `None` when the home directory does not resolve —
/// callers degrade to an empty registry rather than inventing a location.
#[must_use]
pub fn registry_path() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())?;
    let paths = ClaudePaths::for_project(&home).ok()?;
    Some(paths.claude_dir().join("dashboard-projects.json"))
}

/// Extract the trailing path segment as a display name. Handles both forward
/// and back slashes (Windows + POSIX) and trims trailing separators.
#[must_use]
pub fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    match trimmed.rfind(['/', '\\']) {
        Some(idx) => trimmed[idx + 1..].to_string(),
        None => trimmed.to_string(),
    }
}

/// Read the registry from an explicit path. Fail-open: a missing, unreadable or
/// malformed file is an empty list.
#[must_use]
pub fn read_at(path: &Path) -> Vec<ProjectEntry> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Read the machine registry. Fail-open, as [`read_at`].
#[must_use]
pub fn read() -> Vec<ProjectEntry> {
    registry_path().map_or_else(Vec::new, |p| read_at(&p))
}

/// Persist the registry to an explicit path, creating its parent when needed.
///
/// # Errors
/// Returns the failure as a message when the parent cannot be created or the
/// atomic write fails.
pub fn write_at(path: &Path, entries: &[ProjectEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write_atomic(path, body.as_bytes())
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Persist the machine registry.
///
/// # Errors
/// Returns a message when the home directory does not resolve, or the write
/// fails.
pub fn write(entries: &[ProjectEntry]) -> Result<(), String> {
    let path = registry_path().ok_or_else(|| "cannot resolve the home directory".to_string())?;
    write_at(&path, entries)
}

/// What [`register_at`] did — so a caller can report honestly instead of
/// claiming an addition that was already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// The path was not in the registry and now is.
    Added,
    /// The path was already registered; nothing was written.
    AlreadyPresent,
}

/// Register `project_dir` in the registry at `registry`, idempotently.
///
/// Identity is the absolute path, so a second `init` in the same project is a
/// no-op rather than a duplicate row. Nothing is written when the entry is
/// already present — an install that changes nothing should not rewrite a file
/// other processes may be reading.
///
/// # Errors
/// Returns a message when the write fails.
pub fn register_at(registry: &Path, project_dir: &Path) -> Result<RegisterOutcome, String> {
    let path = project_dir.to_string_lossy().to_string();
    let mut entries = read_at(registry);
    if entries.iter().any(|e| e.path == path) {
        return Ok(RegisterOutcome::AlreadyPresent);
    }
    entries.push(ProjectEntry {
        name: basename(&path),
        path,
        added_at: crate::time::now_iso8601(),
    });
    write_at(registry, &entries)?;
    Ok(RegisterOutcome::Added)
}

/// Register `project_dir` in the machine registry, idempotently.
///
/// # Errors
/// Returns a message when the home directory does not resolve, or the write
/// fails. Callers that must not fail an install on this should ignore the
/// error and say so — a dashboard listing is a convenience, never a gate.
pub fn register(project_dir: &Path) -> Result<RegisterOutcome, String> {
    let registry = registry_path().ok_or_else(|| "cannot resolve the home directory".to_string())?;
    register_at(&registry, project_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_registry_reads_as_empty_never_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(read_at(&dir.path().join("nope.json")).is_empty());
    }

    #[test]
    fn a_malformed_registry_reads_as_empty_never_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("registry.json");
        std::fs::write(&p, "{ not json at all").expect("write");
        assert!(read_at(&p).is_empty());
    }

    #[test]
    fn registering_adds_the_project_with_its_folder_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = dir.path().join("my-app");
        std::fs::create_dir_all(&project).expect("project");

        assert_eq!(
            register_at(&registry, &project).expect("register"),
            RegisterOutcome::Added
        );
        let entries = read_at(&registry);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my-app");
        assert!(!entries[0].added_at.is_empty());
    }

    /// Installing twice must not produce two rows — `init` is re-runnable by
    /// design, and a registry that grows on every run is a registry nobody
    /// trusts.
    #[test]
    fn registering_the_same_project_twice_is_a_no_op() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = dir.path().join("my-app");
        std::fs::create_dir_all(&project).expect("project");

        register_at(&registry, &project).expect("first");
        assert_eq!(
            register_at(&registry, &project).expect("second"),
            RegisterOutcome::AlreadyPresent
        );
        assert_eq!(read_at(&registry).len(), 1);
    }

    #[test]
    fn registering_preserves_projects_already_there() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let first = dir.path().join("alpha");
        let second = dir.path().join("beta");
        std::fs::create_dir_all(&first).expect("a");
        std::fs::create_dir_all(&second).expect("b");

        register_at(&registry, &first).expect("first");
        register_at(&registry, &second).expect("second");

        let names: Vec<String> = read_at(&registry).into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn basename_handles_both_separators_and_trailing_slashes() {
        assert_eq!(basename("/home/u/proj"), "proj");
        assert_eq!(basename("/home/u/proj/"), "proj");
        assert_eq!(basename(r"C:\Users\u\proj"), "proj");
        assert_eq!(basename("proj"), "proj");
    }
}
