//! `enforce_registry` — the entity-registry pre-pipeline gate.
//!
//! ## Scope (b3 Wave 4, Skill family)
//!
//! Ports `enforce-registry.js` 1:1: a `PreToolUse(Skill)` gate that blocks the
//! `/mustard:feature` and `/mustard:bugfix` pipelines when
//! `.claude/entity-registry.json` is missing or invalid (stale schema version,
//! no entities, or no `_patterns`).
//!
//! Consolidation here is trivial — `enforce-registry` is a single concern with
//! no sibling hook to merge. It stays its own module so the registry wiring is
//! one-to-one.
//!
//! ## Verdict note
//!
//! `enforce-registry.js` writes `permissionDecision: "block"` on the wire,
//! while the other PreToolUse gates write `"deny"`. Both are *blocking*
//! decisions; the `mustard-core` contract has a single blocking [`Verdict::Deny`]
//! and the dispatcher's `emit_outcome` encodes it as `"deny"`. The **verdict**
//! (block the Skill) is preserved exactly — only the wire string normalises
//! from `"block"` to `"deny"`, which the harness treats identically.
//!
//! ## Mode
//!
//! `enforce-registry.js` has **no `MUSTARD_*_MODE`** — it is always strict
//! (like `bash-safety` / `file-guard`). The dispatcher repasses the verdict.

use mustard_core::error::Error;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use serde_json::Value;
use std::path::Path;

/// The pipeline skills the registry gate applies to. Mirrors the JS array
/// `['mustard:feature', 'mustard:bugfix', 'feature', 'bugfix']`.
const PIPELINE_SKILLS: &[&str] = &["mustard:feature", "mustard:bugfix", "feature", "bugfix"];

/// The entity-registry gate module.
pub struct EnforceRegistry;

/// Assemble a gate message in the `formatGateMessage` shape:
/// `[gate] what. why. Saída: exit.`
fn format_gate_message(gate: &str, what: &str, why: &str, exit: &str) -> String {
    let mut body = String::new();
    if !what.is_empty() {
        body.push_str(what);
    }
    if !why.is_empty() {
        if !body.is_empty() {
            body.push_str(". ");
        }
        body.push_str(why);
    }
    if !body.is_empty() && !body.ends_with(['.', '!', '?', '…']) {
        body.push('.');
    }
    let mut msg = format!("[{gate}] {body}").trim().to_string();
    if !exit.is_empty() {
        let mut tail = exit.to_string();
        if !tail.ends_with(['.', '!', '?', '…']) {
            tail.push('.');
        }
        msg.push_str(&format!(" Saída: {tail}"));
    }
    msg
}

/// Validate a parsed `entity-registry.json` value. Returns the deny reason on
/// failure, or `None` when the registry is valid. Port of `validateRegistry`.
fn validate_registry(registry: &Value) -> Option<String> {
    // Version must start with `3.`.
    let version = registry
        .get("_meta")
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str());
    if !version.is_some_and(|v| v.starts_with("3.")) {
        return Some(format_gate_message(
            "Registry Gate",
            &format!(
                "Registry version {} is outdated",
                version.unwrap_or("unknown")
            ),
            "/feature and /bugfix expect schema v3.1",
            "run /sync-registry to update the registry",
        ));
    }

    // Entities exist (`registry.e`, excluding the `_placeholder` key).
    let entity_count = registry
        .get("e")
        .and_then(Value::as_object)
        .map_or(0, |obj| obj.keys().filter(|k| *k != "_placeholder").count());
    if entity_count == 0 {
        return Some(format_gate_message(
            "Registry Gate",
            "Registry has no entities",
            "the pipeline cannot resolve any known entity",
            "run /sync-registry to populate the registry",
        ));
    }

    // `_patterns` must be a non-empty object.
    let has_patterns = registry
        .get("_patterns")
        .and_then(Value::as_object)
        .is_some_and(|obj| !obj.is_empty());
    if !has_patterns {
        return Some(format_gate_message(
            "Registry Gate",
            &format!(
                "Registry has {entity_count} entities but no _patterns defined"
            ),
            "the pipeline needs reference patterns to scaffold code",
            "run /sync-registry to add reference patterns",
        ));
    }

    None
}

/// Compute the registry gate verdict for a `PreToolUse(Skill)` invocation,
/// rooted at `cwd`. 1:1 with `enforce-registry.js`.
fn registry_verdict(input: &HookInput, cwd: &str) -> Verdict {
    // Only the Skill tool is inspected.
    if input.tool_name.as_deref() != Some("Skill") {
        return Verdict::Allow;
    }
    // Only the pipeline skills are gated.
    let skill = input
        .tool_input
        .get("skill")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !PIPELINE_SKILLS.contains(&skill) {
        return Verdict::Allow;
    }

    // Find `.claude/entity-registry.json` under cwd.
    let registry_path = Path::new(cwd).join(".claude").join("entity-registry.json");
    let Ok(text) = std::fs::read_to_string(&registry_path) else {
        return Verdict::Deny {
            reason: format_gate_message(
                "Registry Gate",
                "Entity registry not found",
                "/feature and /bugfix need it to resolve known entities",
                "run /sync-registry, then retry the command",
            ),
        };
    };
    // A parse failure is fail-open in the JS (`catch` → exit 0).
    let Ok(registry) = serde_json::from_str::<Value>(&text) else {
        return Verdict::Allow;
    };
    match validate_registry(&registry) {
        Some(reason) => Verdict::Deny { reason },
        None => Verdict::Allow,
    }
}

impl Check for EnforceRegistry {
    /// Gate a `PreToolUse(Skill)` invocation of a pipeline skill on the
    /// entity-registry's presence and validity. Always strict — no mode.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        let cwd = if ctx.project_dir.is_empty() {
            input.cwd.as_deref().unwrap_or(".")
        } else {
            ctx.project_dir.as_str()
        };
        Ok(registry_verdict(input, cwd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn skill_input(skill: &str, cwd: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Skill".to_string()),
            tool_input: json!({ "skill": skill }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: cwd.to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        (input, ctx)
    }

    /// Write an `entity-registry.json` under `<dir>/.claude/`.
    fn write_registry(dir: &Path, registry: &Value) {
        let claude = dir.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("entity-registry.json"),
            registry.to_string(),
        )
        .unwrap();
    }

    // --- parity (hooks.test.js "enforce-registry.js") ----------------------

    #[test]
    fn blocks_pipeline_skill_when_registry_missing() {
        let dir = tempdir().unwrap();
        let (input, ctx) = skill_input("feature", dir.path().to_str().unwrap());
        let verdict = EnforceRegistry.evaluate(&input, &ctx).expect("no error");
        assert!(verdict.is_blocking(), "missing registry must block");
    }

    #[test]
    fn allows_non_pipeline_skill() {
        let dir = tempdir().unwrap();
        let (input, ctx) = skill_input("some-random-skill", dir.path().to_str().unwrap());
        assert_eq!(
            EnforceRegistry.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn allows_namespaced_pipeline_skill_with_valid_registry() {
        let dir = tempdir().unwrap();
        write_registry(
            dir.path(),
            &json!({
                "_meta": { "version": "3.1" },
                "e": { "User": {} },
                "_patterns": { "drizzle": {} },
            }),
        );
        let (input, ctx) =
            skill_input("mustard:feature", dir.path().to_str().unwrap());
        assert_eq!(
            EnforceRegistry.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    // --- validateRegistry parity -------------------------------------------

    #[test]
    fn stale_version_is_blocked() {
        let reason = validate_registry(&json!({
            "_meta": { "version": "2.0" },
            "e": { "User": {} },
            "_patterns": { "x": {} },
        }))
        .expect("stale version must fail");
        assert!(reason.contains("outdated"));
    }

    #[test]
    fn no_entities_is_blocked() {
        let reason = validate_registry(&json!({
            "_meta": { "version": "3.1" },
            "e": { "_placeholder": {} },
            "_patterns": { "x": {} },
        }))
        .expect("no entities must fail");
        assert!(reason.contains("no entities"));
    }

    #[test]
    fn no_patterns_is_blocked() {
        let reason = validate_registry(&json!({
            "_meta": { "version": "3.1" },
            "e": { "User": {} },
            "_patterns": {},
        }))
        .expect("no patterns must fail");
        assert!(reason.contains("_patterns"));
    }

    #[test]
    fn valid_registry_passes_validation() {
        assert!(validate_registry(&json!({
            "_meta": { "version": "3.2" },
            "e": { "User": {}, "Order": {} },
            "_patterns": { "drizzle": { "discovered": [] } },
        }))
        .is_none());
    }

    // --- gate routing -------------------------------------------------------

    #[test]
    fn non_skill_tool_allows() {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        assert_eq!(
            EnforceRegistry.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        let dir = tempdir().unwrap();
        let (input, _) = skill_input("feature", dir.path().to_str().unwrap());
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PostToolUse),
        };
        assert_eq!(
            EnforceRegistry.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }
}
