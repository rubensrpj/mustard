//! The `run` face of `mustard-rt` — the b4 script port.
//!
//! `mustard-rt on` / `mustard-rt check` are the enforcement faces: they read
//! the harness JSON from stdin and run hook modules. The `run` face is
//! different — it ports the utility *scripts* that used to live under
//! `templates/scripts/` as standalone `bun` programs. A `run` subcommand takes
//! its inputs as `clap` arguments (a directory, flags), never from stdin, and
//! prints its result to stdout exactly as the JS script did.
//!
//! Each ported script is its own submodule. Wave 1 ports `sync-detect`
//! (subproject discovery + SHA-256 change detection) and the scanner subsystem
//! it shares with the still-JS `sync-registry.js`.

pub mod scan;
mod complete_spec;
mod context_slice;
mod diff_context;
mod emit_phase;
mod env;
mod epic_fold;
mod memory;
mod sync_detect;
mod sync_registry;

use clap::Subcommand;
use std::path::PathBuf;

/// The `run` subcommands — one variant per ported script.
#[derive(Debug, Subcommand)]
pub enum RunCmd {
    /// Discover subprojects, detect roles, and emit the `sync-detect` JSON.
    SyncDetect {
        /// The monorepo root to scan. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Scan entities, clusters and conventions; write `entity-registry.json` v4.0.
    SyncRegistry {
        /// The monorepo root to scan. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Regenerate even when the registry is already populated.
        #[arg(long)]
        force: bool,
    },
    /// Emit a compact git diff summary for agent context.
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
    /// Record a `pipeline.phase` transition event from a SKILL.
    EmitPhase {
        /// Spec identifier.
        #[arg(long)]
        spec: String,
        /// Phase being entered, e.g. `ANALYZE`.
        #[arg(long)]
        to: String,
        /// Prior phase (optional; defaults to the spec's last known phase).
        #[arg(long)]
        from: Option<String>,
    },
    /// Finalize a pipeline spec (followup mark, archive, or stale sweep).
    CompleteSpec {
        /// Spec name (required unless `--archive-stale`/`--archive-followups`).
        spec: Option<String>,
        /// Finalize archival: move the spec to `completed/` and drop state.
        #[arg(long)]
        archive: bool,
        /// Archive every `closed-followup` state older than 24 h.
        #[arg(long = "archive-stale")]
        archive_stale: bool,
        /// Archive every `closed-followup` state regardless of age.
        #[arg(long = "archive-followups")]
        archive_followups: bool,
    },
    /// Cut the relevant term blocks from one or more `CONTEXT.md` glossaries.
    ContextSlice {
        /// A `CONTEXT.md` / `CONTEXT-MAP.md` path. Repeatable.
        #[arg(long)]
        context: Vec<String>,
        /// The spec file to match relevance against.
        #[arg(long)]
        spec: Option<String>,
        /// Override the line cap (`MUSTARD_GLOSSARY_MAX_LINES`).
        #[arg(long = "max-lines")]
        max_lines: Option<usize>,
    },
    /// Persist agent memory, decisions/lessons, or knowledge entries.
    Memory {
        /// Subcommand: `agent`, `decision`, or `knowledge`.
        subcommand: String,
        /// Input JSON (Windows-friendly form; stdin is the POSIX fallback).
        #[arg(long)]
        json: Option<String>,
    },
    /// Detect or fold a completed epic.
    EpicFold {
        /// List epics whose children are all in `CLOSE`.
        #[arg(long)]
        detect: bool,
        /// Fold the named epic.
        #[arg(long)]
        epic: Option<String>,
    },
}

/// Dispatch a `run` subcommand.
///
/// Unlike the enforcement dispatcher this never touches stdin and never
/// produces an [`Outcome`](mustard_core::model::contract::Outcome) — a `run`
/// script writes its own output and the process exits cleanly afterwards.
pub fn dispatch(cmd: RunCmd) {
    match cmd {
        RunCmd::SyncDetect { root } => sync_detect::run(&root),
        RunCmd::SyncRegistry { root, force } => sync_registry::run(&root, force),
        RunCmd::DiffContext {
            parent,
            subproject,
            phase,
        } => diff_context::run(parent.as_deref(), subproject.as_deref(), phase.as_deref()),
        RunCmd::EmitPhase { spec, to, from } => {
            emit_phase::run(&spec, &to, from.as_deref())
        }
        RunCmd::CompleteSpec {
            spec,
            archive,
            archive_stale,
            archive_followups,
        } => complete_spec::run(spec.as_deref(), archive, archive_stale, archive_followups),
        RunCmd::ContextSlice {
            context,
            spec,
            max_lines,
        } => context_slice::run(&context, spec.as_deref(), max_lines),
        RunCmd::Memory { subcommand, json } => memory::run(&subcommand, json.as_deref()),
        RunCmd::EpicFold { detect, epic } => epic_fold::run(detect, epic.as_deref()),
    }
}
