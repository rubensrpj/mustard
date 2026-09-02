//! `seeds` — the bundled project-seed payload, compiled into the binary.
//!
//! ## Why these live in the core
//!
//! The files Mustard lays down in a project (`.claude/settings.json`, the
//! injectable instruction files under `.claude/mustard/`, and the
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
/// the naming. Declared on `userPromptSubmit`, the same event as
/// [`ORCHESTRATOR_MD`] and on a sibling hook of its own.
///
/// The split is structural, not editorial. A hook's `additionalContext` is
/// capped at 10,000 characters and the overflow is saved to a file the window
/// only receives as a preview plus a path — so an over-budget router stops
/// being IN FORCE, which is the one thing a router may not stop being. The cap
/// is per hook RESPONSE, not per event: sibling hooks on one event are separate
/// invocations and every one of their `additionalContext` blocks is kept
/// (measured 2026-08-25 — two siblings emitting 6,000 characters each both
/// arrived intact). So each injectable gets its own hook registration and its
/// own ceiling, and there is no composite budget between them. Mustard's own
/// composer still folds the injectables of ONE invocation into a single
/// `additionalContext` (`hooks::session::*_inject` — the dispatcher fold is
/// last-writer-wins), which is why the split is a hook per file rather than two
/// `Inject`s in one. Rationale in full:
/// `plugin/refs/mustard/router-rationale.md`.
pub const DISPATCH_MD: &str = include_str!("../../templates/mustard/dispatch.md");

/// The material-channel injectable (`.claude/mustard/material.md`) — the three
/// `material-add` calls, the rule that a decision is written when it is
/// SETTLED, and the window (`▸6` on) in which the channel is open.
///
/// Split out of [`DISPATCH_MD`] rather than compressed into it. That document
/// had ten characters of margin under the size alarm on a CRLF checkout, and
/// the two prescriptions the code itself carries disagree about the remedy: the
/// budget test's failure message says SPLIT, the cap's own doc says trim. Cutting
/// a rule's justification is the one thing neither may buy — a rule shipped
/// without the dated measurement behind it is a rule the next reader argues
/// away. Splitting costs neither the rule nor its reason, and the material
/// channel is a self-contained job (what the unit CARRIES), distinct from where
/// a unit starts and what it is called.
///
/// Like the other two it rides its own sibling hook on `userPromptSubmit`, so
/// it is measured alone against the 10,000-character response ceiling — see
/// [`DISPATCH_MD`] for why sibling hooks share no budget.
pub const MATERIAL_MD: &str = include_str!("../../templates/mustard/material.md");

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
        assert!(
            MATERIAL_MD.starts_with("# Material Rules"),
            "material seed keeps its marker heading"
        );
        // The three are ONE router split across three sibling hooks; each must
        // still be the part it claims to be, or a split moved prose into a file
        // nobody's hook delivers — or, worse, into two at once, where an edit
        // corrects one copy and leaves the window reading the other.
        assert!(
            DISPATCH_MD.contains("## Dispatch")
                && !ORCHESTRATOR_MD.contains("## Dispatch")
                && !MATERIAL_MD.contains("## Dispatch"),
            "the dispatch section must live in exactly one of the three parts"
        );
        assert!(
            ORCHESTRATOR_MD.contains("## Intent Routing")
                && !DISPATCH_MD.contains("## Intent Routing")
                && !MATERIAL_MD.contains("## Intent Routing"),
            "intent routing must live in exactly one of the three parts"
        );
        // The material channel MOVED; it did not get copied. A live
        // `material-add` invocation still standing in `dispatch.md` is the state
        // where the rule is corrected in one file and read from the other.
        // Matched on the INVOCATION, not the bare command name — the pointer
        // dispatch.md keeps is allowed to say what it points at.
        const MATERIAL_CALL: &str = "mustard-rt run material-add";
        assert!(
            MATERIAL_MD.contains("## Material")
                && !ORCHESTRATOR_MD.contains("## Material")
                && !DISPATCH_MD.contains("## Material"),
            "the material section must live in exactly one of the three parts"
        );
        assert!(
            MATERIAL_MD.contains(MATERIAL_CALL) && !DISPATCH_MD.contains(MATERIAL_CALL),
            "the `material-add` calls must be in the material part alone"
        );
        // …and the half it left still POINTS at it: a reader of the unit's
        // rules who is never told where the channel went stops using it.
        assert!(
            DISPATCH_MD.contains("material.md"),
            "dispatch.md drops the material channel without saying where it went"
        );
        assert!(CLAUDE_GITIGNORE.contains(".events/"), "gitignore covers the event logs");
    }
}
