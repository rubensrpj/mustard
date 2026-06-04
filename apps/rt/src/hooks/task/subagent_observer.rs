//! `subagent_observer` — emits `agent.start` / `agent.stop` telemetry.
//!
//! Ports `subagent-tracker.js`'s verdict-free emission. Shared plumbing lives
//! in [`super::common`]. The emitted event actor id stays `"subagent-tracker"`
//! (the telemetry/attribution namespace the dashboard reads — unrelated to
//! this module's wire id).

use super::common;
use crate::shared::context::current_spec;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::time::now_iso8601;
use serde_json::{json, Value};

/// `subagent-tracker`: emits `agent.start` / `agent.stop` telemetry.
///
/// CONCERN (Wave 4): the JS `subagent-tracker.js` *also* denies a duplicate
/// explorer dispatch within 60s (the `explorer-dedup` path) and inspects
/// pipeline-state / wave-slice byte measurements. Those depend on a
/// `session_id` / wave on `Ctx` that the contract does not yet carry (see the
/// `Ctx` doc comment — "Wave 1 placeholder"). The dedup `deny` is therefore
/// **not ported here**; it is registered as a Wave-4/5 concern. This module
/// ports only the verdict-free `agent.start` / `agent.stop` emission, which is
/// the dominant behaviour and never affects a verdict.
pub struct SubagentObserver;

impl Observer for SubagentObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let project = if ctx.project_dir.is_empty() {
            common::project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let tool_input = &input.tool_input;
        let is_dispatch =
            matches!(input.tool_name.as_deref(), Some("Task" | "Agent"));

        match ctx.trigger {
            Some(Trigger::PreToolUse) if is_dispatch => {
                let description = tool_input
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let subagent_type = tool_input
                    .get("subagent_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let model = tool_input.get("model").cloned().unwrap_or(Value::Null);

                // Wave 2 (telemetry-separation): make the run born attributed.
                // The dispatch already carries everything needed to attribute
                // its eventual span — resolve it here and (a) stamp it onto the
                // `agent.start` payload (the legacy read-time JOIN keys) and
                // (b) record it write-time into telemetry.db's `run_attribution`
                // so the OTLP collector can stamp the span the moment it lands.
                let spec = current_spec(&project);
                let wave_id = common::current_wave_id();
                // The agent id the reader's attribution CTE keys on:
                // `agent_id` ?? `subagent_type`.
                let agent_id = tool_input
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .map_or_else(|| subagent_type.to_string(), str::to_string);
                let tool_use_id = common::extract_tool_use_id(input);

                let mut payload = json!({
                    "description": description,
                    "model": model,
                    "subagentType": subagent_type,
                    "agent_id": agent_id,
                    "spec_id": spec.clone(),
                    "wave_id": wave_id.clone(),
                });
                if let Some(tu) = &tool_use_id {
                    payload["tool_use_id"] = json!(tu);
                }
                common::emit_event(
                    &project,
                    "subagent-tracker",
                    "agent.start",
                    payload,
                    input.session_id.as_deref(),
                );
                // W7B: legacy SQLite `run_attribution` UPSERT removed.
                // Attribution travels inline on each run event (see
                // `record_task_run` in PostToolUse below).
            }
            Some(Trigger::PostToolUse) if is_dispatch => {
                let tool_response = input
                    .raw
                    .get("tool_response")
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                let summary: String = tool_response.chars().take(800).collect();
                common::emit_event(
                    &project,
                    "subagent-tracker",
                    "agent.stop",
                    json!({ "summary": summary }),
                    input.session_id.as_deref(),
                );
                // Finalise the dispatch as one `pipeline.economy.run` NDJSON event
                // (W7B). Token counts come from the Anthropic `usage` payload when
                // the harness forwards it, else are estimated from byte sizes.
                // Best-effort — never blocks the verdict.
                let tool_input_text = serde_json::to_string(tool_input).unwrap_or_default();
                let model_str = match tool_input.get("model").unwrap_or(&Value::Null) {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                let usage = input.raw.get("tool_response").and_then(|r| r.get("usage"));
                let api_input_tokens = usage
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(serde_json::Value::as_i64);
                let api_output_tokens = usage
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(serde_json::Value::as_i64);
                let is_error = input
                    .raw
                    .get("tool_response")
                    .and_then(|r| r.get("is_error"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                // Synthesise a span id when the harness doesn't supply one —
                // `request_id` is preferred, then a `{session}-{ts}-task`
                // composite that stays unique on the event channel.
                let span_id = input
                    .raw
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .map_or_else(|| {
                        let sid = input.session_id.as_deref().unwrap_or("unknown");
                        format!("{sid}-{}-task", now_iso8601())
                    }, str::to_string);
                // Attribution: re-derive in PostToolUse so the run event carries
                // wave_id / agent_id / tool_use_id inline (replaces the SQLite
                // `run_attribution` UPSERT — W7B).
                let post_wave_id = common::current_wave_id();
                let post_subagent_type = tool_input
                    .get("subagent_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let post_agent_id = tool_input
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .map_or_else(|| post_subagent_type.to_string(), str::to_string);
                let post_tool_use_id = common::extract_tool_use_id(input);
                common::record_task_run(
                    &project,
                    input.session_id.as_deref(),
                    span_id,
                    &model_str,
                    current_spec(&project),
                    &tool_input_text,
                    &tool_response,
                    api_input_tokens,
                    api_output_tokens,
                    post_wave_id.as_deref(),
                    Some(post_agent_id.as_str()),
                    post_tool_use_id.as_deref(),
                    is_error,
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(trigger: Trigger, dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(trigger),
            workspace_root: None,
        }
    }

    #[test]
    fn subagent_observer_observe_is_infallible() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "subagent_type": "Explore", "description": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        SubagentObserver.observe(&input, &ctx(Trigger::PreToolUse, project));
    }
}
