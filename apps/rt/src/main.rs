#![forbid(unsafe_code)]
// `clippy::unwrap_used` / `expect_used` are `deny` workspace-wide so no
// hook-path code can panic (fail-open contract). Clippy does not exempt
// `#[cfg(test)]` code, so — matching `mustard-core`'s `lib.rs` — the carve-out
// is applied explicitly: under `cfg(test)`, `.unwrap()` / `.expect()` are
// allowed (a panicking assertion *is* a test failure). Non-test code keeps the
// `deny`.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::len_zero,
        clippy::format_push_string,
        clippy::needless_range_loop,
        clippy::double_ended_iterator_last,
        clippy::map_unwrap_or,
        clippy::uninlined_format_args,
        clippy::vec_init_then_push,
        clippy::items_after_test_module,
    )
)]
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
//! A third face — `mustard-rt run <name>` — is now implemented (the b4 script
//! port). Unlike `on` / `check` it does not read harness JSON from stdin: a
//! `run` subcommand takes `clap` arguments and prints its own output, porting
//! what used to be a standalone `bun` script under `templates/scripts/`.
//! Wave 3 (economia-moat-unification) adds long-lived ingestion daemons under
//! this face: `otel-collector` and the opt-in `transcript-watcher` (the
//! `transcript_watcher` module is dispatched via [`run::RunCmd::TranscriptWatcher`]).
//!
//! A fourth face — `mustard-rt mcp` — serves the `mustard-memory` Model
//! Context Protocol server over stdio (the re-port of the former TypeScript
//! `bun` MCP server). Like `run` it never reads the harness stdin contract;
//! it speaks JSON-RPC on stdin/stdout and is dispatched early in `main`.
//!
//! ## Protocol parity with the JS hooks
//!
//! The JS hooks (see `_lib/hook-env.js` and each `*.js`) speak a fixed
//! stdin/stdout contract: read the harness JSON from stdin, optionally write
//! one JSON object to stdout, always exit `0` (fail-open — a hook bug must
//! never block the agent). [`emit_outcome`] reproduces that exactly: one
//! stdout write, exit code `0`.

mod dispatch;
mod mcp;
mod registry;
mod hooks;
mod report;
mod run;
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

/// The four-faced binary: the `On` / `Check` enforcement faces, the `Run`
/// face (the b4 script port), and the `Mcp` face (the `mustard-memory` MCP
/// server). `Run` and `Mcp` skip the harness stdin read.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — single-use stack alloc, indirection adds no value
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
    /// Run a ported utility script (the b4 face). Takes `clap` args, not stdin.
    Run {
        #[command(subcommand)]
        command: run::RunCmd,
    },
    /// Serve the `mustard-memory` MCP server (JSON-RPC over stdio).
    Mcp,
}

fn main() {
    // Argv pre-routing: rewrite `run metrics wave-status ...` to
    // `run metrics-wave-status ...` so clap dispatches to the top-level
    // `RunCmd::MetricsWaveStatus` variant (and therefore renders `--spec` in
    // `--help` natively). Keeps `run metrics --help` and `run metrics collect`
    // working unchanged. See wave-network spec AC-6.
    let argv: Vec<String> = rewrite_metrics_wave_status(std::env::args().collect());
    let cli = Cli::parse_from(argv);

    // The `Run` and `Mcp` faces are not enforcement faces: they never read
    // the harness stdin contract. Handle them before the stdin read so they
    // do not block waiting for harness JSON. `Mcp` *does* own stdin — it
    // speaks JSON-RPC there — so dispatching it early is mandatory.
    match cli.command {
        Command::Run { command } => {
            run::dispatch(command);
            return;
        }
        Command::Mcp => {
            mcp::run();
            return;
        }
        _ => {}
    }

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
        // `Run` / `Mcp` are handled above, before the stdin read.
        Command::Run { .. } | Command::Mcp => {
            unreachable!("Run/Mcp are dispatched before stdin read")
        }
    };

    emit_outcome(&outcome);
}

/// Rewrite `mustard-rt run metrics wave-status [args...]` to
/// `mustard-rt run metrics-wave-status [args...]` so clap routes to the
/// top-level `RunCmd::MetricsWaveStatus` variant. All other argv shapes pass
/// through unchanged. This is the one carve-out needed to keep
/// `run metrics --help` and `run metrics {collect,report}` working while
/// surfacing `--spec` in the `wave-status --help` output (AC-6).
fn rewrite_metrics_wave_status(mut argv: Vec<String>) -> Vec<String> {
    // Find `run` index; require `metrics` immediately after, then `wave-status`.
    let Some(run_idx) = argv.iter().position(|a| a == "run") else {
        return argv;
    };
    let metrics_idx = run_idx + 1;
    let wave_idx = run_idx + 2;
    if argv.get(metrics_idx).map(String::as_str) == Some("metrics")
        && argv.get(wave_idx).map(String::as_str) == Some("wave-status")
    {
        // Collapse the two tokens into one: `metrics wave-status` → `metrics-wave-status`.
        argv[metrics_idx] = "metrics-wave-status".to_string();
        argv.remove(wave_idx);
    }
    argv
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
