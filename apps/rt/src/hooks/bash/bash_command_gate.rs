//! `bash_command_gate` — the Bash-tool family dispatcher.
//!
//! The Bash-tool concerns live in sibling modules, one behavior each:
//!
//! - [`lex`] — reads the command the way the terminal splits it.
//! - [`safety`] — the command guard: refuses the commands that destroy work.
//! - [`windows_redirect`] — deny `> C:\...` style redirects the POSIX shell
//!   would mangle.
//! - [`native_redirect`] — deny/advise native-tool equivalents for shell
//!   reads (`grep`/`ls`/`cat` → Grep/Glob/Read).
//! - [`review_gate`] — validate before `git commit` (its own
//!   `MUSTARD_COMMIT_GATE_MODE`, default `warn`).
//! - [`pr_detect`] — DORA telemetry on `gh pr` commands (PostToolUse).
//!   with no passing `qa.result` (the QA ↔ integration coupling).
//!
//! Rewriting a command to `rtk` is not done here: rtk's own hook does it.
//!
//! This module is the ORCHESTRATION face only: it implements [`Check`] for
//! PreToolUse(Bash) and [`Observer`] for PostToolUse(Bash), calling the
//! siblings in order — command guard → Windows path → native redirect →
//! commit review → pull-request advisories. The first gate to reach a
//! decisive verdict wins; gates that pass return `None` and the next runs.
//! No re-exports — callers needing a specific gate use its module directly.

use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};

use super::{lex, native_redirect, pr_detect, review_gate, safety, windows_redirect};

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
    /// Run the PreToolUse(Bash) gates: command guard → Windows path → native
    /// redirect → commit review → pull-request advisories.
    ///
    /// The command guard is the non-negotiable gate (it has no mode — always
    /// strict). The commit review only fires on `git commit` — it computes its
    /// verdict with its own `MUSTARD_COMMIT_GATE_MODE`, independent of the
    /// module enforcement mode the dispatcher applies.
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

        // The command is read once, the way the terminal splits it; the
        // command guard and the Windows-path check both look at those
        // commands, never at the raw text.
        let segments = lex::segments(&cmd);
        // The command guard is checked first: a command that destroys work
        // is denied regardless of any redirect advice.
        if let Some(verdict) = safety::bash_safety(&segments, &cmd, ctx) {
            return Ok(verdict);
        }
        // Catch `> C:\...` style redirects before the POSIX shell mangles them
        // into junk filenames in the CWD.
        let lang = ctx.config.language().text_or_default();
        if let Some(verdict) = windows_redirect::bash_windows_redirect(&segments, &cmd, lang) {
            return Ok(verdict);
        }
        if let Some(verdict) = native_redirect::bash_native_redirect(&cmd) {
            return Ok(verdict);
        }
        if let Some(verdict) = review_gate::review_gate(&cmd, ctx, review_gate::commit_gate_mode()) {
            return Ok(verdict);
        }
        // Os avisos de pull request saíram daqui. Eles olhavam o `gh pr` que
        // alguém digitasse, e por isso só alcançavam quem abrisse ou
        // mergeasse pela linha de comando do provedor — a porta do Mustard,
        // que é por onde a obra passa, não dizia nada. O aviso dos critérios
        // mora agora no `pr-open` e no `pr-merge`, junto da ação que ele
        // qualifica; o do corpo do pull request perdeu o objeto quando o corpo
        // passou a ser montado pelo binário a cada rodada.
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
    use mustard_core::time::now_iso8601;
    use mustard_core::SupportedLocale;
    use serde_json::json;
    use tempfile::tempdir;

    fn pre_bash(command: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": command }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PreToolUse));
        (input, ctx)
    }

    /// Run the `Check` for a PreToolUse(Bash) command.
    fn verdict_for(command: &str) -> Verdict {
        let (input, ctx) = pre_bash(command);
        BashCommandGate.evaluate(&input, &ctx).expect("check never errors")
    }

    // --- dispatch order ------------------------------------------------------

    /// The command guard is the FIRST gate: its refusal wins over every
    /// downstream advice, and names the danger it found.
    #[test]
    fn the_command_guard_refusal_comes_first_in_the_chain() {
        let danger = mustard_core::translate("command_guard.rm_recursive_force", SupportedLocale::PtBr);
        match verdict_for("rm -rvf /tmp/work") {
            Verdict::Deny { reason } => assert!(reason.contains(danger), "reason: {reason}"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    /// End-to-end force push through the dispatcher: `--force` written after
    /// the branch is still a force push and is refused, while
    /// `--force-with-lease` (the safe form the product allows) passes the
    /// whole chain.
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
        let cmd = "cat src/main.rs > C:\\Atiz\\dump.txt";
        let expected = mustard_core::translate("command_guard.windows_path", SupportedLocale::PtBr)
            .replace("{target}", "C:\\Atiz\\dump.txt")
            .replace("{command}", cmd);
        match verdict_for(cmd) {
            Verdict::Deny { reason } => assert_eq!(reason, expected, "expected windows-redirect reason first"),
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
    /// gate only fires on `git commit`).
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
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PreToolUse));
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
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PostToolUse));
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
        let ctx = Ctx::for_test(dir.path().to_string_lossy().into_owned(), Some(Trigger::PostToolUse));
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
