//! `mustard-rt run doctor --check i1` — detect any physical
//! `.claude/.claude/` sequence anywhere in the workspace.
//!
//! W3.T3.9 of `2026-05-26-claude-paths-single-source`. The I1 guard in
//! [`mustard_core::io::claude_paths`] is supposed to make the forbidden sequence
//! impossible to construct programmatically — but if one physically exists on
//! disk, it means an older version of Mustard left it behind, or some external
//! tool re-applied `.claude` over a path that was already inside `.claude/`.
//!
//! Either way it is a hard error: every path-building hot path now relies on
//! the invariant. `severity: "error"` and exit-code-not-zero are deliberate —
//! a healthy install must satisfy the I1 guard at the filesystem level too.
//!
//! Honors `MUSTARD_WORKSPACE_ROOT`: when set, the walker scans that root
//! instead of `start_dir`. Tests pass it explicitly to exercise the violation
//! path without mutating the real environment.

use serde::Serialize;
use std::path::{Path, PathBuf};
use mustard_core::io::fs;

#[derive(Debug, Serialize)]
pub struct I1Report {
    pub ok: bool,
    pub violations: Vec<String>,
}

/// Walk `start_dir` (or `MUSTARD_WORKSPACE_ROOT`) and return every directory
/// path that ends in `.claude/.claude` or contains `.claude/.claude/` anywhere.
#[must_use]
pub fn run(start_dir: &Path) -> I1Report {
    let root = std::env::var("MUSTARD_WORKSPACE_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| start_dir.to_path_buf());

    let mut violations: Vec<String> = Vec::new();
    walk(&root, &mut violations, 0, 8);

    // Deterministic order — easier to diff across runs.
    violations.sort();
    violations.dedup();
    let ok = violations.is_empty();
    I1Report { ok, violations }
}

fn walk(dir: &Path, out: &mut Vec<String>, depth: usize, max_depth: usize) {
    if depth > max_depth {
        return;
    }
    if let Some(name) = dir.file_name().and_then(|s| s.to_str()) {
        if fs::PRUNE_DIRS.contains(&name) {
            return;
        }
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let path = entry.path;
        if is_dot_claude_in_dot_claude(&path) {
            out.push(path.to_string_lossy().into_owned());
            // Do not descend — the I1 violation is the directory itself; any
            // descendants are corollaries of the same root cause.
            continue;
        }
        walk(&path, out, depth + 1, max_depth);
    }
}

/// True iff `path` ends in `.claude/.claude` OR contains `.claude/.claude/`
/// as a sub-sequence (matches the canonical guard in
/// [`mustard_core::io::claude_paths`]).
fn is_dot_claude_in_dot_claude(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains(".claude/.claude/") || s.ends_with(".claude/.claude")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clean_tree_is_ok() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join("skills")).unwrap();
        let report = run(dir.path());
        assert!(report.ok);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn nested_dot_claude_violates() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join(".claude").join(".claude");
        std::fs::create_dir_all(&bad).unwrap();
        let report = run(dir.path());
        assert!(!report.ok);
        assert_eq!(report.violations.len(), 1);
        let v = &report.violations[0].replace('\\', "/");
        assert!(v.ends_with(".claude/.claude"), "got: {v}");
    }

    #[test]
    fn deeply_nested_dot_claude_violates() {
        let dir = tempdir().unwrap();
        let bad = dir
            .path()
            .join("apps")
            .join("api")
            .join(".claude")
            .join(".claude")
            .join("skills");
        std::fs::create_dir_all(&bad).unwrap();
        let report = run(dir.path());
        assert!(!report.ok);
        assert_eq!(report.violations.len(), 1);
    }
}
