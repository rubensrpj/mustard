//! `metrics_observer` — emits a `tool.use` heartbeat after a tool completes.
//!
//! Ports `metrics-tracker.js`'s verdict-free heartbeat. Shared plumbing lives
//! in [`super::common`]. The emitted event actor id stays `"metrics-tracker"`
//! (the telemetry namespace — unrelated to this module's wire id).

use super::common;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use serde_json::{json, Value};

/// `metrics-tracker`: emits a `tool.use` heartbeat after a tool completes.
///
/// CONCERN: the JS hook resolves the active pipeline-state to tag the event
/// with `phase` / `spec` / `wave`. That depends on pipeline-state access that
/// the `Ctx` does not yet expose (Wave-4/5 concern). This port emits the
/// verdict-free heartbeat with the salient `target` fields; the `phase` /
/// `spec` tags are left `null`, exactly as the JS does when no state is found.
pub struct MetricsObserver;

impl Observer for MetricsObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        let project = if ctx.project_dir.is_empty() {
            common::project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let tool_name = input.tool_name.as_deref().unwrap_or_default();
        let tool_input = &input.tool_input;

        // Salient `target` fields, capped — mirrors the JS `target` object.
        let mut target = serde_json::Map::new();
        if let Some(file) = tool_input
            .get("file_path")
            .or_else(|| tool_input.get("notebook_path"))
            .and_then(|v| v.as_str())
        {
            target.insert("file".into(), json!(file));
        }
        if let Some(cmd) = tool_input.get("command").and_then(|v| v.as_str()) {
            target.insert("command".into(), json!(common::cap(cmd, 120)));
        }
        if let Some(pat) = tool_input.get("pattern").and_then(|v| v.as_str()) {
            target.insert("pattern".into(), json!(common::cap(pat, 80)));
        }
        if let Some(desc) = tool_input.get("description").and_then(|v| v.as_str()) {
            target.insert("description".into(), json!(common::cap(desc, 100)));
        }
        if let Some(sub) = tool_input.get("subagent_type").and_then(|v| v.as_str()) {
            target.insert("subagent".into(), json!(sub));
        }
        if let Some(url) = tool_input.get("url").and_then(|v| v.as_str()) {
            target.insert("url".into(), json!(common::cap(url, 120)));
        }

        // Propagate the harness `tool_use_id` (root or nested under
        // `tool_response`, mirroring `tool_result_observer`) onto the heartbeat
        // so the dashboard can pair this `tool.use` to its `tool.result` by
        // exact id. Without it the pairing falls back to a fragile chronological
        // match that misattributes results when parallel wave agents interleave.
        // `null` when the harness did not forward an id (older events / harness
        // versions) — the dashboard keeps the chronological fallback for those.
        let tool_use_id = match common::extract_tool_use_id(input) {
            Some(id) => Value::String(id),
            None => Value::Null,
        };
        let payload = json!({
            "tool": tool_name,
            "tool_use_id": tool_use_id,
            "phase": Value::Null,
            "target": if target.is_empty() { Value::Null } else { Value::Object(target) },
        });
        common::emit_event(
            &project,
            "metrics-tracker",
            "tool.use",
            payload,
            input.session_id.as_deref(),
        );
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
    fn metrics_observer_observe_is_infallible() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "git status" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        MetricsObserver.observe(&input, &ctx(Trigger::PostToolUse, project));
    }

    /// AC-1 regression: a PostToolUse observer receiving `HookInput.session_id`
    /// must write its event under `.session/<id>/`, not `.session/unknown/`.
    /// Fails before the chokepoint fix (event was born `"unknown"`), passes
    /// after. Uses a real id (`"s-x"`) so `route::emit` never falls through to
    /// the env-based resolution path — this proves the threading, not a leak.
    #[test]
    fn session_id_threaded_from_hook_input() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "git status" }),
            hook_event_name: Some("PostToolUse".to_string()),
            session_id: Some("s-x".to_string()),
            ..HookInput::default()
        };
        MetricsObserver.observe(&input, &ctx(Trigger::PostToolUse, project));

        let session_root = dir.path().join(".claude").join(".session");
        // The threaded id wins the session bucket; the `unknown` bucket must
        // never be created.
        let events_dir = session_root.join("s-x").join(".events");
        assert!(
            events_dir.exists(),
            "event must land under .session/s-x/.events/"
        );
        assert!(
            !session_root.join("unknown").exists(),
            "no event may fall into the .session/unknown/ bucket"
        );
        let mut found = false;
        for f in std::fs::read_dir(&events_dir).unwrap() {
            let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
            if body.lines().any(|l| l.contains("\"event\":\"tool.use\"")) {
                found = true;
            }
        }
        assert!(found, "tool.use NDJSON line must live under .session/s-x/");
    }

    /// The heartbeat must propagate the harness `tool_use_id` onto the
    /// `tool.use` payload so the dashboard can pair it to its `tool.result`
    /// by exact id (the chronological fallback misattributes results when
    /// parallel wave agents interleave). The id is read from the same place
    /// `tool_result_observer` reads it — root `tool_use_id` or nested under
    /// `tool_response`.
    #[test]
    fn heartbeat_payload_carries_tool_use_id() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "git status" }),
            hook_event_name: Some("PostToolUse".to_string()),
            session_id: Some("s-id".to_string()),
            raw: json!({ "tool_use_id": "tu_42" }),
            ..HookInput::default()
        };
        MetricsObserver.observe(&input, &ctx(Trigger::PostToolUse, project));

        let events_dir = dir
            .path()
            .join(".claude")
            .join(".session")
            .join("s-id")
            .join(".events");
        let mut payload = None;
        for f in std::fs::read_dir(&events_dir).unwrap() {
            let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
            for line in body.lines() {
                let v: serde_json::Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v["event"] == "tool.use" {
                    payload = Some(v["payload"].clone());
                }
            }
        }
        let payload = payload.expect("tool.use NDJSON line present");
        assert_eq!(
            payload.get("tool_use_id").and_then(|v| v.as_str()),
            Some("tu_42"),
            "heartbeat payload must echo the harness tool_use_id"
        );
    }

    /// When the harness forwards no `tool_use_id`, the heartbeat still emits a
    /// `tool.use` (id is `null`) so the dashboard's chronological fallback can
    /// take over — never a fabricated id.
    #[test]
    fn heartbeat_tool_use_id_is_null_when_absent() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "git status" }),
            hook_event_name: Some("PostToolUse".to_string()),
            session_id: Some("s-noid".to_string()),
            ..HookInput::default()
        };
        MetricsObserver.observe(&input, &ctx(Trigger::PostToolUse, project));

        let events_dir = dir
            .path()
            .join(".claude")
            .join(".session")
            .join("s-noid")
            .join(".events");
        let mut payload = None;
        for f in std::fs::read_dir(&events_dir).unwrap() {
            let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
            for line in body.lines() {
                let v: serde_json::Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v["event"] == "tool.use" {
                    payload = Some(v["payload"].clone());
                }
            }
        }
        let payload = payload.expect("tool.use NDJSON line present");
        assert!(
            payload.get("tool_use_id").is_some_and(serde_json::Value::is_null),
            "tool_use_id must be present-and-null when the harness omits it"
        );
    }
}
