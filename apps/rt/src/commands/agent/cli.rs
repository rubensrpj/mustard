//! The `run` subcommands for agent dispatch (`agent/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`AgentCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run agent <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{agent};

/// The `run` subcommands owned by agent dispatch (`agent/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum AgentCmd {
    /// Finalize open amendment windows for a session (appends `## Amendments` to spec.md,
    /// moves archived specs, updates the DB, and emits `pipeline.amend_close`).
    #[command(display_order = 44)]
    AmendFinalize {
        /// Session identifier whose open windows to finalize.
        #[arg(long = "session-id")]
        session_id: String,
    },
    /// Fold the active session's events into an `analyze.digest.summary`
    /// adherence report: did the scan digest answer (`analyze.digest.used`)
    /// and how many Read/Grep/Glob heartbeats targeted source files directly
    /// (before the first digest use / in total). Emits the event spec-scoped
    /// and prints the same JSON. Fire-and-forget telemetry: fail-open, no
    /// events means zero counts, always exits 0.
    #[command(name = "digest-adherence-finalize")]
    #[command(display_order = 45)]
    DigestAdherenceFinalize {
        /// Spec slug the summary event attributes to.
        #[arg(long)]
        spec: String,
    },
    /// Render the agent dispatch prompt server-side from the embedded
    /// template. Substitutes every `{placeholder}` it can resolve; warns on
    /// stderr for any left unfilled. Stdout = raw prompt string ready for
    /// the Task tool (no JSON framing).
    #[command(display_order = 55)]
    AgentPromptRender {
        /// Spec slug under `.claude/spec/`. Optional: spec-less callers (the
        /// `/scan` Guards enrich step, `/task` with no scope) omit it — the
        /// renderer then derives the locale from `mustard.json#specLang` and
        /// fail-opens every spec-keyed lookup to an empty value.
        #[arg(long)]
        spec: Option<String>,
        /// Wave number (0-based, matching `wave-N-*` directories). Omitted for non-wave specs.
        #[arg(long)]
        wave: Option<u32>,
        /// Agent role token (e.g. `ui`, `backend`).
        #[arg(long)]
        role: String,
        /// Subproject path relative to the project root (e.g. `apps/dashboard`).
        /// Defaults to `.` (the project root) so a dispatch that is not scoped
        /// to a subproject never costs the orchestrator a usage-error
        /// round-trip — the render then reads the root `CLAUDE.md` Guards.
        #[arg(long, default_value = ".")]
        subproject: PathBuf,
        /// Render mode: `first` (default), `granular`, `fix-loop`.
        #[arg(long, default_value = "first")]
        mode: String,
        /// File containing the `{retry_context}` text for granular/fix-loop.
        #[arg(long = "retry-context-file")]
        retry_context_file: Option<PathBuf>,
        /// Keep only task lines whose content matches this pattern (e.g.
        /// `"T0\\.(1|5)"`). Supports literal chars, `\\.` escape, and
        /// `(a|b)` alternation. Omit to include all tasks.
        #[arg(long = "task-filter")]
        task_filter: Option<String>,
        /// Ad-hoc task text for spec-less dispatch (the `/scan` Guards enrich
        /// step, `/task` with no scope). Fills the `## TASK` block when there is
        /// no spec `## Tasks` to read, so the prompt stays self-contained and the
        /// orchestrator never hand-appends the task after the render.
        #[arg(long = "task-text")]
        task_text: Option<String>,
        /// Emit mode: `inline` (default) prints the full rendered prompt;
        /// `ref` writes it to a deterministic `.dispatch/` file and prints a
        /// 2-line stub instead — pass the stub verbatim as the Task prompt
        /// and the PreToolUse hook expands it, so the full text never
        /// transits the orchestrator's context.
        #[arg(long, default_value = "inline")]
        emit: String,
    },
}

/// Dispatch one `agent`-family `run` subcommand.
pub fn dispatch(cmd: AgentCmd) {
    match cmd {
        AgentCmd::AmendFinalize { session_id } => agent::amend_finalize::run_cli(&session_id),
        AgentCmd::DigestAdherenceFinalize { spec } => agent::digest_adherence_finalize::run(&spec),
        AgentCmd::AgentPromptRender {
            spec,
            wave,
            role,
            subproject,
            mode,
            retry_context_file,
            task_filter,
            task_text,
            emit,
        } => agent::agent_prompt_render::run(
            spec.as_deref(),
            wave,
            &role,
            &subproject,
            agent::agent_prompt_render::RenderMode::parse(&mode),
            retry_context_file.as_deref(),
            task_filter.as_deref(),
            task_text.as_deref(),
            agent::agent_prompt_render::EmitMode::parse(&emit),
        ),
    }
}
