//! `notification` — `Notification` lifecycle observer (W9.T9.3).
//!
//! The harness fires `Notification` when Claude Code surfaces an idle prompt,
//! a completion ping, or a permission ask. This module is observe-only: it
//! appends a single `notification.received` event to the per-spec NDJSON event
//! log (or per-session fallback) so dashboards can correlate notifications
//! with pipeline activity, and never tries to auto-resolve the underlying
//! prompt.
//!
//! ## Routing
//!
//! `notification.received` is *not* a `pipeline.*` event, so [`route::emit`]
//! lands it under `<spec>/[wave-N-{role}/]events/*.ndjson` via the W5 NDJSON
//! writer — the same path `tool.use` / `agent.start` already take.
//!
//! ## Fail-open
//!
//! Pure [`Observer`] — never blocks. Every IO step degrades to a no-op.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{Value, json};

/// The `Notification` lifecycle observer.
pub struct Notification;

/// Resolve the project dir for an invocation.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Pull a human-facing reason / message out of the harness payload. The
/// harness shape varies — probe common keys and fall back to a stringified
/// snapshot of `raw`.
fn extract_message(input: &HookInput) -> Value {
    for key in ["message", "notification_type", "reason", "title", "body"] {
        if let Some(v) = input.raw.get(key) {
            if !v.is_null() {
                return v.clone();
            }
        }
    }
    Value::Null
}

/// Append `notification.received` to the per-spec event log via [`route::emit`].
/// Fail-open — a route failure (no spec yet, no writable NDJSON dir) is silently
/// dropped.
fn append_notification_event(cwd: &str, input: &HookInput) {
    let message = extract_message(input);
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: crate::util::now_iso8601(),
        session_id: input
            .session_id
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(crate::shared::context::session_id),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("notification".to_string()),
            actor_type: None,
        },
        event: "notification.received".to_string(),
        payload: json!({ "message": message, "cwd": cwd }),
        spec: crate::shared::context::current_spec(cwd),
    };
    let _ = crate::shared::events::route::emit(cwd, &event);
}

/// Emit `pipeline.economy.operation.invoked`. Fail-open.
/// Routes through `route::emit` (NDJSON sink) — no SQLite dependency.
fn emit_economy_operation(cwd: &str, operation: &str) {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: crate::util::now_iso8601(),
        session_id: crate::shared::context::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("notification".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({ "operation": operation, "duration_ms": 0, "tokens_used": 0 }),
        spec: crate::shared::context::current_spec(cwd),
    };
    let _ = crate::shared::events::route::emit(cwd, &event);
}

impl Observer for Notification {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let cwd = project_dir(input, ctx);
        append_notification_event(&cwd, input);
        emit_economy_operation(&cwd, "notification.received");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::Trigger;
    use tempfile::tempdir;

    fn input_with(payload_key: &str, payload_val: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("Notification".to_string()),
            session_id: Some("s-notif".to_string()),
            raw: json!({ payload_key: payload_val }),
            ..HookInput::default()
        }
    }

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::Notification),
            workspace_root: None,
        }
    }

    #[test]
    fn extract_message_picks_first_present_key() {
        let input = input_with("message", "Idle for 5 minutes");
        assert_eq!(extract_message(&input), Value::String("Idle for 5 minutes".to_string()));
    }

    #[test]
    fn extract_message_is_null_when_no_known_key() {
        let input = HookInput {
            hook_event_name: Some("Notification".to_string()),
            raw: json!({ "future_key": "x" }),
            ..HookInput::default()
        };
        assert_eq!(extract_message(&input), Value::Null);
    }

    #[test]
    fn observe_is_failopen_with_no_project() {
        // No `.claude/` dir at all — observe must not panic / propagate.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        Notification.observe(&input_with("message", "hi"), &ctx(project));
        // No assertion needed — survival is the contract.
    }
}
