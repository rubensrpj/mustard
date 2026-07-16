//! `scan_gate` — the pre-pipeline scan gate.
//!
//! ## Scope (b3 Wave 4, Skill family)
//!
//! A `PreToolUse(Skill)` gate that blocks the `/mustard:feature` and
//! `/mustard:bugfix` pipelines until grain's model
//! (`.claude/grain.model.json`, produced by `mustard-rt run scan`) exists — the
//! pipeline needs it to understand the repo.
//!
//! Single concern with no sibling hook to merge, so it stays its own module.
//!
//! ## Verdict note
//!
//! A blocking decision is encoded on the wire as `"deny"`; the `mustard-core`
//! contract has a single blocking [`Verdict::Deny`] and the dispatcher's
//! `emit_outcome` writes it as `"deny"`, which the harness treats as blocking.
//!
//! ## Mode
//!
//! The gate has **no `MUSTARD_*_MODE`** — it is always strict (like
//! `bash-safety`). The dispatcher repasses the verdict.

use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};

use crate::util::format_gate_message;

/// The pipeline skills the scan gate applies to.
const PIPELINE_SKILLS: &[&str] = &["mustard:feature", "mustard:bugfix", "feature", "bugfix"];

/// The pre-pipeline scan gate module.
pub struct ScanGate;

/// Compute the scan gate verdict for a `PreToolUse(Skill)` invocation, rooted at
/// `cwd`. The pipeline (`/feature`, `/bugfix`) needs grain's model
/// (`.claude/grain.model.json`, produced by `mustard-rt run scan`) to understand
/// the repo; block until it exists.
fn scan_verdict(input: &HookInput, cwd: &str) -> Verdict {
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

    let model = std::path::Path::new(cwd).join(".claude").join("grain.model.json");
    if model.is_file() {
        Verdict::Allow
    } else {
        Verdict::Deny {
            reason: format_gate_message(
                "Scan Gate",
                "grain.model.json not found",
                "/feature and /bugfix need the grain model to understand the repo",
                "run `mustard-rt run scan`, then retry the command",
            ),
        }
    }
}

impl Check for ScanGate {
    /// Gate a `PreToolUse(Skill)` invocation of a pipeline skill on the grain
    /// model's presence. Always strict — no mode.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        let cwd = if ctx.project_dir.is_empty() {
            input.cwd.as_deref().unwrap_or(".")
        } else {
            ctx.project_dir.as_str()
        };
        Ok(scan_verdict(input, cwd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
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
            workspace_root: None,
        };
        (input, ctx)
    }

    /// Write a minimal `grain.model.json` under `<dir>/.claude/`.
    fn write_grain_model(dir: &Path) {
        let claude = dir.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("grain.model.json"), "{}").unwrap();
    }

    // --- parity (the pipeline needs the grain model) -----------------------

    #[test]
    fn blocks_pipeline_skill_when_model_missing() {
        let dir = tempdir().unwrap();
        let (input, ctx) = skill_input("feature", dir.path().to_str().unwrap());
        let verdict = ScanGate.evaluate(&input, &ctx).expect("no error");
        assert!(verdict.is_blocking(), "missing model must block");
    }

    #[test]
    fn allows_non_pipeline_skill() {
        let dir = tempdir().unwrap();
        let (input, ctx) = skill_input("some-random-skill", dir.path().to_str().unwrap());
        assert_eq!(
            ScanGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn allows_namespaced_pipeline_skill_when_model_present() {
        let dir = tempdir().unwrap();
        write_grain_model(dir.path());
        let (input, ctx) =
            skill_input("mustard:feature", dir.path().to_str().unwrap());
        assert_eq!(
            ScanGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
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
            workspace_root: None,
        };
        assert_eq!(
            ScanGate.evaluate(&input, &ctx).expect("no error"),
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
            workspace_root: None,
        };
        assert_eq!(
            ScanGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }
}
