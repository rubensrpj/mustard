//! The `run` subcommands for token economy and telemetry (`economy/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`EconomyCmd`] AND its arm in [`dispatch`] below; forgetting the arm still
//! compiles, but the command vanishes from the CLI. The other two live in
//! the tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run economy <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;

use crate::commands::{economy};

/// The `run` subcommands owned by token economy and telemetry (`economy/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum EconomyCmd {
    /// Cut the relevant term blocks from one or more `CONTEXT.md` glossaries.
    ///
    /// Also accepts `--context-claude-md <path>`: a CLAUDE.md file
    /// whose `## Heading` / `### Heading` sections are kept when their body
    /// contains any spec-derived relevance term. The CLAUDE.md slice is
    /// emitted after the CONTEXT.md slice (separated by a blank line).
    #[command(display_order = 13)]
    ContextSlice {
        /// A `CONTEXT.md` / `CONTEXT-MAP.md` path. Repeatable.
        #[arg(long)]
        context: Vec<String>,
        /// The spec file to match relevance against.
        #[arg(long)]
        spec: Option<String>,
        /// Slice the given CLAUDE.md against the same relevance
        /// terms. Optional; the CONTEXT.md path(s) remain primary.
        #[arg(long = "context-claude-md")]
        context_claude_md: Option<String>,
    },
    /// Render pipeline + hook telemetry (`collect` / `report` subcommand).
    #[command(display_order = 30)]
    Metrics {
        /// Subcommand: `collect` or `report`.
        subcommand: Option<String>,
        /// Subcommand flags (`--hooks-only`, `--since`, `--event`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Output format: `json` (default) or `html`.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Per-wave status + telemetry roll-up for a parent (epic) spec.
    ///
    /// Promoted to a top-level `RunCmd` variant so clap renders `--spec` in
    /// `--help` natively. Aliased to `metrics-wave-status`;
    /// invoked from CLI as `mustard-rt run metrics wave-status --spec <parent>`
    /// via argv pre-routing in `main.rs`.
    #[command(name = "metrics-wave-status")]
    #[command(display_order = 31)]
    MetricsWaveStatus {
        /// Parent (epic) spec name under `.claude/spec/` (flat layout).
        #[arg(long)]
        spec: Option<String>,
    },
}

/// Dispatch one `economy`-family `run` subcommand.
pub fn dispatch(cmd: EconomyCmd) {
    match cmd {
        EconomyCmd::ContextSlice {
            context,
            spec,
            context_claude_md,
        } => economy::context_slice::run(
            &context,
            spec.as_deref(),
            context_claude_md.as_deref(),
        ),
        EconomyCmd::Metrics {
            subcommand,
            args,
            format,
        } => economy::metrics::run(subcommand.as_deref(), &args, &format),
        EconomyCmd::MetricsWaveStatus { spec } => {
            let mut argv: Vec<String> = Vec::new();
            if let Some(s) = spec {
                argv.push("--spec".to_string());
                argv.push(s);
            }
            economy::metrics_wave_status::run(&argv);
        }
    }
}
