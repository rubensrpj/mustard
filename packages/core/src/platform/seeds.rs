//! `seeds` — the bundled project-seed payload, compiled into the binary.
//!
//! ## Why these live in the core
//!
//! The four files Mustard lays down in a project (`.claude/settings.json`,
//! the two injectable instruction files under `.claude/mustard/`, and the
//! `.claude/.gitignore`) used to ship only as loose files under
//! `apps/cli/templates/`, reachable solely by the `mustard` CLI through a
//! `templates/` directory lookup. That made the CLI the only possible
//! installer: `mustard-rt` (the plugin's binary) had no way to seed a project.
//!
//! Moving the files to `packages/core/templates/` and embedding them with
//! `include_str!` makes the core the single source of truth: both the CLI
//! (`mustard init`) and the runtime (`mustard-rt run upsert`) consume the same
//! constants, and no installed-layout `templates/` directory is required for
//! these seeds. The CLI's `MUSTARD_TEMPLATES_DIR` / `resolve_templates_dir`
//! machinery remains only for the payloads that stay CLI-side (`.github/`
//! scaffolding, `grammars-suggestions.json`, `.artifacts.json`).
//!
//! The seeding logic that consumes these constants lives in
//! [`crate::platform::project_seed`].

/// The reduced `.claude/settings.json` seed: env / permissions / statusLine /
/// plansDirectory. Plugin enablement is deliberately absent (a user-scope
/// choice — see `project_seed::retire_planted_plugin_enablement`).
pub const SETTINGS_SEED: &str = include_str!("../../templates/settings.json");

/// The orchestrator-rules injectable (`.claude/mustard/orchestrator.md`) —
/// spliced into the agent's window per `mustard.json#inject`, canonically on
/// `userPromptSubmit` once per session.
pub const ORCHESTRATOR_MD: &str = include_str!("../../templates/mustard/orchestrator.md");

/// The response-style injectable (`.claude/mustard/response-style.md`) —
/// canonically injected on `sessionStart` once per session.
pub const RESPONSE_STYLE_MD: &str = include_str!("../../templates/mustard/response-style.md");

/// The `.claude/.gitignore` seed covering the ephemeral harness state
/// (caches, pipeline states, per-spec event logs, worktrees).
pub const CLAUDE_GITIGNORE: &str = include_str!("../../templates/.gitignore");

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded seeds must be non-empty and carry their identifying
    /// shapes — a broken `include_str!` path fails the build, but an emptied
    /// or mis-moved template file would otherwise seed silence.
    #[test]
    fn seeds_carry_their_identifying_content() {
        let settings: serde_json::Value =
            serde_json::from_str(SETTINGS_SEED).expect("settings seed is valid JSON");
        assert!(settings.get("permissions").is_some(), "settings seed has permissions");
        assert!(settings.get("statusLine").is_some(), "settings seed has statusLine");
        assert!(
            ORCHESTRATOR_MD.starts_with("# Orchestrator Rules"),
            "orchestrator seed keeps its marker heading"
        );
        assert!(
            RESPONSE_STYLE_MD.starts_with("# Response Style"),
            "response-style seed keeps its heading"
        );
        assert!(CLAUDE_GITIGNORE.contains(".events/"), "gitignore covers the event logs");
    }
}
