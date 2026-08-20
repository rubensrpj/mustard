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
/// `userPromptSubmit` once per session. Carries the router's FIRST half:
/// intent routing, delegation, phases, locating code, efficiency.
pub const ORCHESTRATOR_MD: &str = include_str!("../../templates/mustard/orchestrator.md");

/// The dispatch-rules injectable (`.claude/mustard/dispatch.md`) — the
/// router's SECOND half: the question a unit opens with, the base gate, and
/// the naming. Declared on `sessionStart`, deliberately a different event from
/// [`ORCHESTRATOR_MD`].
///
/// The split is structural, not editorial. A hook's `additionalContext` is
/// capped at 10,000 characters and the overflow is saved to a file the window
/// only receives as a preview plus a path — so an over-budget router stops
/// being IN FORCE, which is the one thing a router may not stop being. The
/// composer folds every injectable of ONE event into a single
/// `additionalContext` (`hooks::session::*_inject` — the dispatcher fold is
/// last-writer-wins), so two entries on the same event share one ceiling and a
/// second `Inject` would be dropped. Two events are two hook invocations, each
/// with its own response and its own ceiling.
pub const DISPATCH_MD: &str = include_str!("../../templates/mustard/dispatch.md");

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
            DISPATCH_MD.starts_with("# Dispatch Rules"),
            "dispatch seed keeps its marker heading"
        );
        // The two halves are one router split across two events; each must
        // still be the half it claims to be, or the split moved prose into a
        // file nobody's event delivers.
        assert!(
            DISPATCH_MD.contains("## Dispatch") && !ORCHESTRATOR_MD.contains("## Dispatch"),
            "the dispatch section must live in exactly one of the two halves"
        );
        assert!(
            ORCHESTRATOR_MD.contains("## Intent Routing")
                && !DISPATCH_MD.contains("## Intent Routing"),
            "intent routing must live in exactly one of the two halves"
        );
        assert!(CLAUDE_GITIGNORE.contains(".events/"), "gitignore covers the event logs");
    }
}
