//! `skill_usage_observer` — records every Skill invocation as a `skill.invoked`
//! event, and emits `pipeline.kind` as a deterministic work-type signal for the
//! lean pipeline flows.
//!
//! Ports `skill-usage-tracker.js`. Shared plumbing lives in [`super::common`].
//! The emitted event actor id stays `"skill-usage-tracker"` (the telemetry
//! namespace — unrelated to this module's wire id).
//!
//! ## Why `pipeline.kind` is emitted HERE (porta-unica)
//!
//! The lean paths (`/mustard:task`, `/mustard:bugfix` fast-path) skip the full
//! pipeline ceremony, so they never emit a `pipeline.scope` / `pipeline.stage`
//! event — the dashboard is blind to the type of work and loses the narrative of
//! what was requested. The fix must be a SIDE-EFFECT the prose-driven
//! orchestrator cannot skip, so it rides on this PostToolUse hook (which fires
//! deterministically on every `Skill` call) rather than on an instruction the AI
//! might forget. Only the lean flows emit here; the full feature pipeline emits
//! its own richer lifecycle events through the run-face emitters.

use super::common;
use mustard_core::domain::model::event::EVENT_PIPELINE_KIND;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use serde_json::{json, Value};

/// Map a SKILL name to the `(kind, scope)` of work it routes to, for the LEAN
/// flows that otherwise emit no pipeline event. Accepts the fully-qualified
/// `plugin:skill` form (`mustard:task`) and the bare skill name (`task`) — the
/// `Skill` tool input may carry either. Returns `None` for any non-lean skill
/// (the full feature pipeline, `/scan`, `/git`, …), so `pipeline.kind` is
/// emitted ONLY where today nothing is.
fn kind_for_lean_skill(skill: &str) -> Option<(&'static str, &'static str)> {
    // Normalise: drop a leading `mustard:` (or any `plugin:`) namespace.
    let bare = skill.rsplit(':').next().unwrap_or(skill);
    match bare {
        "task" => Some(("task", "lean")),
        "bugfix" => Some(("bugfix", "lean")),
        "tactical-fix" => Some(("tactical-fix", "lean")),
        _ => None,
    }
}

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

        // Deterministic work-type signal for the LEAN flows (porta-unica): the
        // lean paths skip the full pipeline ceremony and so never emit a
        // `pipeline.scope` event — without this the dashboard cannot separate
        // their work by type. Emitted as a side-effect of the Skill call (not
        // orchestrator prose), so it can never be skipped. Non-lean skills map
        // to `None` and emit nothing here.
        if let Some((kind, scope)) = kind_for_lean_skill(skill) {
            common::emit_event(
                &project,
                "skill-usage-tracker",
                EVENT_PIPELINE_KIND,
                json!({ "kind": kind, "scope": scope }),
                input.session_id.as_deref(),
            );
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

    /// Scan every per-session NDJSON file under `<project>/.claude/.session/`
    /// and report whether any line carries the given `"event":"<name>"`.
    fn session_has_event(dir: &std::path::Path, event_name: &str) -> bool {
        let needle = format!("\"event\":\"{event_name}\"");
        let session_root = dir.join(".claude").join(".session");
        if !session_root.exists() {
            return false;
        }
        for entry in std::fs::read_dir(&session_root).unwrap() {
            let events_dir = entry.unwrap().path().join(".events");
            if !events_dir.exists() {
                continue;
            }
            for f in std::fs::read_dir(&events_dir).unwrap() {
                let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
                if body.lines().any(|l| l.contains(&needle)) {
                    return true;
                }
            }
        }
        false
    }

    /// The mapping is byte-stable and namespace-tolerant: the lean flows map to
    /// `(kind, "lean")` from either the `mustard:task` or bare `task` form; a
    /// non-lean skill maps to `None` (no `pipeline.kind` emitted for it).
    #[test]
    fn kind_for_lean_skill_maps_lean_flows_only() {
        assert_eq!(kind_for_lean_skill("mustard:task"), Some(("task", "lean")));
        assert_eq!(kind_for_lean_skill("task"), Some(("task", "lean")));
        assert_eq!(kind_for_lean_skill("mustard:bugfix"), Some(("bugfix", "lean")));
        assert_eq!(kind_for_lean_skill("mustard:tactical-fix"), Some(("tactical-fix", "lean")));
        // Non-lean flows (full feature, scan, git, third-party) emit nothing.
        assert_eq!(kind_for_lean_skill("mustard:feature"), None);
        assert_eq!(kind_for_lean_skill("mustard:scan"), None);
        assert_eq!(kind_for_lean_skill("karpathy-guidelines"), None);
    }

    /// A lean run (`/mustard:task`) emits `pipeline.kind` as a deterministic
    /// side-effect of the Skill call — the work-type signal the dashboard reads.
    #[test]
    fn lean_skill_emits_pipeline_kind() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Skill".to_string()),
            tool_input: json!({ "skill": "mustard:task", "args": "" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        SkillUsageObserver.observe(&input, &ctx(Trigger::PostToolUse, project));
        assert!(
            session_has_event(dir.path(), "pipeline.kind"),
            "a lean run must emit pipeline.kind"
        );
        // The companion `skill.invoked` event still rides along.
        assert!(session_has_event(dir.path(), "skill.invoked"));
    }

    /// A NON-lean skill (`/mustard:feature`) records `skill.invoked` but emits
    /// NO `pipeline.kind` — the full pipeline owns its own lifecycle events.
    #[test]
    fn non_lean_skill_emits_no_pipeline_kind() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Skill".to_string()),
            tool_input: json!({ "skill": "mustard:feature", "args": "" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        SkillUsageObserver.observe(&input, &ctx(Trigger::PostToolUse, project));
        assert!(session_has_event(dir.path(), "skill.invoked"));
        assert!(
            !session_has_event(dir.path(), "pipeline.kind"),
            "a non-lean skill must not emit pipeline.kind"
        );
    }
}
