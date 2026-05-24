//! `mustard-rt run graph-dead` — list concept-nodes with zero spec backlinks.
//!
//! Wave 5 (`project-profiler`) closes the graph loop: the resolver now writes
//! `[[id]]` backlinks back into each spec when an EXECUTE phase finishes. The
//! cohort of concept-nodes that *no* spec ever links to is the "dead" set —
//! candidates for deletion. This subcommand surfaces that list so a maintainer
//! (or a future janitor pass) can act on it.
//!
//! Output (stdout, byte-stable pretty JSON):
//!
//! ```json
//! {
//!   "dead": ["rt.conv.orphan", "..."],
//!   "count": 1
//! }
//! ```
//!
//! Exit code is always `0` (fail-open). A missing `.claude/graph/` tree or an
//! unreadable `.claude/spec/` directory degrades to `{ "dead": [], "count": 0 }`.

use crate::run::env::project_dir;
use crate::run::scan::graph;
use serde_json::json;
use std::path::PathBuf;

/// Run `mustard-rt run graph-dead`. Fail-open by design.
pub fn run() {
    let project = PathBuf::from(project_dir());
    let dead = graph::dead_node_ids(&project);
    let envelope = json!({
        "dead": dead,
        "count": dead.len(),
    });
    let pretty = serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| "{}".to_string());
    println!("{pretty}");
}
