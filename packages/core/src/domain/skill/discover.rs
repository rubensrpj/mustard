//! SKILL.md filesystem discovery — the shared low-level walk.
//!
//! Every skill discoverer (`mustard-rt run skill-resolve`, `mustard-rt run
//! skills graph/orphans/validate/list`) needs the same primitive: "list the
//! `SKILL.md` files one directory level under a skills root". Each call-site
//! then layers its own parse / dedup / tolerance policy on top — those differ
//! deliberately (resolve requires parseable frontmatter; graph/orphans tolerate
//! a missing one and fall back to the directory name) and stay at the
//! call-site. The **walk itself** has one owner here.

use std::path::{Path, PathBuf};

use crate::io::fs;

/// Collect every `<root>/<child>/SKILL.md` that exists — exactly one directory
/// level under `root`.
///
/// Returns an empty vec when `root` is absent or unreadable (fail-open). Order
/// follows [`fs::read_dir`] (filesystem order, no sort guarantee).
#[must_use]
pub fn collect_skill_md(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let candidate = entry.path.join("SKILL.md");
        if candidate.exists() {
            out.push(candidate);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn collects_skill_md_one_level_down() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // <root>/alpha/SKILL.md — found.
        std::fs::create_dir_all(root.join("alpha")).unwrap();
        std::fs::write(root.join("alpha").join("SKILL.md"), b"x").unwrap();
        // <root>/beta/ with no SKILL.md — skipped.
        std::fs::create_dir_all(root.join("beta")).unwrap();
        // <root>/gamma/nested/SKILL.md — two levels down, NOT found.
        std::fs::create_dir_all(root.join("gamma").join("nested")).unwrap();
        std::fs::write(
            root.join("gamma").join("nested").join("SKILL.md"),
            b"x",
        )
        .unwrap();

        let found = collect_skill_md(root);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("alpha/SKILL.md") || found[0].ends_with("alpha\\SKILL.md"));
    }

    #[test]
    fn missing_root_returns_empty() {
        let dir = tempdir().unwrap();
        assert!(collect_skill_md(&dir.path().join("nope")).is_empty());
    }
}
