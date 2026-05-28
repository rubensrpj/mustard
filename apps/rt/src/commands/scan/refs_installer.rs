//! Install progressive-disclosure refs from `apps/cli/templates/refs/stack-templates/`
//! into a target subproject's `.claude/refs/<cmd>/` directory based on its
//! detected stack signals.
//!
//! Each ref source is a `.md` file whose YAML frontmatter declares a
//! `qualifyingSignals: [signal-token, signal-token, ...]` list. A signal is
//! a free-form token; the convention in shipped refs is `role:<role>` or
//! `stack:<id>` (e.g. `role:ui`, `stack:react`, `role:frontend`).
//!
//! [`install_refs`] walks the stack-templates directory, parses the
//! frontmatter `qualifyingSignals`, and copies any matching ref into
//! `<target_root>/.claude/refs/<cmd>/<basename>.md`. `<cmd>` is currently
//! derived from the source filename family (`browser-debug` → `bugfix`,
//! `fe-craft-check` → `feature`); unknown families default to a `scan/`
//! subfolder so the installer never silently drops a matched ref.
//!
//! **Idempotency:** before writing, the installer compares the destination
//! bytes with the source. Identical bytes ⇒ skip. The `InstallReport`
//! distinguishes `installed`, `skipped_identical`, and `errors` so callers
//! can surface drift without re-issuing writes.
//!
//! Fail-open. Missing templates dir, unreadable frontmatter, or IO errors
//! return an empty / partially-populated report — never a panic.

use mustard_core::io::fs as mfs;
use mustard_core::ClaudePaths;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One subproject's detected stack signals.
///
/// `id` is the stack id reported by `detect_stack` (e.g. `typescript`,
/// `dotnet`). `roles` is the inferred role set (e.g. `ui`, `backend`).
/// Extra free-form tokens may be passed via `extras` and matched verbatim.
#[derive(Debug, Default, Clone)]
pub struct DetectedStack {
    /// Stack id (`typescript`, `rust`, `dotnet`, …).
    pub id: String,
    /// Role tokens (`ui`, `backend`, `mobile-web`, …).
    pub roles: Vec<String>,
    /// Free-form extra signals — joined with role/stack tokens at match time.
    pub extras: Vec<String>,
}

impl DetectedStack {
    /// Build the full set of qualifying tokens (`role:*`, `stack:*`, extras).
    #[must_use]
    pub fn tokens(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        if !self.id.is_empty() {
            out.insert(format!("stack:{}", self.id));
        }
        for role in &self.roles {
            if !role.is_empty() {
                out.insert(format!("role:{role}"));
            }
        }
        for extra in &self.extras {
            if !extra.is_empty() {
                out.insert(extra.clone());
            }
        }
        out
    }
}

/// Outcome of a single template scan pass.
#[derive(Debug, Default, Clone)]
pub struct InstallReport {
    /// Refs newly written to disk.
    pub installed: Vec<PathBuf>,
    /// Refs whose destination already matched source byte-for-byte.
    pub skipped_identical: Vec<PathBuf>,
    /// Refs whose `qualifyingSignals` did not match the detected stack.
    pub skipped_no_match: Vec<PathBuf>,
    /// IO / parse errors — fail-open, never propagated.
    pub errors: Vec<String>,
}

/// Locate the stack-templates directory shipped with `apps/cli`.
///
/// In production the templates live under `apps/cli/templates/refs/stack-templates`
/// relative to the workspace root. The caller passes the workspace root.
/// Returns `None` when the directory does not exist (fresh checkout, custom
/// install layout, etc.) — every caller treats `None` as "no refs to install".
#[must_use]
pub fn stack_templates_dir(workspace_root: &Path) -> Option<PathBuf> {
    let p = workspace_root
        .join("apps")
        .join("cli")
        .join("templates")
        .join("refs")
        .join("stack-templates");
    if p.is_dir() { Some(p) } else { None }
}

/// Parse the leading YAML frontmatter block of a ref body and pull out the
/// `qualifyingSignals: [...]` list. Returns an empty vec when no
/// frontmatter, no signals key, or unparseable input.
#[must_use]
pub fn parse_qualifying_signals(body: &str) -> Vec<String> {
    let trimmed = body.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix("---") else {
        return Vec::new();
    };
    // Find the closing `---` on its own line.
    let mut end_idx: Option<usize> = None;
    for (offset, line) in rest.split_inclusive('\n').scan(0usize, |acc, l| {
        let start = *acc;
        *acc += l.len();
        Some((start, l))
    }) {
        if line.trim_end_matches('\n').trim() == "---" {
            end_idx = Some(offset);
            break;
        }
    }
    let Some(end) = end_idx else { return Vec::new() };
    let front = &rest[..end];
    for line in front.lines() {
        let line = line.trim();
        if let Some(after) = line.strip_prefix("qualifyingSignals:") {
            let after = after.trim();
            // Support inline `[a, b, c]` form only — multi-line YAML lists
            // are out of scope for shipped refs (the two existing templates
            // both use inline form).
            let inner = after
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim();
            if inner.is_empty() {
                return Vec::new();
            }
            return inner
                .split(',')
                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// Map a source basename family to a target `<cmd>` subfolder. The mapping
/// follows the convention recorded in `apps/cli/templates/refs/` proper:
/// `browser-debug.md` is loaded by `/bugfix`, `fe-craft-check.md` by
/// `/feature`. Unknown families land under `scan/` so the ref is still
/// reachable and the installer never silently drops a matched template.
fn target_subdir(stem: &str) -> &'static str {
    if stem.contains("browser") || stem.contains("debug") {
        "bugfix"
    } else if stem.contains("craft") || stem.contains("feature") {
        "feature"
    } else {
        "scan"
    }
}

/// Walk `stack_templates_dir` and install every ref whose
/// `qualifyingSignals` intersects `stack.tokens()`.
///
/// Idempotent: re-installing the same ref against an identical target file
/// reports `skipped_identical`. New / changed content overwrites the
/// destination. Every error is captured in `InstallReport::errors`; the
/// function itself never returns `Result` (fail-open contract).
#[must_use]
pub fn install_refs(stack: &DetectedStack, target_root: &Path) -> InstallReport {
    let mut report = InstallReport::default();
    let Some(workspace_root) = find_workspace_root(target_root) else {
        report
            .errors
            .push(format!("workspace root not found from {}", target_root.display()));
        return report;
    };
    let Some(templates_dir) = stack_templates_dir(&workspace_root) else {
        // No templates shipped (custom install layout) — fail open, empty report.
        return report;
    };
    let Ok(target_paths) = ClaudePaths::for_project(target_root) else {
        report
            .errors
            .push(format!("invalid target root {}", target_root.display()));
        return report;
    };

    let entries = match mfs::read_dir(&templates_dir) {
        Ok(e) => e,
        Err(e) => {
            report.errors.push(format!(
                "read_dir({}): {}",
                templates_dir.display(),
                e
            ));
            return report;
        }
    };

    let tokens = stack.tokens();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        if !entry.file_name.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        let src_path = entry.path.clone();
        let body = match mfs::read_to_string(&src_path) {
            Ok(b) => b,
            Err(e) => {
                report.errors.push(format!("read {}: {}", src_path.display(), e));
                continue;
            }
        };
        let signals = parse_qualifying_signals(&body);
        if signals.is_empty() {
            report.skipped_no_match.push(src_path);
            continue;
        }
        let any_match = signals.iter().any(|s| tokens.contains(s));
        if !any_match {
            report.skipped_no_match.push(src_path);
            continue;
        }
        let stem = src_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("ref");
        let cmd = target_subdir(stem);
        let target_dir = target_paths.refs_dir().join(cmd);
        if let Err(e) = std::fs::create_dir_all(&target_dir) {
            report
                .errors
                .push(format!("create_dir_all({}): {}", target_dir.display(), e));
            continue;
        }
        let target_path = target_dir.join(format!("{stem}.md"));
        // Idempotency check — byte-equal source ⇒ skip.
        if let Ok(existing) = mfs::read_to_string(&target_path) {
            if existing == body {
                report.skipped_identical.push(target_path);
                continue;
            }
        }
        match std::fs::write(&target_path, body.as_bytes()) {
            Ok(()) => report.installed.push(target_path),
            Err(e) => report
                .errors
                .push(format!("write {}: {}", target_path.display(), e)),
        }
    }

    report
}

/// Walk up from `start` until a directory containing `apps/cli/templates`
/// (the canonical workspace marker) is found. Returns `None` after 8
/// levels — a safe cap that covers every nested subproject layout we ship.
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    for _ in 0..8 {
        if cur
            .join("apps")
            .join("cli")
            .join("templates")
            .is_dir()
        {
            return Some(cur);
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => break,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn workspace_with_ref(dir: &Path, name: &str, signals: &[&str], body: &str) {
        let signal_list = signals
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let full = format!("---\nqualifyingSignals: [{signal_list}]\n---\n{body}");
        let path = dir
            .join("apps")
            .join("cli")
            .join("templates")
            .join("refs")
            .join("stack-templates")
            .join(name);
        write(&path, &full);
    }

    #[test]
    fn parse_qualifying_signals_inline() {
        let body = "---\nqualifyingSignals: [role:ui, stack:react]\n---\n# Hi\n";
        let sig = parse_qualifying_signals(body);
        assert_eq!(sig, vec!["role:ui".to_string(), "stack:react".to_string()]);
    }

    #[test]
    fn parse_qualifying_signals_no_frontmatter() {
        assert!(parse_qualifying_signals("# Plain markdown\n").is_empty());
    }

    #[test]
    fn install_writes_matching_ref_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        workspace_with_ref(
            root,
            "fe-craft-check.md",
            &["role:ui", "stack:react"],
            "# Craft check\nbody\n",
        );
        let target = root.join("apps").join("dashboard");
        std::fs::create_dir_all(&target).unwrap();
        let stack = DetectedStack {
            id: "typescript".to_string(),
            roles: vec!["ui".to_string()],
            extras: vec![],
        };

        let first = install_refs(&stack, &target);
        assert_eq!(first.installed.len(), 1, "{first:?}");
        assert!(first.skipped_identical.is_empty());

        // Second run with identical bytes ⇒ skipped_identical.
        let second = install_refs(&stack, &target);
        assert!(second.installed.is_empty());
        assert_eq!(second.skipped_identical.len(), 1);
    }

    #[test]
    fn install_skips_non_matching_signals() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        workspace_with_ref(
            root,
            "fe-craft-check.md",
            &["role:ui"],
            "# Craft\n",
        );
        let target = root.join("apps").join("backend");
        std::fs::create_dir_all(&target).unwrap();
        let stack = DetectedStack {
            id: "rust".to_string(),
            roles: vec!["backend".to_string()],
            extras: vec![],
        };
        let report = install_refs(&stack, &target);
        assert!(report.installed.is_empty(), "{report:?}");
        assert_eq!(report.skipped_no_match.len(), 1);
    }

    #[test]
    fn missing_templates_dir_is_fail_open() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("solo");
        std::fs::create_dir_all(&target).unwrap();
        let stack = DetectedStack::default();
        let report = install_refs(&stack, &target);
        assert!(report.installed.is_empty());
        assert!(report.skipped_identical.is_empty());
        // Either errors (no workspace marker) or empty — never panics.
    }
}
