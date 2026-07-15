//! `model` — pure data types shared across hooks, scripts, and the CLI.
//!
//! Every type in this module is a plain `serde` struct or enum with **no side
//! effects**: no I/O, no filesystem access, no logging. Side-effecting
//! infrastructure lives in the `store` layer.
//!
//! Submodules:
//!
//! - [`event`] — the harness event schema (stored in `mustard.db`).
//! - [`knowledge`] — the unified [`knowledge::Knowledge`] record subsuming the
//!   five legacy knowledge/memory stores (pure data; the on-disk owner is
//!   `io::knowledge_store::KnowledgeStore`).
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
pub mod knowledge;
pub mod pipeline;
pub mod provenance;
pub mod view;

pub use knowledge::{Knowledge, Kind as KnowledgeKind, Origin, Scope as KnowledgeScope, Status};

// Re-export view types for consumers that import from `mustard_core::domain::model`
// directly. Consumers that need the SDD Phase/Scope should import from
// `mustard_core::domain::model::view::{Phase, Scope}` to avoid ambiguity with
// `mustard_core::domain::model::pipeline::{Phase, Scope}`.
pub use view::{
    AcStatus, AcceptanceCriterion, FileCount, Flags, Outcome, PhaseSegment, QualityRollup,
    SegmentState, SpecFilter, SpecState, SpecStatusFilter, SpecSummary, SpecTrack, SpecView, Stage,
    StateError, TimeWindow, TimelineKind, TimelineNode, WaveStatus, WaveView, WorkspaceAlert,
    WorkspaceAlertKind, WorkspaceSummary,
};
