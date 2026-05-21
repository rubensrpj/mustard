//! `model` — pure data types shared across hooks, scripts, and the CLI.
//!
//! Every type in this module is a plain `serde` struct or enum with **no side
//! effects**: no I/O, no filesystem access, no logging. Side-effecting
//! infrastructure lives in the `store` layer.
//!
//! Submodules:
//!
//! - [`event`] — the harness event schema (stored in `mustard.db`).
//! - [`contract`] — the hook contract: [`contract::HookInput`],
//!   [`contract::Verdict`], [`contract::Outcome`], [`contract::Trigger`], and
//!   the [`contract::Check`] / [`contract::Observer`] traits. **Frozen at the
//!   end of Wave 1** — B3/B4 depend on it.
//! - [`pipeline`] — `pipeline-state` types ([`pipeline::PipelineState`],
//!   [`pipeline::Phase`], [`pipeline::Scope`]).
//! - [`provenance`] — the managed-artifact manifest
//!   ([`provenance::ArtifactManifest`], [`provenance::ArtifactRecord`]).
//! - [`view`] — typed `ViewModels` for the SDD domain layer: `SpecView`,
//!   `WaveView`, `QualityRollup`, `WorkspaceSummary`, and the `SpecReader`
//!   filter/window types.

pub mod contract;
pub mod event;
pub mod pipeline;
pub mod provenance;
pub mod view;

// Re-export view types for consumers that import from `mustard_core::model`
// directly. Consumers that need the SDD Phase/Scope should import from
// `mustard_core::model::view::{Phase, Scope}` to avoid ambiguity with
// `mustard_core::model::pipeline::{Phase, Scope}`.
#[allow(deprecated)] // SpecStatus is re-exported during the W1→W7 migration window.
pub use view::SpecStatus;
pub use view::{
    AcStatus, AcceptanceCriterion, FileCount, Flags, Outcome, PhaseSegment, QualityRollup,
    SegmentState, SpecFilter, SpecState, SpecStatusFilter, SpecSummary, SpecTrack, SpecView, Stage,
    StateError, TimeWindow, TimelineKind, TimelineNode, WaveStatus, WaveView, WorkspaceAlert,
    WorkspaceAlertKind, WorkspaceSummary,
};
