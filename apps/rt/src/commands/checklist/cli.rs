//! The `run` subcommands for audit checklists (`checklist/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`ChecklistCmd`] AND its arm in [`dispatch`] below; forgetting the arm still
//! compiles, but the command vanishes from the CLI. The other two live in
//! the tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run checklist <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;

use crate::commands::{checklist};

/// The `run` subcommands owned by audit checklists (`checklist/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum ChecklistCmd {
    /// Mark a `## Checklist` item done in a spec — or, with `--drop
    /// --reason`, record it as dropped on purpose.
    #[command(display_order = 13)]
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
        /// Record the item as dropped on purpose instead of done. Requires
        /// `--reason` — a drop with nothing stated is refused (exit 2).
        #[arg(long = "drop")]
        drop_item: bool,
        /// Why the item was dropped. Only meaningful with `--drop`.
        #[arg(long)]
        reason: Option<String>,
    },
}

/// Dispatch one `checklist`-family `run` subcommand.
pub fn dispatch(cmd: ChecklistCmd) {
    match cmd {
        ChecklistCmd::MarkChecklistItem {
            spec,
            item,
            line,
            cwd,
            drop_item,
            reason,
        } => checklist::mark_checklist_item::run(
            spec.as_deref(),
            item.as_deref(),
            line,
            cwd.as_deref(),
            drop_item,
            reason.as_deref(),
        ),
    }
}
