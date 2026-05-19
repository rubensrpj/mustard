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
    }
}
