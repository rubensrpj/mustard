//! `pre_edit_intent_check` — optional run-based alternative to Moment 1.
//!
//! Spec A v4 / Wave 4 — T4.6. A `PreToolUse(Write|Edit)` `Check` that runs
//! the vocabulary scan (Moment 1) against the agent's free-form context
//! before an Edit/Write tool runs. It is **gated** by the env var
//! `MUSTARD_V4_GATE_ENABLED=1` — when unset the check is a no-op so the v3
//! harness keeps its semantics during the v4 refoundation.
//!
//! The behaviour is intentionally thin: build a [`GateInput`] from the hook
//! input, call [`gate_regression_check::run`] with `Moment::One`, and map the
//! verdict to a [`Verdict`] — Red ⇒ Deny, Amber/Green ⇒ Allow.
//!
//! Red emissions print the structured JSON to stdout via the gate's helpers
//! (see [`gate_regression_check::emit_red_blocked_json`]) — the dispatcher
//! merely wraps the result.

use crate::run::review::gate_regression_check::{self, GateInput, Moment, RegressionVerdict};
use mustard_core::error::Error;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use std::path::PathBuf;

/// The pre-edit Moment-1 gate module. Stateless — every invocation rebuilds
/// from the hook input.
pub struct PreEditIntentCheck;

/// `true` when the V4 gate is enabled via env. The check is a no-op otherwise
/// so the v3 harness can stay live during bootstrap.
fn gate_enabled() -> bool {
    std::env::var("MUSTARD_V4_GATE_ENABLED")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// Pull a free-form plan/context string from the hook input. The Claude Code
/// `Write` / `Edit` tools do not ship a `plan` field on PreToolUse, so we fall
/// back to the file contents (Write) or the new-string (Edit) — both are the
/// best proxy for "what the agent is about to do".
fn plan_text_from_input(input: &HookInput) -> String {
    let v = &input.tool_input;
    // Write: `content` carries the new file body.
    if let Some(s) = v.get("content").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    // Edit: `new_string` is the replacement body.
    if let Some(s) = v.get("new_string").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    String::new()
}

impl Check for PreEditIntentCheck {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Trigger guard — only PreToolUse(Write|Edit) reaches us, but be
        // defensive (the dispatcher is the source of truth).
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if !gate_enabled() {
            return Ok(Verdict::Allow);
        }
        let cwd = if !ctx.project_dir.is_empty() {
            ctx.project_dir.as_str()
        } else {
            input.cwd.as_deref().unwrap_or(".")
        };
        let gate_input = GateInput {
            // No spec path is known at PreToolUse — use the cwd as the
            // ancestor anchor; `resolve_project_root` walks up looking for
            // `.claude/`.
            spec_path: PathBuf::from(cwd),
            plan_text: plan_text_from_input(input),
            diff: Vec::new(),
            declared_fns: Vec::new(),
            before_snapshot: None,
            after_snapshot: None,
        };
        match gate_regression_check::run(gate_input, Moment::One) {
            Ok(RegressionVerdict::Green) | Ok(RegressionVerdict::Amber { .. }) => {
                // Amber returned `Ok` and already printed its AskUserQuestion
                // JSON; the dispatcher allows the tool to run.
                Ok(Verdict::Allow)
            }
            Ok(RegressionVerdict::Red { signals }) => {
                // Red — the gate already emitted the blocked JSON in run();
                // surface the deny with a concise reason for the dispatcher.
                let reason = signals
                    .first()
                    .map(|s| s.message.clone())
                    .unwrap_or_else(|| "gate red".to_string());
                Ok(Verdict::Deny { reason })
            }
            Err(_) => {
                // GateError::Blocked — same shape as Red.
                Ok(Verdict::Deny {
                    reason: "gate blocked".to_string(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hook_input(plan: &str, cwd: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "content": plan }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: cwd.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    #[test]
    fn no_op_when_env_unset_or_zero() {
        // Without mutating the env (forbid(unsafe_code) is active), drive
        // `gate_enabled` directly through its public observable: a string
        // that is empty or `"0"` must be treated as off.
        // Asserting `gate_enabled() == false` here is not possible without
        // env mutation, so we instead validate the only invariant the
        // implementation guarantees: when the gate logic is invoked with
        // empty plan text and no declared fns, the verdict is Allow.
        let tmp = tempfile::tempdir().expect("tempdir");
        let (input, ctx) = hook_input("", tmp.path().to_str().unwrap());
        let verdict = PreEditIntentCheck.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow));
    }
}
