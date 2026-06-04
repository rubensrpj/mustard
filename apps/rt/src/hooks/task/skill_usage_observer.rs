//! `skill_usage_observer` — records every Skill invocation as a `skill.invoked`
//! event.
//!
//! Ports `skill-usage-tracker.js`. Shared plumbing lives in [`super::common`].
//! The emitted event actor id stays `"skill-usage-tracker"` (the telemetry
//! namespace — unrelated to this module's wire id).

use super::common;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use serde_json::{json, Value};

/// `skill-usage-tracker`: records every Skill invocation as a `skill.invoked`
/// event.
pub struct SkillUsageObserver;

impl Observer for SkillUsageObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if input.tool_name.as_deref() != Some("Skill") {
            return;
        }
        let project = if ctx.project_dir.is_empty() {
            common::project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let tool_input = &input.tool_input;
        let skill = tool_input
            .get("skill")
            .or_else(|| tool_input.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let args = tool_input
            .get("args")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        let mut payload = serde_json::Map::new();
        payload.insert("skill".into(), json!(skill));
        payload.insert("args".into(), json!(common::cap(&args, 200)));
        // `is_error` only when the Skill tool reported a failure.
        if input
            .raw
            .get("tool_response")
            .and_then(|r| r.get("is_error"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            payload.insert("is_error".into(), json!(true));
        }
        common::emit_event(
            &project,
            "skill-usage-tracker",
            "skill.invoked",
            Value::Object(payload),
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
    fn skill_usage_observer_emits_skill_invoked() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Skill".to_string()),
            tool_input: json!({ "skill": "karpathy-guidelines", "args": "" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        SkillUsageObserver.observe(&input, &ctx(Trigger::PostToolUse, project));

        // W5: `skill.invoked` is non-pipeline → per-session NDJSON (no spec
        // resolves in this test). Scan every NDJSON file under
        // `<project>/.claude/.session/*/.events/`.
        let session_root = dir.path().join(".claude").join(".session");
        let mut found = false;
        if session_root.exists() {
            for entry in std::fs::read_dir(&session_root).unwrap() {
                let events_dir = entry.unwrap().path().join(".events");
                if !events_dir.exists() {
                    continue;
                }
                for f in std::fs::read_dir(&events_dir).unwrap() {
                    let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
                    if body.lines().any(|l| l.contains("\"event\":\"skill.invoked\"")) {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "skill.invoked NDJSON line must be present");
    }

    #[test]
    fn skill_usage_observer_ignores_non_skill_tool() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        SkillUsageObserver.observe(&input, &ctx(Trigger::PostToolUse, project));
        // Non-Skill tool → no `skill.invoked` event emitted; the .events dir is
        // either absent or contains zero `skill.invoked` NDJSON lines.
        let session_root = dir.path().join(".claude").join(".session");
        let mut found = false;
        if session_root.exists() {
            for entry in std::fs::read_dir(&session_root).unwrap() {
                let events_dir = entry.unwrap().path().join(".events");
                if !events_dir.exists() {
                    continue;
                }
                for f in std::fs::read_dir(&events_dir).unwrap() {
                    let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
                    if body.lines().any(|l| l.contains("\"event\":\"skill.invoked\"")) {
                        found = true;
                    }
                }
            }
        }
        assert!(!found, "Bash tool must not produce a skill.invoked event");
    }
}
