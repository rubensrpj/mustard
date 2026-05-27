//! `wikilink_footer` — PostToolUse(Write|Edit) auto-footer renderer for
//! `.claude/{memory,knowledge,spec}/**/*.md` files.
//!
//! ## Scope (W3E, wave-11-rt of `2026-05-26-no-sqlite-git-source-of-truth`)
//!
//! After a Write/Edit lands on a markdown file under one of the three canonical
//! atomic-md trees, this `Observer` reads the file, recomputes the auto-footer
//! via [`mustard_core::atomic_md::wikilink::render_footer`], and rewrites the
//! file atomically when the rendered output differs from the on-disk content.
//!
//! The footer block lives between
//! `<!-- wikilinks-footer-start -->` / `<!-- wikilinks-footer-end -->`
//! sentinels. Calling `render_footer` twice on the same body produces identical
//! output — re-firing the hook on the just-written file is therefore a no-op.
//!
//! ## Design
//!
//! - **Pure `Observer`** — never blocks, never injects. Outcome equivalence in
//!   the dispatcher is `Allow` (no `Check` is registered, so no verdict is
//!   produced).
//! - **All parser/render logic lives in `mustard_core::atomic_md::wikilink`.**
//!   This module owns only the "when to run" decision and the post-write
//!   rewrite. It does not duplicate the `[[ ]]` scanner or the sentinel
//!   handling — the W3E AC checks for that explicitly.
//! - **Fail-open.** A missing file, a non-markdown path, or a write error all
//!   resolve to no-op.

use mustard_core::atomic_md::wikilink::render_footer;
use mustard_core::fs as core_fs;
use mustard_core::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

/// The auto-footer renderer.
pub struct WikilinkFooter;

/// The `file_path` of a Write/Edit invocation, mirrors `post_edit::file_path_of`.
fn file_path_of(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Resolve the project root the rewrite should be anchored at.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Detect whether `path` lives under one of `.claude/{memory,knowledge,spec}/`
/// **and** ends in `.md`. Works with forward- or back-slash separators.
fn is_atomic_md_path(path: &Path) -> bool {
    // Normalise to forward slashes for the substring check; the path may be
    // absolute (`C:\…`) or relative (`.claude/memory/foo.md`).
    let normalised = path.to_string_lossy().replace('\\', "/");
    if !normalised.ends_with(".md") {
        return false;
    }
    normalised.contains("/.claude/memory/")
        || normalised.contains("/.claude/knowledge/")
        || normalised.contains("/.claude/spec/")
        // Also accept the bare-prefix case (relative path that does not start
        // with `/`): e.g. `.claude/memory/foo.md`.
        || normalised.starts_with(".claude/memory/")
        || normalised.starts_with(".claude/knowledge/")
        || normalised.starts_with(".claude/spec/")
}

/// Compute the search-dir set the renderer resolves wikilinks against.
///
/// All three canonical trees are searched in order: `memory/`, `knowledge/`,
/// `spec/`. Missing directories are tolerated by `resolve` (it returns `None`).
fn search_dirs(project: &str) -> Vec<PathBuf> {
    let claude = match ClaudePaths::for_project(project) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let claude_dir = claude.claude_dir();
    vec![
        claude_dir.join("memory"),
        claude_dir.join("knowledge"),
        claude_dir.join("spec"),
    ]
}

/// Render the footer for `path` and rewrite atomically when the result differs
/// from the current on-disk content. Idempotent by construction.
fn rewrite_if_changed(path: &Path, project: &str) {
    let Ok(current) = core_fs::read_to_string(path) else {
        return;
    };
    let dirs = search_dirs(project);
    let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    let rendered = render_footer(&current, &refs);
    if rendered == current {
        return;
    }
    let _ = core_fs::write_atomic(path, rendered.as_bytes());
}

impl Observer for WikilinkFooter {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if !matches!(input.tool_name.as_deref(), Some("Write" | "Edit")) {
            return;
        }
        let Some(raw_path) = file_path_of(input) else {
            return;
        };
        let path = PathBuf::from(&raw_path);
        if !is_atomic_md_path(&path) {
            return;
        }
        let project = project_dir(input, ctx);
        rewrite_if_changed(&path, &project);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::model::contract::Trigger;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        }
    }

    fn write_input(path: &str) -> HookInput {
        HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": path }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        }
    }

    #[test]
    fn is_atomic_md_path_accepts_memory_knowledge_spec() {
        assert!(is_atomic_md_path(Path::new(
            "/repo/.claude/memory/foo.md"
        )));
        assert!(is_atomic_md_path(Path::new(
            "/repo/.claude/knowledge/bar.md"
        )));
        assert!(is_atomic_md_path(Path::new(
            "/repo/.claude/spec/2026-05-26/spec.md"
        )));
        assert!(is_atomic_md_path(Path::new(".claude/memory/baz.md")));
    }

    #[test]
    fn is_atomic_md_path_rejects_unrelated_paths() {
        assert!(!is_atomic_md_path(Path::new("/repo/src/main.rs")));
        assert!(!is_atomic_md_path(Path::new("/repo/README.md")));
        assert!(!is_atomic_md_path(Path::new(".claude/agents/foo.md")));
        assert!(!is_atomic_md_path(Path::new(
            "/repo/.claude/memory/foo.txt"
        )));
    }

    #[test]
    fn observe_rewrites_file_with_resolved_and_orphan_links() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let memory_dir = project.join(".claude").join("memory");
        fs::create_dir_all(&memory_dir).unwrap();

        // Target wikilink resolves under memory/.
        fs::write(memory_dir.join("bar.md"), "# bar\n").unwrap();

        // The file we are simulating a Write on.
        let foo = memory_dir.join("foo.md");
        let body = "# foo\n\nLinks to [[bar]] and [[ghost]].\n";
        fs::write(&foo, body).unwrap();

        WikilinkFooter.observe(
            &write_input(foo.to_str().unwrap()),
            &ctx(project.to_str().unwrap()),
        );

        let rewritten = fs::read_to_string(&foo).unwrap();
        assert!(
            rewritten.contains("<!-- wikilinks-footer-start -->"),
            "footer start sentinel must be present"
        );
        assert!(
            rewritten.contains("<!-- wikilinks-footer-end -->"),
            "footer end sentinel must be present"
        );
        assert!(
            rewritten.contains("[bar](bar.md)"),
            "resolved link must be clickable: {rewritten}"
        );
        assert!(
            rewritten.contains("⚠ não resolvido"),
            "orphan annotation must be present: {rewritten}"
        );
    }

    #[test]
    fn observe_is_idempotent() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let memory_dir = project.join(".claude").join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        fs::write(memory_dir.join("bar.md"), "# bar\n").unwrap();

        let foo = memory_dir.join("foo.md");
        fs::write(&foo, "Body with [[bar]].\n").unwrap();

        // First fire — footer is appended.
        WikilinkFooter.observe(
            &write_input(foo.to_str().unwrap()),
            &ctx(project.to_str().unwrap()),
        );
        let first = fs::read_to_string(&foo).unwrap();

        // Second fire on the now-stamped file must produce identical content.
        WikilinkFooter.observe(
            &write_input(foo.to_str().unwrap()),
            &ctx(project.to_str().unwrap()),
        );
        let second = fs::read_to_string(&foo).unwrap();

        assert_eq!(first, second, "second fire must be a no-op");
    }

    #[test]
    fn observe_removes_footer_when_links_disappear() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let memory_dir = project.join(".claude").join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        fs::write(memory_dir.join("bar.md"), "# bar\n").unwrap();

        let foo = memory_dir.join("foo.md");
        fs::write(&foo, "Body with [[bar]].\n").unwrap();

        // Render the footer once.
        WikilinkFooter.observe(
            &write_input(foo.to_str().unwrap()),
            &ctx(project.to_str().unwrap()),
        );
        let with_footer = fs::read_to_string(&foo).unwrap();
        assert!(with_footer.contains("<!-- wikilinks-footer-start -->"));

        // Now strip every wikilink and re-fire.
        fs::write(&foo, "Body without any wikilinks at all.\n").unwrap();
        WikilinkFooter.observe(
            &write_input(foo.to_str().unwrap()),
            &ctx(project.to_str().unwrap()),
        );
        let stripped = fs::read_to_string(&foo).unwrap();
        assert!(
            !stripped.contains("<!-- wikilinks-footer-start -->"),
            "footer must be removed when no links remain: {stripped}"
        );
    }

    #[test]
    fn observe_skips_non_atomic_paths() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let other_dir = project.join("src");
        fs::create_dir_all(&other_dir).unwrap();
        let target = other_dir.join("README.md");
        let body = "References [[ghost]].";
        fs::write(&target, body).unwrap();

        WikilinkFooter.observe(
            &write_input(target.to_str().unwrap()),
            &ctx(project.to_str().unwrap()),
        );

        // Unchanged — the path is not under .claude/{memory,knowledge,spec}/.
        let after = fs::read_to_string(&target).unwrap();
        assert_eq!(body, after);
    }

    #[test]
    fn observe_skips_non_write_edit_tools() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let memory_dir = project.join(".claude").join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        let foo = memory_dir.join("foo.md");
        let body = "Body with [[ghost]].";
        fs::write(&foo, body).unwrap();

        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "file_path": foo.to_str().unwrap() }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        WikilinkFooter.observe(&input, &ctx(project.to_str().unwrap()));

        // Unchanged — wrong tool.
        let after = fs::read_to_string(&foo).unwrap();
        assert_eq!(body, after);
    }
}
