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
//! - [`list_registered`] / [`register`] / [`hide`] / [`unhide`] /
//!   [`unregister`] — the registry itself, persisted by
//!   `mustard_core::dashboard_registry`.
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
//! Taking a folder off the list is [`hide`], not [`unregister`]: the dashboard
//! folds the scan back into the registry on every load, so a dropped row for a
//! folder inside the scanned root returns on the next open. [`unregister`]
//! survives for the opposite case — forgetting a path entirely, mark included.
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
use std::collections::HashMap;
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

/// One row of the sidebar: a registry entry plus what only the WHOLE list can
/// say about it.
///
/// `hidden` rides along so the frontend can render a folder the operator took
/// off the list (and offer to put it back) instead of having to guess from an
/// absence. `parent` is the disambiguating label — see [`to_rows`].
#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct ProjectRow {
    /// Absolute filesystem path. Doubles as the entry's identity.
    pub path: String,
    /// Display label — the trailing segment of `path`.
    pub name: String,
    /// ISO-8601 timestamp the entry was registered (UTC).
    pub added_at: String,
    /// `true` when the operator took this folder off the list.
    pub hidden: bool,
    /// The parent segment that tells this row apart from another one ending in
    /// the same name (`suzano` vs `suzano.old`). `None` when the name is
    /// unique across the list — an unambiguous folder needs no qualifier.
    pub parent: Option<String>,
}

/// The registered projects, oldest first, hidden ones included and marked —
/// with the discovery scan of `root` folded in first.
///
/// **The fold lives here, on the server, and it used to live in the browser.**
/// The frontend store re-registered everything the scan found on every page
/// load, which is what made a removal last exactly until the next open. Simply
/// deleting that loop would have cost the other half of the behaviour the
/// spec's Non-Goals keep on purpose: the automatic registrations are "what
/// makes a new project appear with no gesture at all". So the fold moved
/// rather than died. The frontend now only READS, and the one writer is the
/// server.
///
/// It respects the mark by construction: it folds through
/// [`registry::register_at`], which leaves an entry it already holds exactly
/// as it stands, `hidden` included. A folder the scan finds and the operator
/// hid stays hidden; a folder the scan finds for the first time appears.
///
/// A scan failure is not an error here — the registry is the answer, the scan
/// is an addition to it, and a root that cannot be walked must not empty the
/// sidebar.
pub fn list_registered(root: &Path) -> Result<Vec<ProjectRow>, String> {
    let registry_file = registry_file()?;
    fold_scan_into(&registry_file, root);
    Ok(list_in(&registry_file))
}

/// Write everything the scan of `root` finds into the registry at
/// `registry_file`, idempotently and without ever clearing a mark.
///
/// Fail-open at every step: neither an unwalkable root nor an unwritable
/// registry may keep the list from being returned.
fn fold_scan_into(registry_file: &Path, root: &Path) {
    let Ok(found) = crate::discovery::discover(root) else {
        return;
    };
    for project in found {
        // The result is deliberately dropped, and `register_at` is the only
        // door: it is the function that refuses to touch a row it already
        // holds, which is where the operator's removal survives the scan.
        let _ = registry::register_at(registry_file, Path::new(&project.path));
    }
}

/// Where the machine registry lives, or the error the commands report when the
/// home directory does not resolve.
fn registry_file() -> Result<std::path::PathBuf, String> {
    registry::registry_path().ok_or_else(|| "cannot resolve the home directory".to_string())
}

/// [`list_registered`] against an explicit registry file, without the scan
/// fold — the projection every command returns once it has written.
///
/// **The registry path is a PARAMETER, and that is the point.** With the home
/// directory resolved inside, a test that drove these commands would write into
/// the operator's own `~/.claude/dashboard-projects.json`, and `$HOME` cannot be
/// redirected in-process (`std::env::set_var` is `unsafe` and process-global,
/// and this crate forbids `unsafe`). The seam already exists one layer down in
/// `dashboard_registry::{read_at, hide_at, unhide_at}`; this mirrors it.
fn list_in(registry_file: &Path) -> Vec<ProjectRow> {
    to_rows(registry::read_at(registry_file))
}

/// Register `path`, returning the whole list so the caller re-renders from one
/// answer. Registering an already-registered path is a no-op, not an error —
/// the operator's intent ("track this folder") is already satisfied.
///
/// A path the operator has hidden STAYS hidden: this is the command the
/// dashboard funnels its discovery scan through, so clearing the mark here
/// would undo every removal on the next load. A deliberate "show this again"
/// is [`unhide`], and the sidebar's manual add sends both.
///
/// A path that is not a folder on this machine is REFUSED, with a message the
/// UI can show. Accepted, it would become a row nothing can ever justify: the
/// scan will not find it, so it just sits there, and the only thing the
/// operator can do to it is hide it — a permanent row born from a typo. A
/// refusal is the smaller failure.
pub fn register(path: String) -> Result<Vec<ProjectRow>, String> {
    register_in(&registry_file()?, &path)
}

/// [`register`] against an explicit registry file — see [`list_in`].
fn register_in(registry_file: &Path, path: &str) -> Result<Vec<ProjectRow>, String> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err(format!("not a folder on this machine: {path}"));
    }
    registry::register_at(registry_file, dir)?;
    Ok(list_in(registry_file))
}

/// Take `path` off the list, returning the list as it now stands.
///
/// The row is marked, not dropped — the scan would write a dropped row back on
/// the next load. Hiding a path the registry does not hold yet records the
/// exclusion anyway, so it survives the scan that has not run.
///
/// Unlike [`register`], this does NOT require the folder to still exist. The
/// rows an operator most wants gone are the ones whose folders are already
/// deleted (135 of the 142 measured in the field were `cargo test` tempdirs);
/// demanding a live directory here would leave exactly those unhidable.
pub fn hide(path: String) -> Result<Vec<ProjectRow>, String> {
    hide_in(&registry_file()?, &path)
}

/// [`hide`] against an explicit registry file — see [`list_in`].
fn hide_in(registry_file: &Path, path: &str) -> Result<Vec<ProjectRow>, String> {
    registry::hide_at(registry_file, Path::new(path))?;
    Ok(list_in(registry_file))
}

/// Put `path` back on the list, returning the list as it now stands. Unhiding
/// a path the registry does not hold is a no-op.
pub fn unhide(path: String) -> Result<Vec<ProjectRow>, String> {
    unhide_in(&registry_file()?, &path)
}

/// [`unhide`] against an explicit registry file — see [`list_in`].
fn unhide_in(registry_file: &Path, path: &str) -> Result<Vec<ProjectRow>, String> {
    registry::unhide_at(registry_file, Path::new(path))?;
    Ok(list_in(registry_file))
}

/// Forget `path` entirely — row and `hidden` mark alike — returning what
/// remains. Removing an absent path is a no-op. This does NOT touch the
/// project on disk (that is [`uninstall_mustard`]), and it is NOT how the
/// sidebar removes a folder: without the mark, a path the scan still finds
/// comes back on the next load. Use [`hide`] for that.
pub fn unregister(path: String) -> Result<Vec<ProjectRow>, String> {
    // Through `identity` for the same reason every writer goes through it: the
    // row is found by comparing this string byte for byte, so `/x/solo/` has to
    // name the row `/x/solo` holds rather than miss it.
    let path = registry::identity(Path::new(&path));
    let mut entries = registry::read();
    let before = entries.len();
    entries.retain(|e| e.path != path);
    if entries.len() != before {
        registry::write(&entries)?;
    }
    Ok(to_rows(entries))
}

/// Project the registry into sidebar rows, qualifying every name that more
/// than one row shares.
///
/// The qualifier cannot be decided one row at a time — whether `backend` needs
/// to say which project it belongs to depends on whether ANOTHER `backend` is
/// on the list — so it is computed here, where the whole list is in hand,
/// rather than in the frontend or in the registry writer.
///
/// Ambiguity is measured across the entire registry, hidden rows included: the
/// frontend is free to show the hidden ones next to the visible ones, and a
/// label that changes meaning depending on which section it lands in would be
/// worse than a qualifier shown once too often.
fn to_rows(entries: Vec<ProjectEntry>) -> Vec<ProjectRow> {
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    for entry in &entries {
        *occurrences.entry(entry.name.clone()).or_insert(0) += 1;
    }
    entries
        .into_iter()
        .map(|entry| {
            let ambiguous = occurrences.get(&entry.name).copied().unwrap_or(0) > 1;
            let parent = if ambiguous { parent_segment(&entry.path) } else { None };
            ProjectRow {
                path: entry.path,
                name: entry.name,
                added_at: entry.added_at,
                hidden: entry.hidden,
                parent,
            }
        })
        .collect()
}

/// The segment ABOVE the trailing one — `/home/u/suzano/backend` → `suzano`.
///
/// `None` for a path with nothing above it (`/backend`, `backend`), where
/// there is no parent to name. Both separators are handled for the same reason
/// `basename` handles both.
fn parent_segment(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let cut = trimmed.rfind(['/', '\\'])?;
    let parent = registry::basename(&trimmed[..cut]);
    if parent.is_empty() { None } else { Some(parent) }
}

#[cfg(test)]
mod tests {
    use super::{fold_scan_into, hide_in, list_in, register_in, to_rows, unhide_in, ProjectEntry};
    use mustard_core::dashboard_registry::basename;
    use std::path::Path;

    /// Make `name` under `dir` and hand back the path as the commands take it
    /// — a string, spelled the way the registry will store it. Registering
    /// REFUSES a path that is not a folder, so a test that means to register
    /// has to build one.
    fn folder(dir: &Path, name: &str) -> String {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).expect("create the folder");
        path.canonicalize()
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }

    fn entry(path: &str) -> ProjectEntry {
        ProjectEntry {
            name: basename(path),
            path: path.to_string(),
            added_at: "2026-01-01T00:00:00Z".to_string(),
            hidden: false,
        }
    }

    #[test]
    fn basename_takes_the_trailing_segment_on_both_separators() {
        assert_eq!(basename("/home/u/projects/mustard"), "mustard");
        assert_eq!(basename("/home/u/projects/mustard/"), "mustard");
        assert_eq!(basename(r"C:\repos\mustard"), "mustard");
        assert_eq!(basename("mustard"), "mustard");
    }

    #[test]
    fn a_unique_name_carries_no_qualifier() {
        let rows = to_rows(vec![entry("/home/u/mustard"), entry("/home/u/atiz")]);
        assert!(rows.iter().all(|r| r.parent.is_none()));
    }

    /// Two `backend`s are told apart by the project above them — the segment
    /// that actually differs.
    #[test]
    fn same_folder_name_gets_the_parent_that_distinguishes() {
        let rows = to_rows(vec![
            entry("/home/u/suzano/backend"),
            entry("/home/u/suzano.old/backend"),
            entry("/home/u/mustard"),
        ]);
        assert_eq!(rows[0].parent.as_deref(), Some("suzano"));
        assert_eq!(rows[1].parent.as_deref(), Some("suzano.old"));
        assert_eq!(rows[2].parent, None, "an unshared name stays bare");
    }

    /// Hide then list: the folder comes back marked instead of vanishing from
    /// the file. A row deleted here would be written again by the discovery
    /// scan the dashboard folds in on its next load, which is what made
    /// removing a folder inside the scanned root impossible.
    #[test]
    fn hide_marks_the_row_instead_of_dropping_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("dashboard-projects.json");
        let gone = folder(dir.path(), "gone");
        let kept = folder(dir.path(), "kept");
        register_in(&registry, &kept).expect("register kept");
        register_in(&registry, &gone).expect("register gone");

        let rows = hide_in(&registry, &gone).expect("hide");

        assert_eq!(rows.len(), 2, "the hidden folder is still listed");
        let hidden_row = rows.iter().find(|r| r.path == gone).expect("row for the hidden folder");
        assert!(hidden_row.hidden, "it comes back marked, not missing");
        assert!(!rows.iter().any(|r| r.path == kept && r.hidden));

        // And the mark is in the FILE, so the next scan cannot undo it.
        let on_disk = mustard_core::dashboard_registry::read_at(&registry);
        assert_eq!(on_disk.len(), 2, "hiding must not delete the row");
        assert!(on_disk.iter().any(|e| e.path == gone && e.hidden));

        // Listing again reads the same answer back off the file.
        let listed = list_in(&registry);
        assert!(listed.iter().any(|r| r.path == gone && r.hidden));
    }

    /// The mirror gesture: unhide puts it back on the list.
    #[test]
    fn unhide_puts_the_folder_back_on_the_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("dashboard-projects.json");
        let project = folder(dir.path(), "back");
        register_in(&registry, &project).expect("register");
        hide_in(&registry, &project).expect("hide");

        let rows = unhide_in(&registry, &project).expect("unhide");
        assert!(rows.iter().all(|r| !r.hidden));
    }

    /// A path that is not a folder on this machine is refused rather than
    /// written: accepted, it would be a row the scan can never justify and the
    /// operator can only hide.
    #[test]
    fn adding_a_path_that_is_not_a_folder_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("dashboard-projects.json");
        let ghost = dir.path().join("typo").to_string_lossy().to_string();

        let refused = register_in(&registry, &ghost).expect_err("a phantom path must be refused");
        assert!(refused.contains("not a folder"), "the message says why: {refused}");
        assert!(
            list_in(&registry).is_empty(),
            "and nothing was written for it"
        );
    }

    /// The fold that moved out of the browser: a project the scan finds gets a
    /// row with no gesture from the operator, and a second pass adds nothing.
    #[test]
    fn the_scan_fold_registers_what_it_finds_exactly_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("dashboard-projects.json");
        let root = dir.path().join("tree");
        let found = folder(&root, "discovered");
        std::fs::write(Path::new(&found).join("mustard.json"), "{}").expect("mustard.json");

        fold_scan_into(&registry, &root);
        fold_scan_into(&registry, &root);

        let rows = list_in(&registry);
        assert_eq!(rows.len(), 1, "the fold is idempotent");
        assert_eq!(rows[0].path, found);
        assert!(!rows[0].hidden);
    }

    /// The other half, and the whole point of the mark: the scan still finds
    /// the folder, and it stays off the list anyway.
    #[test]
    fn the_scan_fold_never_clears_the_operators_mark() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = dir.path().join("dashboard-projects.json");
        let root = dir.path().join("tree");
        let unwanted = folder(&root, "unwanted");
        std::fs::write(Path::new(&unwanted).join("mustard.json"), "{}").expect("mustard.json");

        fold_scan_into(&registry, &root);
        hide_in(&registry, &unwanted).expect("hide");
        fold_scan_into(&registry, &root);

        let rows = list_in(&registry);
        assert_eq!(rows.len(), 1, "no second row for the same folder");
        assert!(rows[0].hidden, "the scan must not put it back on the list");
    }

    #[test]
    fn a_path_with_nothing_above_it_has_no_parent_to_name() {
        let rows = to_rows(vec![entry("/mustard"), entry("mustard")]);
        assert!(rows.iter().all(|r| r.parent.is_none()));
    }
}
