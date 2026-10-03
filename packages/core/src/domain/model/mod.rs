//! `model` — pure data types shared across hooks, scripts, and the CLI.
//!
//! Every type in this module is a plain `serde` struct or enum with **no side
//! effects**: no I/O, no filesystem access, no logging. Side-effecting
//! infrastructure lives in the `io` layer.
//!
//! Submodules:
//!
//! - [`contract`] — the hook contract: [`contract::HookInput`],
//!   [`contract::Verdict`], [`contract::Outcome`], [`contract::Trigger`], and
//!   the [`contract::Check`] / [`contract::Observer`] traits. **Frozen**: every
//!   hook module depends on it.

pub mod contract;
