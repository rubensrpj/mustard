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
//! `mustard-rt` — o binário do Mustard.
//!
//! Duas faces:
//!
//! - `mustard-rt on <evento>` — roda os ganchos de um evento do Claude Code:
//!   lê o JSON do evento no stdin, escreve no máximo um JSON no stdout e sai
//!   sempre com 0. Barrar é o JSON, nunca o código de saída.
//! - `mustard-rt run <nome>` — um comando: lê os argumentos, nunca o stdin, e
//!   imprime o próprio relatório.

mod dispatch;
mod registry;
mod hooks;
mod report;
// The harness response shape, and the five tests that hold it. It lives in a
// module the LIBRARY also declares, so `test = false` on this binary stops the
// whole of `src/` being tested twice without taking those five with it — see
// the module doc for the measurement that motivated it.
mod hook_output;
mod commands;
mod shared;
mod util;

use clap::{Parser, Subcommand};
use mustard_core::domain::model::contract::{HookInput, Outcome, Trigger};
use std::io::{Read, Write};

/// The `mustard-rt` command line.
#[derive(Debug, Parser)]
#[command(name = "mustard-rt", version = env!("MUSTARD_VERSION_FULL"), about = "Mustard enforcement runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// As faces do binário. `Run` não lê o stdin.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — single-use stack alloc, indirection adds no value
enum Command {
    /// Roda os ganchos de um evento do Claude Code.
    On {
        /// O nome do evento, como `PreToolUse` ou `Stop`.
        event: String,
    },
    /// Roda um comando. Recebe argumentos, não o stdin.
    Run {
        #[command(subcommand)]
        command: commands::RunCmd,
    },
}

fn main() {
    // Before ANY face runs: if the plugin registry records a strictly newer
    // install of this binary, hand the whole invocation to it. This is what
    // makes the version of a system-installed copy (`.deb`, `.pkg`, `.exe`)
    // irrelevant — every entry door (statusline, `run upsert`, a terminal
    // call) converges on the plugin's self-updated binary, on every OS,
    // without any installer changing. See `mustard_core::newer_installed_rt`
    // for why the handover lives here and not in the installers.
    delegate_to_newer_install();

    let cli = Cli::parse_from(std::env::args());

    match cli.command {
        // Um comando não lê o stdin: ele é tratado antes da leitura, para não
        // ficar esperando um JSON que não vem.
        Command::Run { command } => commands::dispatch(command),
        // Uma entrada que não se lê vira uma entrada vazia, que todo gancho
        // deixa passar: a regra de nunca falhar mora aqui, uma vez.
        Command::On { event } => {
            let input = read_stdin_input();
            let outcome = dispatch::run_event(Trigger::from_event_name(&event), &input);
            // O nome do evento volta como veio: o Claude Code recusa a
            // resposta cujo `hookEventName` não é o do evento que ele chamou.
            emit_outcome(&event, &outcome);
        }
    }
}

/// Hand this whole invocation to the newer `mustard-rt` the plugin registry
/// records, when there is one; return and run as ourselves otherwise.
///
/// The decision (is there a strictly newer install, and where is its binary?)
/// lives in `mustard_core::newer_installed_rt`; this function only performs
/// the handover, which is why it belongs in `main.rs`: it is argv routing —
/// to another process.
///
/// One hop only: the delegate runs with `MUSTARD_RT_DELEGATED` set and never
/// delegates again. The version check already makes a loop impossible (the
/// newest install is not behind itself), so the variable is a belt over
/// braces — it also covers a corrupted install whose directory holds an older
/// binary than the registry claims.
///
/// Fail-open, like every path in this binary: if the handover cannot start,
/// we answer with this binary — exactly what happened before it existed.
fn delegate_to_newer_install() {
    if std::env::var_os("MUSTARD_RT_DELEGATED").is_some() {
        return;
    }
    let Some(target) = mustard_core::newer_installed_rt() else {
        return;
    };
    let mut cmd = std::process::Command::new(&target);
    cmd.args(std::env::args_os().skip(1)).env("MUSTARD_RT_DELEGATED", "1");
    #[cfg(unix)]
    {
        // `exec` replaces this process wholesale — stdin (the harness JSON),
        // stdout, exit code and signals all belong to the delegate, with no
        // parent left to double-report. It only ever RETURNS on failure, and
        // then we fall through to run as ourselves.
        use std::os::unix::process::CommandExt;
        let _handover_failed: std::io::Error = cmd.exec();
    }
    #[cfg(not(unix))]
    {
        // Windows has no `exec`: run the delegate as a child sharing our
        // stdio and forward its exit code. A code-less exit forwards 0 — the
        // fail-open direction, and the code every hook exits with anyway.
        if let Ok(status) = cmd.status() {
            std::process::exit(status.code().unwrap_or(0));
        }
    }
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
fn emit_outcome(event_name: &str, outcome: &Outcome) {
    if let Some(json) = hook_output::hook_specific_output(event_name, outcome) {
        let mut stdout = std::io::stdout();
        // A write failure on stdout is non-fatal — fail open, exit clean.
        let _ = writeln!(stdout, "{json}");
        let _ = stdout.flush();
    }
    std::process::exit(0);
}
