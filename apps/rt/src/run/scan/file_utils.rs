//! File-collection and path helpers shared across scanners — a port of
//! `registry/file-utils.js`.
//!
//! Only filesystem utilities live here: no scanning logic, no schema building.
//! Every function is fail-open (an unreadable directory yields an empty result,
//! never an error), matching the JS module's `try { … } catch { … }` shape.

use mustard_core::fs as mfs;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Universal directory skip-list — mirrors `DEFAULT_IGNORE` in `file-utils.js`.
pub const DEFAULT_IGNORE: &[&str] = &[
    "node_modules",
    "bin",
    "obj",
    "dist",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    "target",
    "build",
    ".git",
    "migrations",
    "Migrations",
];

/// Extract directory-name patterns from a `.gitignore` string.
///
/// A faithful port of `parseGitignoreDirs()` — conservative: keeps only entries
/// that look like a plain folder name (non-empty, no whitespace, no glob chars,
/// no slashes, not a negation, not a comment). Trailing slashes are stripped.
#[must_use]
pub fn parse_gitignore_dirs(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if line.starts_with('/') {
            continue; // path-anchored
        }
        if line
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '*' | '?' | '[' | ']'))
        {
            continue; // glob or whitespace
        }
        let name = line.strip_suffix('/').unwrap_or(line);
        if name.contains('/') {
            continue; // nested path, not a bare name
        }
        out.push(name.to_string());
    }
    out
}

/// Build the merged skip-set for a walk rooted at `dir`.
///
/// Combines `DEFAULT_IGNORE`, the explicit `ignore` argument, the
/// `MUSTARD_SCAN_IGNORE` env var (comma-separated), and directory entries
/// parsed from the subproject's `.gitignore` — exactly the four sources
/// `collectFiles` merges.
fn ignore_set(dir: &Path, ignore: &[&str]) -> BTreeSet<String> {
    let mut set: BTreeSet<String> = DEFAULT_IGNORE.iter().map(|s| (*s).to_string()).collect();
    for extra in ignore {
        set.insert((*extra).to_string());
    }
    if let Ok(env) = std::env::var("MUSTARD_SCAN_IGNORE") {
        for name in env.split(',') {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                set.insert(trimmed.to_string());
            }
        }
    }
    if let Ok(content) = mfs::read_to_string(&dir.join(".gitignore")) {
        for name in parse_gitignore_dirs(&content) {
            set.insert(name);
        }
    }
    set
}

/// Recursively collect every file with `extension` under `dir`.
///
/// Skips ignored directories, dot-directories, and the `ignore` argument —
/// a faithful port of `collectFiles()`. `extension` includes the dot
/// (e.g. `.rs`). Fail-open: unreadable directories are silently skipped.
#[must_use]
pub fn collect_files(dir: &Path, extension: &str, ignore: &[&str]) -> Vec<PathBuf> {
    let skip = ignore_set(dir, ignore);
    let mut results = Vec::new();
    walk(dir, extension, &skip, &mut results);
    results
}

fn walk(current: &Path, extension: &str, skip: &BTreeSet<String>, results: &mut Vec<PathBuf>) {
    let Ok(entries) = mfs::read_dir(current) else {
        return;
    };
    for entry in entries {
        let name: &str = &entry.file_name;
        if entry.is_dir {
            if skip.contains(name) || name.starts_with('.') {
                continue;
            }
            walk(&entry.path, extension, skip, results);
        } else if name.ends_with(extension) {
            results.push(entry.path);
        }
    }
}

/// Relative path from `base` to `file_path`, normalised with forward slashes.
///
/// A faithful port of `relativePath()`.
#[must_use]
pub fn relative_path(base: &Path, file_path: &Path) -> String {
    let rel = file_path.strip_prefix(base).unwrap_or(file_path);
    rel.to_string_lossy().replace('\\', "/")
}

/// Read a file as UTF-8, returning `None` on any error — a port of `readFileSafe()`.
#[must_use]
pub fn read_file_safe(file_path: &Path) -> Option<String> {
    mfs::read_to_string(file_path).ok()
}

/// Most common parent folder across a list of relative file paths.
///
/// A faithful port of `inferCommonFolder()` — returns the most frequent parent
/// directory with a trailing slash, or `None` for an empty input.
#[must_use]
pub fn infer_common_folder(file_paths: &[String]) -> Option<String> {
    if file_paths.is_empty() {
        return None;
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    for fp in file_paths {
        let normalized = fp.replace('\\', "/");
        let dir = match normalized.rfind('/') {
            Some(idx) => normalized[..idx].to_string(),
            None => ".".to_string(),
        };
        if let Some(entry) = counts.iter_mut().find(|(d, _)| *d == dir) {
            entry.1 += 1;
        } else {
            counts.push((dir, 1));
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(dir, _)| format!("{dir}/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_gitignore_keeps_plain_names_only() {
        let content = "# comment\nnode_modules\n/anchored\n*.log\n  \nvendor/\n!keep\nsrc/nested\n";
        assert_eq!(
            parse_gitignore_dirs(content),
            vec!["node_modules".to_string(), "vendor".to_string()]
        );
    }

    #[test]
    fn collect_files_skips_ignored_dirs() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target").join("b.rs"), "").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("c.rs"), "").unwrap();

        let mut found: Vec<String> = collect_files(dir.path(), ".rs", &[])
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        found.sort();
        assert_eq!(found, vec!["a.rs".to_string(), "c.rs".to_string()]);
    }

    #[test]
    fn infer_common_folder_picks_most_frequent() {
        let paths = vec![
            "src/domain/user.rs".to_string(),
            "src/domain/order.rs".to_string(),
            "src/api/route.rs".to_string(),
        ];
        assert_eq!(infer_common_folder(&paths), Some("src/domain/".to_string()));
    }

    #[test]
    fn infer_common_folder_empty_is_none() {
        assert_eq!(infer_common_folder(&[]), None);
    }
}
