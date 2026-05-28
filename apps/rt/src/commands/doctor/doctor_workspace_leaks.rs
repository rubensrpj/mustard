//! `mustard-rt run doctor --check workspace-leaks` — detect non-root
//! `.claude/` directories that hold pipeline state.
//!
//! W3.T3.8 of `2026-05-26-claude-paths-single-source`. Only the workspace
//! root anchor (the directory with both `mustard.json` and `.claude/`) is
//! supposed to own pipeline state. Any nested `.claude/` that picks up
//! `.harness/`, `.agent-state/`, `.pipeline-states/`, `memory/`, `plans/`,
//! `.metrics/`, `spec/`, or `.agent-memory/` is a bug — the harness is writing
//! to the wrong root.
//!
//! Legitimate nested `.claude/` content includes only scan-emitted artifacts
//! (`commands/`, `skills/`, `agents/`, `services.json`, `refs/`,
//! `CLAUDE.md`, `.cluster-cache.json`, `.interpret-cache.json`).
//!
//! Output: never deletes. Each leak gets a `suggested_cleanup` command the
//! user can run if they confirm.

use mustard_core::workspace::workspace_root;
use mustard_core::ClaudePaths;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Directory or file basenames that *may* legitimately live under a nested
/// `.claude/` (scan output, per-subproject context). Kept for documentation:
/// the leak detector is allow-by-default (anything not in [`LEAK_ENTRIES`] is
/// fine), so we never have to read this list at runtime — but it codifies the
/// contract for future maintainers and is exported for tests.
#[allow(dead_code)]
pub(crate) const LEGITIMATE_ENTRIES: &[&str] = &[
    "commands",
    "skills",
    "agents",
    "services.json",
    "refs",
    "CLAUDE.md",
    ".cluster-cache.json",
    ".interpret-cache.json",
];

/// Basenames that MUST live only at the workspace root `.claude/`. Finding
/// any of these under a nested `.claude/` is a leak.
const LEAK_ENTRIES: &[&str] = &[
    ".harness",
    ".agent-state",
    ".pipeline-states",
    "memory",
    "plans",
    ".metrics",
    "spec",
    ".agent-memory",
];

#[derive(Debug, Serialize)]
pub struct WorkspaceLeak {
    pub path: String,
    pub leaked_entries: Vec<String>,
    pub suggested_cleanup: String,
    pub severity: &'static str,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceLeaksReport {
    pub ok: bool,
    pub root: Option<String>,
    pub leaks: Vec<WorkspaceLeak>,
}

/// Walk the workspace starting at `start_dir` and report every nested
/// `.claude/` that contains a pipeline-state entry. The workspace root itself
/// is excluded.
#[must_use]
pub fn run(start_dir: &Path) -> WorkspaceLeaksReport {
    // Resolve the workspace root (anchor walk). If absent, there is nothing
    // to compare against — return an empty (ok=true) report.
    let Ok(root) = workspace_root(start_dir) else {
        return WorkspaceLeaksReport { ok: true, root: None, leaks: Vec::new() };
    };
    let Ok(paths) = ClaudePaths::for_project(&root) else {
        return WorkspaceLeaksReport {
            ok: true,
            root: Some(root.to_string_lossy().into_owned()),
            leaks: Vec::new(),
        };
    };

    let root_claude = paths.claude_dir();
    let root_claude_canon = canonicalize_or_self(&root_claude);

    let mut leaks: Vec<WorkspaceLeak> = Vec::new();
    walk_for_claude_dirs(&root, &root_claude_canon, &mut leaks, 0, 6);

    let ok = leaks.is_empty();
    WorkspaceLeaksReport {
        ok,
        root: Some(root.to_string_lossy().into_owned()),
        leaks,
    }
}

/// Recursive walker — finds every `.claude/` under `dir` that is NOT the
/// workspace root's `.claude/` and inspects it for leak entries. Capped at
/// `max_depth` to keep the audit bounded; large monorepos with many vendored
/// `.claude/` would otherwise scan forever.
fn walk_for_claude_dirs(
    dir: &Path,
    root_claude_canon: &Path,
    leaks: &mut Vec<WorkspaceLeak>,
    depth: usize,
    max_depth: usize,
) {
    if depth > max_depth {
        return;
    }
    // Skip directories whose name implies vendored/build content.
    if let Some(name) = dir.file_name().and_then(|s| s.to_str()) {
        if matches!(
            name,
            "node_modules" | "target" | ".git" | "dist" | "build" | "bin" | "obj"
        ) {
            return;
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ty) = entry.file_type() else { continue };
        if !ty.is_dir() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };

        if name_str == ".claude" {
            // Compare canonical form to the root's own `.claude/`.
            let candidate = canonicalize_or_self(&path);
            if candidate != *root_claude_canon {
                inspect_nested_claude(&path, leaks);
            }
            // Do NOT descend into a `.claude/` — its contents are leaf state.
            continue;
        }

        walk_for_claude_dirs(&path, root_claude_canon, leaks, depth + 1, max_depth);
    }
}

/// Read a non-root `.claude/`, classify each entry, and append a leak record
/// if any [`LEAK_ENTRIES`] hit.
fn inspect_nested_claude(claude_dir: &Path, leaks: &mut Vec<WorkspaceLeak>) {
    let Ok(entries) = std::fs::read_dir(claude_dir) else {
        return;
    };
    let leak_set: HashSet<&str> = LEAK_ENTRIES.iter().copied().collect();
    let mut hit: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if leak_set.contains(name.as_str()) {
            hit.push(name);
        }
    }
    if hit.is_empty() {
        // Optionally cross-check that every entry is legitimate; we don't
        // upgrade to a leak when the only finding is an unknown-but-harmless
        // entry — the catalog check (`claude-paths`) covers the root.
        return;
    }

    let path_str = claude_dir.to_string_lossy().into_owned();
    let suggested = build_cleanup_command(&path_str, &hit);
    leaks.push(WorkspaceLeak {
        path: path_str,
        leaked_entries: hit,
        suggested_cleanup: suggested,
        severity: "warn",
    });
}

fn build_cleanup_command(claude_path: &str, entries: &[String]) -> String {
    let joined: Vec<String> = entries
        .iter()
        .map(|e| format!("{claude_path}/{e}"))
        .collect();
    format!("rm -rf {}", joined.join(" "))
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Build a minimal Mustard anchor (mustard.json + .claude/).
    fn make_anchor(at: &Path) {
        std::fs::write(at.join("mustard.json"), b"{}").unwrap();
        std::fs::create_dir_all(at.join(".claude")).unwrap();
    }

    #[test]
    fn no_nested_claude_dirs_is_ok() {
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        let report = run(dir.path());
        assert!(report.ok);
        assert!(report.leaks.is_empty());
    }

    #[test]
    fn legitimate_nested_claude_with_only_skills_is_ok() {
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        let sub = dir.path().join("apps").join("api").join(".claude");
        std::fs::create_dir_all(sub.join("skills")).unwrap();
        std::fs::write(sub.join("CLAUDE.md"), b"# sub").unwrap();

        let report = run(dir.path());
        assert!(report.ok, "leaks: {:?}", report.leaks);
    }

    #[test]
    fn nested_claude_with_pipeline_states_is_a_leak() {
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        let sub = dir.path().join("apps").join("api").join(".claude");
        std::fs::create_dir_all(sub.join(".pipeline-states")).unwrap();
        std::fs::create_dir_all(sub.join("memory")).unwrap();

        let report = run(dir.path());
        assert!(!report.ok);
        assert_eq!(report.leaks.len(), 1);
        assert!(report.leaks[0].leaked_entries.contains(&".pipeline-states".to_string()));
        assert!(report.leaks[0].leaked_entries.contains(&"memory".to_string()));
        assert!(report.leaks[0].suggested_cleanup.contains(".pipeline-states"));
    }

    #[test]
    fn root_claude_is_not_flagged() {
        // Plant a `.harness/` only at the root — the root's own `.claude/`
        // must be skipped, never compared against the leak list.
        let dir = tempdir().unwrap();
        make_anchor(dir.path());
        std::fs::create_dir_all(dir.path().join(".claude").join(".harness")).unwrap();
        let report = run(dir.path());
        assert!(report.ok, "root .harness must not be flagged: {:?}", report.leaks);
    }
}
