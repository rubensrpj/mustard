//! Drift guards pinning the SHIPPED `plugin/agents/*.md` surface to the code.
//!
//! These files carry keys Claude Code reads, not Mustard: nothing in this
//! workspace fails to compile when one is dropped or renamed, the behaviour just
//! changes silently at runtime for every installation. That is the same failure
//! mode `command_frontmatter.rs` guards for the instruction surface, and these
//! tests are the same kind of ratchet for the agent surface.
//!
//! What is locked, and why it matters:
//!
//! - `mustard-impl` declares `isolation: worktree`. That one key is the whole
//!   reason the file exists: without it every writing role of a dispatch round
//!   lands back in the SHARED working tree, where two implementers overwrite
//!   each other and the per-wave commit boundary stops meaning anything. The
//!   loss is silent — the dispatch still succeeds.
//! - Its `name` must equal the file stem. Claude Code does not require the two
//!   to match, which is exactly why they drift; the harness resolves plugin
//!   agents as `mustard:<stem>`, so a `name` that disagrees with the filename
//!   yields a `subagent_type` nobody answers and the dispatch falls back to the
//!   shared built-in agent.
//! - The canonical role→`subagent_type` map in
//!   `plugin/refs/agent-prompt/agent-prompt.md` must name the type the code
//!   actually returns. That ref is what the orchestrator reads to dispatch, so
//!   prose that contradicts `recommended_subagent_type` sends the reader to a
//!   different agent than the pipeline uses — and no compiler ever sees it.
//!
//! Reads outside the crate fail open (skip) per this codebase's test convention:
//! a workspace root this test cannot resolve is a fact about the checkout, not
//! about the shipped surface.

use std::fs;
use std::path::{Path, PathBuf};

use mustard_rt::commands::agent::render::recommended_subagent_type;

/// The workspace root, from `apps/rt` up two levels. `None` when the layout
/// cannot be walked — callers skip rather than fail on it.
fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

/// The frontmatter block (between the first two `---` fences) of a shipped
/// plugin agent. Panics with the path when the file or the block is missing —
/// in a ratchet an unreadable input is a failure, never a silent pass. Only the
/// unresolvable-root case above is allowed to skip.
fn agent_frontmatter(root: &Path, agent: &str) -> String {
    let path = root.join(format!("plugin/agents/{agent}.md"));
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("shipped agent {} is unreadable: {e}", path.display()));
    let body = text
        .strip_prefix("---")
        .unwrap_or_else(|| panic!("{} must open with a frontmatter fence", path.display()));
    let end = body
        .find("\n---")
        .unwrap_or_else(|| panic!("{} has no closing frontmatter fence", path.display()));
    body[..end].to_string()
}

/// Read one `key: value` line out of a frontmatter block.
fn field<'a>(frontmatter: &'a str, key: &str) -> Option<&'a str> {
    frontmatter.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim())
    })
}

#[test]
fn impl_agent_declares_worktree_isolation() {
    let Some(root) = workspace_root() else {
        eprintln!("[skip] cannot resolve workspace root from CARGO_MANIFEST_DIR");
        return;
    };
    let stem = "mustard-impl";
    let fm = agent_frontmatter(&root, stem);

    assert_eq!(
        field(&fm, "isolation"),
        Some("worktree"),
        "plugin/agents/{stem}.md must declare `isolation: worktree`. Without it every writing \
         role of a dispatch round runs in the SHARED working tree, so parallel waves overwrite \
         each other and no wave commit has a real boundary — and the dispatch reports success \
         either way. Frontmatter:\n{fm}"
    );
    assert_eq!(
        field(&fm, "name"),
        Some(stem),
        "plugin/agents/{stem}.md must declare `name: {stem}`. The harness resolves plugin agents \
         as `mustard:<file stem>`; a `name` that disagrees with the filename names a \
         subagent_type nobody answers, and the dispatch silently falls back to the shared \
         built-in agent. Frontmatter:\n{fm}"
    );
}

#[test]
fn agent_prompt_ref_matches_subagent_map() {
    let Some(root) = workspace_root() else {
        eprintln!("[skip] cannot resolve workspace root from CARGO_MANIFEST_DIR");
        return;
    };
    let path = root.join("plugin/refs/agent-prompt/agent-prompt.md");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("canonical map {} is unreadable: {e}", path.display()));

    // The writing-role row of the canonical table, located by shape (the role
    // label plus the catch-all wording) rather than by line number, so the ref
    // can be reworded around it without breaking the guard.
    let row = text
        .lines()
        .find(|line| line.starts_with('|') && line.contains("`impl`") && line.contains("any other"))
        .unwrap_or_else(|| {
            panic!(
                "{} must keep a role table row for `impl` / any other — the canonical \
                 role→subagent_type map other refs point at",
                path.display()
            )
        });

    let expected = recommended_subagent_type("impl");
    assert!(
        row.contains(&format!("`{expected}`")),
        "{} documents a different subagent_type for writing roles than the code dispatches. \
         `recommended_subagent_type(\"impl\")` returns `{expected}`; the row reads:\n{row}",
        path.display()
    );
    assert!(
        !row.contains("general-purpose"),
        "{} still routes writing roles to the shared built-in `general-purpose` in its role \
         table. That agent has no `isolation: worktree`, so a reader following the map puts \
         every wave back in one working tree. Row:\n{row}",
        path.display()
    );
}
