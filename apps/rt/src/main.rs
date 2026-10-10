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

// A linha de comando mora num módulo que a biblioteca também declara, para o
// teste da ajuda percorrer a árvore inteira a partir da raiz.
mod cli;
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
// A pasta do pacote lida na hora de rodar, que os testes de dentro de `src/`
// usam: a conferência de todos os alvos também compila esta face como teste.
#[cfg(test)]
#[path = "../tests/support/manifest_dir.rs"]
mod manifest_dir;
// O programa falso que os testes de dentro de `src/` gravam e depois rodam.
#[cfg(test)]
#[path = "../tests/support/executable.rs"]
mod executable;

use clap::Parser;
use cli::{Cli, Command};
use mustard_core::domain::model::contract::{HookInput, Outcome, Trigger};
use std::io::{Read, Write};

fn main() {
    let internal:Vec<_>=std::env::args_os().collect();
    if internal.get(1).is_some_and(|arg|arg=="--mustard-lsp-worker") {
        if internal.len()==4 && let Some(language)=internal[3].to_str() {
            let _=mustard_core::io::knowledge::precise::lsp::worker::run(std::path::Path::new(&internal[2]),language);
        }
        return;
    }
    // Before ANY face runs: inside the Mustard source repository, hand the
    // whole invocation to the program compiled from the branch; otherwise, if
    // the plugin registry records a strictly newer install of this binary,
    // hand it to that. This is what makes the version of a system-installed
    // copy (`.deb`, `.pkg`, `.exe`) irrelevant — every entry door (statusline,
    // `run upsert`, a terminal call) converges on the plugin's self-updated
    // binary, on every OS, without any installer changing. See
    // `mustard_core::newer_installed_rt` for why the handover lives here and
    // not in the installers.
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

/// Hand this whole invocation to another `mustard-rt` when there is one that
/// should answer, and return to run as ourselves otherwise.
///
/// Two programs can take the call, tried in this order:
///
/// 1. Inside the Mustard source repository, the program COMPILED from the
///    branch (`mustard_core::development_rt`): the session of the Mustard's own
///    work runs the code it is changing, not the release the plugin shipped.
///    Only an installed program hands over; one the repository built itself
///    never does, so `cargo test` tests the program it just built. Without a
///    compiled program, or when it does not open, the next one is tried.
/// 2. The newer install the plugin registry records
///    (`mustard_core::newer_installed_rt`), which makes the version of a
///    system-installed copy (`.deb`, `.pkg`, `.exe`) irrelevant: every entry
///    door (statusline, `run upsert`, a terminal call) converges on the
///    plugin's self-updated binary, on every OS, without any installer
///    changing.
///
/// The decisions live in `mustard_core`; this function only performs the
/// handover, which is why it belongs in `main.rs`: it is argv routing — to
/// another process.
///
/// One hop only: the delegate runs with `MUSTARD_RT_DELEGATED` set and never
/// delegates again. For the registry the version check already makes a loop
/// impossible (the newest install is not behind itself), so the variable is a
/// belt over braces — it also covers a corrupted install whose directory holds
/// an older binary than the registry claims. For the compiled program it is the
/// whole guard: a compiled program that handed over again would answer with
/// the release the plugin shipped.
///
/// Fail-open, like every path in this binary: if the handover cannot start,
/// we answer with this binary — exactly what happened before it existed.
fn delegate_to_newer_install() {
    if std::env::var_os("MUSTARD_RT_DELEGATED").is_some() {
        return;
    }
    let compiled = std::env::current_exe()
        .ok()
        .zip(std::env::current_dir().ok())
        .and_then(|(running, here)| mustard_core::development_rt(&here, &running));
    for target in compiled.into_iter().chain(mustard_core::newer_installed_rt()) {
        hand_over_to(&target);
    }
}

/// Run this whole invocation as `target`; return only when it could not start.
fn hand_over_to(target: &std::path::Path) {
    let mut cmd = std::process::Command::new(target);
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
