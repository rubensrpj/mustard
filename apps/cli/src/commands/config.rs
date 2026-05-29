//! `mustard config` — (re)configure the project-root `mustard.json` git flow.
//!
//! Ported from `commands/config.ts`, which was a 13-line wrapper that called
//! `generateMustardJson`. The Rust port is just as thin: it delegates to
//! [`crate::commands::git_flow::configure`], the same routine `init` runs for
//! its git-flow step. No logic lives here — keeping it a wrapper means the flow
//! rules have a single home (`git_flow`).

use std::path::Path;

use anyhow::Result;

use crate::commands::git_flow;

/// Flags accepted by `mustard config`.
#[derive(Debug, Default, Clone)]
pub struct ConfigOptions {
    /// Accept defaults without prompting.
    pub yes: bool,
}

/// Run `mustard config` against `project_path`.
///
/// `interactive` is `!yes`, exactly as the JS port passed `options` straight
/// through. `git_flow` itself falls back to non-interactive derivation when
/// stdin is not a TTY, so a scripted run never blocks.
pub fn config(project_path: &Path, options: &ConfigOptions) -> Result<()> {
    println!("\nMustard - Git Flow Configuration\n");
    git_flow::configure(project_path, !options.yes)
}
