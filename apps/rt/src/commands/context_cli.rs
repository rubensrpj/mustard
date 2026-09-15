//! The `run` subcommands for repo-model retrieval (`feature` / `orient`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`ContextCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run context <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{feature, orient};

/// The `run` subcommands owned by repo-model retrieval (`feature` / `orient`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum ContextCmd {
    /// Research a feature request against the repo via the `scan` digest (no
    /// source reading) and emit the structured insumos for decomposition +
    /// `scan spec`. The grounding step of the elicitation loop.
    #[command(display_order = 2)]
    Feature {
        /// The free-text feature/bugfix request to research. The orchestration
        /// layer passes any cross-lingual translation INSIDE this text
        /// (`--intent "<user prompt> <english translation>"`); the command stays
        /// pure deterministic and queries the DISTINCT union of the tokens.
        #[arg(long)]
        intent: String,
        /// Workspace root. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Project `.claude/grain.model.json` into the orientation census — the
    /// terrain map the AI reads instead of cold-starting with `grep`.
    ///
    /// One line per architectural subproject (`name · kind · Nf — role`),
    /// reusing the same `Project.kind` / `Project.code_files` the
    /// subproject-`CLAUDE.md` footer renders, with the architectural layer
    /// (`L0`/`L1`/`L2`) joined from grain's `skeleton[]`. Fail-open: a
    /// missing / unreadable model prints nothing, exit 0. Byte-stable output.
    /// (The per-prompt Level-2 entrypoints were removed: lexical prompt×path
    /// matching measured 1 useful hit in 17 across two field sessions.)
    #[command(display_order = 3)]
    Orient {
        /// Workspace root (holds `.claude/grain.model.json`). Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `context`-family `run` subcommand.
pub fn dispatch(cmd: ContextCmd) {
    match cmd {
        ContextCmd::Feature { intent, root } => feature::run(&intent, &root),
        ContextCmd::Orient { root } => orient::run(&root),
    }
}
