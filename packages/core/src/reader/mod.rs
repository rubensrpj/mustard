//! The [`SpecReader`] trait — the public contract every consumer programs
//! against.
//!
//! Two implementations are shipped:
//!
//! - [`SqliteSpecReader`] — production adapter over
//!   [`SqliteEventStore`](crate::store::sqlite_store::SqliteEventStore).
//! - [`InMemorySpecReader`] — test double seeded with an event vector. Same
//!   contract, runs without IO, the property tests exercise both behind a
//!   single set of assertions.
//!
//! Add a new adapter (e.g. a cached decorator) by `impl SpecReader for …` —
//! consumers depend on the trait, so nothing else has to change.

pub mod error;
mod memory;
mod sqlite;

pub use error::{ReadError, Result as ReadResult};
pub use memory::InMemorySpecReader;
pub use sqlite::SqliteSpecReader;

use crate::reader::error::Result;
use crate::model::view::{
    QualityRollup, SpecFilter, SpecSummary, SpecView, TimeWindow, TimelineNode, WaveView,
    WorkspaceSummary,
};

/// Read-side contract for the SDD domain layer.
///
/// All methods are fail-open at the data level: missing rows return
/// `Ok(None)` or empty collections. They only return [`Err`] for genuine
/// infrastructure failures (DB unreachable, malformed argument).
///
/// `Send + Sync` so consumers can put the reader behind an `Arc` and share
/// it across threads — Tauri command handlers, in particular, need this.
pub trait SpecReader: Send + Sync {
    /// Rich per-spec view for the drill-down UI.
    ///
    /// Returns `Ok(None)` when the spec has no events at all (truly unknown to
    /// the harness). Returns `Ok(Some(view))` even when the spec exists with
    /// only a few events — `view.status` will be [`SpecStatus::NoEvents`]
    /// when only orphans were attributed to it.
    ///
    /// [`SpecStatus::NoEvents`]: crate::model::view::SpecStatus::NoEvents
    ///
    /// # Errors
    /// Returns [`ReadError`] for IO or decode failures.
    fn spec_view(&self, spec: &str) -> Result<Option<SpecView>>;

    /// Lean per-spec summary for list UIs.
    ///
    /// # Errors
    /// Returns [`ReadError`] for IO or decode failures.
    fn spec_summary(&self, spec: &str) -> Result<Option<SpecSummary>>;

    /// List specs matching the supplied filter. Ordered by `last_event_at`
    /// descending so the most recently active spec comes first.
    ///
    /// # Errors
    /// Returns [`ReadError`] for IO or decode failures.
    fn list_specs(&self, filter: &SpecFilter) -> Result<Vec<SpecSummary>>;

    /// Wave-by-wave breakdown for the spec drill-down.
    ///
    /// # Errors
    /// Returns [`ReadError`] for IO or decode failures.
    fn waves(&self, spec: &str) -> Result<Vec<WaveView>>;

    /// Acceptance Criteria roll-up.
    ///
    /// # Errors
    /// Returns [`ReadError`] for IO or decode failures.
    fn quality(&self, spec: &str) -> Result<QualityRollup>;

    /// Chronological event timeline for a single spec, filtered by `window`.
    ///
    /// # Errors
    /// Returns [`ReadError`] for IO or decode failures.
    fn timeline(&self, spec: &str, window: TimeWindow) -> Result<Vec<TimelineNode>>;

    /// Workspace-wide summary (Visão Geral).
    ///
    /// # Errors
    /// Returns [`ReadError`] for IO or decode failures.
    fn workspace_summary(&self) -> Result<WorkspaceSummary>;
}
