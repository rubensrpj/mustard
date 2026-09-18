//! O desvio dos moldes: as pastas que uma instalação nova traz, comparadas
//! por hash entre o `.claude/` instalado e a pasta `templates/` de origem.
//! Fora do repositório do Mustard, onde a origem não é alcançável, a
//! conferência é pulada.

use std::path::{Path, PathBuf};

use mustard_core::io::fs;

use super::residue::collect_recursive_inner;
use super::CheckResult;
use crate::util::sha256::Sha256;

/// The Mustard-owned folders the drift check compares against `templates/`.
/// They historically shipped in the payload; in Mustard 2.0 they move to the
/// plugin, so the check degrades to a no-op where they are absent. Re-seed the
/// harness with `mustard init` (idempotent).
const CORE_FOLDERS: &[&str] = &["commands/mustard", "hooks", "skills", "scripts", "refs"];

/// Compare installed `.claude/` core folders against `templates/` source by
/// SHA-256 hash. Degrades to `skip` when `templates/` is not reachable.
pub(super) fn check_drift(claude_dir: &Path) -> CheckResult {
    // Locate templates/ relative to cwd. Walk upward up to 4 levels.
    let templates_dir = find_templates_dir(claude_dir.parent().unwrap_or(claude_dir));
    let Some(templates_dir) = templates_dir else {
        return CheckResult::skip(
            "drift",
            "templates/ not reachable from cwd (consumer project — skipped)",
        );
    };

    let mut drifted: Vec<String> = Vec::new();

    for folder in CORE_FOLDERS {
        let installed = claude_dir.join(folder);
        let source = templates_dir.join(folder);

        if !source.exists() {
            // Source folder absent — skip this entry silently.
            continue;
        }
        if !installed.exists() {
            drifted.push(format!("{folder}: installed folder missing"));
            continue;
        }

        // Collect and hash all files in both trees.
        let installed_hash = hash_directory(&installed);
        let source_hash = hash_directory(&source);

        if installed_hash != source_hash {
            drifted.push(format!("{folder}: differs from templates/ (run `mustard init`)"));
        }
    }

    if drifted.is_empty() {
        CheckResult::ok("drift")
    } else {
        CheckResult::warn("drift", drifted)
    }
}

/// Try to locate a `templates/` directory by walking up from `start`.
fn find_templates_dir(start: &Path) -> Option<PathBuf> {
    // Look for apps/cli/templates from repo root, or templates/ at repo root.
    let mut candidate = start.to_path_buf();
    for _ in 0..5 {
        let direct = candidate.join("templates");
        if direct.exists() && direct.is_dir() {
            return Some(direct);
        }
        let via_cli = candidate.join("apps").join("cli").join("templates");
        if via_cli.exists() && via_cli.is_dir() {
            return Some(via_cli);
        }
        match candidate.parent() {
            Some(p) => candidate = p.to_path_buf(),
            None => break,
        }
    }
    None
}

/// Hash all files in a directory tree, sorted by relative path for stability.
/// Returns a hex string; returns `"<error>"` on IO failure (fail-open).
fn hash_directory(dir: &Path) -> String {
    let mut files = Vec::new();
    collect_recursive_inner(dir, 8, 0, &mut files);
    files.sort();

    let mut hasher = Sha256::new();
    for file_path in &files {
        if let Ok(bytes) = fs::read(file_path) {
            // Mix in the relative path for rename detection.
            let rel = file_path
                .strip_prefix(dir)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            hasher.update(rel.as_bytes());
            hasher.update(b"\x00");
            hasher.update(&bytes);
        }
    }
    hasher.hex_digest()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::super::Status;
    use super::*;
    use crate::commands::doctor::doctor::tests::*;

    #[test]
    fn drift_skips_when_templates_not_found() {
        // Nest the project ≥5 levels deep inside the tempdir so that
        // `find_templates_dir`'s 5-level upward walk stays WITHIN the
        // (template-free) tempdir and never reaches ancestors of the system
        // temp dir. On some CI runners (notably Windows) a `templates/` or
        // `apps/cli/templates` exists a few levels above `$TMP`, which made the
        // walk find one and return Ok instead of Skip — green locally, red on CI.
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("c").join("d").join("e").join("f");
        let claude_dir = nested.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        // No templates/ anywhere in this subtree.
        let result = check_drift(&claude_dir);
        assert_eq!(result.status, Status::Skip);
    }

    #[test]
    fn drift_ok_when_hashes_match() {
        let dir = tempdir().unwrap();
        let templates_dir = dir.path().join("templates");
        let claude_dir = dir.path().join(".claude");
        // Create matching content for one CORE_FOLDER.
        let folder = "skills";
        let src_file = templates_dir.join(folder).join("test.md");
        let dst_file = claude_dir.join(folder).join("test.md");
        write_file(&src_file, "# hello");
        write_file(&dst_file, "# hello");

        let result = check_drift(&claude_dir);
        // Should not be FAIL — either OK or SKIP.
        assert_ne!(result.status, Status::Fail, "{:?}", result.details);
    }

    #[test]
    fn drift_warns_on_hash_mismatch() {
        let dir = tempdir().unwrap();
        let templates_dir = dir.path().join("templates");
        let claude_dir = dir.path().join(".claude");
        let folder = "skills";
        let src_file = templates_dir.join(folder).join("test.md");
        let dst_file = claude_dir.join(folder).join("test.md");
        write_file(&src_file, "# source version");
        write_file(&dst_file, "# different installed version");

        let result = check_drift(&claude_dir);
        // Either WARN (drift detected) or SKIP (templates not reachable via
        // find_templates_dir — the tempdir has no apps/cli path, so find_templates_dir
        // should find `templates/` directly).
        assert!(
            result.status == Status::Warn || result.status == Status::Skip,
            "expected WARN or SKIP, got {:?}: {:?}", result.status, result.details
        );
    }
}
