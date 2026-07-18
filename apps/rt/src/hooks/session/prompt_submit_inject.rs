//! `prompt_submit_inject` — the UserPromptSubmit gate module.
//!
//! ## Scope (b3 Wave 5, prompt family + orchestrator-redesign injectables)
//!
//! Three concerns ride `UserPromptSubmit`, in this order:
//!
//! - **installation gate** (orchestrator-redesign): a `/mustard:*` command in
//!   a project with NO `mustard.json` at the root is denied with a didactic
//!   pointer to `/mustard:upsert` — the one command exempted (it is the
//!   bootstrap door; the bare `/mustard` help never matches the `/mustard:`
//!   prefix and passes too). The gate runs BEFORE the injectables: without an
//!   installation there is nothing to inject. A free-text prompt is never
//!   gated — the hooks stay silent on uninstalled projects.
//! - `followup-cancel-gate` (the b3 port): when the prompt invokes
//!   `/mustard:feature`, `/mustard:bugfix`, or `/mustard:task`, close any open
//!   per-session amendment window — the previous follow-up window is over, so
//!   subsequent edits belong to a new context.
//! - **declared injectables** (orchestrator-redesign): the
//!   `mustard.json#inject` entries with `on: userPromptSubmit` (canonically
//!   the orchestrator rules in `.claude/mustard/orchestrator.md`) are spliced
//!   into the window via [`crate::hooks::session::injectables::collect`] —
//!   once per session when `once: true`. A `/mustard:*` prompt gets NO
//!   injectables (the slash command is already inside the flow). The
//!   injectable text and the W8.T8.2 banner compose into a SINGLE
//!   [`Verdict::Inject`] (the dispatcher fold is last-writer-wins, so two
//!   separate Injects would drop one): injectables first, banner after.
//!
//! ## Contract shape
//!
//! `followup-cancel-gate.js` never blocked — it always `process.exit(0)`. The
//! b3 spec classes `prompt_gate` as a [`Check`], which is exactly why the
//! installation gate could land here: `UserPromptSubmit` is the seam where a
//! prompt gate denies, and `main.rs` maps a [`Verdict::Deny`] on this event to
//! the harness's `{"decision": "block", "reason": …}` shape. Every other path
//! still allows.
//!
//! ## Single-stage close
//!
//! The old `closed-followup` archival sweep was removed with the single-stage
//! close (a spec now goes straight to `completed`, with no follow-up grace
//! window to archive). What remains on a new-pipeline prompt is closing the
//! session's amendment window.
//!
//! ## W3C migration
//!
//! `emit_economy_operation` routes economy events via
//! `crate::shared::events::route::emit` (NDJSON path) instead of the old SQLite
//! event sink.

use mustard_core::domain::model::event::ActorKind;
use crate::shared::events::economy;
use crate::hooks::observe::amend_window_inject::close_amend_windows_for_session;
use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::ProjectConfig;
use std::path::Path;

/// W8.T8.2 — pipeline-in-flight reminder: surfaced when the user's prompt is
/// NOT a `/mustard:*` invocation AND a spec is active. Keeps the agent aware
/// that a pipeline is owning the conversation without bloating every prompt.
const PIPELINE_IN_FLIGHT_BANNER: &str = "Pipeline em curso";

/// The UserPromptSubmit gate module.
pub struct PromptSubmitInject;


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
/// a slash command always knows its own context. The bare `/mustard` help
/// (no colon) deliberately does NOT match: it is the orientation door and
/// must keep working on an uninstalled project.
fn is_mustard_command(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    t.starts_with("/mustard:")
}

/// `true` if `prompt` invokes `/mustard:upsert` — the bootstrap door the
/// installation gate exempts. Same word-boundary rule as
/// [`is_pipeline_prompt`], so `/mustard:upsertish` does not sneak through.
fn is_upsert_prompt(prompt: &str) -> bool {
    let t = prompt.trim_start().to_ascii_lowercase();
    let Some(rest) = t.strip_prefix("/mustard:") else {
        return false;
    };
    const CMD: &str = "upsert";
    rest.starts_with(CMD)
        && rest
            .as_bytes()
            .get(CMD.len())
            .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'))
}

/// The installation-gate refusal (didactic, short, technical EN).
const NOT_INSTALLED_REASON: &str = "Mustard is not installed in this project (no mustard.json at \
     the root). Run /mustard:upsert to install it — everything else stays disabled until then.";

impl Check for PromptSubmitInject {
    /// On `UserPromptSubmit`: first the installation gate — a `/mustard:*`
    /// command (except `/mustard:upsert`) is denied when the project has no
    /// `mustard.json` at the root. Then close the session's amendment window
    /// when the prompt starts a new pipeline. For a non-`/mustard:*` prompt
    /// the verdict composes the declared injectables (`mustard.json#inject`,
    /// `on: userPromptSubmit`) and the W8.T8.2 pipeline-in-flight banner into
    /// ONE `Inject` — injectables first, banner after; either alone also
    /// injects. A `/mustard:*` prompt never injects (it is already inside the
    /// flow). Any non-`UserPromptSubmit` trigger self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::UserPromptSubmit) {
            return Ok(Verdict::Allow);
        }
        let prompt = input
            .raw
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let cwd = ctx.project_dir_or_cwd(input);
        // Installation gate — BEFORE everything else (without an installation
        // there is no amend window to close and nothing to inject): any
        // `/mustard:*` command except the bootstrap door `/mustard:upsert` is
        // denied when `mustard.json` is absent from the project root. Normal
        // prompts are never gated — the hooks stay silent on uninstalled
        // projects.
        if is_mustard_command(prompt)
            && !is_upsert_prompt(prompt)
            && !ProjectConfig::exists(Path::new(&cwd))
        {
            return Ok(Verdict::Deny { reason: NOT_INSTALLED_REASON.to_string() });
        }
        if is_pipeline_prompt(prompt) {
            // Close any open amendment windows for this session — the user is
            // starting a new pipeline, so the window's context is done.
            if let Some(session_id) = input.session_id.as_deref() {
                if !session_id.is_empty() {
                    close_amend_windows_for_session(&cwd, session_id);
                }
            }
        }
        // A `/mustard:*` prompt receives neither injectables nor the banner —
        // a slash command always knows its own context.
        if is_mustard_command(prompt) {
            return Ok(Verdict::Allow);
        }
        // Declared injectables (`on: userPromptSubmit`) — fail-open; `once`
        // entries are tracked per session via `injected-*` markers.
        let injected = crate::hooks::session::injectables::collect(
            &cwd,
            input.session_id.as_deref(),
            "userpromptsubmit",
            false,
        );
        // W8.T8.2 — inject a single-line reminder when a spec is active. The
        // per-prompt entrypoints census that used to fill the no-spec branch
        // was REMOVED: lexical prompt-token × path-token matching measured 1
        // useful hit in 17 across two field sessions — location is on-demand
        // work (Grep for literals, the digest for concepts), not a per-prompt
        // guess. Fail-open throughout.
        let banner = crate::shared::context::current_spec(&cwd)
            .filter(|s| !s.is_empty())
            .map(|spec| {
                economy::emit(&cwd, ActorKind::Hook, "prompt_gate", "pipeline.economy.operation.invoked", None, serde_json::json!({"operation": "prompt_gate.pipeline_in_flight_banner", "duration_ms": 0, "tokens_used": 0}));
                format!("{PIPELINE_IN_FLIGHT_BANNER}: {spec}")
            });
        // ONE composed Inject — the dispatcher fold is last-writer-wins, so
        // the concerns must share a verdict. Injectables first, banner after.
        let context = match (injected, banner) {
            (Some(inj), Some(ban)) => Some(format!("{inj}\n\n{ban}")),
            (Some(inj), None) => Some(inj),
            (None, Some(ban)) => Some(ban),
            (None, None) => None,
        };
        Ok(match context {
            Some(context) => Verdict::Inject { context },
            None => Verdict::Allow,
        })
    }
}

/// Emit a `pipeline.economy.operation.invoked` event via the NDJSON route.
/// Fail-open: any error degrades to a no-op.
///
/// W3C: routes via `crate::shared::events::route::emit` (NDJSON for
/// non-`pipeline.*` events, SQLite lifecycle index for `pipeline.*`).

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::ClaudePaths;

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
            raw: serde_json::json!({ "prompt": prompt }),
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
        // The amendment-window close is a no-op without an open window; the
        // verdict is Allow when no spec is active (and the prompt itself is a
        // `/mustard:*` command, so the W8.T8.2 banner is suppressed either way).
        // The project is INSTALLED (mustard.json present) so the installation
        // gate stays out of the way — this is the historical behavior.
        let (dir, c) = ctx();
        std::fs::write(dir.path().join("mustard.json"), "{}").unwrap();
        let v = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:feature x"), &c)
            .unwrap();
        // For a `/mustard:*` command, never Inject regardless of spec state.
        assert!(matches!(v, Verdict::Allow), "unexpected verdict: {v:?}");
    }

    // --- installation gate --------------------------------------------------

    #[test]
    fn gate_denies_mustard_command_without_installation() {
        // No mustard.json in the tempdir: any `/mustard:*` command (pipeline
        // or not) is denied with the didactic pointer to /mustard:upsert.
        let (_dir, c) = ctx();
        for prompt in ["/mustard:feature x", "/mustard:status", "  /MUSTARD:QA"] {
            let v = PromptSubmitInject.evaluate(&prompt_input(prompt), &c).unwrap();
            match v {
                Verdict::Deny { reason } => {
                    assert!(
                        reason.contains("/mustard:upsert"),
                        "reason must point at the bootstrap door: {reason}"
                    );
                    assert!(
                        reason.contains("mustard.json"),
                        "reason must name the missing anchor: {reason}"
                    );
                }
                other => panic!("expected Deny for {prompt:?} without mustard.json, got {other:?}"),
            }
        }
    }

    #[test]
    fn gate_allows_upsert_without_installation() {
        // The bootstrap door itself must pass — it is how the project gets
        // installed. Word-boundary: a hypothetical `/mustard:upsertish` is a
        // different (unknown) command and stays gated.
        let (_dir, c) = ctx();
        let v = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:upsert"), &c)
            .unwrap();
        assert_eq!(v, Verdict::Allow, "/mustard:upsert must pass the gate");
        let v = PromptSubmitInject
            .evaluate(&prompt_input("/mustard:upsertish"), &c)
            .unwrap();
        assert!(matches!(v, Verdict::Deny { .. }), "boundary must hold: {v:?}");
    }

    #[test]
    fn gate_allows_bare_mustard_help_without_installation() {
        // The bare `/mustard` (no colon) is the orientation door — it must
        // keep working so it can point the user at /mustard:upsert.
        let (_dir, c) = ctx();
        let v = PromptSubmitInject.evaluate(&prompt_input("/mustard"), &c).unwrap();
        assert!(
            matches!(v, Verdict::Allow | Verdict::Inject { .. }),
            "bare /mustard must not be denied: {v:?}"
        );
    }

    #[test]
    fn gate_ignores_normal_prompts_without_installation() {
        // Free-text prompts are never gated — the hooks stay silent on
        // uninstalled projects (Allow, or an env-var banner Inject; never Deny).
        let (_dir, c) = ctx();
        let v = PromptSubmitInject.evaluate(&prompt_input("hello there"), &c).unwrap();
        assert!(
            !matches!(v, Verdict::Deny { .. }),
            "a normal prompt must never be denied: {v:?}"
        );
    }

    #[test]
    fn non_pipeline_prompt_allows_without_active_spec() {
        // No `.claude/.pipeline-states/` in our tempdir, so `current_spec`
        // returns None and the W8.T8.2 banner stays silent.
        let (_dir, c) = ctx();
        // The env-var branch can still inject; guard by checking either Allow
        // (the expected case in CI) or Inject (when MUSTARD_ACTIVE_SPEC is set
        // by the outer shell).
        let v = PromptSubmitInject.evaluate(&prompt_input("hello there"), &c).unwrap();
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
        let v = PromptSubmitInject
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
            PromptSubmitInject
                .evaluate(&prompt_input("/mustard:feature x"), &other)
                .unwrap(),
            Verdict::Allow
        );
    }

    // --- declared injectables (orchestrator-redesign) ----------------------

    fn prompt_input_with_session(prompt: &str, session: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some(session.to_string()),
            raw: serde_json::json!({ "prompt": prompt }),
            ..HookInput::default()
        }
    }

    /// Declare one `on: userPromptSubmit, once: true` injectable + its file.
    fn seed_injectable(dir: &std::path::Path, body: &str) {
        std::fs::write(
            dir.join("mustard.json"),
            r#"{"inject":[{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true}]}"#,
        )
        .unwrap();
        let mustard_dir = dir.join(".claude").join("mustard");
        std::fs::create_dir_all(&mustard_dir).unwrap();
        std::fs::write(mustard_dir.join("orchestrator.md"), body).unwrap();
    }

    #[test]
    fn first_prompt_injects_declared_file_and_records_marker() {
        let (dir, c) = ctx();
        seed_injectable(dir.path(), "ORCH-RULES-BODY\n");

        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("how do I add a field?", "sess-1"), &c)
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("ORCH-RULES-BODY"), "injectable missing: {context}");
            }
            other => panic!("expected Inject with the declared file, got {other:?}"),
        }
        assert!(
            dir.path()
                .join(".claude/.session/sess-1/injected-orchestrator.md")
                .is_file(),
            "delivery marker must be recorded"
        );
    }

    #[test]
    fn second_prompt_same_session_does_not_repeat_once_injectable() {
        let (dir, c) = ctx();
        seed_injectable(dir.path(), "ORCH-RULES-BODY\n");
        let input = prompt_input_with_session("first question", "sess-1");
        let _ = PromptSubmitInject.evaluate(&input, &c).unwrap();

        // Same session, next prompt: the once-entry stays quiet. The verdict
        // may still be an Inject when the outer shell exports
        // MUSTARD_ACTIVE_SPEC (the W8.T8.2 banner) — assert on the CONTENT.
        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("second question", "sess-1"), &c)
            .unwrap();
        if let Verdict::Inject { context } = v {
            assert!(
                !context.contains("ORCH-RULES-BODY"),
                "once injectable must not re-deliver in the same session: {context}"
            );
        }
    }

    #[test]
    fn mustard_command_prompt_gets_no_injectables() {
        let (dir, c) = ctx();
        seed_injectable(dir.path(), "ORCH-RULES-BODY\n");
        // A `/mustard:*` prompt is already inside the flow — strict Allow, and
        // no delivery marker is burned (the next free-text prompt still gets it).
        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("/mustard:status", "sess-1"), &c)
            .unwrap();
        assert_eq!(v, Verdict::Allow, "slash command must not receive injectables");
        assert!(
            !dir.path()
                .join(".claude/.session/sess-1/injected-orchestrator.md")
                .exists(),
            "no marker burned on a slash-command prompt"
        );
    }

    #[test]
    fn missing_declared_file_stays_fail_open() {
        let (dir, c) = ctx();
        // Declared, but the file was never materialised on disk.
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"inject":[{"on":"userPromptSubmit","file":".claude/mustard/gone.md","once":true}]}"#,
        )
        .unwrap();
        let v = PromptSubmitInject
            .evaluate(&prompt_input_with_session("hello", "sess-1"), &c)
            .unwrap();
        // Allow in a clean environment; an env-var active spec may still
        // banner-inject — either way the missing file must not break the hook.
        assert!(
            matches!(v, Verdict::Allow | Verdict::Inject { .. }),
            "unexpected verdict: {v:?}"
        );
        assert!(
            !dir.path().join(".claude/.session/sess-1/injected-gone.md").exists(),
            "no marker for an undelivered entry"
        );
    }
}
