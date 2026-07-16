//! The `run` face of `mustard-rt` — the b4 script port.
//!
//! `mustard-rt on` / `mustard-rt check` are the enforcement faces: they read
//! the harness JSON from stdin and run hook modules. The `run` face is
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
//! `cli.rs` (`spec/cli.rs`, `wave/cli.rs`, …) holding BOTH its `…Cmd` enum and
//! the `dispatch()` arms that run it. `flatten` hoists the child subcommands to
//! THIS level, so every published name stays flat and unchanged:
//! `mustard-rt run wave-advance`, never `mustard-rt run wave advance`.
//!
//! THE INVARIANT, now scoped per family: a new `run` subcommand needs TWO
//! registrations — the variant in that family's enum AND the arm in that
//! family's `dispatch()`; forgetting the second compiles but the command
//! vanishes. `tests/run_command_surface.rs` locks the full name list, so a
//! dropped registration (or an accidental rename) fails CI instead of silently
//! disappearing from the CLI the hooks and SKILLs call.

pub mod agent;
pub mod checklist;
pub mod doctor;
pub mod review;
pub mod economy;
pub mod pipeline;
pub mod event;
pub mod wave;
pub mod spec;
pub mod maint;
pub mod git_settle;
pub mod work_unit_open;
pub mod scan;
pub mod scan_claude;
pub mod scan_equivalences;
pub mod scan_guards;
pub mod scan_patterns;
pub mod feature;
pub mod orient;
pub mod capability;
pub mod glossary_coverage;
pub mod grill_capture;
pub mod statusline;
// Families whose commands are ported scripts living in flat modules (no
// `<family>/` directory of their own) keep their clap enum in a `*_cli.rs`
// sibling — same contract as a `<family>/cli.rs`.
pub mod context_cli;
pub mod git_cli;
pub mod scan_cli;
// W3 of `2026-05-26-claude-paths-single-source` — three typed doctor checks
// (claude-paths, workspace-leaks, i1) that emit native JSON shapes. They are
// dispatched by `doctor.rs` but live in dedicated modules so the legacy
// `CheckResult` envelope stays out of their way.
pub use event::event_projections::{pipeline_state_from_events, PipelineStateView};

use clap::Subcommand;

/// The `run` subcommands — one flattened family per variant.
///
/// Every variant is `#[command(flatten)]`, so clap hoists the family's own
/// subcommands to the `run` level: the names the hooks, `settings.json` and the
/// SKILL templates call are unchanged and stay flat.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — clap-Subcommand; boxing breaks derive
pub enum RunCmd {
    /// Agent dispatch: prompt render, amendment + digest finalisers.
    #[command(flatten)]
    Agent(agent::cli::AgentCmd),
    /// Durable capability docs under `.claude/capabilities/`.
    #[command(flatten)]
    Capability(capability::cli::CapabilityCmd),
    /// Audit checklists (spec items + canonical domain checklists).
    #[command(flatten)]
    Checklist(checklist::cli::ChecklistCmd),
    /// Repo-model retrieval: the feature digest, the orientation census and
    /// the glossary loop.
    #[command(flatten)]
    Context(context_cli::ContextCmd),
    /// Installation health, docs-staleness and language audits.
    #[command(flatten)]
    Doctor(doctor::cli::DoctorCmd),
    /// Token economy, metrics and OTEL telemetry.
    #[command(flatten)]
    Economy(economy::cli::EconomyCmd),
    /// The harness event stream: emit, project, verify.
    #[command(flatten)]
    Event(event::cli::EventCmd),
    /// The git exit ritual of a delivered work unit.
    #[command(flatten)]
    Git(git_cli::GitCmd),
    /// Installation maintenance: deps, validate, refresh, prune, (un)hook.
    #[command(flatten)]
    Maint(maint::cli::MaintCmd),
    /// Pipeline orchestration: status, dispatch, resume, close.
    #[command(flatten)]
    Pipeline(pipeline::cli::PipelineCmd),
    /// The REVIEW and QA gates.
    #[command(flatten)]
    Review(review::cli::ReviewCmd),
    /// The `/scan` chain: mine the repo model and enrich it.
    #[command(flatten)]
    Scan(scan_cli::ScanCmd),
    /// The spec lifecycle: draft, scope, validate, link, close.
    #[command(flatten)]
    Spec(spec::cli::SpecCmd),
    /// The Claude Code status bar.
    #[command(flatten)]
    Statusline(statusline::cli::StatuslineCmd),
    /// Wave plans: scaffold, tree, dependencies, collapse.
    #[command(flatten)]
    Wave(wave::cli::WaveCmd),
}

/// Dispatch a `run` subcommand to its family.
///
/// Unlike the enforcement dispatcher this never touches stdin and never
/// produces an [`Outcome`](mustard_core::domain::model::contract::Outcome) — a `run`
/// script writes its own output and the process exits cleanly afterwards.
pub fn dispatch(cmd: RunCmd) {
    match cmd {
        RunCmd::Agent(c) => agent::cli::dispatch(c),
        RunCmd::Capability(c) => capability::cli::dispatch(c),
        RunCmd::Checklist(c) => checklist::cli::dispatch(c),
        RunCmd::Context(c) => context_cli::dispatch(c),
        RunCmd::Doctor(c) => doctor::cli::dispatch(c),
        RunCmd::Economy(c) => economy::cli::dispatch(c),
        RunCmd::Event(c) => event::cli::dispatch(c),
        RunCmd::Git(c) => git_cli::dispatch(c),
        RunCmd::Maint(c) => maint::cli::dispatch(c),
        RunCmd::Pipeline(c) => pipeline::cli::dispatch(c),
        RunCmd::Review(c) => review::cli::dispatch(c),
        RunCmd::Scan(c) => scan_cli::dispatch(c),
        RunCmd::Spec(c) => spec::cli::dispatch(c),
        RunCmd::Statusline(c) => statusline::cli::dispatch(c),
        RunCmd::Wave(c) => wave::cli::dispatch(c),
    }
}
