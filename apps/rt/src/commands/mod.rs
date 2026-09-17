//! The `run` face of `mustard-rt` — the script port.
//!
//! `mustard-rt on` is the enforcement face: it reads the harness JSON from
//! stdin and runs the hooks. The `run` face is
//! different — it ports the utility *scripts* that used to live under
//! `templates/scripts/` as standalone `bun` programs. A `run` subcommand takes
//! its inputs as `clap` arguments (a directory, flags), never from stdin, and
//! prints its result to stdout exactly as the JS script did.
//!
//! Each ported script is its own submodule. (The early `sync-detect` /
//! `sync-registry` scanner ports were since removed — subproject discovery now
//! comes from grain's `grain.model.json` via the scan tool.)
//!
//! ## Layout — one clap enum per family, no god-enum
//!
//! [`RunCmd`] owns NO leaf command: it is a thin router of
//! `#[command(flatten)]` variants, one per family. Each family owns its own
//! `cli.rs` (`spec/cli.rs`, `flow/cli.rs`, …) holding BOTH its `…Cmd` enum and
//! the `dispatch()` arms that run it. `flatten` hoists the child subcommands to
//! THIS level, so every published name stays flat and unchanged:
//! `mustard-rt run pr-open`, never `mustard-rt run review pr-open`.
//!
//! THE INVARIANT, scoped per family: a new `run` subcommand takes the variant
//! in its family's enum and the arm in that family's `dispatch()` (the
//! compiler demands the arm). Two tests hold the rest:
//! `tests/run_command_surface.rs` compares the clap tree with
//! `tests/fixtures/run-surface.txt`, so a dropped registration (or an
//! accidental rename) fails CI instead of silently disappearing from the CLI
//! the hooks and the prose call; and `tests/template_parity.rs` refuses a
//! command that no prose or argv calls, with no exception list.

pub mod agent;
pub mod wave;
pub mod doctor;
pub mod review;
pub mod event;
pub mod spec;
pub mod maint;
pub mod git_delete;
pub mod git_settle;
pub mod scan;
pub mod scan_claude;
pub mod map;
// O `orient` e o `work-unit-open` deixaram de ser comandos: sobraram como
// motor do mapa do inicio da sessao e da porta do pull request.
pub mod orient;
pub mod work_unit_open;
pub mod spec_events;
pub mod flow;
pub mod retired;
pub mod statusline;
// Families whose commands are ported scripts living in flat modules (no
// `<family>/` directory of their own) keep their clap enum in a `*_cli.rs`
// sibling — same contract as a `<family>/cli.rs`.
pub mod scan_cli;

use clap::Subcommand;

/// The `run` subcommands — one flattened family per variant.
///
/// Every variant is `#[command(flatten)]`, so clap hoists the family's own
/// subcommands to the `run` level: the names the hooks, `settings.json` and the
/// SKILL templates call are unchanged and stay flat.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — clap-Subcommand; boxing breaks derive
pub enum RunCmd {
    /// Installation health: the closed check list the contract names.
    #[command(flatten)]
    Doctor(doctor::cli::DoctorCmd),
    /// A lista de pendências.
    #[command(flatten)]
    Event(event::cli::EventCmd),
    /// O fluxo da spec, um comando por passo.
    #[command(flatten)]
    Flow(flow::cli::FlowCmd),
    /// Instalação: a atualização e a faxina das cópias abandonadas.
    #[command(flatten)]
    Maint(maint::cli::MaintCmd),
    /// As portas do pull request: abrir, revisar e integrar.
    #[command(flatten)]
    Review(review::cli::ReviewCmd),
    /// The `/scan` chain: mine the repo model and enrich it.
    #[command(flatten)]
    Scan(scan_cli::ScanCmd),
    /// A porta das páginas avulsas.
    #[command(flatten)]
    Spec(spec::cli::SpecCmd),
    /// The spec event file: read one block, write one event.
    #[command(flatten)]
    SpecEvents(spec_events::cli::SpecEventsCmd),
    /// The Claude Code status bar.
    #[command(flatten)]
    Statusline(statusline::cli::StatuslineCmd),
}

/// Dispatch a `run` subcommand to its family.
///
/// Unlike the enforcement dispatcher this never touches stdin and never
/// produces an [`Outcome`](mustard_core::domain::model::contract::Outcome) — a `run`
/// script writes its own output and the process exits cleanly afterwards.
pub fn dispatch(cmd: RunCmd) {
    match cmd {
        RunCmd::Doctor(c) => doctor::cli::dispatch(c),
        RunCmd::Event(c) => event::cli::dispatch(c),
        RunCmd::Flow(c) => flow::cli::dispatch(c),
        RunCmd::Maint(c) => maint::cli::dispatch(c),
        RunCmd::Review(c) => review::cli::dispatch(c),
        RunCmd::Scan(c) => scan_cli::dispatch(c),
        RunCmd::Spec(c) => spec::cli::dispatch(c),
        RunCmd::SpecEvents(c) => spec_events::cli::dispatch(c),
        RunCmd::Statusline(c) => statusline::cli::dispatch(c),
    }
}
