//! Filesystem primitives for `init`/`update`.
//!
//! - [`copy_dir`] — recursive directory copy, the engine behind
//!   `templates/` → `.claude/`. It honours an *overwrite* flag (a fresh
//!   install overwrites; a merge skips existing files so user edits survive)
//!   and a *top-level skip* list (`.github` lives at project root, not under
//!   `.claude/`).
//! - [`read_json_object`] — fail-open read of a JSON object, behind the
//!   `settings.json` mergers in `init`/`add`.
//!
//! The project config (`mustard.json`) is no longer merged here — it has a
//! single typed owner, [`mustard_core::ProjectConfig`].

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

/// Recursively copy `src` into `dest`, creating `dest` if absent.
///
/// - `overwrite`: when `false`, a destination file that already exists is left
///   untouched (and not counted). When `true`, it is replaced. The skip only
///   ever applies to files — directories are always recursed into.
/// - `skip_top_level`: directory/file *names* skipped **at the top level
///   only** — nested entries with the same name are still copied. This is how
///   `.github` is excluded from the `.claude/` copy.
///
/// Returns the number of files actually written.
pub fn copy_dir(
    src: &Path,
    dest: &Path,
    overwrite: bool,
    skip_top_level: &[&str],
) -> Result<usize> {
    fs::create_dir_all(dest)
        .with_context(|| format!("creating directory {}", dest.display()))?;

    let mut count = 0usize;
    let entries = fs::read_dir(src)
        .with_context(|| format!("reading directory {}", src.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| format!("reading an entry of {}", src.display()))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if skip_top_level.iter().any(|s| *s == name_str) {
            continue;
        }

        let src_path = entry.path();
        let dest_path = dest.join(&name);
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat {}", src_path.display()))?;

        if file_type.is_dir() {
            // Nested calls never skip — the skip list is top-level only.
            count += copy_dir(&src_path, &dest_path, overwrite, &[])?;
        } else if overwrite || !dest_path.exists() {
            fs::copy(&src_path, &dest_path).with_context(|| {
                format!("copying {} → {}", src_path.display(), dest_path.display())
            })?;
            count += 1;
        }
    }

    Ok(count)
}

/// Read `path` as a JSON object. An absent file, an I/O failure, malformed
/// JSON, or a non-object top-level value all collapse to an empty map — the
/// caller never has to distinguish them. The fail-open read behind the
/// `settings.json` mergers in `init`/`add`.
pub fn read_json_object(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn copy_dir_copies_nested_tree() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dest = dir.path().join("dest");
        write(&src.join("a.txt"), "a");
        write(&src.join("nested/b.txt"), "b");

        let count = copy_dir(&src, &dest, true, &[]).unwrap();

        assert_eq!(count, 2);
        assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "a");
        assert_eq!(fs::read_to_string(dest.join("nested/b.txt")).unwrap(), "b");
    }

    #[test]
    fn copy_dir_skips_existing_when_not_overwriting() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dest = dir.path().join("dest");
        write(&src.join("keep.txt"), "fresh");
        write(&dest.join("keep.txt"), "user-edit");

        let count = copy_dir(&src, &dest, false, &[]).unwrap();

        assert_eq!(count, 0);
        assert_eq!(fs::read_to_string(dest.join("keep.txt")).unwrap(), "user-edit");
    }

    #[test]
    fn copy_dir_skip_list_is_top_level_only() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dest = dir.path().join("dest");
        write(&src.join(".github/ci.yml"), "top");
        write(&src.join("inner/.github/ci.yml"), "nested");

        copy_dir(&src, &dest, true, &[".github"]).unwrap();

        assert!(!dest.join(".github").exists());
        assert!(dest.join("inner/.github/ci.yml").exists());
    }

    #[test]
    fn read_json_object_recovers_from_malformed_input() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        write(&path, "{ not valid");

        assert!(read_json_object(&path).is_empty());
    }
}
