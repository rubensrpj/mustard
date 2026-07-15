#![forbid(unsafe_code)]
// `clippy::unwrap_used` is `deny` workspace-wide. Test modules are exempt so a
// panicking assertion *is* a test failure (mirrors `mustard-core`).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `mustard-cli` — the Mustard command-line tool, ported from TypeScript.
//!
//! Mustard scaffolds the `.claude/` folder Claude Code projects need: prompts,
//! commands, hooks, and rules. The JavaScript CLI ran under Bun; this crate is
//! the native Rust port (epic B5).
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
//! - [`commands`] — one module per subcommand (`init`, `update`, `config`,
//!   `add`).
//! - [`fs_ops`] — recursive directory copy and surgical JSON merge, shared by
//!   `init` and `update`.

pub mod cli;
pub mod commands;
pub mod fs_ops;

pub use commands::init::{InitOptions, init};
pub use commands::update::{UpdateOptions, update};

/// The version stamped into `mustard.json` by `init`/`update`.
///
/// Sourced from this crate's `Cargo.toml` at compile time so the package
/// version is the single source of truth — no `package.json` lookup, no
/// runtime file read.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
