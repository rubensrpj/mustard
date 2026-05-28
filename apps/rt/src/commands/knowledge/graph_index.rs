//! `mustard-rt run graph-index` — build the concept-node graph index.
//!
//! Walks `<project_root>/.claude/graph/`, parses every concept-node markdown
//! file's frontmatter `id` plus inline `[[id]]` edges, builds the `id → path`
//! lookup table + adjacency map, validates (orphan / cycle → warning), writes
//! the `index.md` MOC, and (best-effort) injects `aliases:[id]` into matching
//! `.claude/skills/*/SKILL.md` files.
//!
//! Output (stdout, byte-stable pretty JSON):
//!
//! ```json
//! {
//!   "nodes": { "id": "relative/path.md", ... },
//!   "edges": { "id": ["edge-target-id", ...] },
//!   "warnings": ["warning: orphan edge a -> b", ...],
//!   "aliased_skills": ["_root.skill.foo", ...]
//! }
//! ```
//!
//! Exit code is always `0` (fail-open). A missing `.claude/graph/` tree
//! degrades to an empty index — the Wave 4 resolver then sees zero edges
//! and treats the closure as the singleton `{scope}`.

use crate::shared::context::project_dir;
use crate::commands::scan::graph;
use std::path::PathBuf;

/// Run `mustard-rt run graph-index`. Fail-open by design.
pub fn run() {
    let project = PathBuf::from(project_dir());
    let index = graph::build_index(&project);
    let pretty = serde_json::to_string_pretty(&index).unwrap_or_else(|_| "{}".to_string());
    println!("{pretty}");
}
