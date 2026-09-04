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
//!
//! ## Removal is a mark, not a deletion
//!
//! The dashboard folds its discovery scan back into the registry every time it
//! loads, so a deleted row for a folder inside the scanned root is written
//! again on the next open — which made "remove from the sidebar" useless for
//! exactly the folders an operator wanted gone. So a removal sets
//! [`ProjectEntry::hidden`] ([`hide_at`]) and [`register_at`] leaves that mark
//! alone, whether the path arrives from the scan, the session observer or
//! `mustard init`.

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
    /// `true` when the operator took the folder off the dashboard list.
    ///
    /// The row is MARKED, never deleted, because deleting it does not stick:
    /// the dashboard folds its discovery scan back into the registry on every
    /// load, so a row removed for a folder inside the scanned root is written
    /// again on the next open — and that is precisely the folder an operator
    /// wants gone. The mark is what [`register_at`] respects.
    ///
    /// `#[serde(default)]` so a registry written before this field existed
    /// still reads, with every folder visible.
    #[serde(default)]
    pub hidden: bool,
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
    // A path the registry already holds is left EXACTLY as it stands —
    // `hidden` included. This is where a removal holds: the dashboard folds
    // its discovery scan back in through this function on every load, and the
    // session observer and `mustard init` arrive here too. Clearing the mark
    // on the way past would make removing a folder inside the scanned root
    // impossible. An operator who wants it back asks for it by name, through
    // [`unhide_at`].
    if entries.iter().any(|e| e.path == path) {
        return Ok(RegisterOutcome::AlreadyPresent);
    }
    entries.push(ProjectEntry {
        name: basename(&path),
        path,
        added_at: crate::time::now_iso8601(),
        hidden: false,
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

/// What a visibility change actually did — so a caller can say "already off
/// the list" instead of claiming a change it did not make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityOutcome {
    /// The mark changed (or a hidden row was recorded for a path the registry
    /// did not yet hold) and the registry was written.
    Changed,
    /// The path already carried the mark; nothing was written.
    Unchanged,
}

/// Take `project_dir` off the dashboard list, in the registry at `registry`.
///
/// Marks the row rather than dropping it, and RECORDS a hidden row when the
/// registry does not hold the path yet: the exclusion has to outlive a scan
/// that has not re-added the folder, otherwise the removal lasts exactly until
/// the next dashboard load.
///
/// # Errors
/// Returns a message when the write fails.
pub fn hide_at(registry: &Path, project_dir: &Path) -> Result<VisibilityOutcome, String> {
    set_hidden_at(registry, project_dir, true)
}

/// Put `project_dir` back on the dashboard list, in the registry at `registry`.
///
/// Unhiding a path the registry does not hold is a no-op — there is nothing to
/// reveal, and inventing a row here would register a folder nobody asked to
/// track. [`register_at`] is how a path enters the registry.
///
/// # Errors
/// Returns a message when the write fails.
pub fn unhide_at(registry: &Path, project_dir: &Path) -> Result<VisibilityOutcome, String> {
    set_hidden_at(registry, project_dir, false)
}

/// The one writer behind [`hide_at`] and [`unhide_at`] — the two directions of
/// a single operation, so they share one implementation and cannot drift on
/// what "already like that" means.
fn set_hidden_at(
    registry: &Path,
    project_dir: &Path,
    hidden: bool,
) -> Result<VisibilityOutcome, String> {
    let path = project_dir.to_string_lossy().to_string();
    let mut entries = read_at(registry);
    if let Some(entry) = entries.iter_mut().find(|e| e.path == path) {
        if entry.hidden == hidden {
            return Ok(VisibilityOutcome::Unchanged);
        }
        entry.hidden = hidden;
    } else {
        if !hidden {
            return Ok(VisibilityOutcome::Unchanged);
        }
        entries.push(ProjectEntry {
            name: basename(&path),
            path,
            added_at: crate::time::now_iso8601(),
            hidden: true,
        });
    }
    write_at(registry, &entries)?;
    Ok(VisibilityOutcome::Changed)
}

/// Take `project_dir` off the dashboard list, in the machine registry.
///
/// # Errors
/// Returns a message when the home directory does not resolve, or the write
/// fails.
pub fn hide(project_dir: &Path) -> Result<VisibilityOutcome, String> {
    let registry = registry_path().ok_or_else(|| "cannot resolve the home directory".to_string())?;
    hide_at(&registry, project_dir)
}

/// Put `project_dir` back on the dashboard list, in the machine registry.
///
/// # Errors
/// Returns a message when the home directory does not resolve, or the write
/// fails.
pub fn unhide(project_dir: &Path) -> Result<VisibilityOutcome, String> {
    let registry = registry_path().ok_or_else(|| "cannot resolve the home directory".to_string())?;
    unhide_at(&registry, project_dir)
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

    /// A registry written before `hidden` existed must keep reading, with every
    /// folder visible — the alternative is an upgrade that empties the sidebar.
    #[test]
    fn a_registry_written_before_the_hidden_field_still_reads() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("registry.json");
        std::fs::write(
            &p,
            r#"[{"path":"/x/alpha","name":"alpha","added_at":"2026-01-01T00:00:00Z"}]"#,
        )
        .expect("write");

        let entries = read_at(&p);
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].hidden);
    }

    #[test]
    fn hiding_marks_the_row_instead_of_dropping_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = dir.path().join("my-app");

        register_at(&registry, &project).expect("register");
        assert_eq!(
            hide_at(&registry, &project).expect("hide"),
            VisibilityOutcome::Changed
        );

        let entries = read_at(&registry);
        assert_eq!(entries.len(), 1, "the row survives the removal");
        assert!(entries[0].hidden);
    }

    /// The whole point of the mark: the dashboard re-registers everything its
    /// scan finds on every load, and a folder inside the scanned root must not
    /// come back because of it.
    #[test]
    fn registering_a_hidden_path_leaves_it_hidden() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = dir.path().join("my-app");

        register_at(&registry, &project).expect("register");
        hide_at(&registry, &project).expect("hide");

        assert_eq!(
            register_at(&registry, &project).expect("re-register"),
            RegisterOutcome::AlreadyPresent
        );
        let entries = read_at(&registry);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].hidden, "the scan must not clear the mark");
    }

    /// Hiding a path the registry does not hold yet still has to stick: the
    /// scan that would add it may not have run.
    #[test]
    fn hiding_a_path_the_registry_does_not_hold_records_the_exclusion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = dir.path().join("never-registered");

        assert_eq!(
            hide_at(&registry, &project).expect("hide"),
            VisibilityOutcome::Changed
        );
        let entries = read_at(&registry);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "never-registered");
        assert!(entries[0].hidden);

        register_at(&registry, &project).expect("register");
        assert!(read_at(&registry)[0].hidden);
    }

    #[test]
    fn unhiding_puts_the_folder_back_on_the_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = dir.path().join("my-app");

        register_at(&registry, &project).expect("register");
        hide_at(&registry, &project).expect("hide");
        assert_eq!(
            unhide_at(&registry, &project).expect("unhide"),
            VisibilityOutcome::Changed
        );
        assert!(!read_at(&registry)[0].hidden);
    }

    #[test]
    fn marking_what_already_carries_the_mark_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("registry.json");
        let project = dir.path().join("my-app");

        register_at(&registry, &project).expect("register");
        hide_at(&registry, &project).expect("hide");
        assert_eq!(
            hide_at(&registry, &project).expect("hide twice"),
            VisibilityOutcome::Unchanged
        );
        assert_eq!(
            unhide_at(&registry, &dir.path().join("absent")).expect("unhide absent"),
            VisibilityOutcome::Unchanged
        );
        assert_eq!(read_at(&registry).len(), 1);
    }

    #[test]
    fn basename_handles_both_separators_and_trailing_slashes() {
        assert_eq!(basename("/home/u/proj"), "proj");
        assert_eq!(basename("/home/u/proj/"), "proj");
        assert_eq!(basename(r"C:\Users\u\proj"), "proj");
        assert_eq!(basename("proj"), "proj");
    }
}
