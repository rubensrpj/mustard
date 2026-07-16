//! The `run` subcommands for the Claude Code status bar (`statusline/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`StatuslineCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run statusline <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;

use crate::commands::{statusline};

/// The `run` subcommands owned by the Claude Code status bar (`statusline/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum StatuslineCmd {
    /// Render the Claude Code status bar (reads the payload JSON from stdin),
    /// or `--preview` every shipped theme on its own labelled line.
    #[command(display_order = 36)]
    Statusline {
        /// Skip stdin; render every theme with a synthetic payload.
        #[arg(long)]
        preview: bool,
    },
}

/// Dispatch one `statusline`-family `run` subcommand.
pub fn dispatch(cmd: StatuslineCmd) {
    match cmd {
        StatuslineCmd::Statusline { preview } => statusline::run(preview),
    }
}
