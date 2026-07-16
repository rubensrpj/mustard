//! One module per `mustard` subcommand.
//!
//! Mustard 2.0 thin bootstrap: [`init`] seeds the harness (settings.json,
//! orchestrator `CLAUDE.md`, `.gitignore`, `mustard.json`) and enables the
//! `mustard` plugin; [`config`] reconfigures git flow; [`add`] installs a
//! third-party community template (the one fetch the plugin marketplace does
//! not cover). `update` was retired — versioned refreshes come from the plugin
//! marketplace, and re-running `init` re-stamps `mustard.json#version`.
//!
//! [`git_flow`] is not a subcommand but the shared git-flow configuration
//! routine `init` runs to produce the project-root `mustard.json`; [`config`]
//! is a thin wrapper over it.

pub mod add;
pub mod config;
pub mod git_flow;
pub mod init;
pub mod install_grammars;
pub mod install_nerd_font;