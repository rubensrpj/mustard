//! `mustard-rt run doctor --check claude-paths` — audit the filesystem
//! against the [`ClaudePaths`] catalog.
//!
//! W3.T3.4 of `2026-05-26-claude-paths-single-source`. The catalog
//! ([`ClaudePaths::documented_dirs`] + [`ClaudePaths::cache_files`]) is now the
//! single source of truth for "what belongs under `.claude/`". This check
//! verifies the live filesystem matches the catalog and reports both
//! directions of divergence:
//!
//! - **unexpected** — a directory or cache file exists on disk but is not in
//!   the catalog. `severity: "warn"` (drift to investigate; may be a new
//!   feature that forgot to update `claude_paths.rs`).
//! - **missing** — the catalog lists an entry but the filesystem does not have
//!   it. `severity: "warn"` (clean installs legitimately omit unused dirs;
//!   never an error).
//!
//! Fail-open: an unreadable `.claude/` returns `{ok: true, divergences: []}`.
//! The check never blocks on IO problems — that's the doctor's job globally.

use mustard_core::ClaudePaths;
use serde::Serialize;
use std::path::Path;

/// One audit divergence — `unexpected` (FS has it, catalog doesn't) or
/// `missing` (catalog lists it, FS doesn't).
#[derive(Debug, Serialize)]
pub struct Divergence {
    pub path: String,
    pub kind: &'static str,
    pub severity: &'static str,
}

/// Aggregate audit result.
#[derive(Debug, Serialize)]
pub struct ClaudePathsReport {
    pub ok: bool,
    pub divergences: Vec<Divergence>,
}

/// Run the check against `root` (the project root that contains `.claude/`).
/// Honors [`ClaudePaths::for_project`]'s I1 guard — a `.claude/.claude/`
/// path returns an empty (ok=true) report rather than crashing.
#[must_use]
pub fn run(root: &Path) -> ClaudePathsReport {
    let Ok(paths) = ClaudePaths::for_project(root) else {
        return ClaudePathsReport { ok: true, divergences: Vec::new() };
    };

    let mut divergences: Vec<Divergence> = Vec::new();

    // ---- unexpected dirs + unexpected cache files -------------------------
    for orphan in paths.audit_orphans() {
        divergences.push(Divergence {
            path: orphan.to_string_lossy().into_owned(),
            kind: "unexpected",
            severity: "warn",
        });
    }

    // ---- missing dirs (catalog vs filesystem) -----------------------------
    let claude_dir = paths.claude_dir();
    for documented in ClaudePaths::documented_dirs() {
        let candidate = claude_dir.join(documented);
        if !candidate.exists() {
            divergences.push(Divergence {
                path: candidate.to_string_lossy().into_owned(),
                kind: "missing",
                severity: "warn",
            });
        }
    }

    // ---- missing cache files ---------------------------------------------
    let cache_dir = paths.cache_dir();
    for cache_file in ClaudePaths::cache_files() {
        let candidate = cache_dir.join(cache_file);
        if !candidate.exists() {
            divergences.push(Divergence {
                path: candidate.to_string_lossy().into_owned(),
                kind: "missing",
                severity: "warn",
            });
        }
    }

    // `ok` is true unless there is at least one `severity: "error"`. The
    // current check emits warn-only, so a healthy install with divergences
    // still returns ok=true (divergences are advisory).
    let ok = !divergences.iter().any(|d| d.severity == "error");
    ClaudePathsReport { ok, divergences }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clean_tree_with_full_catalog_is_ok() {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let claude = paths.claude_dir();
        std::fs::create_dir_all(&claude).unwrap();
        for d in ClaudePaths::documented_dirs() {
            std::fs::create_dir_all(claude.join(d)).unwrap();
        }
        std::fs::create_dir_all(paths.cache_dir()).unwrap();
        for f in ClaudePaths::cache_files() {
            std::fs::write(paths.cache_dir().join(f), b"{}").unwrap();
        }

        let report = run(dir.path());
        assert!(report.ok);
        assert!(report.divergences.is_empty(), "{:?}", report.divergences);
    }

    #[test]
    fn unexpected_dir_surfaces_as_warn() {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let claude = paths.claude_dir();
        std::fs::create_dir_all(&claude).unwrap();
        // Plant only ONE orphan; let every documented dir + cache stay missing.
        std::fs::create_dir_all(claude.join("legacy-bucket")).unwrap();
        let report = run(dir.path());
        let unexpected: Vec<&Divergence> = report.divergences.iter()
            .filter(|d| d.kind == "unexpected")
            .collect();
        assert_eq!(unexpected.len(), 1);
        assert_eq!(unexpected[0].severity, "warn");
        assert!(unexpected[0].path.contains("legacy-bucket"));
        // The report is still ok=true because no divergence carries severity="error".
        assert!(report.ok);
    }

    #[test]
    fn missing_documented_dir_surfaces_as_warn() {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        std::fs::create_dir_all(paths.claude_dir()).unwrap();
        // No documented dirs planted — every one should report missing.
        let report = run(dir.path());
        let missing: Vec<&Divergence> = report.divergences.iter()
            .filter(|d| d.kind == "missing")
            .collect();
        // documented_dirs() + cache_files() both contribute.
        let expected = ClaudePaths::documented_dirs().len() + ClaudePaths::cache_files().len();
        assert_eq!(missing.len(), expected);
        for m in missing {
            assert_eq!(m.severity, "warn");
        }
    }
}
