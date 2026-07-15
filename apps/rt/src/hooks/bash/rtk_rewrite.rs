//! `rtk_rewrite` — rewrite a Bash command through RTK (the Golden Rule).
//!
//! Shells out to `rtk rewrite <cmd>` with a 2-second timeout. On exit-0 with
//! a non-empty, distinct stdout the gate returns `Verdict::Rewrite` carrying
//! the rewritten command. Every other path — already `rtk`-prefixed, `rtk`
//! missing, non-zero exit, timeout, empty or identical output — falls through
//! to the blanket-prefix fallback (prepend `rtk `), or returns `None`
//! (silent allow, fail-open) when the command cannot be wrapped at all.

use mustard_core::platform::config::Mode;
use serde_json::json;
use std::process::{Command, Stdio};
use std::time::Duration;

use mustard_core::domain::model::contract::Verdict;

use super::lex::{is_cmd_separator, mask_quoted_operators};

/// Subprocess timeout for `rtk rewrite` calls. The RTK binary is a local
/// process with no network I/O; 2 s is generous.
const RTK_REWRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// The literal prefix of the rtk "no hook installed" advisory that rtk 0.34.1
/// emits on every invocation when no Claude Code hook is registered.
///
/// Background: `rtk hook` only supports Gemini CLI and Copilot (`gemini` /
/// `copilot` subcommands); it has no `claude` subcommand, so running `rtk init
/// -g` would violate the Mustard install contract (no writes to
/// `~/.claude/settings.json`). Mustard redirects all Bash through `bash_guard`,
/// so the advisory is pure noise inside the harness. RTK exposes no env var to
/// suppress it — the filter lives here, on the consumer side.
const RTK_NOISE_PREFIX: &str = "[rtk] /!\\ No hook installed";

/// Remove every line whose content starts with [`RTK_NOISE_PREFIX`] from `s`.
///
/// Lines are split on `\n`; a trailing newline produces a trailing empty token
/// that is preserved unchanged. Only the exact advisory prefix is matched —
/// all other output (including other `[rtk]` lines) is left intact.
///
/// # Example
///
/// ```text
/// "[rtk] /!\\ No hook installed — run `rtk init -g`\nrtk ls\n"
/// → "rtk ls\n"
/// ```
fn filter_rtk_noise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut lines = s.split('\n').peekable();
    while let Some(line) = lines.next() {
        if line.starts_with(RTK_NOISE_PREFIX) {
            // Drop this line; do not re-add a newline separator.
            continue;
        }
        out.push_str(line);
        // Re-add the separator between lines (not after the last token when the
        // original string did not end with `\n`, to preserve trailing behaviour).
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

/// Spawn `rtk rewrite <cmd>` in a worker thread and return its stdout on
/// success.
///
/// Fail-open on four paths — returns `None` when:
/// 1. The binary cannot be spawned (ENOENT — `rtk` not installed).
/// 2. The process exits with a non-zero status (no RTK equivalent for `cmd`).
/// 3. The subprocess does not finish within [`RTK_REWRITE_TIMEOUT`].
/// 4. Stdout is empty or pure whitespace (RTK signalled "no rewrite").
///
/// The binary name defaults to `"rtk"` but can be overridden via the
/// `MUSTARD_RTK_BIN` environment variable — required for the fail-open unit
/// test (AC-7) so tests never depend on a real `rtk` in `PATH`.
#[must_use]
fn run_rtk_rewrite_subprocess(cmd: &str) -> Option<String> {
    let binary = std::env::var("MUSTARD_RTK_BIN").unwrap_or_else(|_| "rtk".into());
    run_rtk_rewrite_subprocess_with_bin(cmd, &binary)
}

/// Inner implementation of [`run_rtk_rewrite_subprocess`], accepting an
/// explicit binary name. Extracted so tests can inject a fake binary name
/// without mutating process environment (which is `unsafe` in edition 2024).
fn run_rtk_rewrite_subprocess_with_bin(cmd: &str, binary: &str) -> Option<String> {
    let mut command = Command::new(binary);
    command
        .args(["rewrite", cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    // Path 1: binary not found / ENOENT — fail open.
    let Ok(mut child) = command.spawn() else { return None };

    let (tx, rx) = std::sync::mpsc::channel();
    let stdout_handle = child.stdout.take();

    std::thread::spawn(move || {
        let status = child.wait();
        let _ = tx.send((status, child));
    });

    match rx.recv_timeout(RTK_REWRITE_TIMEOUT) {
        Ok((Ok(status), _child)) => {
            // Path 2: non-zero exit → no RTK equivalent — fail open.
            if !status.success() {
                return None;
            }
            let mut output = String::new();
            if let Some(mut out) = stdout_handle {
                use std::io::Read;
                let _ = out.read_to_string(&mut output);
            }
            // Filter the rtk "no hook installed" advisory before trimming.
            // Defense-in-depth: if a future rtk version emits the warning on
            // stdout instead of stderr (already null'd), this prevents a valid
            // rewritten command from being discarded as noise-only output.
            let output = filter_rtk_noise(&output);
            let trimmed = output.trim();
            // Path 4: empty / whitespace stdout — fail open.
            if trimmed.is_empty() {
                return None;
            }
            Some(trimmed.to_string())
        }
        // Wait itself errored — fail open.
        Ok((Err(_), _child)) => None,
        // Path 3: timeout — kill the child and fail open.
        Err(_) => {
            if let Ok((_, mut child)) = rx.recv_timeout(Duration::from_millis(0)) {
                let _ = child.kill();
            }
            None
        }
    }
}

/// Read `MUSTARD_RTK_GATE_MODE` and resolve to a [`Mode`]. Default
/// [`Mode::Warn`] — the `rtk`-on-everything mandate of
/// `2026-05-20-rtk-mandatory-everywhere` is enforced by **auto-rewriting** the
/// command (prepend `rtk`), not by rejecting it. Rewriting reaches the exact
/// same end state (every command runs under `rtk`) with ZERO round-trip: the
/// harness applies the [`Verdict::Rewrite`] through `updatedInput` (see
/// `protocol.rs`), and the eligibility guard [`should_blanket_prefix`] already
/// excludes the forms `rtk` cannot wrap (builtins, subshells), which pass
/// untouched. Denying-to-teach (`Mode::Strict`) is retained as an explicit
/// opt-in for a setup that wants a hard block, but it is no longer the default:
/// for an agent it only adds a re-submit round-trip with no durable learning.
/// Parse goes through [`Mode::parse`]; any unrecognised value collapses to
/// `Warn`.
fn rtk_gate_mode() -> Mode {
    std::env::var("MUSTARD_RTK_GATE_MODE")
        .ok()
        .and_then(|raw| Mode::parse(&raw))
        .unwrap_or(Mode::Warn)
}

/// The production `rtk-rewrite` gate: shells out via
/// [`run_rtk_rewrite_subprocess`] under [`rtk_gate_mode`].
///
/// Split into two layers so unit tests can inject a closure instead of the
/// real subprocess and a [`Mode`] instead of mutating process env (`unsafe`
/// under edition 2024 because of `#![forbid(unsafe_code)]`):
/// - [`rtk_rewrite_with`] — pure logic, closure- and mode-injectable.
/// - [`rtk_rewrite`]      — production wrapper passing the real subprocess
///   and [`rtk_gate_mode`].
///
/// Returns `(Verdict, coverage_tag)` so the caller can emit `coverage` in
/// telemetry without embedding it in `tool_input`. The coverage tag for the
/// strict-mode Deny path is `"deny"`.
pub(super) fn rtk_rewrite(cmd: &str) -> Option<(Verdict, &'static str)> {
    #[cfg(test)]
    {
        if RTK_REWRITE_TEST_OVERRIDE.with(|c| c.get()) {
            // Test override forces the gate off so unrelated `verdict_for`
            // tests can exercise bash-safety / native-redirect without the
            // strict-mode rtk gate denying every unprefixed command. Tests
            // that exercise the gate itself call `rtk_rewrite_with` directly
            // with an explicit `Mode` (see `tests` below) or drive the binary
            // as a subprocess (see `tests/rtk_rewrite_emission.rs`).
            return rtk_rewrite_with(cmd, |_| None, Mode::Off);
        }
    }
    rtk_rewrite_with(cmd, run_rtk_rewrite_subprocess, rtk_gate_mode())
}

/// Pure, testable core of `rtk-rewrite`. Accepts any `rewriter` closure so
/// tests can inject a fake subprocess without touching `PATH`, and a [`Mode`]
/// so tests can drive the gate without mutating process env (`unsafe` under
/// edition 2024 because of `#![forbid(unsafe_code)]`).
///
/// Short-circuits (returns `None`) when the command is already `rtk`-prefixed.
///
/// **`Mode::Off`** — gate disabled: returns `None` (no rewrite, no deny).
///
/// **`Mode::Strict`** (opt-in) — when the command is eligible for blanket
/// wrapping (not a builtin, no leading subshell/backtick) and the user did
/// not pre-prefix `rtk`, returns `Verdict::Deny` so the UI surfaces both the
/// original command and the required form. The agent then re-submits the
/// command with the `rtk` prefix. Coverage tag `"deny"`. NOT the default — a
/// deny only buys a round-trip; the rewrite below reaches the same end state.
///
/// **`Mode::Warn`** (default) — auto-rewrite the command:
/// - **Specific path**: delegates to `rewriter` for the actual rewrite attempt.
///   If the rewriter yields a non-empty, distinct string, returns
///   `Verdict::Rewrite` tagged `coverage = "specific"`.
/// - **Blanket-prefix fallback** (Golden Rule): when the specific path produces
///   nothing, [`should_blanket_prefix`] decides whether to prepend `rtk` to the
///   whole command. Returns `Verdict::Rewrite` tagged `coverage = "blanket"`.
///
/// Returns `(Verdict, coverage_tag)` so the caller can emit the tag in
/// telemetry without embedding it in `tool_input` (which is forwarded to
/// Claude Code as-is).
fn rtk_rewrite_with<F: FnOnce(&str) -> Option<String>>(
    cmd: &str,
    rewriter: F,
    mode: Mode,
) -> Option<(Verdict, &'static str)> {
    // Gate disabled — no rewrite, no deny.
    if mode == Mode::Off {
        return None;
    }

    // Short-circuit: already wrapped — never double-prefix, never deny.
    // Any segment of a pipeline/chain that starts with `rtk` counts as
    // wrapped — the user has demonstrated rtk awareness.
    if has_rtk_in_any_segment(cmd) {
        return None;
    }

    // Upfront eligibility gate. A command that isn't safe to blanket-wrap
    // (real subshell, leading backtick-exec, head token is a shell builtin)
    // isn't safe to hand to the external `rtk rewrite` either — RTK's own
    // rewriter can corrupt those forms (e.g. `(cargo build; cargo test)`
    // → `(cargo build; rtk cargo test)`). Skip both paths *and* the strict
    // deny path: a builtin like `cd` cannot be exec'd through `rtk`, so we
    // must not ask the agent to "reenvie como: rtk cd /tmp" — that would
    // fail. Builtins / subshells are silently allowed in every mode.
    if !should_blanket_prefix(cmd) {
        return None;
    }

    // --- Strict mode: deny so the UI surfaces the rule ---
    if mode == Mode::Strict {
        let trimmed = cmd.trim();
        let head: String = trimmed.chars().take(120).collect();
        let suffix = if trimmed.chars().count() > 120 { "..." } else { "" };
        return Some((
            Verdict::Deny {
                reason: format!(
                    "[bash_guard rtk] Comando sem prefixo `rtk` — Mustard exige rtk em todo Bash.\n\
                     Reenvie como: rtk {head}{suffix}"
                ),
            },
            "deny",
        ));
    }

    // --- Warn mode: specific path ---
    if let Some(rewritten) = rewriter(cmd) {
        // Strip any rtk "no hook installed" advisory before evaluating the
        // rewriter's output. rtk 0.34.1 emits this line on every invocation
        // when no Claude Code hook is registered; Mustard uses bash_guard
        // instead, so the advisory is pure noise. Filtering here covers both
        // the real subprocess path and the injected-closure path used by tests.
        let rewritten_clean = filter_rtk_noise(&rewritten);
        let rewritten = rewritten_clean.trim();
        if !rewritten.is_empty() && rewritten != cmd.trim() {
            return Some((
                Verdict::Rewrite {
                    tool_input: json!({ "command": rewritten }),
                },
                "specific",
            ));
        }
    }

    // --- Warn mode: blanket-prefix fallback (Golden Rule) ---
    // When RTK has no specific filter for `cmd`, prepend `rtk ` so that:
    //   a) all future RTK filters automatically apply retroactively, and
    //   b) every rewrite is captured in telemetry for coverage analysis.
    blanket_prefix(cmd).map(|prefixed| {
        (
            Verdict::Rewrite {
                tool_input: json!({ "command": prefixed }),
            },
            "blanket",
        )
    })
}

/// Shell builtins and keywords that must never be wrapped with `rtk`.
/// RTK would try to exec them as a binary and fail (exit 127).
const SHELL_BUILTINS: &[&str] = &[
    "cd", "pwd", "export", "set", "unset", "alias", "unalias", "source", ".",
    "eval", "exec", "exit", "return", ":", "read", "shift", "let", "local",
    "declare", "typeset", "readonly", "hash", "times", "umask", "wait",
    "getopts", "caller", "enable", "bind", "bg", "fg", "disown", "jobs",
    "kill", "command", "builtin", "type", "true", "false", "test", "[",
    "[[", "((", "if", "then", "else", "elif", "fi", "for", "do", "done",
    "while", "until", "case", "esac", "select", "function", "time", "coproc",
    "in",
];

/// Returns the slice of `s` after stripping any leading `VAR=value` env
/// assignments (tokens matching `[A-Za-z_][A-Za-z0-9_]*=…` followed by
/// whitespace). If no env assignments are present the original slice is
/// returned unchanged.
fn strip_env_prefix(s: &str) -> &str {
    let mut rest = s.trim_start();
    loop {
        // An env assignment token starts with an identifier character.
        let bytes = rest.as_bytes();
        if bytes.is_empty() {
            break;
        }
        let first = bytes[0] as char;
        if !(first.is_ascii_alphabetic() || first == '_') {
            break;
        }
        // Find the boundary of this whitespace-separated token.
        let token_end = rest
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let token = &rest[..token_end];
        // Must contain `=` to be an env assignment.
        if !token.contains('=') {
            break;
        }
        // The part before `=` must be a valid identifier.
        let eq_pos = token.find('=').unwrap_or(0); // safe: contains '=' confirmed above
        let name = &token[..eq_pos];
        let is_ident = !name.is_empty()
            && name
                .chars()
                .enumerate()
                .all(|(i, c)| {
                    if i == 0 {
                        c.is_ascii_alphabetic() || c == '_'
                    } else {
                        c.is_ascii_alphanumeric() || c == '_'
                    }
                });
        if !is_ident {
            break;
        }
        // Advance past this env token and any trailing whitespace.
        rest = rest[token_end..].trim_start();
    }
    rest
}

/// Returns `true` when it is safe to prepend `rtk ` to `cmd`.
///
/// Returns `false` when:
/// - `cmd` is empty.
/// - The first segment (after any `VAR=value` env assignments) is a real
///   subshell `(…)` or backtick exec `` `…` `` — those forms have no head
///   binary, so `rtk` has nothing to wrap.
/// - The first executable token is a shell builtin or keyword (RTK would
///   exit 127 trying to exec it).
///
/// A literal `(`, `` ` ``, or `$(` *inside an argument* (e.g.
/// `git log --grep="(scope)"`, `echo $(date)`) is fine — the shell expands
/// those before invoking `rtk`, which then execs the real binary.
fn should_blanket_prefix(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Strip compound operators (&&, ||, ;, |) and inspect only the first
    // segment. Prepending `rtk` before the whole pipeline is fine — the shell
    // expands `&&` / `||` after `rtk` exits. Mask quoted operators first so a
    // quoted argument containing `|`/`;` does not split the head off early.
    let masked = mask_quoted_operators(trimmed);
    let seg_end = masked.find(is_cmd_separator).unwrap_or(masked.len());
    let first_segment = trimmed[..seg_end].trim();
    // After stripping env assignments, inspect the head of the segment.
    let after_env = strip_env_prefix(first_segment);
    // A segment that *starts* with `(` or `` ` `` has no head binary for `rtk`
    // to wrap (real subshell / backtick exec). Skip.
    if after_env.starts_with('(') || after_env.starts_with('`') {
        return false;
    }
    // The executable token: a shell builtin/keyword cannot be exec'd by `rtk`.
    let executable = after_env.split_whitespace().next().unwrap_or("");
    if SHELL_BUILTINS.contains(&executable) {
        return false;
    }
    true
}

/// Builds the blanket-prefixed command string, handling `VAR=val` env
/// assignments that must remain before the shell expands the line.
///
/// For plain commands (`cargo run -p foo`) this returns `"rtk cargo run -p
/// foo"`. For env-prefixed commands (`RUST_LOG=debug cargo run`) the env
/// assignments are kept at the front and `rtk` is inserted before the
/// executable: `"RUST_LOG=debug rtk cargo run"`. This is the form that both
/// the shell and RTK accept — the shell sets the env vars before invoking
/// `rtk`, which then execs the real program.
///
/// Returns `None` when [`should_blanket_prefix`] rejects the command.
fn blanket_prefix(cmd: &str) -> Option<String> {
    if !should_blanket_prefix(cmd) {
        return None;
    }
    let trimmed = cmd.trim();
    // Detect how many leading env-assignment tokens precede the executable.
    let after_env = strip_env_prefix(trimmed);
    if after_env.len() == trimmed.len() {
        // No env prefix — simple case.
        Some(format!("rtk {trimmed}"))
    } else {
        // There are env assignments. Insert `rtk` between them and the rest.
        // env_part_len is derived from pointer arithmetic below; no separate var needed.
        // env_part includes the trailing space(s) stripped by strip_env_prefix,
        // so we re-take the original slice up to the start of after_env.
        let env_part = trimmed[..trimmed.len() - after_env.len()].trim_end();
        Some(format!("{env_part} rtk {after_env}"))
    }
}

/// Returns `true` when any `&&`/`||`/`;`/`|` segment of `cmd` has `rtk`
/// as its first executable token (after stripping `VAR=value` env
/// assignments).
///
/// Used as the "already wrapped" short-circuit for the rtk gate: when
/// the user has piped to or chained an `rtk`-prefixed stage (e.g.
/// `echo '{...}' | rtk mustard-rt run emit-event --event decision`), neither
/// denying nor blanket-prefixing is correct — the user has already
/// demonstrated rtk awareness and chosen where the wrap applies.
fn has_rtk_in_any_segment(cmd: &str) -> bool {
    // Split on the masked view so a quoted operator (e.g. `|` inside a Grep
    // pattern) does not create a phantom segment that hides a real `rtk` stage.
    // The head token of each segment is a command name, never quoted, so it is
    // identical in the masked and original strings.
    mask_quoted_operators(cmd)
        .split(is_cmd_separator)
        .any(|seg| {
            let after_env = strip_env_prefix(seg.trim());
            after_env.split_whitespace().next() == Some("rtk")
        })
}

#[cfg(test)]
thread_local! {
    /// In test builds, when set, this short-circuits `rtk_rewrite` to return `None`,
    /// isolating gate tests from the real `rtk` binary on PATH and from the
    /// strict-mode rtk gate (default since spec
    /// `2026-05-20-rtk-mandatory-everywhere`).
    ///
    /// **Side-effect warning for future authors:** any test that calls
    /// `verdict_for(...)` (dispatcher tests in `bash_command_gate`) will have
    /// the rtk gate forced to `Mode::Off` — it will NEVER see a
    /// `Verdict::Rewrite` *or* a strict `Verdict::Deny` from the production
    /// `rtk_rewrite()` path. Tests that need to exercise the rewrite or
    /// strict-deny logic must call `rtk_rewrite_with` directly with an
    /// explicit `Mode` (see `tests` below) or drive the binary as a
    /// subprocess (see `tests/rtk_rewrite_emission.rs`).
    pub(super) static RTK_REWRITE_TEST_OVERRIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// Behavioral tests for `rtk_rewrite` decision logic.
//
// Strategy: [`rtk_rewrite_with`] accepts a closure that stands in for the real
// `rtk rewrite` subprocess. Each test injects a purpose-built closure so the
// eight AC scenarios can be verified without requiring `rtk` on PATH and without
// touching process environment. The subprocess fail-open test (AC-7) calls
// `run_rtk_rewrite_subprocess_with_bin` directly with a fake binary name,
// avoiding `std::env::set_var` which is `unsafe` in edition 2024.

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // AC-1: Already rtk-prefixed → short-circuit, rewriter never called.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_passes_through_when_rtk_prefixed() {
        // The panic closure must never be invoked when the command already
        // starts with `rtk` — the short-circuit returns `None` immediately.
        // Tested under `Mode::Warn` (historical behavior); `Mode::Strict`
        // also short-circuits via the same already-prefixed check.
        let result = rtk_rewrite_with(
            "rtk grep x",
            |_| panic!("should not be called"),
            Mode::Warn,
        );
        assert!(result.is_none(), "expected None for already-prefixed command");
    }

    // -----------------------------------------------------------------------
    // AC-2: Rewriter returns a changed string → Verdict::Rewrite with the
    //       rewritten command in tool_input["command"], coverage = "specific".
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_emits_updated_input_when_rewriter_returns_change() {
        let result = rtk_rewrite_with("grep -n x src/", |c| Some(format!("rtk {c}")), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "rtk grep -n x src/",
                    "tool_input[\"command\"] mismatch: {tool_input}"
                );
                assert_eq!(coverage, "specific", "expected specific coverage tag");
            }
            other => panic!("expected Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-3: Rewriter returns None for a non-builtin command → blanket prefix
    //       kicks in and returns Verdict::Rewrite, coverage = "blanket".
    //       Builtins (e.g. `cd`) still return None — see
    //       rtk_rewrite_skips_shell_builtin_cd below.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_blanket_when_rewriter_returns_none_for_non_builtin() {
        let result = rtk_rewrite_with("grep -n x src/", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "rtk grep -n x src/",
                    "expected blanket-prefixed command; got: {tool_input}"
                );
                assert_eq!(coverage, "blanket", "expected blanket coverage tag");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-4: Rewriter returns the same string → specific path skipped;
    //       blanket path fires for non-builtins, returning Rewrite.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_blanket_when_rewriter_returns_identical() {
        let result = rtk_rewrite_with("grep -n x", |_| Some("grep -n x".to_string()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "rtk grep -n x",
                    "expected blanket-prefixed command; got: {tool_input}"
                );
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-5: Rewriter returns empty string → specific path skipped;
    //       blanket path fires for non-builtins.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_blanket_when_rewriter_returns_empty() {
        let result = rtk_rewrite_with("grep -n x src/", |_| Some(String::new()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk grep -n x src/");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-6: Rewriter returns whitespace-only → specific path skipped;
    //       blanket path fires for non-builtins.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_blanket_when_rewriter_returns_whitespace_only() {
        let result = rtk_rewrite_with("grep -n x src/", |_| Some("   \n  ".to_string()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk grep -n x src/");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-7 (closure path): Trailing newline is stripped from the rewritten
    //       command before it is placed in tool_input["command"].
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_strips_trailing_whitespace() {
        let result = rtk_rewrite_with("grep x", |_| Some("rtk grep x\n".to_string()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "rtk grep x",
                    "trailing newline must be stripped; got: {tool_input}"
                );
                assert_eq!(coverage, "specific");
            }
            other => panic!("expected Verdict::Rewrite, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // AC-7 (subprocess path): When the binary name is a nonexistent executable,
    //       run_rtk_rewrite_subprocess_with_bin must return None (fail-open).
    //
    // Uses the with_bin helper directly so no process environment mutation is
    // needed — `std::env::set_var` is `unsafe` in edition 2024, and
    // `#![forbid(unsafe_code)]` prevents using an `unsafe` block.
    // -----------------------------------------------------------------------

    #[test]
    fn rtk_rewrite_fail_open() {
        // Invoke the subprocess layer with a binary name that cannot exist on
        // any PATH — the spawn must fail and the function must return None.
        let result = run_rtk_rewrite_subprocess_with_bin("grep x", "__not_a_real_binary_xyzzy__");
        assert!(
            result.is_none(),
            "expected None when subprocess binary does not exist; got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Blanket-prefix Golden Rule tests (new AC set)
    // -----------------------------------------------------------------------

    // BL-1: Unknown command with no specific RTK filter → blanket prefix.
    #[test]
    fn rtk_rewrite_blanket_prefixes_unknown_command() {
        let result = rtk_rewrite_with("cargo run -p foo", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk cargo run -p foo");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-2: `node` command with no specific RTK filter → blanket prefix.
    #[test]
    fn rtk_rewrite_blanket_prefixes_node_command() {
        let result = rtk_rewrite_with(r#"node -e "42""#, |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], r#"rtk node -e "42""#);
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-3: Env-prefixed command — `rtk` is inserted AFTER env assignments so
    //       the shell sets the variable before invoking RTK.
    //       `RUST_LOG=debug rtk cargo run` is the correct form;
    //       `rtk RUST_LOG=debug cargo run` would fail (RTK tries to exec
    //       `RUST_LOG=debug` as a binary name).
    #[test]
    fn rtk_rewrite_blanket_with_env_prefix() {
        let result = rtk_rewrite_with("RUST_LOG=debug cargo run", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(
                    tool_input["command"],
                    "RUST_LOG=debug rtk cargo run",
                    "rtk must be inserted after env assignments, not before them"
                );
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-4: Shell builtin `cd` → blanket must be suppressed (RTK cannot exec
    //       a shell builtin; it would exit 127 with "Binary 'cd' not found").
    #[test]
    fn rtk_rewrite_skips_shell_builtin_cd() {
        let result = rtk_rewrite_with("cd /tmp", |_| None, Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for shell builtin 'cd'; got {result:?}"
        );
    }

    // BL-5: Env prefix before a builtin — env assignments do not change the
    //       fact that the executable token is a builtin.
    #[test]
    fn rtk_rewrite_skips_shell_builtin_with_env() {
        let result = rtk_rewrite_with("FOO=bar cd /tmp", |_| None, Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for env-prefixed builtin 'cd'; got {result:?}"
        );
    }

    // BL-6: Subshell expression `(cmd)` — blanket-prefix is ambiguous here;
    //       `rtk (cargo build; cargo test)` is not valid shell.
    #[test]
    fn rtk_rewrite_skips_subshell() {
        let result = rtk_rewrite_with("(cargo build; cargo test)", |_| None, Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for subshell expression; got {result:?}"
        );
    }

    // BL-7: Command substitution mid-command (`echo $(date)`) — the head
    //       binary is `echo`, and the shell expands `$(date)` BEFORE running
    //       `rtk`, so blanket-prefix is safe (and desirable for token savings).
    #[test]
    fn rtk_rewrite_blanket_with_command_substitution_in_arg() {
        let result = rtk_rewrite_with("echo $(date)", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk echo $(date)");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-7b: A real backtick exec at the start of the command has no head
    //        binary for `rtk` to wrap, so it must still be skipped.
    #[test]
    fn rtk_rewrite_skips_leading_backtick_exec() {
        let result = rtk_rewrite_with("`echo hi`", |_| None, Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for leading backtick exec; got {result:?}"
        );
    }

    // BL-7c: A literal `(` *inside an argument* is fine — the shell sees it as
    //        text, not a subshell. Blanket must still wrap the head binary.
    #[test]
    fn rtk_rewrite_blanket_with_paren_inside_arg() {
        let result = rtk_rewrite_with(r"git log --grep=(scope)", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], r"rtk git log --grep=(scope)");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite for paren-in-arg, got {other:?}"),
        }
    }

    // BL-7d: Backtick inside an argument (`echo \`date\``) — the head binary
    //        is `echo`. Wrap.
    #[test]
    fn rtk_rewrite_blanket_with_backtick_inside_arg() {
        let result = rtk_rewrite_with("echo `date`", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk echo `date`");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite for backtick-in-arg, got {other:?}"),
        }
    }

    // BL-8: When the specific rewriter returns a real rewrite, the specific
    //       path wins — no blanket path is attempted.
    #[test]
    fn rtk_rewrite_specific_takes_precedence_over_blanket() {
        let result = rtk_rewrite_with("cargo build", |_| Some("rtk cargo build".to_string()), Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk cargo build");
                assert_eq!(coverage, "specific", "specific rewriter must win over blanket");
            }
            other => panic!("expected specific Verdict::Rewrite, got {other:?}"),
        }
    }

    // BL-9: Compound command with `&&` — first segment token (`cargo`) is not
    //       a builtin so blanket prefix applies to the whole string.
    //       The shell expands `&&` after RTK exits, so `rtk cargo run &&
    //       cargo test` correctly runs `cargo test` in the same shell context.
    #[test]
    fn rtk_rewrite_compound_blanket_uses_first_segment_check() {
        // RTK wraps only the first segment's executable; the `&& cargo test`
        // tail is carried along unchanged. The shell parent handles `&&`.
        let result = rtk_rewrite_with("cargo run && cargo test", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk cargo run && cargo test");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected blanket Verdict::Rewrite for compound cmd, got {other:?}"),
        }
    }

    // BL-10: Already-prefixed command → short-circuit returns None immediately
    //        (rewriter closure is never invoked).
    #[test]
    fn rtk_rewrite_already_prefixed_no_op() {
        let result = rtk_rewrite_with("rtk cargo run", |_| panic!("should not be called"), Mode::Warn);
        assert!(
            result.is_none(),
            "expected None for already-prefixed command; got {result:?}"
        );
    }

    /// Bug-fix regression (2026-05-20): a pipeline whose first stage is not
    /// `rtk` but whose second stage IS `rtk` must short-circuit, never deny.
    #[test]
    fn rtk_rewrite_pipe_with_rtk_in_second_segment_no_op() {
        let cmd = "echo '{\"type\":\"decision\"}' | rtk mustard-rt run emit-event --event decision";
        let result = rtk_rewrite_with(cmd, |_| panic!("should not be called"), Mode::Strict);
        assert!(result.is_none(), "rtk in second pipe segment must short-circuit, got {result:?}");
    }

    /// Compound command: first stage prefixed with rtk, second not.
    /// Either segment having rtk counts as wrapped — do not deny.
    #[test]
    fn rtk_rewrite_compound_with_rtk_first_no_op() {
        let cmd = "rtk cargo build && cargo test";
        let result = rtk_rewrite_with(cmd, |_| panic!("should not be called"), Mode::Strict);
        assert!(result.is_none(), "rtk in first compound segment must short-circuit, got {result:?}");
    }

    /// Bug-fix regression (2026-05-21): a multi-line `command` string whose
    /// first line is a non-`rtk` sanity command (`echo …`) but whose second
    /// line IS `rtk`-prefixed must short-circuit. A newline separates shell
    /// commands exactly like `;`, so the later `rtk` stage counts as wrapped —
    /// the gate must not deny the whole multi-line command.
    #[test]
    fn rtk_rewrite_newline_with_rtk_on_second_line_no_op() {
        let cmd = "echo \"--- sanity ---\"\nrtk mustard-rt run emit-pipeline --help";
        let result = rtk_rewrite_with(cmd, |_| panic!("should not be called"), Mode::Strict);
        assert!(
            result.is_none(),
            "rtk on a later line must short-circuit, got {result:?}"
        );
    }

    /// `\r\n` (Windows) line endings split the same way as `\n`.
    #[test]
    fn rtk_rewrite_crlf_with_rtk_on_second_line_no_op() {
        let cmd = "echo sanity\r\nrtk mustard-rt run emit-pipeline --help";
        let result = rtk_rewrite_with(cmd, |_| panic!("should not be called"), Mode::Strict);
        assert!(
            result.is_none(),
            "rtk on a later CRLF line must short-circuit, got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Gate-mode tests (spec: 2026-05-20-rtk-mandatory-everywhere)
    //
    // The gate-mode parameter is passed in directly so tests do not need to
    // mutate `MUSTARD_RTK_GATE_MODE` (`std::env::set_var` is `unsafe` under
    // edition 2024, and the crate forbids `unsafe`).
    // -----------------------------------------------------------------------

    /// `Mode::Strict` on a non-prefixed, eligible command → `Verdict::Deny`
    /// carrying the operator-facing reason. The rewriter closure must not be
    /// invoked — strict mode short-circuits before the specific path.
    #[test]
    fn rtk_strict_denies_unprefixed() {
        let result = rtk_rewrite_with(
            "grep -n x src/",
            |_| panic!("rewriter must not be called in strict mode"),
            Mode::Strict,
        );
        match result {
            Some((Verdict::Deny { reason }, coverage)) => {
                assert_eq!(coverage, "deny", "expected coverage tag 'deny'");
                assert!(
                    reason.contains("Mustard exige rtk"),
                    "reason should explain the rule; got: {reason}"
                );
                assert!(
                    reason.contains("rtk grep -n x src/"),
                    "reason should suggest the rtk-prefixed FULL command (not just the head token); got: {reason}"
                );
            }
            other => panic!("expected Verdict::Deny in strict mode, got {other:?}"),
        }
    }

    /// `Mode::Warn` preserves the historical rewrite behavior — a non-prefixed
    /// command becomes a `Verdict::Rewrite` with `coverage = "blanket"`.
    /// Regression guard for the spec's opt-out path.
    #[test]
    fn rtk_warn_rewrites_unprefixed() {
        let result = rtk_rewrite_with("grep -n x src/", |_| None, Mode::Warn);
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                assert_eq!(tool_input["command"], "rtk grep -n x src/");
                assert_eq!(coverage, "blanket");
            }
            other => panic!("expected Verdict::Rewrite in warn mode, got {other:?}"),
        }
    }

    /// `Mode::Off` disables the gate entirely — the rewriter closure must not
    /// be invoked and the result must be `None` (Allow at the caller).
    #[test]
    fn rtk_off_allows_unprefixed() {
        let result = rtk_rewrite_with(
            "grep -n x src/",
            |_| panic!("rewriter must not be called in off mode"),
            Mode::Off,
        );
        assert!(
            result.is_none(),
            "expected None (Allow) in off mode; got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // filter_rtk_noise — spec 2026-05-26-rtk-quiet-hook-warning
    //
    // rtk 0.34.1 emits "[rtk] /!\ No hook installed — run `rtk init -g`…"
    // on every invocation when no Claude Code hook is registered. Since
    // Mustard uses bash_guard (not rtk hook) for redirection, the line is
    // pure noise. The filter must strip it and leave all other output intact.
    // -----------------------------------------------------------------------

    /// Sole rtk-noise line → empty string after filtering.
    #[test]
    fn filter_rtk_noise_removes_advisory_line() {
        let input = "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings\n";
        let result = filter_rtk_noise(input);
        assert!(
            !result.contains("No hook installed"),
            "advisory must be removed; got: {result:?}"
        );
    }

    /// Mixed output: noise line + valid rewritten command → only command remains.
    #[test]
    fn filter_rtk_noise_keeps_other_lines() {
        let input = "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings\nrtk cargo build\n";
        let result = filter_rtk_noise(input);
        assert!(
            !result.contains("No hook installed"),
            "advisory must be removed; got: {result:?}"
        );
        assert!(
            result.contains("rtk cargo build"),
            "command line must survive; got: {result:?}"
        );
    }

    /// Output with no noise line → returned unchanged.
    #[test]
    fn filter_rtk_noise_passthrough_when_no_noise() {
        let input = "rtk git status\n";
        let result = filter_rtk_noise(input);
        assert_eq!(result, input, "clean output must pass through unchanged");
    }

    /// Multiple lines including the noise prefix: only that line is dropped.
    #[test]
    fn filter_rtk_noise_drops_only_noise_line() {
        let other = "[rtk] some other rtk annotation\n";
        let noise = "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings\n";
        let cmd   = "rtk cargo test\n";
        let input = format!("{other}{noise}{cmd}");
        let result = filter_rtk_noise(&input);
        assert!(!result.contains("No hook installed"), "noise must be gone; got: {result:?}");
        assert!(result.contains("[rtk] some other rtk annotation"), "other [rtk] line must survive; got: {result:?}");
        assert!(result.contains("rtk cargo test"), "command must survive; got: {result:?}");
    }

    /// Subprocess path: when the rewriter returns only the noise advisory,
    /// filtering leaves the output empty → specific path skipped → blanket
    /// fires. The final rewritten command must not contain the noise text.
    #[test]
    fn filter_rtk_noise_noise_only_falls_through_to_blanket() {
        let noise_only = "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings\n";
        let result = rtk_rewrite_with(
            "cargo build",
            |_| Some(noise_only.to_string()),
            Mode::Warn,
        );
        // After filtering the specific output is empty → blanket fires.
        match result {
            Some((Verdict::Rewrite { tool_input }, coverage)) => {
                let cmd = tool_input["command"].as_str().unwrap_or("");
                assert!(
                    !cmd.contains("No hook installed"),
                    "noise must not appear in the rewritten command; got: {cmd:?}"
                );
                assert_eq!(coverage, "blanket", "noise-only specific path must fall through to blanket");
            }
            other => panic!("expected Verdict::Rewrite (blanket fallback), got {other:?}"),
        }
    }
}
