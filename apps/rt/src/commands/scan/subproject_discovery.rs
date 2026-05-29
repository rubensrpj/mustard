//! Canonical subproject discovery — the single source of truth shared by
//! `sync-detect` and `sync-registry`.
//!
//! ## Why one walk, not two
//!
//! Two divergent discovery walks used to live side by side:
//!
//! * `sync_detect::scan_for_subprojects` — a **build-manifest BFS**: a directory
//!   is a subproject when it carries a recognised build manifest (`Cargo.toml`
//!   with `[package]`, `package.json`, `*.csproj`, `go.mod`, `pyproject.toml`,
//!   `pubspec.yaml`). Language-agnostic; never requires `mustard init`.
//! * `sync_entity_registry::discover_subprojects` — a **`CLAUDE.md`-only BFS**:
//!   a directory was a subproject only when it carried a `CLAUDE.md`.
//!
//! They disagreed: a manifest-bearing subproject *without* a `CLAUDE.md` was
//! discovered by `sync-detect` but silently **filtered out** by the registry, so
//! the registry could be strictly smaller than — and inconsistent with — what
//! `sync-detect` reported.
//!
//! ## The decision
//!
//! The **build-manifest BFS is the source of truth**. It is the agnostic signal
//! that a directory is an independent buildable unit, it matches what
//! `sync-detect` already emits (and what the `maint` commands parse from that
//! output), and it does not penalise a subproject for lacking a `CLAUDE.md` (a
//! `CLAUDE.md` marks a *mustard-initialised* project, not a *buildable* one).
//!
//! The `CLAUDE.md`-aware behaviour is preserved — but as an explicit
//! [`DiscoveryOptions::require_claude_md`] **strategy flag** on the one
//! function, not as a second implementation. No current caller sets it; it
//! exists so a future consumer that genuinely wants "only mustard-initialised
//! subprojects" can ask for that semantics without forking the walk.
//!
//! Both behaviours share one ignore-list, one BFS, one `mustard.json`
//! override pass, and one single-root fallback.

use mustard_core::io::fs;
use std::path::Path;

/// One discovered subproject: its leaf name and its repo-root-relative path
/// (forward slashes; `.` for a single-root project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subproject {
    /// Leaf directory name (or the root dir name for `.`).
    pub name: String,
    /// Path relative to the monorepo root, forward-slashed.
    pub rel_path: String,
}

/// Strategy knobs for [`discover_subprojects`].
///
/// The default ([`DiscoveryOptions::default`]) is the agnostic build-manifest
/// BFS — the source of truth used by both `sync-detect` and `sync-registry`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiscoveryOptions {
    /// When `true`, a manifest-bearing directory is only kept if it *also*
    /// carries a `CLAUDE.md` (the legacy registry semantics). Default `false`:
    /// the build manifest alone qualifies a subproject.
    pub require_claude_md: bool,
}

/// Directory names never descended into during discovery.
const IGNORE: &[&str] = &[
    "node_modules",
    "bin",
    "obj",
    "dist",
    ".next",
    "_backup",
    "migrations",
    ".claude",
    ".git",
];

/// Max BFS depth (matches both legacy walks).
const MAX_DEPTH: usize = 3;

/// Manifests that — beyond the [`has_build_manifest`] set — qualify the *repo
/// root itself* as a single subproject when the BFS finds no nested manifest
/// dir. Java / PHP / `requirements.txt`-only roots are not part of the nested
/// BFS signal (so `sync-detect`'s emitted subproject list is unchanged), but a
/// single-root project of those stacks should still be scannable by the
/// registry. This list only matters in the empty-discovery fallback path.
const EXTRA_ROOT_FALLBACK_MANIFESTS: &[&str] =
    &["requirements.txt", "pom.xml", "build.gradle", "composer.json"];

/// `true` if the repo root qualifies as a single-root project: a recognised
/// build manifest (the nested-BFS set) or one of the broader single-root
/// manifests.
fn root_has_fallback_manifest(root: &Path) -> bool {
    has_build_manifest(root)
        || EXTRA_ROOT_FALLBACK_MANIFESTS
            .iter()
            .any(|m| root.join(m).is_file())
}

/// Read a file, returning an empty string on any error.
fn read_safe(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// `true` if a `*.csproj` (or other glob with a single `*`) matches an entry
/// directly inside `dir`; non-glob patterns test for direct existence.
fn file_exists(dir: &Path, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return dir.join(pattern).exists();
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() != 2 {
        return false;
    }
    fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .into_iter()
            .any(|e| e.file_name.starts_with(parts[0]) && e.file_name.ends_with(parts[1]))
    })
}

/// `true` if `dir` directly contains a recognised build manifest — the
/// language-agnostic signal that the directory is an independent subproject.
/// A `Cargo.toml` counts only when it declares a `[package]`: a virtual
/// workspace root (`[workspace]` only) is not itself a subproject.
pub fn has_build_manifest(dir: &Path) -> bool {
    if dir.join("package.json").is_file()
        || dir.join("go.mod").is_file()
        || dir.join("pyproject.toml").is_file()
        || dir.join("pubspec.yaml").is_file()
        || file_exists(dir, "*.csproj")
    {
        return true;
    }
    let cargo = dir.join("Cargo.toml");
    cargo.is_file() && read_safe(&cargo).contains("[package]")
}

/// Apply the `mustard.json` override to a detected path list: drop excluded
/// entries, append included ones not already present.
fn apply_overrides(root: &Path, paths: &mut Vec<String>) {
    let (exclude, include) = mustard_core::ProjectConfig::load(root).subproject_overrides();
    paths.retain(|p| !exclude.contains(p));
    for inc in include {
        if !inc.is_empty() && !paths.contains(&inc) {
            paths.push(inc);
        }
    }
}

/// BFS for directories carrying a build manifest (max depth 3). When
/// `require_claude_md` is set, a manifest dir only qualifies if it *also* has a
/// `CLAUDE.md`; the walk still descends into manifest-less branches so a nested
/// initialised subproject deeper in the tree is still reachable.
fn walk(
    abs_dir: &Path,
    rel_dir: &str,
    depth: usize,
    require_claude_md: bool,
    out: &mut Vec<String>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    if depth > 0 && has_build_manifest(abs_dir) {
        if !require_claude_md || abs_dir.join("CLAUDE.md").exists() {
            out.push(rel_dir.replace('\\', "/"));
            return;
        }
        // Manifest present but `CLAUDE.md` required and absent: do not record
        // this dir, and (matching the build-manifest walk) stop descending —
        // a buildable unit is a leaf for discovery purposes.
        return;
    }
    let Ok(entries) = fs::read_dir(abs_dir) else {
        return;
    };
    for e in entries {
        if !e.is_dir {
            continue;
        }
        if e.file_name.starts_with('.') || IGNORE.contains(&e.file_name.as_str()) {
            continue;
        }
        let next_rel = if rel_dir.is_empty() {
            e.file_name.clone()
        } else {
            format!("{rel_dir}/{}", e.file_name)
        };
        walk(&e.path, &next_rel, depth + 1, require_claude_md, out);
    }
}

/// Derive a subproject's leaf name from its relative path. `.` resolves to the
/// root directory's own name (falling back to `"project"`).
fn derive_name(root: &Path, rel_path: &str) -> String {
    if rel_path == "." {
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string()
    } else {
        rel_path.rsplit('/').next().unwrap_or(rel_path).to_string()
    }
}

/// Discover the subprojects of the monorepo rooted at `root`.
///
/// The canonical walk (build-manifest BFS, agnostic) shared by both
/// `sync-detect` and `sync-registry`. The pipeline:
///
/// 1. BFS (max depth 3) for directories carrying a build manifest, honouring
///    [`DiscoveryOptions::require_claude_md`].
/// 2. Apply the `mustard.json` `subprojects.exclude` / `.include` override.
/// 3. Single-root fallback: if nothing was found but the root itself carries a
///    build manifest (or a `CLAUDE.md`), treat the root as the one subproject
///    `.`.
///
/// Entries with a non-existent path (e.g. a stale `mustard.json` `include`) are
/// dropped so callers always receive directories that exist.
#[must_use]
pub fn discover_subprojects(root: &Path, opts: &DiscoveryOptions) -> Vec<Subproject> {
    let mut rel_paths: Vec<String> = Vec::new();
    walk(root, "", 0, opts.require_claude_md, &mut rel_paths);
    apply_overrides(root, &mut rel_paths);

    if rel_paths.is_empty() {
        // Single-root fallback: a project with no nested manifest dirs. A
        // `CLAUDE.md` or a root-level build manifest both qualify the root.
        let root_qualifies = root.join("CLAUDE.md").exists() || root_has_fallback_manifest(root);
        if root_qualifies {
            rel_paths.push(".".to_string());
        }
    }

    rel_paths
        .into_iter()
        .filter_map(|rel| {
            let abs = if rel == "." {
                root.to_path_buf()
            } else {
                root.join(&rel)
            };
            if !abs.is_dir() {
                return None;
            }
            Some(Subproject {
                name: derive_name(root, &rel),
                rel_path: rel,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn finds_manifest_dirs() {
        let dir = tempdir().unwrap();
        let app = dir.path().join("apps").join("web");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("package.json"), "{}").unwrap();
        let found = discover_subprojects(dir.path(), &DiscoveryOptions::default());
        assert_eq!(
            found,
            vec![Subproject {
                name: "web".to_string(),
                rel_path: "apps/web".to_string()
            }]
        );
    }

    #[test]
    fn ignores_manifestless_dir_even_with_claude_md() {
        // A payload dir (e.g. `templates/`) carries a `CLAUDE.md` but no build
        // manifest — it must not be mistaken for a subproject.
        let dir = tempdir().unwrap();
        let payload = dir.path().join("apps").join("cli").join("templates");
        std::fs::create_dir_all(&payload).unwrap();
        std::fs::write(payload.join("CLAUDE.md"), "# template").unwrap();
        let crate_dir = dir.path().join("apps").join("cli");
        std::fs::write(crate_dir.join("Cargo.toml"), "[package]\nname = \"cli\"").unwrap();
        let found = discover_subprojects(dir.path(), &DiscoveryOptions::default());
        assert_eq!(
            found,
            vec![Subproject {
                name: "cli".to_string(),
                rel_path: "apps/cli".to_string()
            }]
        );
    }

    #[test]
    fn default_strategy_keeps_manifest_without_claude_md() {
        // The core consistency guarantee: a manifest-bearing subproject with NO
        // CLAUDE.md is discovered (the old registry-only walk dropped it).
        let dir = tempdir().unwrap();
        let app = dir.path().join("services").join("api");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("go.mod"), "module api\n").unwrap();
        let found = discover_subprojects(dir.path(), &DiscoveryOptions::default());
        assert_eq!(
            found,
            vec![Subproject {
                name: "api".to_string(),
                rel_path: "services/api".to_string()
            }]
        );
    }

    #[test]
    fn require_claude_md_filters_manifest_without_claude_md() {
        let dir = tempdir().unwrap();
        let bare = dir.path().join("services").join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("go.mod"), "module bare\n").unwrap();
        let inited = dir.path().join("services").join("inited");
        std::fs::create_dir_all(&inited).unwrap();
        std::fs::write(inited.join("go.mod"), "module inited\n").unwrap();
        std::fs::write(inited.join("CLAUDE.md"), "# inited").unwrap();

        let found = discover_subprojects(
            dir.path(),
            &DiscoveryOptions {
                require_claude_md: true,
            },
        );
        assert_eq!(
            found,
            vec![Subproject {
                name: "inited".to_string(),
                rel_path: "services/inited".to_string()
            }]
        );
    }

    #[test]
    fn has_build_manifest_rejects_virtual_workspace_root() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = []").unwrap();
        assert!(!has_build_manifest(dir.path()));
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
        assert!(has_build_manifest(dir.path()));
    }

    #[test]
    fn applies_mustard_json_overrides() {
        let dir = tempdir().unwrap();
        // Two real manifest dirs; mustard.json drops one and adds an explicit one.
        for sub in ["apps/keep", "apps/drop"] {
            let p = dir.path().join(sub);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("package.json"), "{}").unwrap();
        }
        let edge = dir.path().join("infra").join("edge");
        std::fs::create_dir_all(&edge).unwrap();
        // mustard.json lives at the project root (not `.claude/`).
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"subprojects":{"exclude":["apps/drop"],"include":["infra/edge"]}}"#,
        )
        .unwrap();
        let found = discover_subprojects(dir.path(), &DiscoveryOptions::default());
        let rels: Vec<&str> = found.iter().map(|s| s.rel_path.as_str()).collect();
        assert_eq!(rels, vec!["apps/keep", "infra/edge"]);
    }

    #[test]
    fn falls_back_to_root_for_single_root_project() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
        let found = discover_subprojects(dir.path(), &DiscoveryOptions::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rel_path, ".");
    }

    #[test]
    fn falls_back_to_root_with_only_claude_md() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "# root").unwrap();
        let found = discover_subprojects(dir.path(), &DiscoveryOptions::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rel_path, ".");
    }

    #[test]
    fn drops_nonexistent_override_include() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
        // `include` points at a path that does not exist on disk. mustard.json
        // lives at the project root (not `.claude/`).
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"subprojects":{"include":["ghost/dir"]}}"#,
        )
        .unwrap();
        let found = discover_subprojects(dir.path(), &DiscoveryOptions::default());
        // Root fallback does NOT fire (the walk found `ghost/dir`), but the
        // non-existent include is dropped, leaving an empty result.
        assert!(found.iter().all(|s| s.rel_path != "ghost/dir"));
    }
}
