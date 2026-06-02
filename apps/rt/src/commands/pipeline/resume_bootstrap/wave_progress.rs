//! Wave-plan filesystem reconnaissance: progress counting, operational-spec
//! path resolution, and role derivation.
//!
//! All readers fail-open: an unreadable `spec_dir` degrades to `(0, 0)` /
//! `None` / empty so [`super::run`] never panics. Wave directories are 0-based
//! (`wave-0-*`, `wave-1-*`, …).

use super::stage_resolver::{parse_header_value, read_first_lines};
use mustard_core::io::fs as mfs;
use std::path::{Path, PathBuf};

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

/// Best-effort FS-side (current, total) progress for a wave-plan when no
/// events are available. `current = done + 1` capped at `total`.
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
    // Wave directories are 0-based: `current` is the first incomplete wave.
    // When nothing is done yet, current = 0; after N waves complete, current = N.
    let current = done.min(total.saturating_sub(1));
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
