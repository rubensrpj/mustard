//! The `run` subcommands for agent memory (`knowledge/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`KnowledgeCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run knowledge <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{knowledge};

/// The `run` subcommands owned by agent memory (`knowledge/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum KnowledgeCmd {
    /// Persist agent memory, decisions/lessons, or knowledge entries.
    /// `list` emits all memory entries (knowledge_patterns + decisions + lessons).
    ///
    /// W7 (deep-refactor) adds three subcommands sharing this clap variant:
    /// `write` (`agent_memory` insert + `--verify` round-trip), `search`
    /// (FTS5 + scope filter on `agent_memory`), and `feedback`
    /// (`memory_feedback` append for `deprecate|bump|supersede|use`).
    #[command(display_order = 14)]
    Memory {
        /// Subcommand: `agent`, `decision`, `knowledge`, `list`,
        /// `write`, `search`, or `feedback`.
        subcommand: String,
        /// Input JSON (Windows-friendly form; stdin is the POSIX fallback).
        #[arg(long)]
        json: Option<String>,
        /// `agent` / `write` / `search` — spec name.
        #[arg(long)]
        spec: Option<String>,
        /// `agent` / `write` — wave number (1-based).
        #[arg(long)]
        wave: Option<u32>,
        /// `agent` only — agent identifier/role (becomes `agent_type`).
        #[arg(long)]
        agent: Option<String>,
        /// `agent` / `write` — one-line summary of what the agent produced.
        #[arg(long)]
        summary: Option<String>,
        /// `agent` only — comma-separated list of files affected
        /// (recorded under `details.files`).
        #[arg(long)]
        files: Option<String>,
        /// `list` only — group entries by type (pattern / decision / convention).
        #[arg(long)]
        grouped: bool,
        /// `list` only — output format: `json` (default) or `table`.
        #[arg(long, default_value = "json")]
        format: String,
        /// `search` — scope to a single cluster (`agent_memory.role`).
        #[arg(long)]
        cluster: Option<String>,
        /// `search` only — FTS5 query string.
        #[arg(long)]
        query: Option<String>,
        /// `feedback` only — target memory file path
        /// (e.g. `.claude/memory/decisions/2026-05-26-foo.md`).
        ///
        /// wave-18-rt-followups (W4#3): was `--id <i64>` while memory lived
        /// in SQLite (`agent_memory.id`). After the W4B migration the memory
        /// store is a flat `MarkdownStore`, so the addressable unit is the
        /// file path; the integer id is meaningless. The dispatcher now
        /// forwards this into `FeedbackOpts.path`.
        #[arg(long)]
        path: Option<PathBuf>,
        /// `feedback` only — one of `deprecate|bump|supersede|use`.
        #[arg(long)]
        kind: Option<String>,
        /// `write` only — role label (e.g. `rt`, `dashboard`).
        #[arg(long)]
        role: Option<String>,
        /// `write` only — body text (free-form, may contain JSON).
        #[arg(long)]
        details: Option<String>,
        /// `write` only — initial confidence (0.0–1.0, default 0.5).
        #[arg(long)]
        confidence: Option<f64>,
        /// `write` only — round-trip the row after insert to confirm the
        /// schema + FTS5 mirror are healthy.
        #[arg(long)]
        verify: bool,
        /// `search` only — include rows whose effective confidence (after
        /// lazy decay) sits below the default 0.3 threshold.
        #[arg(long = "include-low")]
        include_low: bool,
        /// `search` only — result cap (default 20).
        #[arg(long)]
        limit: Option<usize>,
        /// `feedback` only — attribution token for the agent supplying the signal.
        #[arg(long = "by-role")]
        by_role: Option<String>,
        /// `feedback` only — free-form note recorded alongside the signal.
        #[arg(long)]
        note: Option<String>,
    },
}

/// Dispatch one `knowledge`-family `run` subcommand.
pub fn dispatch(cmd: KnowledgeCmd) {
    match cmd {
        KnowledgeCmd::Memory {
            subcommand,
            json,
            spec,
            wave,
            agent,
            summary,
            files,
            grouped,
            format,
            cluster,
            query,
            path,
            kind,
            role,
            details,
            confidence,
            verify,
            include_low,
            limit,
            by_role,
            note,
        } => knowledge::memory::dispatch(
            &subcommand,
            json.as_deref(),
            spec.as_deref(),
            wave,
            agent.as_deref(),
            summary.as_deref(),
            files.as_deref(),
            grouped,
            &format,
            knowledge::memory::DispatchExtras {
                cluster,
                query,
                kind,
                role,
                details,
                confidence,
                verify,
                include_low,
                limit,
                by_role,
                note,
                feedback_path: path,
            },
        ),
    }
}
