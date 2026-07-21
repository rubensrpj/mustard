//! The `run` subcommands for pipeline orchestration (`pipeline/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`PipelineCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run pipeline <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{pipeline};

/// The `run` subcommands owned by pipeline orchestration (`pipeline/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum PipelineCmd {
    /// Emit a compact git diff summary for agent context.
    #[command(display_order = 7)]
    DiffContext {
        /// Branch to compare against (auto-detects `main`/`master`).
        #[arg(long)]
        parent: Option<String>,
        /// Scope the diff to a path.
        #[arg(long)]
        subproject: Option<String>,
        /// Pipeline phase — `analyze` is a silent no-op.
        #[arg(long)]
        phase: Option<String>,
    },
    /// Finalize a completed wave in ONE call (token-economy composite): emit
    /// `pipeline.wave.complete` (the completion event + the wave's
    /// `meta.json`/`spec.md` → Close + the parent progress bump, reusing
    /// `emit-pipeline` verbatim) AND cache the wave diff
    /// (`git diff HEAD~1 HEAD --stat` → `wave-{N}-{role}/diff.md`, atomic LF
    /// write). Folds the two bookkeeping steps the orchestrator did by hand
    /// after a committed wave; the diff cache replaces a fragile shell redirect
    /// (no CRLF / absolute-path-redirect footgun).
    #[command(display_order = 11)]
    WaveDone {
        /// Spec the completed wave belongs to.
        #[arg(long)]
        spec: String,
        /// The completed wave number.
        #[arg(long)]
        wave: u64,
        /// Wall-clock duration of the wave in milliseconds (telemetry only).
        #[arg(long = "duration-ms")]
        duration_ms: Option<u64>,
    },
    /// Run build/test verification for the active pipeline's subprojects.
    #[command(display_order = 33)]
    VerifyPipeline {
        /// Output format: `json` (default) or `html`.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Render a CLOSE-phase Done/Left/Next-Steps summary for a spec.
    ///
    /// With `--self-test`: instantiate a minimal [`mustard_core::SpecSummaryDoc`],
    /// serialise it to pretty JSON, print to stdout, and exit 0. Used by
    /// `cargo run -p mustard-rt -- run pipeline-summary --self-test` in AC-1A-1.
    #[command(display_order = 34)]
    PipelineSummary {
        /// Path to the spec directory (must contain `spec.md`). Also accepts a
        /// `.../spec.md` path or a bare slug. `--spec` / `--from-spec` are
        /// hidden aliases, so the sibling commands' spelling parses here too.
        #[arg(long = "spec-dir", alias = "spec", alias = "from-spec")]
        spec_dir: Option<String>,
        /// Output format: `markdown` (default) or `json`.
        #[arg(long, default_value = "markdown")]
        format: String,
        /// Self-test mode: serialise a minimal SpecSummaryDoc and exit 0.
        #[arg(long = "self-test")]
        self_test: bool,
    },
    /// Project + harness status snapshot.
    ///
    /// Default mode: git branch, modified files, active vs orphaned pipelines,
    /// last build result, repo-model summary (grain.model.json).
    ///
    /// `--harness` mode: reads `.claude/settings.json`, groups hooks by lifecycle
    /// event, resolves enforcement mode from env vars, and renders a 4-column
    /// table (Hook | Matcher | Enforces | Mode).
    #[command(display_order = 50)]
    Status {
        /// Include hooks table (harness view).
        #[arg(long)]
        harness: bool,
        /// Output format: `table` (default) or `json`.
        #[arg(long, default_value = "table")]
        format: String,
        /// Project root directory (default: current working directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Single-shot resume decision for `/mustard:spec`: mode, stage, operational
    /// spec path, wave progress, dispatch failure, refresh flags, wave model,
    /// resumo, agent roles. Emits `pipeline.resume_mode` before returning
    /// (idempotent — debounced 10 s). Fail-open: every IO error degrades a
    /// field to `null`/`false`; exit 0 always.
    #[command(display_order = 52)]
    ResumeBootstrap {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Emit pretty JSON instead of the compact text table.
        #[arg(long)]
        json: bool,
    },
    /// W5.T5.1 — Drive the CLOSE-phase gates (verify → qa → docs-stale → summary).
    #[command(name = "close-orchestrate")]
    #[command(display_order = 65)]
    CloseOrchestrate {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
        /// Skip the docs-stale-check gate.
        #[arg(long = "skip-docs")]
        skip_docs: bool,
    },
    /// Composite PLAN materialisation: the wave-scaffold renderer +
    /// analyze-validation + `pipeline.scope` (full) + `pipeline.phase` PLAN,
    /// all in-process. Pressupposes `spec.md`/`meta.json` already drafted by
    /// `spec-draft`. Re-runnable: before the spec is approved it reconciles the
    /// layout onto the plan (rewriting what differs, deleting waves the plan
    /// dropped); after approval the layout is frozen.
    /// Output: `"events"`, `"scaffold"` (created_files, skipped, refreshed,
    /// removed) and `"validation"` (ok, issues) — byte-stable, ordered.
    #[command(name = "plan-materialize")]
    #[command(display_order = 73)]
    PlanMaterialize {
        /// Target spec directory. Also accepts a `.../spec.md` path or a bare
        /// slug. `--spec` / `--from-spec` are hidden aliases.
        #[arg(long = "spec-dir", alias = "spec", alias = "from-spec")]
        spec_dir: String,
        /// Path to the plan JSON file.
        ///
        /// Shape: a `waves` array whose entries carry `n`, `role`, `summary`,
        /// `depends_on` (always the `wave-N-role` form), `tasks`, `files`,
        /// `acceptance` and `satisfies`, plus a top-level `total_waves` and
        /// `lang`. Every parent acceptance criterion MUST be claimed by some
        /// wave — an uncovered one refuses the PLAN. Full schema with a worked
        /// example: the /feature reference full-plan.md, section
        /// `Plan JSON schema`.
        #[arg(long)]
        plan: String,
    },
    /// Composite dispatch face: dispatch-plan + inline agent-prompt-render for
    /// the NEXT pending wave level. Emits
    /// `[{wave, role, subproject, subagent_type, prompt}]` with the prompt
    /// text ready for `Task`. Pending = first dependency level with a wave not
    /// yet carrying `pipeline.wave.complete`; everything done → `[]`.
    #[command(name = "wave-advance")]
    #[command(display_order = 74)]
    WaveAdvance {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
    },
    /// Composite CLOSE face: list `review.result` verdicts (advisory), run QA
    /// in-process, and — only with QA `overall=pass` — finalize via
    /// complete-spec + render pipeline-summary. QA fail/skip →
    /// `completed:false` with the reproved ACs, spec NOT closed (the
    /// `pipeline.complete` QA gate stays the authority — no bypass).
    /// Output: `{"completed":bool,"qa":{overall,criteria},"reviews":[...],`
    /// `"summary":...}`.
    #[command(name = "close-pipeline")]
    #[command(display_order = 75)]
    ClosePipeline {
        /// Spec slug under `.claude/spec/`.
        #[arg(long)]
        spec: String,
    },
}

/// Dispatch one `pipeline`-family `run` subcommand.
pub fn dispatch(cmd: PipelineCmd) {
    match cmd {
        PipelineCmd::DiffContext {
            parent,
            subproject,
            phase,
        } => pipeline::diff_context::run(parent.as_deref(), subproject.as_deref(), phase.as_deref()),
        PipelineCmd::WaveDone { spec, wave, duration_ms } => {
            pipeline::wave_done::run(&spec, wave, duration_ms);
        }
        PipelineCmd::VerifyPipeline { format } => pipeline::verify_pipeline::run(&format),
        PipelineCmd::PipelineSummary { spec_dir, format, self_test } => {
            pipeline::pipeline_summary::run(spec_dir.as_deref(), &format, self_test);
        }
        PipelineCmd::Status { harness, format, root } => {
            pipeline::status::run(pipeline::status::StatusOpts { harness, format, root });
        }
        PipelineCmd::ResumeBootstrap { spec, json } => pipeline::resume_bootstrap::run(&spec, json),
        // --- W5 deep-refactor: T5.1–T5.16 -------------------------------------
        PipelineCmd::CloseOrchestrate { spec, skip_docs } => {
            pipeline::close_orchestrate::run(pipeline::close_orchestrate::CloseOrchestrateOpts { spec, skip_docs });
        }
        PipelineCmd::PlanMaterialize { spec_dir, plan } => {
            pipeline::plan_materialize::run(pipeline::plan_materialize::PlanMaterializeOpts {
                spec_dir,
                plan,
            });
        }
        PipelineCmd::WaveAdvance { spec } => pipeline::wave_advance::run(&spec),
        PipelineCmd::ClosePipeline { spec } => pipeline::close_pipeline::run(&spec),
    }
}
