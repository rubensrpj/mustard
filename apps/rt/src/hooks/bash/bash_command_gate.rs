//! `bash_command_gate` — the Bash-tool family dispatcher.
//!
//! ## Scope (b3 Bash family, 5/5)
//!
//! The five Bash-tool concerns live in sibling modules, one behavior each:
//!
//! - [`safety`] — the destructive-ops residue (structural predicates only;
//!   the law's home is `settings.json permissions.deny`).
//! - [`windows_redirect`] — deny `> C:\...` style redirects the POSIX shell
//!   would mangle.
//! - [`native_redirect`] — deny/advise native-tool equivalents for shell
//!   reads (`grep`/`ls`/`cat` → Grep/Glob/Read).
//! - [`rtk_rewrite`] — rewrite commands through RTK (the Golden Rule).
//! - [`review_gate`] — validate before `git commit` (its own
//!   `MUSTARD_COMMIT_GATE_MODE`, default `warn`).
//! - [`pr_detect`] — DORA telemetry on `gh pr` commands (PostToolUse).
//! - [`pr_qa_gate`] — advisory when a `gh pr create`/`merge` integrates a spec
//!   with no passing `qa.result` (the QA ↔ integration coupling).
//!
//! This module is the ORCHESTRATION face only: it implements [`Check`] for
//! PreToolUse(Bash) and [`Observer`] for PostToolUse(Bash), calling the
//! siblings in the exact historical order — `safety` → `windows-redirect` →
//! `native-redirect` → `rtk-rewrite` → `review-gate`. The first gate to reach
//! a decisive verdict wins; gates that pass return `None` and the next runs.
//! No re-exports — callers needing a specific gate use its module directly.

use crate::shared::context::current_spec;
use mustard_core::domain::economy::estimator;
use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::time::now_iso8601;
use serde_json::json;

use super::{
    native_redirect, pr_detect, pr_qa_gate, review_gate, rtk_rewrite, safety, windows_redirect,
};

/// The consolidated Bash-tool enforcement module (dispatcher).
pub struct BashCommandGate;

impl BashCommandGate {
    /// Pull the `command` string out of a Bash tool input.
    fn command_of(input: &HookInput) -> Option<String> {
        input
            .tool_input
            .get("command")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

impl Check for BashCommandGate {
    /// Run the PreToolUse(Bash) gates in `bash-safety` →
    /// `bash-windows-redirect` → `bash-native-redirect` → `rtk-rewrite` →
    /// `review-gate` order.
    ///
    /// `bash-safety` is the non-negotiable gate (it has no mode — always
    /// strict). `review-gate` runs last and only fires on `git commit` — it
    /// computes its verdict with its own `MUSTARD_COMMIT_GATE_MODE`,
    /// independent of the module enforcement mode the dispatcher applies.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Only PreToolUse(Bash) is a gate.
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() != Some("Bash") {
            return Ok(Verdict::Allow);
        }
        let Some(cmd) = Self::command_of(input) else {
            return Ok(Verdict::Allow);
        };

        // `bash-safety` is checked first: a dangerous command must be denied
        // regardless of any redirect/rewrite advice.
        if let Some(verdict) = safety::bash_safety(&cmd) {
            return Ok(verdict);
        }
        // `bash-windows-redirect`: catch `> C:\...` style redirects before the
        // POSIX shell mangles them into junk filenames in the CWD.
        if let Some(verdict) = windows_redirect::bash_windows_redirect(&cmd) {
            return Ok(verdict);
        }
        if let Some(verdict) = native_redirect::bash_native_redirect(&cmd) {
            return Ok(verdict);
        }
        if let Some((verdict, coverage)) = rtk_rewrite::rtk_rewrite(&cmd) {
            // Emit `rtk-rewrite` telemetry before returning. Best-effort —
            // a store failure must never block the tool call.
            if let Verdict::Rewrite { ref tool_input } = verdict {
                let rewritten = tool_input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let spec_slug = current_spec(&ctx.project_dir);
                // Emit a `pipeline.economy.savings.rtk-rewrite` NDJSON event
                // (W3A: SQLite savings writes → NDJSON). Tokens we did NOT have
                // to ship as a verbose Bash response because `rtk` summarised
                // the command. `RtkRewrite` bucket — `BashCommandGateBlock` is
                // reserved for deny verdicts so the dashboard can surface
                // "rewrites vs blocks" without conflating the two.
                {
                    let model = std::env::var("CLAUDE_MODEL").unwrap_or_default();
                    let saved = i64::from(estimator::estimate_input_tokens(&cmd, &model));
                    let saved = saved.max(1);
                    let savings_event = HarnessEvent {
                        v: SCHEMA_VERSION,
                        ts: now_iso8601(),
                        session_id: input.session_id.as_deref().unwrap_or("unknown").to_string(),
                        wave: 0,
                        actor: Actor {
                            kind: ActorKind::Hook,
                            id: Some("bash_guard".to_string()),
                            actor_type: None,
                        },
                        event: "pipeline.economy.savings.rtk-rewrite".to_string(),
                        payload: json!({
                            "source": "RtkRewrite",
                            "tokens_saved": saved,
                            "spec_id": spec_slug.clone(),
                            "wave_id": std::env::var("MUSTARD_ACTIVE_WAVE").ok().filter(|s| !s.is_empty()),
                            "agent_id": "bash_guard",
                        }),
                        spec: spec_slug.clone(),
                    };
                    let _ = crate::shared::events::route::emit(&ctx.project_dir, &savings_event);
                }
                // Harness event for downstream readers.
                let event = HarnessEvent {
                    v: SCHEMA_VERSION,
                    ts: now_iso8601(),
                    session_id: input.session_id.as_deref().unwrap_or("unknown").to_string(),
                    wave: 0,
                    actor: Actor {
                        kind: ActorKind::Hook,
                        id: Some("rtk-rewrite".to_string()),
                        actor_type: None,
                    },
                    event: "rtk-rewrite".to_string(),
                    payload: json!({
                        "event": "rtk-rewrite",
                        "tokens_affected": i64::try_from(cmd.len()).unwrap_or(i64::MAX),
                        "note": "rewritten via rtk",
                        "coverage": coverage,
                        "command_head": &cmd[..cmd.len().min(60)],
                        "rewritten_head": &rewritten[..rewritten.len().min(60)],
                    }),
                    spec: spec_slug,
                };
                // `rtk-rewrite` is non-pipeline → NDJSON via W5 router.
                let _ = crate::shared::events::route::emit(&ctx.project_dir, &event);
            }
            return Ok(verdict);
        }
        if let Some(verdict) = review_gate::review_gate(&cmd, ctx, review_gate::commit_gate_mode()) {
            return Ok(verdict);
        }
        // `pr-qa-gate` runs LAST: it must see the command AFTER `rtk_rewrite`
        // had its say (an unprefixed `gh pr …` is rewritten first, and the
        // re-issued `rtk gh pr …` reaches here — `classify_pr` sees through the
        // wrapper). Advisory only; never blocks integration.
        if let Some(verdict) = pr_qa_gate::pr_qa_gate(&cmd, &ctx.project_dir) {
            return Ok(verdict);
        }
        Ok(Verdict::Allow)
    }
}

impl Observer for BashCommandGate {
    /// `pr-detect`: emit a DORA `pr.opened` / `pr.merged` event when a
    /// `gh pr create` / `gh pr merge` command succeeds on PostToolUse(Bash).
    ///
    /// Pure telemetry — never affects a verdict. Fail-open throughout.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if input.tool_name.as_deref() != Some("Bash") {
            return;
        }
        let Some(cmd) = Self::command_of(input) else {
            return;
        };
        let Some(event) = pr_detect::classify_pr(&cmd) else {
            return;
        };
        // Only emit on success — a non-zero exit code suppresses the event.
        if pr_detect::bash_failed(input) {
            return;
        }
        let session = input.session_id.as_deref();
        pr_detect::emit_pr_event(&ctx.project_dir, session, event, &cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn pre_bash(command: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": command }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    /// Run the `Check` for a PreToolUse(Bash) command. Forces the rtk gate off
    /// (see `rtk_rewrite::RTK_REWRITE_TEST_OVERRIDE`) so chain tests exercise
    /// the other gates deterministically.
    fn verdict_for(command: &str) -> Verdict {
        rtk_rewrite::RTK_REWRITE_TEST_OVERRIDE.with(|c| c.set(true));
        let (input, ctx) = pre_bash(command);
        BashCommandGate.evaluate(&input, &ctx).expect("check never errors")
    }

    // --- dispatch order ------------------------------------------------------

    /// The safety residue is the FIRST gate: its deny wins over every
    /// downstream advice, and the reason carries the rule id.
    #[test]
    fn safety_deny_wins_first_in_chain() {
        match verdict_for("rm -rvf /tmp/work") {
            Verdict::Deny { reason } => assert!(reason.contains("BG01"), "reason: {reason}"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    /// End-to-end force-push law through the dispatcher: the reordered force
    /// push is caught by the residue, while `--force-with-lease` (the safe
    /// form the product allows) passes the whole chain.
    #[test]
    fn force_push_denied_lease_allowed_through_chain() {
        assert!(verdict_for("git push origin dev --force").is_blocking());
        assert!(!verdict_for("git push --force-with-lease origin dev").is_blocking());
    }

    /// The windows-redirect gate runs BEFORE native-redirect: `cat … > C:\…`
    /// would also be denied by native-redirect (cat → Read), but the
    /// Windows-path gate wins with its more specific reason.
    #[test]
    fn windows_redirect_gate_wins_over_native_redirect() {
        let v = verdict_for("cat src/main.rs > C:\\Atiz\\dump.txt");
        match v {
            Verdict::Deny { reason } => assert!(
                reason.contains("bash-windows-redirect"),
                "expected windows-redirect reason first, got: {reason}"
            ),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    /// A tree-scan `grep -r` flows past safety/windows and reaches the
    /// native-redirect gate.
    #[test]
    fn native_redirect_reached_through_chain() {
        match verdict_for("grep -r pattern src/") {
            Verdict::Deny { reason } => assert!(reason.contains("Grep"), "reason: {reason}"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    /// Non-commit commands pass the full chain without blocking (the review
    /// gate only fires on `git commit`; rtk gate is forced off in tests).
    #[test]
    fn non_commit_commands_pass_the_chain() {
        assert!(!verdict_for("git status").is_blocking());
        assert!(!verdict_for("npm run build").is_blocking());
    }

    // --- gate routing --------------------------------------------------------

    #[test]
    fn non_bash_tool_allows() {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            BashCommandGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        // The gate only runs on PreToolUse — any other trigger self-allows.
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "rm -rf /" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        assert_eq!(
            BashCommandGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    // --- pr-detect Observer wiring -------------------------------------------

    /// The `Observer` only emits on a successful PostToolUse(Bash) `gh pr`
    /// command — a non-zero `exit_code` suppresses it, and a non-PostToolUse
    /// trigger is a no-op. (Smoke test: `observe` is infallible.)
    #[test]
    fn pr_detect_observer_is_infallible() {
        let dir = tempdir().unwrap();
        let ctx = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        let ok = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "gh pr create --fill" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        // Must not panic; emits an event to the temp project's harness log.
        BashCommandGate.observe(&ok, &ctx);

        let failed = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "gh pr create --fill" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": { "exit_code": 1 } }),
            ..HookInput::default()
        };
        assert!(pr_detect::bash_failed(&failed));
        // Failed command → observer is a no-op (no panic, nothing emitted).
        BashCommandGate.observe(&failed, &ctx);
    }

    /// The civil-date timestamp is well-formed (`YYYY-MM-DDThh:mm:ss.sssZ`).
    #[test]
    fn iso8601_timestamp_is_well_formed() {
        let ts = now_iso8601();
        assert_eq!(ts.len(), 24, "ts: {ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }
}
