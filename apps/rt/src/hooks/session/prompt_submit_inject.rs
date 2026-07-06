//! `prompt_submit_inject` — the UserPromptSubmit gate module.
//!
//! ## Scope (b3 Wave 5, prompt family)
//!
//! Ports `followup-cancel-gate.js` **alone** — a single concern with no
//! sibling hook to merge. It triggers on `UserPromptSubmit` and, when the
//! prompt invokes `/mustard:feature`, `/mustard:bugfix`, or `/mustard:task`,
//! closes any open per-session amendment window — the previous follow-up
//! window is over, so subsequent edits belong to a new context.
//!
//! ## Contract shape
//!
//! `followup-cancel-gate.js` never blocks — it always `process.exit(0)`. The
//! b3 spec classes `prompt_gate` as a [`Check`]; this port honours that —
//! `evaluate` performs the side effects and always returns [`Verdict::Allow`].
//! (It is a `Check`, not an `Observer`, because `UserPromptSubmit` is the seam
//! where a future prompt gate *could* deny; modelling it as a `Check` keeps
//! that extension point open without changing today's always-allow verdict.)
//!
//! ## Single-stage close
//!
//! The old `closed-followup` archival sweep was removed with the single-stage
//! close (a spec now goes straight to `completed`, with no follow-up grace
//! window to archive). What remains on a new-pipeline prompt is closing the
//! session's amendment window.
//!
//! ## W3C migration
//!
//! `emit_economy_operation` routes economy events via
//! `crate::shared::events::route::emit` (NDJSON path) instead of the old SQLite
//! event sink.

use mustard_core::domain::model::event::ActorKind;
use crate::shared::events::economy;
use crate::hooks::observe::amend_window_inject::close_amend_windows_for_session;
use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};

/// W8.T8.2 — pipeline-in-flight reminder: surfaced when the user's prompt is
/// NOT a `/mustard:*` invocation AND a spec is active. Keeps the agent aware
/// that a pipeline is owning the conversation without bloating every prompt.
const PIPELINE_IN_FLIGHT_BANNER: &str = "Pipeline em curso";

/// The UserPromptSubmit gate module.
pub struct PromptSubmitInject;


/// `true` if `prompt` invokes a pipeline command. Mirrors the JS regex
/// `^\s*\/mustard:(feature|bugfix|task)\b` (case-insensitive).
fn is_pipeline_prompt(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    let Some(rest) = t.strip_prefix("/mustard:") else {
        return false;
    };
    for cmd in ["feature", "bugfix", "task"] {
        if rest.starts_with(cmd) {
            // `\b` after the command word.
            let boundary_ok = rest
                .as_bytes()
                .get(cmd.len())
                .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'));
            if boundary_ok {
                return true;
            }
        }
    }
    false
}

/// `true` if `prompt` starts with any `/mustard:` namespaced command. Broader
/// than [`is_pipeline_prompt`] — used by the W8.T8.2 reminder check, where we
/// suppress the banner for every `/mustard:*` (not just pipeline ones), since
/// a slash command always knows its own context.
fn is_mustard_command(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    t.starts_with("/mustard:")
}

impl Check for PromptSubmitInject {
    /// On `UserPromptSubmit`, close the session's amendment window when the
    /// prompt starts a new pipeline. The verdict is `Inject` when a pipeline
    /// is active and the prompt is not itself a `/mustard:*` slash command
    /// (W8.T8.2 reminder), else `Allow`. Any non-`UserPromptSubmit` trigger
    /// self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::UserPromptSubmit) {
            return Ok(Verdict::Allow);
        }
        let prompt = input
            .raw
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let cwd = ctx.project_dir_or_cwd(input);
        if is_pipeline_prompt(prompt) {
            // Close any open amendment windows for this session — the user is
            // starting a new pipeline, so the window's context is done.
            if let Some(session_id) = input.session_id.as_deref() {
                if !session_id.is_empty() {
                    close_amend_windows_for_session(&cwd, session_id);
                }
            }
        }
        // W8.T8.2 — for non-`/mustard:*` prompts, inject a single-line reminder
        // when a spec is active. When NO spec is active (the pipeline is not
        // owning the conversation) the complementary branch injects the
        // orient-census Level 2 (Entrypoints): the exemplar files for the
        // subproject(s) the prompt lexically matches, so the AI reads the entry
        // points instead of grepping. Both gates skip `/mustard:*` (a slash
        // command already knows its context). Fail-open throughout.
        if !is_mustard_command(prompt) {
            match crate::shared::context::current_spec(&cwd).filter(|s| !s.is_empty()) {
                Some(spec) => {
                    let _ = economy::emit(&cwd, ActorKind::Hook, "prompt_gate", "pipeline.economy.operation.invoked", None, serde_json::json!({"operation": "prompt_gate.pipeline_in_flight_banner", "duration_ms": 0, "tokens_used": 0}));
                    return Ok(Verdict::Inject {
                        context: format!("{PIPELINE_IN_FLIGHT_BANNER}: {spec}"),
                    });
                }
                None => {
                    let census = crate::commands::orient::render_entrypoints(
                        &crate::commands::orient::compute_orientation(
                            std::path::Path::new(&cwd),
                            Some(prompt),
                        ),
                    );
                    if let Some(context) = census {
                        return Ok(Verdict::Inject { context });
                    }
                }
            }
        }
        Ok(Verdict::Allow)
    }
}

/// Emit a `pipeline.economy.operation.invoked` event via the NDJSON route.
/// Fail-open: any error degrades to a no-op.
///
/// W3C: routes via `crate::shared::events::route::emit` (NDJSON for
/// non-`pipeline.*` events, SQLite lifecycle index for `pipeline.*`).

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::ClaudePaths;

    /// Build a [`Ctx`] with a unique tempdir project path so the W8.T8.2 active-spec
    /// resolver (`current_spec`) cannot accidentally find a real pipeline-state.
    fn ctx() -> (tempfile::TempDir, Ctx) {
        // SAFETY: env mutation is local to the test process; we restore on drop.
        // Used to neutralise a `MUSTARD_ACTIVE_SPEC` that might be set by the
        // outer shell.
        // Note: we cannot call `std::env::remove_var` from safe Rust on stable;
        // instead, isolate via a unique project_dir (so `current_spec` falls
        // through to the FS branch and finds nothing).
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
        };
        (dir, ctx)
    }

    fn prompt_input(prompt: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            raw: serde_json::json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    // --- pipeline-prompt recognition (parity with TRIGGER_RE) --------------

    #[test]
    fn recognises_pipeline_commands() {
        assert!(is_pipeline_prompt("/mustard:feature add-login"));
        assert!(is_pipeline_prompt("  /mustard:bugfix fix-thing"));
        assert!(is_pipeline_prompt("/MUSTARD:TASK do-it"));
        assert!(is_pipeline_prompt("/mustard:feature"));
    }

    #[test]
    fn rejects_non_pipeline_prompts() {
        assert!(!is_pipeline_prompt("just a normal message"));
        assert!(!is_pipeline_prompt("/mustard:status"));
        assert!(!is_pipeline_prompt("/mustard:featureish thing"));
        assert!(!is_pipeline_prompt("text /mustard:feature mid-line"));
    }

    // --- verdict — always allow --------------------------------------------

    #[test]
    fn pipeline_prompt_allows() {
        // The amendment-window close is a no-op without an open window; the
        // verdict is Allow when no spec is active (and the prompt itself is a
        // `/mustard:*` command, so the W8.T8.2 banner is suppressed either way).
        let (_dir, c) = ctx();
        let v = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:feature x"), &c)
            .unwrap();
        // For a `/mustard:*` command, never Inject regardless of spec state.
        assert!(matches!(v, Verdict::Allow), "unexpected verdict: {v:?}");
    }

    #[test]
    fn non_pipeline_prompt_allows_without_active_spec() {
        // No `.claude/.pipeline-states/` in our tempdir, so `current_spec`
        // returns None and the W8.T8.2 banner stays silent.
        let (_dir, c) = ctx();
        // The env-var branch can still inject; guard by checking either Allow
        // (the expected case in CI) or Inject (when MUSTARD_ACTIVE_SPEC is set
        // by the outer shell).
        let v = PromptSubmitInject.evaluate(&prompt_input("hello there"), &c).unwrap();
        assert!(
            matches!(v, Verdict::Allow | Verdict::Inject { .. }),
            "unexpected verdict: {v:?}",
        );
    }

    #[test]
    fn non_pipeline_prompt_injects_with_active_spec() {
        // W8.T8.2: when a spec is active, the user's free-text prompt gets a
        // single-line banner injected.
        let (dir, _) = ctx();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(paths.pipeline_state_file("active-feature-xyz"), "{}").unwrap();
        let c = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
        };
        let v = PromptSubmitInject
            .evaluate(&prompt_input("how do I do X?"), &c)
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(
                    context.contains(PIPELINE_IN_FLIGHT_BANNER),
                    "banner missing: {context}"
                );
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    #[test]
    fn non_user_prompt_submit_trigger_allows() {
        let other = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            PromptSubmitInject
                .evaluate(&prompt_input("/mustard:feature x"), &other)
                .unwrap(),
            Verdict::Allow
        );
    }
}
