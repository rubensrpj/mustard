#![forbid(unsafe_code)]
// `clippy::unwrap_used` is `deny` workspace-wide so no hook-path code can
// panic (b2 spec § Preocupações — fail-open). Clippy does *not* exempt
// `#[cfg(test)]` code from that lint, so the spec's "exceto em módulos de
// teste" carve-out is applied explicitly here: under `cfg(test)`, `.unwrap()`
// / `.expect()` are allowed — a panicking assertion *is* a test failure.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `mustard-core` — shared foundation crate for the Mustard Rust migration.
//!
//! This crate concentrates the logic that hooks, scripts, and the CLI all
//! depend on, so the port from JavaScript (epics B3/B4/B5) stays lean instead
//! of re-implementing the same primitives dozens of times.
//!
//! Layers:
//!
//! - [`model`] — pure `serde` data types with zero side effects: the harness
//!   event schema, the hook contract, pipeline-state, and the SDD ViewModels
//!   under [`model::view`].
//! - [`store`] — side-effecting infrastructure behind traits: the event log,
//!   `pipeline-state` read/write, and fail-open filesystem primitives.
//! - [`projection`] — pure folds over `&[HarnessEvent]`: one function per
//!   ViewModel. No IO, no side effects — deterministic and testable in
//!   isolation.
//! - [`reader`] — the [`reader::SpecReader`] trait + two adapters:
//!   [`reader::SqliteSpecReader`] (production) and
//!   [`reader::InMemorySpecReader`] (tests). Thin wrappers that supply the
//!   event slice from the store and delegate to `projection`.
//! - [`error`] — the crate's typed error plus fail-open helpers.
//! - cross-cutting foundation — [`config`] (enforcement modes), [`env`] (the
//!   `hook-env.js` port), [`metrics`] (the `metrics-emit.js` port), and
//!   [`knowledge`] (knowledge extraction + the inter-agent context-selection
//!   API).

pub mod config;
pub mod env;
pub mod error;
pub mod store;
pub mod knowledge;
pub mod metrics;
pub mod model;
pub mod projection;
pub mod reader;

// Root re-exports — consumers can write `use mustard_core::…` without
// remembering which sub-module owns each name.
pub use model::view::{
    AcStatus, AcceptanceCriterion, FileCount, Phase, PhaseSegment, QualityRollup, Scope,
    SegmentState, SpecFilter, SpecStatus, SpecStatusFilter, SpecSummary, SpecTrack, SpecView,
    TimeWindow, TimelineKind, TimelineNode, WaveStatus, WaveView, WorkspaceAlert,
    WorkspaceAlertKind, WorkspaceSummary,
};
pub use reader::{InMemorySpecReader, ReadError, SpecReader, SqliteSpecReader};
