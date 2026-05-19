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
mod analyze_validation;
mod complete_spec;
mod context_slice;
mod diff_context;
mod emit_phase;
mod env;
mod epic_fold;
mod exec_rewave_check;
mod mark_checklist_item;
mod memory;
mod recipe_match;
mod scope_decompose;
mod spec_extract;
mod spec_link;
mod spec_sections;
mod sync_detect;
mod sync_registry;
mod wave_dependency;
mod wave_lib;
mod wave_size_check;
mod wave_tree;

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
    /// Cut a single wave slice (or AC block) from a `spec.md`.
    SpecExtract {
        /// Path to the spec file.
        #[arg(long)]
        spec: String,
        /// Wave number to extract.
        #[arg(long)]
        wave: Option<u32>,
        /// Extract the `## Acceptance Criteria` section instead.
        #[arg(long)]
        ac: bool,
        /// Emit a JSON omission-measurement instead of the slice text.
        #[arg(long)]
        measure: bool,
    },
    /// Link a child spec to a parent (epic) spec.
    SpecLink {
        /// Parent (epic) spec name.
        #[arg(long)]
        parent: Option<String>,
        /// Child spec name.
        #[arg(long)]
        child: Option<String>,
        /// Why the split happened (recorded in the `spec.link` event).
        #[arg(long)]
        reason: Option<String>,
    },
    /// Validate a spec's structure (WARN-level — never blocks).
    AnalyzeValidation {
        /// Path to the spec file.
        #[arg(long)]
        spec: Option<String>,
    },
    /// Mark a `## Checklist` item done in a spec.
    MarkChecklistItem {
        /// Spec name or absolute `spec.md` path.
        #[arg(long)]
        spec: Option<String>,
        /// Substring of the checklist item text to match.
        #[arg(long)]
        item: Option<String>,
        /// 1-based line number of the checkbox (alternative to `--item`).
        #[arg(long)]
        line: Option<usize>,
        /// Project root override.
        #[arg(long)]
        cwd: Option<String>,
    },
    /// Render a spec's wave structure as an ASCII or JSON tree.
    WaveTree {
        /// Path to the spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: String,
        /// Output format: `ascii` (default) or `json`.
        #[arg(long, default_value = "ascii")]
        format: String,
    },
    /// Analyze file dependencies across waves (reads JSON from stdin).
    WaveDependency,
    /// Suggest wave decomposition by file/entity count (reads JSON from stdin).
    ScopeDecompose,
    /// Check whether a spec should be decomposed at EXECUTE entry.
    ExecRewaveCheck {
        /// Path to the spec file.
        #[arg(long)]
        spec: Option<String>,
    },
    /// Audit per-wave file/layer counts inside a wave-plan.
    WaveSizeCheck {
        /// Path to the spec directory.
        #[arg(long = "spec-dir")]
        spec_dir: Option<String>,
    },
    /// Match an entity + operation to a code recipe skeleton.
    RecipeMatch {
        /// Entity name.
        #[arg(long)]
        entity: Option<String>,
        /// Operation type.
        #[arg(long)]
        operation: Option<String>,
        /// Subproject path used for placeholder resolution.
        #[arg(long)]
        subproject: Option<String>,
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
        RunCmd::SpecExtract {
            spec,
            wave,
            ac,
            measure,
        } => spec_extract::run(&spec, wave, ac, measure),
        RunCmd::SpecLink {
            parent,
            child,
            reason,
        } => spec_link::run(parent.as_deref(), child.as_deref(), reason.as_deref()),
        RunCmd::AnalyzeValidation { spec } => analyze_validation::run(spec.as_deref()),
        RunCmd::MarkChecklistItem {
            spec,
            item,
            line,
            cwd,
        } => mark_checklist_item::run(spec.as_deref(), item.as_deref(), line, cwd.as_deref()),
        RunCmd::WaveTree { spec_dir, format } => wave_tree::run(&spec_dir, &format),
        RunCmd::WaveDependency => wave_dependency::run(),
        RunCmd::ScopeDecompose => scope_decompose::run(),
        RunCmd::ExecRewaveCheck { spec } => exec_rewave_check::run(spec.as_deref()),
        RunCmd::WaveSizeCheck { spec_dir } => wave_size_check::run(spec_dir.as_deref()),
        RunCmd::RecipeMatch {
            entity,
            operation,
            subproject,
        } => recipe_match::run(entity.as_deref(), operation.as_deref(), subproject.as_deref()),
    }
}
