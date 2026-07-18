//! `injectables` — the declared-injection engine behind the session hooks.
//!
//! ## What it does
//!
//! `mustard.json#inject` declares `[{on, file, once}]`: instruction files
//! (canonically `.claude/mustard/*.md`, seeded by `mustard init`, freely
//! editable by the user) that ride a hook trigger as `additionalContext`.
//! This module is the shared engine both consumer hooks call:
//!
//! - [`super::prompt_submit_inject`] collects the `on: userPromptSubmit`
//!   entries (skipped entirely for `/mustard:*` prompts — a slash command is
//!   already inside the flow);
//! - [`super::session_start_inject`] collects the `on: sessionStart` entries
//!   (and, on a post-compaction `SessionStart`, re-arms everything via
//!   [`clear_markers`]).
//!
//! ## Once-per-session markers
//!
//! An entry with `once: true` is delivered a single time per session. The
//! delivery record is a marker file
//! `.claude/.session/<session_id>/injected-<basename>` — the same per-session
//! layout as the `active-spec` marker (`crate::shared::context`). A session
//! without a usable id (empty / `"unknown"`) cannot record markers, so `once`
//! degrades to "every time" — delivering twice is the safe failure, silently
//! never delivering is not.
//!
//! ## Fail-open
//!
//! Missing/unreadable config → no injectables. Missing/empty declared file →
//! that entry is skipped silently. Unwritable marker → the injection still
//! happens. Nothing here ever blocks a hook.

use mustard_core::io::fs;
use mustard_core::{ClaudePaths, ProjectConfig};
use std::path::{Path, PathBuf};

/// Filename prefix of the per-session delivery markers.
const MARKER_PREFIX: &str = "injected-";

/// Collect the injectable payload for `trigger_on` (an already-lowercase
/// trigger name, e.g. `"userpromptsubmit"` / `"sessionstart"`).
///
/// For each declared entry matching the trigger: honour its `once` marker
/// (unless `ignore_markers` — the post-compaction re-delivery), read the file
/// project-root-relative, and record a delivery marker for what was read.
/// Returns the blocks joined by a blank line, or `None` when nothing applies.
pub fn collect(
    project_dir: &str,
    session_id: Option<&str>,
    trigger_on: &str,
    ignore_markers: bool,
) -> Option<String> {
    let root = Path::new(project_dir);
    let config = ProjectConfig::load(root);
    let mut blocks: Vec<String> = Vec::new();
    let mut delivered: Vec<String> = Vec::new();

    for entry in config.injectables() {
        if entry.on != trigger_on {
            continue;
        }
        let marker_name = marker_basename(&entry.file);
        if entry.once
            && !ignore_markers
            && marker_path(project_dir, session_id, &marker_name)
                .is_some_and(|marker| marker.is_file())
        {
            continue; // already delivered this session.
        }
        // Root-relative read; an absent or unreadable file skips silently.
        let Ok(text) = fs::read_to_string(root.join(&entry.file)) else {
            continue;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        blocks.push(trimmed.to_string());
        delivered.push(marker_name);
    }

    if blocks.is_empty() {
        return None;
    }
    // Record the delivery so `once` holds for the rest of the session. The
    // marker body carries the source path — a debugging breadcrumb, not data.
    for name in &delivered {
        write_marker(project_dir, session_id, name);
    }
    Some(blocks.join("\n\n"))
}

/// Delete every `injected-*` marker of the session. Called on a
/// post-compaction `SessionStart` so the `once` entries re-deliver into the
/// freshly compacted window (`userPromptSubmit` ones on the next prompt,
/// `sessionStart` ones immediately via `ignore_markers`).
pub fn clear_markers(project_dir: &str, session_id: Option<&str>) {
    let Some(dir) = session_dir(project_dir, session_id) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries {
        if entry.is_dir || !entry.file_name.starts_with(MARKER_PREFIX) {
            continue;
        }
        let _ = fs::remove_file(&entry.path);
    }
}

/// `injected-<basename>` for a declared file path (either separator accepted).
fn marker_basename(file: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    format!("{MARKER_PREFIX}{base}")
}

/// `.claude/.session/<session_id>/` for a usable session id — the same base
/// the `active-spec` marker lives in (see `crate::shared::context`). `None`
/// for an empty/`"unknown"` id or an I1-rejected project root.
fn session_dir(project_dir: &str, session_id: Option<&str>) -> Option<PathBuf> {
    let sid = session_id?.trim();
    if sid.is_empty() || sid == "unknown" {
        return None;
    }
    Some(
        ClaudePaths::for_project(Path::new(project_dir))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(sid),
    )
}

/// Full path of one delivery marker, when the session can hold markers.
fn marker_path(project_dir: &str, session_id: Option<&str>, marker_name: &str) -> Option<PathBuf> {
    Some(session_dir(project_dir, session_id)?.join(marker_name))
}

/// Persist one delivery marker, best-effort (an unwritable marker never blocks
/// the injection — it only means a `once` entry may deliver again).
fn write_marker(project_dir: &str, session_id: Option<&str>, marker_name: &str) {
    let Some(marker) = marker_path(project_dir, session_id, marker_name) else {
        return;
    };
    let Some(parent) = marker.parent() else {
        return;
    };
    let _ = fs::create_dir_all(parent);
    let _ = fs::write_atomic(&marker, marker_name.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Write a project with a `mustard.json#inject` declaration + the file.
    fn seed_project(dir: &Path, on: &str, file: &str, once: bool, body: &str) {
        let json = format!(
            r#"{{"inject":[{{"on":"{on}","file":"{file}","once":{once}}}]}}"#
        );
        std::fs::write(dir.join("mustard.json"), json).unwrap();
        let target = dir.join(file);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, body).unwrap();
    }

    #[test]
    fn collect_reads_declared_file_and_writes_marker() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_project(dir.path(), "userPromptSubmit", ".claude/mustard/orchestrator.md", true, "RULES\n");

        let got = collect(project, Some("s1"), "userpromptsubmit", false);
        assert_eq!(got.as_deref(), Some("RULES"));
        assert!(
            dir.path()
                .join(".claude/.session/s1/injected-orchestrator.md")
                .is_file(),
            "delivery marker recorded"
        );

        // Second collect in the same session: once → nothing.
        let again = collect(project, Some("s1"), "userpromptsubmit", false);
        assert_eq!(again, None, "once entry must not re-deliver in the session");

        // A DIFFERENT session delivers again (its own marker namespace).
        let other = collect(project, Some("s2"), "userpromptsubmit", false);
        assert_eq!(other.as_deref(), Some("RULES"));
    }

    #[test]
    fn collect_skips_missing_file_and_foreign_trigger() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        // Declared but the file does not exist → None, no marker, no panic.
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"inject":[{"on":"sessionStart","file":".claude/mustard/nope.md","once":true}]}"#,
        )
        .unwrap();
        assert_eq!(collect(project, Some("s1"), "sessionstart", false), None);
        assert!(
            !dir.path().join(".claude/.session/s1/injected-nope.md").exists(),
            "no marker for an undelivered entry"
        );
        // A trigger with no declared entry → None.
        assert_eq!(collect(project, Some("s1"), "userpromptsubmit", false), None);
    }

    #[test]
    fn once_without_session_id_degrades_to_every_time() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_project(dir.path(), "userPromptSubmit", "rules.md", true, "X");
        // No usable session id: markers cannot be recorded, so the entry
        // delivers every time (fail-open: deliver, never silently drop).
        assert!(collect(project, None, "userpromptsubmit", false).is_some());
        assert!(collect(project, Some("unknown"), "userpromptsubmit", false).is_some());
        assert!(collect(project, None, "userpromptsubmit", false).is_some());
    }

    #[test]
    fn clear_markers_removes_only_injected_prefix() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let session = dir.path().join(".claude/.session/s1");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("injected-a.md"), "x").unwrap();
        std::fs::write(session.join("injected-b.md"), "x").unwrap();
        std::fs::write(session.join("active-spec"), "my-spec").unwrap();

        clear_markers(project, Some("s1"));

        assert!(!session.join("injected-a.md").exists(), "marker a cleared");
        assert!(!session.join("injected-b.md").exists(), "marker b cleared");
        assert!(session.join("active-spec").exists(), "sibling markers untouched");
    }

    #[test]
    fn ignore_markers_redelivers_despite_marker() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_project(dir.path(), "sessionStart", "style.md", true, "STYLE");
        // First delivery records the marker…
        assert!(collect(project, Some("s1"), "sessionstart", false).is_some());
        // …the guarded path now skips…
        assert_eq!(collect(project, Some("s1"), "sessionstart", false), None);
        // …but the post-compaction path (ignore_markers) re-delivers.
        assert_eq!(
            collect(project, Some("s1"), "sessionstart", true).as_deref(),
            Some("STYLE")
        );
    }
}
