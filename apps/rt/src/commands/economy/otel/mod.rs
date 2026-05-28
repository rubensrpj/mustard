//! The OTEL ports — NDJSON-backed since W5A of the no-sqlite refactor.
//!
//! Two `run` subcommands share an OTLP/JSON projection:
//!
//! - [`collector`] — `mustard-rt run otel-collector`, the local OTLP/JSON
//!   receiver (port of `scripts/otel-collector.js`).
//! - [`diagnose`] — `mustard-rt run diagnose-otel`, the pipeline health check
//!   (port of `scripts/diagnose-otel.js`).
//!
//! ## Persistence model (post-W5A)
//!
//! Telemetry events are appended to the per-spec NDJSON event log under
//! `.claude/spec/<spec>/.events/` (cross-session when no spec is active) as
//! `pipeline.telemetry.metric` records. The legacy `telemetry.db` SQLite sink
//! is gone — the collector now calls
//! [`crate::shared::events::writer_ndjson::write_event_with_ts`] directly, and the
//! diagnose face reads the same channel back via [`mustard_core::EventReader`].
//!
//! Filtering remains: the collector still drops every metric outside its
//! local `CONSUMED_METRICS` list (see [`collector`]) before writing, so the
//! NDJSON sink only carries the handful the dashboard actually reads.

pub mod collector;
pub mod diagnose;
pub mod project;

use mustard_core::ClaudePaths;
use std::path::PathBuf;

/// One projected metric datapoint — the argument bundle for the collector's
/// metric ingestion path. Lives here (was in the deleted `store.rs`) so
/// `project.rs` can produce it and `collector.rs` can serialize it to NDJSON.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricRow {
    /// Minute-floored ms-epoch — the `updated_at` freshness signal.
    pub ts_bucket: i64,
    /// OTLP metric name, e.g. `claude_code.token.usage`.
    pub metric: String,
    /// `session.id` attribute, if present.
    pub session_id: Option<String>,
    /// `model` attribute, if present.
    pub model: Option<String>,
    /// `type` attribute (only on token.usage), if present. Retained on the
    /// projection struct for the `project` walker's compatibility, but no
    /// longer persisted in the reduced schema.
    pub token_type: Option<String>,
    /// The datapoint's numeric value.
    pub sum: f64,
    /// JSON of the remaining (non-projected) attributes. Retained on the
    /// projection struct but not promoted to dedicated NDJSON columns.
    pub attrs: String,
}

/// A single `[data]` sample row, surfaced by the diagnose face. Mirrors the
/// pre-W5A `SampleRow` shape so the diagnose JSON output stays byte-stable.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleRow {
    /// OTLP metric name (or log body).
    pub metric: String,
    /// `session.id` attribute.
    pub session_id: Option<String>,
    /// `model` attribute.
    pub model: Option<String>,
    /// Aggregated value across every contributing datapoint.
    pub sum: f64,
    /// ms-epoch of the most recent contributing datapoint.
    pub updated_at: Option<i64>,
}

/// Resolve `<project>/.claude` for the OTEL ports.
///
/// Routed through `ClaudePaths` so the I1 guard fires at the boundary; a
/// rejection collapses to an empty `PathBuf` (the caller's downstream IO will
/// then degrade gracefully — every harness path is a fail-open read).
#[must_use]
pub fn claude_dir() -> PathBuf {
    ClaudePaths::for_project(PathBuf::from(crate::shared::context::project_dir()))
        .map(|p| p.claude_dir())
        .unwrap_or_default()
}
