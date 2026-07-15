#![forbid(unsafe_code)]
// `clippy::unwrap_used` is `deny` workspace-wide. Test modules are exempt so a
// panicking assertion *is* a test failure (mirrors `mustard-core`).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `mustard-cli` — the Mustard command-line tool.
//!
//! Mustard 2.0 scaffolds the thin `.claude/` bootstrap Claude Code projects
//! need and enables the `mustard` plugin (which ships the commands, skills,
//! agents, refs, and hooks). The heavy payload is no longer copied by the CLI.
//!
//! The crate ships **two faces**:
//!
//! - the `mustard` binary ([`main`](../main/index.html)) — what a user runs in
//!   a terminal;
//! - this library — what the Tauri dashboard backend links against, so the
//!   desktop app can install Mustard into a folder without spawning a sidecar.
//!
//! Both faces share the same modules:
//!
//! - [`cli`] — `clap` argument parsing and the subcommand dispatch table.
//! - [`commands`] — one module per subcommand (`init`, `config`, `add`).
//! - [`fs_ops`] — recursive directory copy and surgical JSON merge.

pub mod cli;
pub mod commands;
pub mod fs_ops;

pub use commands::init::{InitOptions, init};

/// The version stamped into `mustard.json` by `init`.
///
/// Sourced from this crate's `Cargo.toml` at compile time so the package
/// version is the single source of truth — no `package.json` lookup, no
/// runtime file read.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");