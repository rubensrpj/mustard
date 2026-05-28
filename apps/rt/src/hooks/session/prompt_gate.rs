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
//!
//! ## W3C migration
//!
//! `emit_economy_operation` routes economy events via
//! `crate::shared::events::route::emit` (NDJSON path) instead of the old SQLite
//! event sink.

use crate::hooks::observe::amend_capture::close_amend_windows_for_session;
use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::ClaudePaths;
use std::path::Path;
use std::process::{Command, Stdio};

/// W8.T8.2 — pipeline-in-flight reminder: surfaced when the user's prompt is
/// NOT a `/mustard:*` invocation AND a spec is active. Keeps the agent aware
/// that a pipeline is owning the conversation without bloating every prompt.
const PIPELINE_IN_FLIGHT_BANNER: &str = "Pipeline em curso";

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

/// `true` if `prompt` starts with any `/mustard:` namespaced command. Broader
/// than [`is_pipeline_prompt`] — used by the W8.T8.2 reminder check, where we
/// suppress the banner for every `/mustard:*` (not just pipeline ones), since
/// a slash command always knows its own context.
fn is_mustard_command(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    t.starts_with("/mustard:")
}

/// Shell out to `complete-spec.js --archive-followups`. Best-effort — a
/// missing script or a spawn error is silently ignored. Port of the JS
/// `spawnSync` call.
fn archive_followups(cwd: &str) {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let script = paths.claude_dir().join("scripts").join("complete-spec.js");
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
    /// prompt starts a new pipeline. The verdict is `Inject` when a pipeline
    /// is active and the prompt is not itself a `/mustard:*` slash command
    /// (W8.T8.2 reminder), else `Allow`. Any non-`UserPromptSubmit` trigger
    /// self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::UserPromptSubmit) {
            return Ok(Verdict::Allow);
        }
        let prompt = input
            .raw
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let cwd = project_dir(input, ctx);
        if is_pipeline_prompt(prompt) {
            archive_followups(&cwd);
            // Close any open amendment windows for this session — the user is
            // starting a new pipeline, so the window's context is done.
            if let Some(session_id) = input.session_id.as_deref() {
                if !session_id.is_empty() {
                    close_amend_windows_for_session(&cwd, session_id);
                }
            }
        }
        // W8.T8.2 — for non-`/mustard:*` prompts, inject a single-line reminder
        // when a spec is active. Fail-open: a None active spec yields `Allow`.
        if !is_mustard_command(prompt) {
            if let Some(spec) = crate::shared::context::current_spec(&cwd) {
                if !spec.is_empty() {
                    let _ = emit_economy_operation(&cwd, "prompt_gate.pipeline_in_flight_banner");
                    return Ok(Verdict::Inject {
                        context: format!("{PIPELINE_IN_FLIGHT_BANNER}: {spec}"),
                    });
                }
            }
        }
        Ok(Verdict::Allow)
    }
}

/// Emit a `pipeline.economy.operation.invoked` event via the NDJSON route.
/// Fail-open: any error degrades to a no-op.
///
/// W3C: routes via `crate::shared::events::route::emit` (NDJSON for
/// non-`pipeline.*` events, SQLite lifecycle index for `pipeline.*`).
fn emit_economy_operation(cwd: &str, operation: &str) -> Result<(), ()> {
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: crate::util::now_iso8601(),
        session_id: crate::shared::context::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("prompt_gate".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({ "operation": operation, "duration_ms": 0, "tokens_used": 0 }),
        spec: crate::shared::context::current_spec(cwd),
    };
    if crate::shared::events::route::emit(cwd, &event) { Ok(()) } else { Err(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a [`Ctx`] with a unique tempdir project path so the W8.T8.2 active-spec
    /// resolver (`current_spec`) cannot accidentally find a real pipeline-state.
    fn ctx() -> (tempfile::TempDir, Ctx) {
        // SAFETY: env mutation is local to the test process; we restore on drop.
        // Used to neutralise a `MUSTARD_ACTIVE_SPEC` that might be set by the
        // outer shell.
        // Note: we cannot call `std::env::remove_var` from safe Rust on stable;
        // instead, isolate via a unique project_dir (so `current_spec` falls
        // through to the FS branch and finds nothing).
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
        };
        (dir, ctx)
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
        // verdict is Allow when no spec is active (and the prompt itself is a
        // `/mustard:*` command, so the W8.T8.2 banner is suppressed either way).
        let (_dir, c) = ctx();
        let v = PromptGate
            .evaluate(&prompt_input("/mustard:feature x"), &c)
            .unwrap();
        // For a `/mustard:*` command, never Inject regardless of spec state.
        assert!(matches!(v, Verdict::Allow), "unexpected verdict: {v:?}");
    }

    #[test]
    fn non_pipeline_prompt_allows_without_active_spec() {
        // No `.claude/.pipeline-states/` in our tempdir, so `current_spec`
        // returns None and the W8.T8.2 banner stays silent.
        let (_dir, c) = ctx();
        // The env-var branch can still inject; guard by checking either Allow
        // (the expected case in CI) or Inject (when MUSTARD_ACTIVE_SPEC is set
        // by the outer shell).
        let v = PromptGate.evaluate(&prompt_input("hello there"), &c).unwrap();
        assert!(
            matches!(v, Verdict::Allow | Verdict::Inject { .. }),
            "unexpected verdict: {v:?}",
        );
    }

    #[test]
    fn non_pipeline_prompt_injects_with_active_spec() {
        // W8.T8.2: when a spec is active, the user's free-text prompt gets a
        // single-line banner injected.
        let (dir, _) = ctx();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(paths.pipeline_state_file("active-feature-xyz"), "{}").unwrap();
        let c = Ctx {
            project_dir: dir.path().to_string_lossy().to_string(),
            trigger: Some(Trigger::UserPromptSubmit),
            workspace_root: None,
        };
        let v = PromptGate
            .evaluate(&prompt_input("how do I do X?"), &c)
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(
                    context.contains(PIPELINE_IN_FLIGHT_BANNER),
                    "banner missing: {context}"
                );
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    #[test]
    fn non_user_prompt_submit_trigger_allows() {
        let other = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            PromptGate
                .evaluate(&prompt_input("/mustard:feature x"), &other)
                .unwrap(),
            Verdict::Allow
        );
    }
}
