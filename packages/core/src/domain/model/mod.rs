//! `model` — pure data types shared across hooks, scripts, and the CLI.
//!
//! Every type in this module is a plain `serde` struct or enum with **no side
//! effects**: no I/O, no filesystem access, no logging. Side-effecting
//! infrastructure lives in the `store` layer.
//!
//! Submodules:
//!
//! - [`contract`] — the hook contract: [`contract::HookInput`],
//!   [`contract::Verdict`], [`contract::Outcome`], [`contract::Trigger`], and
//!   the [`contract::Check`] / [`contract::Observer`] traits. **Frozen**: every
//!   hook module depends on it.
//! - [`view`] — typed `ViewModels` for the SDD domain layer: `SpecView`,
//!   `WaveView`, `QualityRollup`, `WorkspaceSummary`, and the `SpecReader`
//!   filter/window types.

pub mod contract;
pub mod view;


// Re-export view types for consumers that import from `mustard_core::domain::model`
// directly.
pub use view::{Flags, Outcome, SpecState, SpecSummary, SpecView, Stage, StateError};
