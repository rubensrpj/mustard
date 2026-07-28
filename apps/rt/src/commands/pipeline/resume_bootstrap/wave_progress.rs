//! Wave-plan progress reconnaissance: the filesystem census, the dispatch
//! record that says whether that census was ever acted on, operational-spec
//! path resolution, and role derivation.
//!
//! All readers fail-open: an unreadable `spec_dir` degrades to `(0, 0)` /
//! `None` / empty and an unreadable event log degrades to "no dispatch on
//! record", so [`super::run`] never panics. Wave directories are 1-based
//! (`wave-1-*`, `wave-2-*`, …) — there is no `wave-0-*`.

use super::stage_resolver::{parse_header_value, read_first_lines};
use mustard_core::domain::model::event::{
    HarnessEvent, EVENT_PIPELINE_TASK_DISPATCH, EVENT_PIPELINE_WAVE_COMPLETE,
    EVENT_PIPELINE_WAVE_START,
};
use mustard_core::io::fs as mfs;
use std::path::{Path, PathBuf};

/// Event kinds that PROVE at least one wave of a spec was actually handed out.
///
/// `pipeline.wave.start` is the strongest: `wave-advance` emits it itself for
/// every wave it returns, so it lands whether or not the orchestrator relayed
/// anything back. `pipeline.task.dispatch` is orchestrator-relayed (see the
/// `wave-advance` module docs — not enforced) and `pipeline.wave.complete` is
/// retroactive proof; both are kept so an older log still answers.
const DISPATCH_RECORD_EVENTS: &[&str] = &[
    EVENT_PIPELINE_WAVE_START,
    EVENT_PIPELINE_TASK_DISPATCH,
    EVENT_PIPELINE_WAVE_COMPLETE,
];

/// Whether ANY wave of `spec` was ever dispatched, per the event log.
///
/// This is the one fact the filesystem cannot supply. `wave-scaffold`
/// materialises every `wave-N-*` directory up front, so a plan that was
/// scaffolded and never started is byte-identical on disk to one whose first
/// wave is in flight — [`count_wave_progress_from_fs`] reports `wave 1 of N`
/// for both. Only the dispatch record separates them, and it is what
/// [`super::ResumeBootstrap::never_dispatched`] reports.
///
/// Fail-open: an empty or unreadable log answers `false`, which is exactly what
/// an empty log means — nothing on record.
pub(super) fn wave_dispatch_recorded(events: &[HarnessEvent], spec: &str) -> bool {
    events.iter().any(|e| {
        e.spec.as_deref() == Some(spec) && DISPATCH_RECORD_EVENTS.contains(&e.event.as_str())
    })
}

/// Walk the spec dir for `wave-{N}-*/spec.md`. Returns the first match.
pub(super) fn find_wave_spec_path(spec_dir: &Path, wave: u32) -> Option<PathBuf> {
    let entries = mfs::read_dir(spec_dir).ok()?;
    let prefix = format!("wave-{wave}-");
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        if !entry.file_name.starts_with(&prefix) {
            continue;
        }
        let candidate = entry.path.join("spec.md");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Best-effort FS-side `(current, total)` census for a wave-plan when no events
/// are available. `total` counts `wave-N-*` directories; `done` counts the ones
/// whose header reads closed + completed; `current = done + 1`.
///
/// **A directory count is not progress.** `current` names the wave that comes
/// NEXT, never a wave that started: `wave-scaffold` writes all N directories
/// before anything is dispatched, so a plan nobody touched yields the same
/// `(1, N)` as a plan whose first wave is in flight. The caller must pair this
/// with [`wave_dispatch_recorded`] before rendering the pair as progress.
pub(super) fn count_wave_progress_from_fs(spec_dir: &Path) -> (u32, u32) {
    let Ok(entries) = mfs::read_dir(spec_dir) else {
        return (0, 0);
    };
    let mut total: u32 = 0;
    let mut done: u32 = 0;
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let name = &entry.file_name;
        if !name.starts_with("wave-") {
            continue;
        }
        // Must be `wave-<digits>-...`.
        let after = &name[5..];
        let digits_end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if digits_end == 0 || !after[digits_end..].starts_with('-') {
            continue;
        }
        total += 1;
        let spec_md = entry.path.join("spec.md");
        if let Some(head) = read_first_lines(&spec_md, 30) {
            let stage = parse_header_value(&head, "stage").unwrap_or_default();
            let outcome = parse_header_value(&head, "outcome").unwrap_or_default();
            if stage.eq_ignore_ascii_case("close") && outcome.eq_ignore_ascii_case("completed") {
                done += 1;
            }
        }
    }
    // Wave directories are 1-based (`wave-1-*` first): `current` is the first
    // incomplete wave = done + 1. 0 done → wave 1; after N complete → wave N+1
    // (past the last wave, where the post-execute gate takes over). No wave
    // dirs at all → 0.
    let current = if total == 0 { 0 } else { done + 1 };
    (current, total)
}

/// Derive the role token from a wave spec path like
/// `.claude/spec/{name}/wave-{N}-{role}/spec.md`.
pub(super) fn derive_role_from_wave_path(spec_path: &Path) -> Option<String> {
    let parent = spec_path.parent()?;
    let dir_name = parent.file_name()?.to_string_lossy();
    // Strip `wave-<digits>-` prefix.
    let rest = dir_name.strip_prefix("wave-")?;
    let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
    if digit_end == 0 {
        return None;
    }
    let after = &rest[digit_end..];
    let role = after.strip_prefix('-')?;
    if role.is_empty() {
        return None;
    }
    Some(role.to_string())
}

/// Resolve the wave directory name for `current_wave` from `spec_dir`. Returns
/// `None` when no matching directory exists (non-wave spec or pre-execute).
pub(super) fn find_wave_dir_name(spec_dir: &Path, wave: u32) -> Option<String> {
    let prefix = format!("wave-{wave}-");
    let entries = mfs::read_dir(spec_dir).ok()?;
    for entry in entries {
        if entry.is_dir && entry.file_name.starts_with(&prefix) {
            return Some(entry.file_name);
        }
    }
    None
}
