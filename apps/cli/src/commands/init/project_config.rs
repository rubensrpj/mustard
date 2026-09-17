//! Writing the project-root `mustard.json`: the git-flow answers, the
//! detected commands, the default `inject` declarations and the
//! `runtime`/`version` stamp, in one write.

use std::path::Path;

use anyhow::Result;
use mustard_core::{ProjectConfig, Runtime};

use crate::commands::git_flow;

/// Build and write the single project-root `mustard.json`.
///
/// Loads any existing config (so a re-run preserves user edits), folds in the
/// git-flow + language choices and agnostically-detected commands — only when
/// interactive or on a fresh project; otherwise the existing git-flow is left
/// untouched — then stamps `runtime` + `version` and writes **once**. There is
/// no `.claude/mustard.json`: the file lives at the project root (the workspace
/// anchor), the single source of truth.
pub(super) fn write_project_config(project_path: &Path, runtime: &Runtime, interactive: bool) -> Result<()> {
    let mut config = ProjectConfig::load(project_path);
    let fresh = !ProjectConfig::exists(project_path);

    if interactive || fresh {
        let facts = git_flow::probe_git(project_path);
        let choices = git_flow::collect_choices(&facts, &config, interactive)?;
        git_flow::apply_choices(&mut config, &choices, project_path);
    } else {
        println!("  mustard.json already exists - git flow preserved");
    }

    // Seed the default inject declarations only when the user has none — a
    // curated (non-empty) list is theirs and is preserved verbatim. The
    // defaults live in the core (`project_seed::default_inject_entries`).
    if config.inject.is_empty() {
        config.inject = mustard_core::default_inject_entries();
        println!("  seeded inject declarations (.claude/mustard/*.md ride the session hooks)");
    }

    config.runtime = Some(runtime.clone());
    // The stamp is the HARNESS version (plugin manifest when launched from the
    // plugin, the core line otherwise) — no longer this CLI crate's version.
    // The drift advisory + `/mustard:upsert` compare against the same source.
    config.version = Some(mustard_core::harness_version());
    config.write(project_path)?;
    println!("  wrote mustard.json");
    Ok(())
}
