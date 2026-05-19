#![forbid(unsafe_code)]
// `clippy::unwrap_used` / `expect_used` are `deny` workspace-wide so no
// hook-path code can panic (fail-open contract). Clippy does not exempt
// `#[cfg(test)]` code, so — matching `mustard-core`'s `lib.rs` — the carve-out
// is applied explicitly: under `cfg(test)`, `.unwrap()` / `.expect()` are
// allowed (a panicking assertion *is* a test failure). Non-test code keeps the
// `deny`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `mustard-rt` — the Mustard enforcement runtime.
//!
//! One binary replaces the dozens of per-hook `node` processes the harness
//! used to spawn. It has two faces (b3 spec § Arquitetura):
//!
//! - `mustard-rt on <event>` — run every enforcement module applicable to a
//!   whole harness lifecycle event (the target shape after the b3 collapse).
//! - `mustard-rt check <id>` — run a single named module, so `settings.json`
//!   can migrate entry-by-entry while `node` and `mustard-rt` coexist.
//!
//! The third face — `mustard-rt <script>` for the b4 script port — is **out of
//! scope** here. The [`Cli`] enum leaves a clear extension point (a future
//! `Script` subcommand) without implementing it.
//!
//! ## Protocol parity with the JS hooks
//!
//! The JS hooks (see `_lib/hook-env.js` and each `*.js`) speak a fixed
//! stdin/stdout contract: read the harness JSON from stdin, optionally write
//! one JSON object to stdout, always exit `0` (fail-open — a hook bug must
//! never block the agent). [`emit_outcome`] reproduces that exactly: one
//! stdout write, exit code `0`.

mod dispatch;
mod registry;
mod hooks;
mod util;

use clap::{Parser, Subcommand};
use mustard_core::model::contract::{HookInput, Outcome, Trigger, Verdict};
use std::io::{Read, Write};

/// The `mustard-rt` command line.
#[derive(Debug, Parser)]
#[command(name = "mustard-rt", about = "Mustard enforcement runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// The two-faced binary. `Script` (b4) is intentionally absent — adding it is
/// a new variant here plus a handler, the dispatcher stays untouched.
#[derive(Debug, Subcommand)]
enum Command {
    /// Run every module applicable to a whole harness lifecycle event.
    On {
        /// The harness event name, e.g. `PreToolUse`, `PostToolUse`.
        event: String,
    },
    /// Run a single named enforcement module.
    Check {
        /// The module id, e.g. `bash_guard`.
        id: String,
    },
}

fn main() {
    let cli = Cli::parse();

    // Read the harness JSON from stdin. A read failure or a parse failure is
    // *not* fatal: the central fail-open contract (b3 spec § Arquitetura step
    // 1) says any bad input degrades to `Allow`. We never replicate fail-open
    // inside a module — it lives here, once.
    let input = read_stdin_input();

    let outcome = match cli.command {
        Command::On { event } => {
            let trigger = Trigger::from_event_name(&event);
            dispatch::run_event(trigger, &input)
        }
        Command::Check { id } => dispatch::run_check(&id, &input),
    };

    emit_outcome(&outcome);
}

/// Read stdin and parse it into a [`HookInput`].
///
/// Fail-open: an I/O error or malformed JSON yields a default [`HookInput`],
/// so the dispatcher proceeds and every check sees a benign empty input
/// (which they all treat as `Allow`).
fn read_stdin_input() -> HookInput {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return HookInput::default();
    }
    if buf.trim().is_empty() {
        return HookInput::default();
    }
    serde_json::from_str(&buf).unwrap_or_default()
}

/// Turn a consolidated [`Outcome`] into one stdout write and the process exit
/// code, matching the JS hook protocol.
///
/// The JS `PreToolUse` hooks emit `{ "hookSpecificOutput": { ... } }` and
/// always `process.exit(0)`. This port mirrors that: a single JSON object on
/// stdout when the outcome carries a decision, nothing when it is a bare
/// `Allow`, and exit code `0` regardless (fail-open — blocking is expressed in
/// the JSON, never via a non-zero exit).
fn emit_outcome(outcome: &Outcome) {
    if let Some(json) = hook_specific_output(outcome) {
        let mut stdout = std::io::stdout();
        // A write failure on stdout is non-fatal — fail open, exit clean.
        let _ = writeln!(stdout, "{json}");
        let _ = stdout.flush();
    }
    std::process::exit(0);
}

/// Build the `hookSpecificOutput` JSON for an outcome, or `None` for a bare
/// `Allow` with no warnings (the JS hooks stay silent in that case).
fn hook_specific_output(outcome: &Outcome) -> Option<String> {
    let mut hook_output = serde_json::Map::new();
    hook_output.insert(
        "hookEventName".to_string(),
        serde_json::Value::String("PreToolUse".to_string()),
    );

    match &outcome.verdict {
        Verdict::Allow if outcome.warnings.is_empty() => return None,
        Verdict::Deny { reason } => {
            hook_output.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String("deny".to_string()),
            );
            hook_output.insert(
                "permissionDecisionReason".to_string(),
                serde_json::Value::String(reason.clone()),
            );
        }
        Verdict::Rewrite { tool_input } => {
            hook_output.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String("allow".to_string()),
            );
            hook_output.insert("updatedInput".to_string(), tool_input.clone());
        }
        Verdict::Inject { context } => {
            hook_output.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String("allow".to_string()),
            );
            hook_output.insert(
                "additionalContext".to_string(),
                serde_json::Value::String(context.clone()),
            );
        }
        Verdict::Allow | Verdict::Warn { .. } => {
            // `Allow` only reaches here with warnings present; `Warn` verdicts
            // never sit in `outcome.verdict` (the fold routes them to
            // `warnings`). Either way it is an advisory: allow + a message.
            hook_output.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String("allow".to_string()),
            );
        }
        _ => {
            // `Verdict` is `#[non_exhaustive]`; an unknown future variant
            // degrades to a silent allow rather than a panic (fail-open).
            return None;
        }
    }

    if !outcome.warnings.is_empty() {
        hook_output.insert(
            "additionalContext".to_string(),
            serde_json::Value::String(outcome.warnings.join("\n")),
        );
    }

    let mut root = serde_json::Map::new();
    root.insert(
        "hookSpecificOutput".to_string(),
        serde_json::Value::Object(hook_output),
    );
    Some(serde_json::Value::Object(root).to_string())
}
