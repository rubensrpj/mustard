//! `waiting` — the command guard's third check, on top of [`super::safety`]
//! and [`super::windows_redirect`]: nothing an agent runs may get stuck.
//!
//! Two different things go wrong, and only one of them is refused:
//!
//! - **A loop that waits for another process.** `while` or `until`, with
//!   `pgrep`, `pidof` or `ps` anywhere between the keyword and the matching
//!   `done`, never ends on its own: the loop and the process it is waiting
//!   for can name each other's own search text and run forever. This is
//!   refused, with the instruction to run the command in the foreground
//!   instead.
//! - **A build or a test sent to the background**, through the Bash tool's
//!   `run_in_background` field or a `&` at the end of the line, and a test
//!   called by cargo's full path (`~/.cargo/bin/cargo test`), which escapes
//!   the separate `rtk` hook that only rewrites the bare `cargo`. Neither is
//!   refused: the request is rewritten to run in the foreground, with the
//!   command's own time limit, the same way [`crate::hooks::task::subagent_inject`]
//!   rewrites a dispatch prompt.
//! - **A cargo build or test with no time limit, or one under 600 seconds**,
//!   in the foreground already: the Bash tool's own default (120 seconds)
//!   sends a long suite to the background on its own. This is not refused
//!   either: the request is rewritten with the 600-second ceiling. A command
//!   that already carries a 600-second ceiling or more passes exactly as it
//!   came.
//!
//! It reads the commands [`super::lex::segments`] found, never the raw text:
//! `git commit -m "espera com pgrep"` names the danger inside a quote and
//! passes, same as the other two checks in the family.

use serde_json::Value;

use mustard_core::domain::model::contract::{HookInput, Verdict};
use mustard_core::{translate, SupportedLocale};

use super::lex::{truncate, Segment};

/// The command's own time limit, in milliseconds, once it stops waiting in
/// the background (the Bash tool's ceiling: `up to 600000ms / 10 minutes`).
const FOREGROUND_TIMEOUT_MS: u64 = 600_000;

/// `true` when `script` — the text handed to a shell (`bash -c "…"`), or a
/// leftover process's own command line — is a loop waiting for another
/// process. Reused by [`crate::commands::flow::stuck`], which reads a
/// running process's command line the same way this gate reads a proposed
/// one, so the two never learn the shape of a waiting loop differently.
pub(crate) fn is_a_waiting_loop(script: &str) -> bool {
    waits_in_a_loop(&super::lex::segments(script))
}

/// `true` when a `while` or `until` loop, anywhere in the command, tests for
/// another process with `pgrep`, `pidof` or `ps` — in its condition or in its
/// body, since either one can wait forever on a search text another loop
/// also uses. A `for` loop, or a bare `pgrep` with no loop around it, is not
/// this danger.
fn waits_in_a_loop(segments: &[Segment]) -> bool {
    let mut depth = 0u32;
    for seg in segments {
        if seg.leading.iter().any(|k| k == "while" || k == "until") {
            depth += 1;
        }
        if depth > 0 && matches!(seg.name(), "pgrep" | "pidof" | "ps") {
            return true;
        }
        if seg.leading.iter().any(|k| k == "done") {
            depth = depth.saturating_sub(1);
        }
    }
    false
}

/// The raw spelling of the `cargo` program when it was called by its full
/// path (`~/.cargo/bin/cargo`, `/home/x/.cargo/bin/cargo`), which the
/// separate `rtk` hook — matching the bare word `cargo` — does not catch.
fn full_path_cargo(segments: &[Segment]) -> Option<&str> {
    segments
        .iter()
        .find(|seg| seg.name() == "cargo" && seg.program.text != "cargo" && seg.program.raw.ends_with("/cargo"))
        .map(|seg| seg.program.raw.as_str())
}

/// `true` when the line itself backgrounds the command with a lone `&` at
/// the end: not `&&` (a second command) and not `>&` (a redirect).
fn ends_in_background(cmd: &str) -> bool {
    let trimmed = cmd.trim_end();
    trimmed.ends_with('&') && !trimmed.ends_with("&&") && !trimmed.ends_with(">&")
}

/// The command with its trailing `&` (and the blanks before it) removed.
fn without_trailing_background(cmd: &str) -> String {
    cmd.trim_end().trim_end_matches('&').trim_end().to_string()
}

/// The waiting check: deny a loop that waits for another process; rewrite a
/// `cargo` build or test sent to the background — through `run_in_background`,
/// a trailing `&`, or a full path — to run in the foreground instead; and
/// rewrite a `cargo` build or test with no time limit, or one under 600
/// seconds, to carry the 600-second ceiling.
pub(super) fn bash_waiting(segments: &[Segment], cmd: &str, input: &HookInput, lang: SupportedLocale) -> Option<Verdict> {
    if waits_in_a_loop(segments) {
        let reason = translate("command_guard.waiting_loop", lang).replace("{command}", truncate(cmd, 120));
        return Some(Verdict::Deny { reason });
    }

    let is_cargo = segments.iter().any(|seg| seg.name() == "cargo");
    let backgrounded = is_cargo && input.tool_input.get("run_in_background").and_then(Value::as_bool).unwrap_or(false);
    let trailing_background = is_cargo && ends_in_background(cmd);
    let full_path = full_path_cargo(segments);
    let current_timeout = input.tool_input.get("timeout").and_then(Value::as_u64);
    let short_ceiling = is_cargo && current_timeout.is_none_or(|timeout| timeout < FOREGROUND_TIMEOUT_MS);
    if !backgrounded && !trailing_background && full_path.is_none() && !short_ceiling {
        return None;
    }

    let mut command = cmd.to_string();
    if let Some(path) = full_path {
        command = command.replacen(path, "rtk cargo", 1);
    }
    if trailing_background {
        command = without_trailing_background(&command);
    }

    let mut tool_input = input.tool_input.clone();
    let fields = tool_input.as_object_mut()?;
    fields.insert("command".to_string(), Value::String(command));
    if backgrounded || trailing_background || short_ceiling {
        fields.remove("run_in_background");
        fields.insert("timeout".to_string(), Value::from(FOREGROUND_TIMEOUT_MS));
    }
    Some(Verdict::Rewrite { tool_input, note: None })
}

#[cfg(test)]
mod tests {
    use super::super::lex::segments;
    use super::*;
    use serde_json::json;

    fn input(command: &str, run_in_background: Option<bool>) -> HookInput {
        let mut tool_input = json!({ "command": command, "description": "roda a suíte" });
        if let Some(background) = run_in_background {
            tool_input["run_in_background"] = json!(background);
        }
        HookInput { tool_name: Some("Bash".to_string()), tool_input, ..HookInput::default() }
    }

    fn check(cmd: &str, run_in_background: Option<bool>) -> Option<Verdict> {
        let hook_input = input(cmd, run_in_background);
        bash_waiting(&segments(cmd), cmd, &hook_input, SupportedLocale::PtBr)
    }

    /// An input with an explicit `timeout` (`None` leaves the field out, the
    /// same as a command with no ceiling at all).
    fn input_with_timeout(command: &str, timeout: Option<u64>) -> HookInput {
        let mut tool_input = json!({ "command": command, "description": "roda a suíte" });
        if let Some(timeout) = timeout {
            tool_input["timeout"] = json!(timeout);
        }
        HookInput { tool_name: Some("Bash".to_string()), tool_input, ..HookInput::default() }
    }

    fn check_with_timeout(cmd: &str, timeout: Option<u64>) -> Option<Verdict> {
        let hook_input = input_with_timeout(cmd, timeout);
        bash_waiting(&segments(cmd), cmd, &hook_input, SupportedLocale::PtBr)
    }

    /// The three examples that got stuck for five hours on 19/09: `while` and
    /// `until` loops that test with `pgrep`, in the condition or split across
    /// `&&`, are refused, with the command shown in the reason.
    #[test]
    fn a_waiting_loop_is_denied_with_the_command_in_the_reason() {
        for cmd in [
            r#"until ! pgrep -f "cargo test" >/dev/null; do sleep 3; done; echo "FINISHED""#,
            r#"while pgrep -f "cargo test" >/dev/null 2>&1; do sleep 2; done"#,
            r#"until [ -s /tmp/full_test_run.log ] && ! pgrep -f "cargo test --workspace" > /dev/null; do sleep 5; done"#,
        ] {
            match check(cmd, None) {
                Some(Verdict::Deny { reason }) => assert!(reason.contains(cmd), "{reason}"),
                other => panic!("{cmd} must be denied, got {other:?}"),
            }
        }
    }

    /// `pidof` and `ps` in the same loop shape are the same danger; a `pgrep`
    /// in the loop's body, not only its condition, is caught the same way.
    #[test]
    fn pidof_and_ps_and_a_body_check_are_the_same_danger() {
        for cmd in ["while pidof cargo >/dev/null; do sleep 1; done", "until ps -p 123 >/dev/null; do sleep 1; done", "while true; do pgrep -f x >/dev/null && break; sleep 1; done"] {
            assert!(matches!(check(cmd, None), Some(Verdict::Deny { .. })), "{cmd}");
        }
    }

    /// A bare `pgrep`, with no loop around it, and a `for` loop that calls
    /// `pgrep` are not this danger: only `while`/`until` count.
    #[test]
    fn a_bare_pgrep_and_a_for_loop_pass() {
        for cmd in ["pgrep -f x", "for i in 1 2 3; do pgrep -f x; done"] {
            assert_eq!(check(cmd, None), None, "{cmd}");
        }
    }

    /// `pgrep` named inside a quoted message, or as a `grep` argument, is
    /// text, not a command, and passes — same rule the family already
    /// follows for a destructive command spelled inside a quote.
    #[test]
    fn pgrep_named_in_text_is_not_a_loop() {
        for cmd in [r#"git commit -m "rodar pgrep até acabar""#, "grep -l pgrep scripts/*.sh"] {
            assert_eq!(check(cmd, None), None, "{cmd}");
        }
    }

    /// A `cargo` build or test with `run_in_background: true` is rewritten to
    /// the foreground, with the ceiling set and the field gone — not denied.
    #[test]
    fn a_backgrounded_cargo_run_is_rewritten_not_denied() {
        match check("rtk cargo test -p mustard-rt", Some(true)) {
            Some(Verdict::Rewrite { tool_input, .. }) => {
                assert_eq!(tool_input["command"], "rtk cargo test -p mustard-rt");
                assert_eq!(tool_input["timeout"], 600_000);
                assert!(tool_input.get("run_in_background").is_none(), "{tool_input}");
            }
            other => panic!("expected a rewrite, got {other:?}"),
        }
    }

    /// The same rewrite for a trailing `&`: the operator is cut from the
    /// command text, and `&&` (a second command, not backgrounding) is left
    /// alone.
    #[test]
    fn a_trailing_ampersand_is_cut_and_the_ceiling_is_set() {
        match check("cargo test -p mustard-rt &", None) {
            Some(Verdict::Rewrite { tool_input, .. }) => {
                assert_eq!(tool_input["command"], "cargo test -p mustard-rt");
                assert_eq!(tool_input["timeout"], 600_000);
            }
            other => panic!("expected a rewrite, got {other:?}"),
        }
        // `&&` is a second command, not backgrounding; with the ceiling
        // already met it passes unchanged.
        assert_eq!(check_with_timeout("cargo build && cargo test", Some(600_000)), None);
    }

    /// The full path to `cargo` — which the separate `rtk` hook does not
    /// catch, since it only rewrites the bare word — is rewritten to `rtk
    /// cargo`, keeping the rest of the line.
    #[test]
    fn a_full_path_cargo_test_is_rewritten_through_rtk() {
        match check("/home/rubens/.cargo/bin/cargo test -p mustard-rt", None) {
            Some(Verdict::Rewrite { tool_input, .. }) => {
                assert_eq!(tool_input["command"], "rtk cargo test -p mustard-rt");
                assert!(tool_input.get("run_in_background").is_none(), "{tool_input}");
            }
            other => panic!("expected a rewrite, got {other:?}"),
        }
    }

    /// The bare `cargo test`, in the foreground, with no loop and with the
    /// 600-second ceiling already met: the third check has nothing to say,
    /// and the command passes as it came — the bare-word rewrite is the
    /// separate `rtk` hook's own job.
    #[test]
    fn the_same_command_in_the_foreground_with_no_loop_passes_unchanged() {
        assert_eq!(check_with_timeout("cargo test -p mustard-rt", Some(600_000)), None);
        assert_eq!(check("git status", None), None);
    }

    /// Backgrounding a command that is not `cargo` is outside this check: it
    /// is not the build-or-test danger the guard corrects.
    #[test]
    fn backgrounding_a_non_cargo_command_is_left_alone() {
        assert_eq!(check("npm run dev &", None), None);
        assert_eq!(check("node server.js", Some(true)), None);
    }

    /// O critério inteiro, os cinco casos do `when`/`then`: o laço é
    /// recusado com a instrução de rodar em primeiro plano; o segundo plano
    /// vira primeiro plano com o teto de tempo, sem recusa; o teste pelo
    /// caminho completo passa pelo `rtk`; a compilação ou o teste sem teto,
    /// ou com teto menor que 600 segundos (599), ganha o teto de 600
    /// segundos, sem recusa; e o mesmo comando em primeiro plano, sem laço e
    /// já com o teto de 600 segundos, passa como veio.
    #[test]
    fn the_guard_refuses_waiting_loops_and_background_builds() {
        match check("while pgrep -f x >/dev/null; do sleep 1; done", None) {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("primeiro plano"), "{reason}"),
            other => panic!("expected a loop to be denied, got {other:?}"),
        }

        match check("cargo test -p mustard-rt", Some(true)) {
            Some(Verdict::Rewrite { tool_input, .. }) => {
                assert_eq!(tool_input["command"], "cargo test -p mustard-rt");
                assert_eq!(tool_input["timeout"], 600_000);
                assert!(tool_input.get("run_in_background").is_none(), "{tool_input}");
            }
            other => panic!("expected the background run to be rewritten, got {other:?}"),
        }

        match check("/home/rubens/.cargo/bin/cargo test -p mustard-rt", None) {
            Some(Verdict::Rewrite { tool_input, .. }) => assert_eq!(tool_input["command"], "rtk cargo test -p mustard-rt"),
            other => panic!("expected the full path to be rewritten through rtk, got {other:?}"),
        }

        for timeout in [None, Some(599_000)] {
            match check_with_timeout("cargo test -p mustard-rt", timeout) {
                Some(Verdict::Rewrite { tool_input, .. }) => {
                    assert_eq!(tool_input["command"], "cargo test -p mustard-rt");
                    assert_eq!(tool_input["timeout"], 600_000);
                }
                other => panic!("expected the ceiling for timeout {timeout:?}, got {other:?}"),
            }
        }

        assert_eq!(check_with_timeout("cargo test -p mustard-rt", Some(600_000)), None);
    }
}
