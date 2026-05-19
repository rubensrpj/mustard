//! One module per `mustard` subcommand.
//!
//! Wave 1 ported [`init`]. Wave 2 ports the rest: [`update`], [`config`],
//! [`add`], [`review`], and [`auto_update`].
//!
//! [`git_flow`] is not a subcommand but the shared git-flow configuration
//! routine `init` runs to produce the project-root `mustard.json`; [`config`]
//! is a thin wrapper over it.

pub mod add;
pub mod auto_update;
pub mod config;
pub mod git_flow;
pub mod init;
pub mod review;
pub mod update;
