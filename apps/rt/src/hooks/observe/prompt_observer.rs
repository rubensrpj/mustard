//! `prompt_observer` — `UserPromptSubmit` lifecycle observer.
//!
//! The harness fires `UserPromptSubmit` whenever the user submits a prompt.
//! This module is observe-only: it appends a single `user.prompt` event to the
//! per-spec NDJSON event log (or the per-session fallback under
//! `.claude/.session/{id}/.events/`) so the dashboard can render "what I asked"
//! in the execution trace. It never blocks the prompt and never tries to act on
//! its content.
//!
//! ## Unconditional capture
//!
//! Unlike `amend_window_inject::observe_user_prompt` — which only emits its
//! `pipeline.amend_intent` event while a post-close amend window is open — this
//! observer fires on EVERY prompt, regardless of pipeline state. The two events
//! are distinct (`user.prompt` vs `pipeline.amend_intent`) and never conflict.
//!
//! ## Routing
//!
//! `user.prompt` is *not* a `pipeline.*` event, so [`route::emit`] lands it
//! under `<spec>/[wave-N-{role}/]events/*.ndjson` when a spec is resolvable, or
//! falls back to `<project>/.claude/.session/<id>/.events/*.ndjson` when the
//! spec chain yields `None` — the spec-less session sink the dashboard's
//! sessions view consumes.
//!
//! ## Fail-open
//!
//! Pure [`Observer`] — never blocks. An empty/absent prompt is a no-op, and
//! every IO step degrades to a no-op.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;

/// The `UserPromptSubmit` lifecycle observer.
pub struct PromptObserver;

/// Append `user.prompt` to the per-spec (or per-session) event log via
/// [`route::emit`]. Fail-open — a route failure (no writable NDJSON dir) is
/// silently dropped, and an empty/absent prompt emits nothing.
fn append_prompt_event(cwd: &str, input: &HookInput) {
    // Confirmed shape (amend_window_inject.rs:466, prompt_submit_inject.rs):
    // the harness carries the submitted text at `raw.prompt`.
    // "Every prompt" means every prompt a PERSON submitted. The runtime speaks
    // through this same channel, so a finished background task or a completed
    // subagent would otherwise land in the trace as something the user said —
    // and this log is what `metrics collect` reads, so the noise would reach the
    // instruments too. See [`crate::shared::prompt`].
    let Some(prompt) = input
        .raw
        .get("prompt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .filter(|s| !crate::shared::prompt::is_harness_notice(s))
    else {
        return;
    };

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: mustard_core::time::now_iso8601(),
        session_id: input
            .session_id
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(crate::shared::context::session_id),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("user_prompt_observer".to_string()),
            actor_type: None,
        },
        event: "user.prompt".to_string(),
        payload: json!({ "prompt": prompt }),
        spec: crate::shared::context::current_spec(cwd),
    };
    let _ = crate::shared::events::route::emit(cwd, &event);
}

impl Observer for PromptObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let cwd = ctx.project_dir_or_cwd(input);
        append_prompt_event(&cwd, input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::Trigger;
    use mustard_core::ClaudePaths;
    use tempfile::tempdir;

    fn input_with_prompt(prompt: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some("s-prompt".to_string()),
            raw: json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
        }
    }

    /// The session-sink `.events/` dir for a spec-less event (mirrors
    /// `writer_ndjson::event_dir` with `spec = None`).
    fn session_events_dir(project: &std::path::Path, session: &str) -> std::path::PathBuf {
        ClaudePaths::for_project(project)
            .unwrap()
            .claude_dir()
            .join(".session")
            .join(session)
            .join(".events")
    }

    #[test]
    fn emits_user_prompt_event_with_text_in_payload() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let prompt = "add a payables list view";

        PromptObserver.observe(&input_with_prompt(prompt), &ctx(project));

        // No spec markers in the temp project → the event lands in the
        // per-session sink `<project>/.claude/.session/s-prompt/.events/`.
        let events_dir = session_events_dir(dir.path(), "s-prompt");
        assert!(events_dir.exists(), "session .events dir must exist");

        let mut found = String::new();
        for entry in std::fs::read_dir(&events_dir).unwrap() {
            found.push_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap());
        }
        assert!(found.contains("\"user.prompt\""), "event name in NDJSON: {found}");
        assert!(found.contains(prompt), "prompt text in payload: {found}");
    }

    #[test]
    fn empty_prompt_emits_nothing() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();

        PromptObserver.observe(&input_with_prompt(""), &ctx(project));

        let events_dir = session_events_dir(dir.path(), "s-prompt");
        assert!(!events_dir.exists(), "empty prompt must not write any event");
    }

    #[test]
    fn observe_is_failopen_with_no_project() {
        // Missing `prompt` key entirely — observe must not panic / propagate.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some("s-prompt".to_string()),
            raw: json!({ "other": "x" }),
            ..HookInput::default()
        };
        PromptObserver.observe(&input, &ctx(project));
        // Survival is the contract — no event, no panic.
    }
}
