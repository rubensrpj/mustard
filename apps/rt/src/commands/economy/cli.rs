//! The `run` subcommands for token economy and telemetry (`economy/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`EconomyCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
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
    /// W8.T8.8 also accepts `--context-claude-md <path>`: a CLAUDE.md file
    /// whose `## Heading` / `### Heading` sections are kept when their body
    /// contains any spec-derived relevance term. The CLAUDE.md slice is
    /// emitted after the CONTEXT.md slice (separated by a blank line).
    #[command(display_order = 14)]
    ContextSlice {
        /// A `CONTEXT.md` / `CONTEXT-MAP.md` path. Repeatable.
        #[arg(long)]
        context: Vec<String>,
        /// The spec file to match relevance against.
        #[arg(long)]
        spec: Option<String>,
        /// W8.T8.8 — slice the given CLAUDE.md against the same relevance
        /// terms. Optional; the CONTEXT.md path(s) remain primary.
        #[arg(long = "context-claude-md")]
        context_claude_md: Option<String>,
    },
    /// Render pipeline + hook telemetry (`collect` / `report` subcommand).
    #[command(display_order = 37)]
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
    /// `--help` natively (wave-network spec AC-6). Aliased to `metrics-wave-status`;
    /// invoked from CLI as `mustard-rt run metrics wave-status --spec <parent>`
    /// via argv pre-routing in `main.rs`.
    #[command(name = "metrics-wave-status")]
    #[command(display_order = 38)]
    MetricsWaveStatus {
        /// Parent (epic) spec name under `.claude/spec/` (flat layout).
        #[arg(long)]
        spec: Option<String>,
    },
    /// Normalise `rtk gain` analytics into the Mustard JSON shape.
    #[command(display_order = 47)]
    RtkGain,
    /// Run the local OTLP/JSON receiver for Claude Code native telemetry.
    ///
    /// Binds a loopback HTTP server on `MUSTARD_OTEL_PORT` (default 4318).
    /// Metrics/logs project into `claude_code_otel` (mustard.db); traces land
    /// span-level token usage as `run_usage` rows in telemetry.db via the
    /// telemetry writer (rows stamped with attribution at write time). Runs
    /// until a shutdown signal — the harness spawns it as a long-lived child
    /// via [`crate::hooks::session::session_start_inject`].
    #[command(display_order = 48)]
    OtelCollector,
    /// Stop the local OTEL collector for this project.
    ///
    /// Resolves the OTLP port from `MUSTARD_OTEL_PORT` (default 4318), kills
    /// whatever process is listening on it, and deletes the stale
    /// `.otel-collector.pid` file under `<project>/.claude/.harness/`. Killing
    /// by port (not by the drift-prone PID file) is the reliable teardown. Used
    /// by `install.ps1` before a reinstall so the previous daemon releases its
    /// exclusive lock on `mustard-rt.exe`. Fully fail-open; never exits non-zero.
    #[command(display_order = 49)]
    OtelStop,
    /// Watch `~/.claude/projects/**/*.jsonl` and re-ingest each session
    /// transcript into telemetry.db's `run_usage` table on every change.
    ///
    /// Opt-in daemon (Wave 3 — economia-moat-unification) spawned by
    /// [`crate::hooks::session::session_start_inject`] when `MUSTARD_TRANSCRIPT_WATCH=1`.
    /// Runs until process termination. With `--once`, performs a single
    /// backfill sweep of the current cwd's transcript directory and exits.
    #[command(display_order = 50)]
    TranscriptWatcher {
        /// Backfill mode: ingest every transcript currently in
        /// `~/.claude/projects/<encoded(cwd)>/` once, then exit. Default `false`
        /// (long-lived daemon).
        #[arg(long)]
        once: bool,
    },
    /// End-to-end health check of the Mustard ↔ Claude Code OTEL pipeline.
    #[command(display_order = 51)]
    DiagnoseOtel {
        /// Emit the machine-readable JSON report.
        #[arg(long)]
        json: bool,
        /// Wait `Xs`/`Xms`, then assert the row count grew (exit 1 on fail).
        #[arg(long = "expect-rows-after")]
        expect_rows_after: Option<String>,
    },
    /// W5.T5.10 — Compute the recommended prompt budget for a role + wave.
    #[command(name = "context-budget")]
    #[command(display_order = 93)]
    ContextBudget {
        /// Agent role token.
        #[arg(long)]
        role: String,
        /// Spec slug (optional — only echoed in the report).
        #[arg(long)]
        spec: Option<String>,
        /// Wave number (optional).
        #[arg(long)]
        wave: Option<u32>,
    },
    /// W5.T5.15 — Auditable economy operations: capture-baseline / reconcile / report.
    #[command(name = "economy")]
    #[command(display_order = 96)]
    Economy {
        /// Subcommand: `capture-baseline` / `reconcile` / `report`.
        subcommand: String,
        /// Operation name (capture-baseline).
        #[arg(long)]
        operation: Option<String>,
        /// Wave number (capture-baseline, reconcile).
        #[arg(long)]
        wave: Option<u32>,
        /// Use historical telemetry as the baseline source (capture-baseline).
        #[arg(long = "from-history")]
        from_history: bool,
        /// Output format: `json` (default) or `table` (report only).
        #[arg(long, default_value = "json")]
        format: String,
        /// Spec name (per-spec baseline file; W2 path catalog).
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
        EconomyCmd::RtkGain => economy::rtk_gain::run(),
        EconomyCmd::OtelCollector => economy::otel::collector::run(),
        EconomyCmd::OtelStop => economy::otel::stop::run(),
        EconomyCmd::TranscriptWatcher { once } => economy::transcript_watcher::run(once),
        EconomyCmd::DiagnoseOtel {
            json,
            expect_rows_after,
        } => economy::otel::diagnose::run(json, expect_rows_after.as_deref()),
        EconomyCmd::ContextBudget { role, spec, wave } => {
            economy::context_budget::run(economy::context_budget::ContextBudgetOpts { role, spec, wave });
        }
        EconomyCmd::Economy {
            subcommand,
            operation,
            wave,
            from_history,
            format,
            spec,
        } => match subcommand.as_str() {
            "capture-baseline" => economy::economy_capture_baseline::run(
                economy::economy_capture_baseline::CaptureBaselineOpts {
                    operation: operation.unwrap_or_default(),
                    wave: wave.unwrap_or(0),
                    from_history,
                    spec: spec.clone(),
                },
            ),
            "reconcile" => economy::economy_reconcile::run(economy::economy_reconcile::ReconcileOpts {
                wave: wave.unwrap_or(0),
                spec: spec.clone(),
            }),
            "report" => economy::economy_report::run(economy::economy_report::ReportOpts {
                format,
                spec: spec.clone(),
            }),
            other => {
                eprintln!(
                    "economy: unknown subcommand {other:?}. Try: capture-baseline / reconcile / report"
                );
                std::process::exit(1);
            }
        },
    }
}
