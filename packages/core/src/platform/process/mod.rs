//! `process` — cross-crate helpers for spawning subprocesses.
//!
//! Currently exposes [`rtk_command`], the Golden Rule helper that prepends
//! `rtk` to every subprocess `mustard-rt` and `mustard-cli` spawn. RTK is a
//! mandatory dependency of Mustard, so the helper does no fail-open probing —
//! callers can assume `rtk` is reachable on `PATH` (the `mustard init` flow
//! enforces this at install time).

pub mod rtk_command;

pub use rtk_command::rtk_command;
