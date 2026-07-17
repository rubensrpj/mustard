//! `scan-patterns-sweep` — delete every mustard-generated pattern skill under a
//! workspace BEFORE the enrich re-authors them, so each mold is written fresh
//! from the current exemplars with no bias from its previous text.
//!
//! Walks `<subproject>/.claude/skills/*-pattern/SKILL.md` across the tree and
//! removes the `*-pattern/` folder whenever its SKILL.md is
//! [`super::origin::is_mustard_generated`] (frontmatter `source: scan`).
//! A hand-authored or adopted mold (`source: manual`, or no frontmatter) is
//! PRESERVED. Never touches `.claude/scan-declined.json` (declines are about
//! clusters, not skills) nor any non-`-pattern` skill.
//!
//! Output: a byte-stable JSON `{removed:[…], preserved:[…]}` (paths sorted).
//! Fail-open per the `mustard-rt run` contract: an unreadable dir or file is
//! skipped, and any error degrades to a partial result with exit 0.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Folders whose descent never yields project skills — pruned so a large repo
/// sweep stays cheap and never wanders into dependency or VCS trees.
const PRUNE_DIRS: &[&str] = &["node_modules", "target", ".git", "dist", "build", "bin", "obj"];

#[derive(Serialize, Default)]
pub(crate) struct SweepReport {
    removed: Vec<String>,
    preserved: Vec<String>,
}

/// Run `scan-patterns-sweep`. Prints the JSON report; exit 0 always.
pub fn run(root: &Path) {
    let report = sweep(root);
    println!("{}", serde_json::to_string(&report).unwrap_or_else(|_| "{\"removed\":[],\"preserved\":[]}".to_string()));
}

/// The testable core: find every `*-pattern/SKILL.md`, delete the generated
/// ones, and return the byte-stable report (paths relative to `root`,
/// forward-slashed, sorted).
pub(crate) fn sweep(root: &Path) -> SweepReport {
    let mut molds: Vec<PathBuf> = Vec::new();
    collect_molds(root, &mut molds);
    molds.sort();

    let mut report = SweepReport::default();
    for skill_md in molds {
        let rel = rel_display(root, &skill_md);
        let generated = std::fs::read_to_string(&skill_md)
            .map(|t| super::origin::is_mustard_generated(&t))
            .unwrap_or(false);
        if !generated {
            report.preserved.push(rel);
            continue;
        }
        // Remove the whole `*-pattern/` folder (SKILL.md + any siblings).
        let Some(folder) = skill_md.parent() else {
            report.preserved.push(rel);
            continue;
        };
        if std::fs::remove_dir_all(folder).is_ok() {
            report.removed.push(rel);
        } else {
            report.preserved.push(rel); // could not delete → left in place
        }
    }
    report.removed.sort();
    report.preserved.sort();
    report
}

/// Recursively collect `*-pattern/SKILL.md` paths, pruning heavy/VCS dirs.
fn collect_molds(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if PRUNE_DIRS.contains(&name.as_str()) {
                continue;
            }
            // A `<slug>-pattern` dir under a `skills` parent holds one mold.
            if name.ends_with("-pattern") && parent_is_skills(&path) {
                let skill_md = path.join("SKILL.md");
                if skill_md.is_file() {
                    out.push(skill_md);
                }
                continue; // no skills nest inside a mold folder
            }
            collect_molds(&path, out);
        }
    }
}

/// `true` when `path`'s parent directory is named `skills` (so `path` is a mold
/// folder sitting at `…/.claude/skills/<slug>-pattern`).
fn parent_is_skills(path: &Path) -> bool {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n == "skills")
        .unwrap_or(false)
}

/// `path` relative to `root`, forward-slashed. Falls back to the full path.
fn rel_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_mold(root: &Path, subproject: &str, slug: &str, source: &str) -> PathBuf {
        let dir = root.join(subproject).join(".claude").join("skills").join(format!("{slug}-pattern"));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("SKILL.md");
        std::fs::write(&p, format!("---\nname: {slug}-pattern\nsource: {source}\n---\n\n## Purpose\nbody\n")).unwrap();
        p
    }

    #[test]
    fn removes_generated_preserves_manual() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_mold(root, "apps/api", "api-service", "scan");
        write_mold(root, "apps/api", "api-legacy", "manual");
        write_mold(root, "packages/core", "core-store", "scan");

        let report = sweep(root);
        assert_eq!(report.removed, vec![
            "apps/api/.claude/skills/api-service-pattern/SKILL.md",
            "packages/core/.claude/skills/core-store-pattern/SKILL.md",
        ]);
        assert_eq!(report.preserved, vec!["apps/api/.claude/skills/api-legacy-pattern/SKILL.md"]);
        // Generated folders gone, manual one intact.
        assert!(!root.join("apps/api/.claude/skills/api-service-pattern").exists());
        assert!(root.join("apps/api/.claude/skills/api-legacy-pattern/SKILL.md").exists());
    }

    #[test]
    fn preserves_mold_without_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let dir = root.join("apps/api/.claude/skills/api-old-pattern");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "# legacy hand skill\n\nbody\n").unwrap();

        let report = sweep(root);
        assert!(report.removed.is_empty(), "no frontmatter → not generated → preserved");
        assert_eq!(report.preserved.len(), 1);
        assert!(dir.join("SKILL.md").exists());
    }

    #[test]
    fn empty_tree_is_fail_open() {
        let tmp = tempfile::tempdir().unwrap();
        let report = sweep(tmp.path());
        assert!(report.removed.is_empty() && report.preserved.is_empty());
    }

    #[test]
    fn prunes_node_modules() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // A generated mold buried in node_modules must NOT be swept.
        write_mold(root, "node_modules/pkg", "pkg-thing", "scan");
        write_mold(root, "apps/api", "api-service", "scan");
        let report = sweep(root);
        assert_eq!(report.removed, vec!["apps/api/.claude/skills/api-service-pattern/SKILL.md"]);
    }
}
