//! `prompt_gate` — the UserPromptSubmit gate module.
//!
//! ## Scope (b3 Wave 5, prompt family)
//!
//! Ports `followup-cancel-gate.js` **alone** — a single concern with no
//! sibling hook to merge. It triggers on `UserPromptSubmit` and, when the
//! prompt invokes `/mustard:feature`, `/mustard:bugfix`, or `/mustard:task`,
//! archives any pending `closed-followup` pipeline-state — the previous
//! follow-up window is over, so subsequent edits belong to a new context.
//!
//! ## Contract shape
//!
//! `followup-cancel-gate.js` never blocks — it always `process.exit(0)`. Its
//! one job is the archival *side effect*. The b3 spec classes `prompt_gate` as
//! a [`Check`]; this port honours that — `evaluate` performs the archival and
//! always returns [`Verdict::Allow`]. (It is a `Check`, not an `Observer`,
//! because `UserPromptSubmit` is the seam where a future prompt gate *could*
//! deny; modelling it as a `Check` keeps that extension point open without
//! changing today's always-allow verdict.)
//!
//! ## Archival mechanism
//!
//! The archival itself is done by the B4 script
//! `.claude/scripts/complete-spec.js --archive-followups` — a JavaScript
//! script that is out of bounds for b3 and intentionally still JS. This port
//! shells out to it exactly as the JS hook's `spawnSync` did. When the script
//! is absent the gate is a silent no-op — parity with the JS
//! `if (!fs.existsSync(script)) process.exit(0)`.

use mustard_core::error::Error;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use std::path::Path;
use std::process::{Command, Stdio};

/// The UserPromptSubmit gate module.
pub struct PromptGate;

/// Resolve the project dir for an invocation: the harness `cwd`, else `.`.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

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

/// Shell out to `complete-spec.js --archive-followups`. Best-effort — a
/// missing script or a spawn error is silently ignored. Port of the JS
/// `spawnSync` call.
fn archive_followups(cwd: &str) {
    let script = Path::new(cwd)
        .join(".claude")
        .join("scripts")
        .join("complete-spec.js");
    if !script.exists() {
        return;
    }
    // The JS uses `process.execPath` (the node/bun runtime); the Rust port has
    // no such handle, so it invokes the runtime by name — `bun` then `node`.
    for runtime in ["bun", "node"] {
        let spawned = Command::new(runtime)
            .arg(&script)
            .arg("--archive-followups")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            let _ = child.wait();
            return;
        }
    }
}

impl Check for PromptGate {
    /// On `UserPromptSubmit`, archive pending `closed-followup` specs when the
    /// prompt starts a new pipeline. Always returns `Verdict::Allow` — this
    /// gate never blocks (parity with `followup-cancel-gate.js`). Any
    /// non-`UserPromptSubmit` trigger self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::UserPromptSubmit) {
            return Ok(Verdict::Allow);
        }
        let prompt = input
            .raw
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if is_pipeline_prompt(prompt) {
            let cwd = project_dir(input, ctx);
            archive_followups(&cwd);
        }
        Ok(Verdict::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> Ctx {
        Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
        }
    }

    fn prompt_input(prompt: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            raw: json!({ "prompt": prompt }),
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
        // The archival side effect is a no-op without complete-spec.js; the
        // verdict is always Allow.
        assert_eq!(
            PromptGate
                .evaluate(&prompt_input("/mustard:feature x"), &ctx())
                .unwrap(),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pipeline_prompt_allows() {
        assert_eq!(
            PromptGate
                .evaluate(&prompt_input("hello there"), &ctx())
                .unwrap(),
            Verdict::Allow
        );
    }

    #[test]
    fn non_user_prompt_submit_trigger_allows() {
        let other = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        assert_eq!(
            PromptGate
                .evaluate(&prompt_input("/mustard:feature x"), &other)
                .unwrap(),
            Verdict::Allow
        );
    }
}
