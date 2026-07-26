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
//! - The placeholder table of `plugin/refs/agent-prompt/agent-prompt.md` must
//!   document every key in `TEMPLATE_PLACEHOLDERS`. A placeholder the renderer
//!   fills and the ref never mentions is material an author cannot know how to
//!   supply — which is how a channel ships and stays unused.
//!
//! Reads outside the crate fail open (skip) per this codebase's test convention:
//! a workspace root this test cannot resolve is a fact about the checkout, not
//! about the shipped surface.

use std::fs;
use std::path::{Path, PathBuf};

use mustard_rt::commands::agent::render::TEMPLATE_PLACEHOLDERS;

/// The workspace root, from `apps/rt` up two levels. `None` when the layout
/// cannot be walked — callers skip rather than fail on it.
fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

/// The placeholder keys the ref's table documents, located by SHAPE — any table
/// row whose FIRST cell is a single backticked `{token}` — so the surrounding
/// prose can be reworded, reordered or retitled without touching this guard.
///
/// The first cell only: a `{placeholder}` named inside a Notes column is a
/// cross-reference, not a documented row, and counting it would let a mention in
/// passing stand in for an entry.
fn documented_placeholders(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter_map(|line| {
            let cell = line.trim().trim_start_matches('|').split('|').next()?.trim();
            let token = cell.strip_prefix('`')?.strip_suffix('`')?;
            (token.starts_with('{') && token.ends_with('}')).then(|| token.to_string())
        })
        .collect()
}

/// The ref's placeholder table and the renderer's substitution list must be the
/// SAME SET.
///
/// A set, deliberately, and never the count the prose states: a reworded
/// sentence would then break this test for a cosmetic reason, which is exactly
/// the false red the spec this guard ships with exists to prevent. The size is a
/// consequence of the set — it is never the claim being checked.
///
/// Both directions matter. An UNDOCUMENTED key is material the author of a spec
/// cannot know how to supply, so the channel ships and nobody uses it (that was
/// the live drift when this guard was written: `{conversation_material}` was
/// being substituted and the table did not mention it). A STALE row is worse in
/// the other direction — it sends a reader to configure something the renderer
/// no longer fills.
#[test]
fn agent_prompt_ref_documents_every_placeholder() {
    let Some(root) = workspace_root() else {
        eprintln!("[skip] cannot resolve workspace root from CARGO_MANIFEST_DIR");
        return;
    };
    let path = root.join("plugin/refs/agent-prompt/agent-prompt.md");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("placeholder contract {} is unreadable: {e}", path.display()));

    let documented = documented_placeholders(&text);
    assert!(
        !documented.is_empty(),
        "{} must keep a table whose first column is the `{{placeholder}}` keys — the contract \
         whoever plans a wave reads to know what a prompt carries",
        path.display()
    );

    let missing: Vec<&&str> = TEMPLATE_PLACEHOLDERS
        .iter()
        .filter(|key| !documented.iter().any(|d| d == *key))
        .collect();
    assert!(
        missing.is_empty(),
        "{} does not document {missing:?}. The renderer substitutes these, so a spec author \
         reading the ref cannot tell what reaches a wave — add one table row per key (source + \
         when it is empty). Documented: {documented:?}",
        path.display()
    );

    let stale: Vec<&String> = documented
        .iter()
        .filter(|d| !TEMPLATE_PLACEHOLDERS.contains(&d.as_str()))
        .collect();
    assert!(
        stale.is_empty(),
        "{} documents {stale:?}, which the renderer no longer substitutes — a reader would \
         supply material nothing reads. Renderer keys: {TEMPLATE_PLACEHOLDERS:?}",
        path.display()
    );
}
