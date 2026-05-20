//! [`SpecView`] — the rich per-spec ViewModel rendered in drill-down UIs, and
//! [`SpecSummary`] — the lean sibling used in list views.
//!
//! Both share [`SpecStatus`]. Notice the absence of an `Unknown` variant:
//! when an event stream cannot produce a status (a spec with zero events, or
//! one whose only events are the `__orphan__` backfill bucket), we resolve to
//! the explicit [`SpecStatus::NoEvents`] variant. UIs render the variant
//! deliberately; they don't paint a grey "UNKNOWN" badge by accident.

use super::{Phase, Scope};
use serde::{Deserialize, Serialize};

/// The lifecycle status of a single spec.
///
/// Ordered roughly from "earliest active" to "terminal" so a UI can paint a
/// reasonable colour ramp by variant. The serialized form is kebab-case so it
/// round-trips with the on-disk spec header (`### Status: closed-followup`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpecStatus {
    /// Spec exists on disk but no harness events have arrived yet — typically
    /// a fresh spec before `/mustard:feature` has dispatched. Distinct from
    /// `Planning` because no `pipeline.scope` event has been emitted.
    NoEvents,
    /// Spec is in the PLAN phase but EXECUTE has not started.
    Planning,
    /// Pipeline is actively running waves.
    Implementing,
    /// Pipeline finished EXECUTE; REVIEW agents are running.
    Reviewing,
    /// Pipeline finished REVIEW; QA agents are running.
    Qa,
    /// Pipeline finished, dir still under `active/` for the follow-up window.
    ClosedFollowup,
    /// Pipeline finished and archived to `completed/`.
    Completed,
    /// Pipeline was cancelled before completing.
    Cancelled,
    /// Pipeline is paused — explicit user intervention required.
    Blocked,
    /// Wave plan reached a wave that failed twice in a row.
    WaveFailed,
}

impl SpecStatus {
    /// Whether this status counts as "active" for filters in the workspace
    /// (Visão Geral) and Specs list.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Planning
                | Self::Implementing
                | Self::Reviewing
                | Self::Qa
                | Self::ClosedFollowup
                | Self::Blocked
                | Self::WaveFailed
        )
    }

    /// Whether this status is terminal (the pipeline is done, success or not).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// Parse the on-disk header string. Accepts the legacy `"closed-followup"`
    /// spelling. Returns `None` for unknown values so the caller (a
    /// projection) can decide whether to fall back to `NoEvents`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "no-events" => Some(Self::NoEvents),
            "planning" | "draft" | "approved" => Some(Self::Planning),
            "implementing" | "in-progress" | "in_progress" => Some(Self::Implementing),
            "reviewing" => Some(Self::Reviewing),
            "qa" => Some(Self::Qa),
            "closed-followup" | "closed_followup" => Some(Self::ClosedFollowup),
            "completed" | "closed" => Some(Self::Completed),
            "cancelled" | "canceled" => Some(Self::Cancelled),
            "blocked" | "paused" => Some(Self::Blocked),
            "wave-failed" | "wave_failed" => Some(Self::WaveFailed),
            _ => None,
        }
    }
}

/// Rich per-spec view — the shape the dashboard drill-down renders.
///
/// Every field is `Option<…>` or a counter; absence is encoded as `None` or
/// zero, never a literal `"unknown"` string. Counters default to zero so an
/// empty event stream produces a coherent zeroed view rather than panicking
/// or returning an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecView {
    /// Spec name — the directory name under `.claude/spec/`.
    pub spec: String,
    /// Lifecycle status, projected from `pipeline.scope` + `pipeline.status`.
    pub status: SpecStatus,
    /// Latest phase from `pipeline.phase` events.
    pub phase: Option<Phase>,
    /// Scope from `pipeline.scope.payload.scope`.
    pub scope: Option<Scope>,
    /// Language tag from `pipeline.scope.payload.lang` (`"pt"` or `"en"`).
    pub lang: Option<String>,
    /// Model name from `pipeline.scope.payload.model`.
    pub model: Option<String>,
    /// ISO-8601 timestamp of the first event for this spec.
    pub started_at: Option<String>,
    /// ISO-8601 timestamp of the most recent event for this spec.
    pub last_event_at: Option<String>,
    /// Milliseconds between `started_at` and `last_event_at`. None until both
    /// timestamps are present.
    pub duration_ms: Option<i64>,
    /// Index of the current wave (`pipeline.wave.complete` max + 1, capped at
    /// `total_waves`). None for non-wave-plan specs.
    pub current_wave: Option<u32>,
    /// Total number of waves declared in the wave plan. None for single-spec
    /// pipelines.
    pub total_waves: Option<u32>,
    /// Waves the pipeline has finished, in order.
    pub completed_waves: Vec<u32>,
    /// Waves recorded as failed via `pipeline.wave.failed` or a fix-loop cap.
    pub failed_waves: Vec<u32>,
    /// Number of Acceptance Criteria that returned `pass` in the latest
    /// `qa.result` event.
    pub ac_passed: u32,
    /// Total Acceptance Criteria listed in the latest `qa.result` event.
    pub ac_total: u32,
    /// Number of Acceptance Criteria that returned `fail` or `error`.
    pub ac_failed: u32,
    /// Number of distinct files touched across all `tool.use` events scoped
    /// to this spec.
    pub files_touched: u32,
    /// Count of `tool.use` events for this spec.
    pub tools_used: u32,
    /// Count of `agent.start` events for this spec.
    pub agents_dispatched: u32,
    /// `true` when `pipeline.scope.payload.is_wave_plan` was set.
    pub is_wave_plan: bool,
}

impl SpecView {
    /// Construct an empty view for `spec` — the starting point for any fold
    /// over the event stream. Status defaults to [`SpecStatus::NoEvents`]
    /// until evidence to the contrary lands.
    #[must_use]
    pub fn empty(spec: impl Into<String>) -> Self {
        Self {
            spec: spec.into(),
            status: SpecStatus::NoEvents,
            phase: None,
            scope: None,
            lang: None,
            model: None,
            started_at: None,
            last_event_at: None,
            duration_ms: None,
            current_wave: None,
            total_waves: None,
            completed_waves: Vec::new(),
            failed_waves: Vec::new(),
            ac_passed: 0,
            ac_total: 0,
            ac_failed: 0,
            files_touched: 0,
            tools_used: 0,
            agents_dispatched: 0,
            is_wave_plan: false,
        }
    }
}

/// Lean per-spec view — the shape rendered in the Specs list, the workspace
/// `spec_tracks`, and the Topbar dropdown. Drops the heavy collections
/// (`completed_waves`, etc.) so a list of 100 specs stays light.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecSummary {
    /// Spec name.
    pub spec: String,
    /// Lifecycle status.
    pub status: SpecStatus,
    /// Latest phase.
    pub phase: Option<Phase>,
    /// Scope tag.
    pub scope: Option<Scope>,
    /// ISO-8601 of the most recent event.
    pub last_event_at: Option<String>,
    /// ISO-8601 of the first event.
    pub started_at: Option<String>,
    /// Current wave (1-based) when this is a wave plan.
    pub current_wave: Option<u32>,
    /// Total waves declared.
    pub total_waves: Option<u32>,
    /// Acceptance Criteria pass count.
    pub ac_passed: u32,
    /// Acceptance Criteria total.
    pub ac_total: u32,
}

impl From<&SpecView> for SpecSummary {
    /// Project a rich view into the lean summary shape. Useful when the same
    /// projection has already paid the cost of computing the rich view.
    fn from(view: &SpecView) -> Self {
        Self {
            spec: view.spec.clone(),
            status: view.status,
            phase: view.phase,
            scope: view.scope,
            last_event_at: view.last_event_at.clone(),
            started_at: view.started_at.clone(),
            current_wave: view.current_wave,
            total_waves: view.total_waves,
            ac_passed: view.ac_passed,
            ac_total: view.ac_total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_view_starts_at_no_events_with_zero_counters() {
        let view = SpecView::empty("feature-x");
        assert_eq!(view.spec, "feature-x");
        assert_eq!(view.status, SpecStatus::NoEvents);
        assert_eq!(view.ac_total, 0);
        assert_eq!(view.tools_used, 0);
        assert!(view.completed_waves.is_empty());
        assert!(!view.is_wave_plan);
    }

    #[test]
    fn status_parse_accepts_canonical_and_synonyms() {
        assert_eq!(SpecStatus::parse("draft"), Some(SpecStatus::Planning));
        assert_eq!(SpecStatus::parse("approved"), Some(SpecStatus::Planning));
        assert_eq!(SpecStatus::parse("in_progress"), Some(SpecStatus::Implementing));
        assert_eq!(SpecStatus::parse("CLOSED-FOLLOWUP"), Some(SpecStatus::ClosedFollowup));
        assert_eq!(SpecStatus::parse("done"), None); // explicitly NOT "completed"
    }

    #[test]
    fn status_classification_buckets_match_pipeline_lifecycle() {
        assert!(SpecStatus::Implementing.is_active());
        assert!(SpecStatus::ClosedFollowup.is_active());
        assert!(!SpecStatus::Completed.is_active());
        assert!(SpecStatus::Completed.is_terminal());
        assert!(SpecStatus::Cancelled.is_terminal());
        assert!(!SpecStatus::Implementing.is_terminal());
    }

    #[test]
    fn spec_summary_from_view_preserves_identity_fields() {
        let mut view = SpecView::empty("auth");
        view.status = SpecStatus::Implementing;
        view.ac_passed = 3;
        view.ac_total = 5;
        view.current_wave = Some(2);

        let summary: SpecSummary = (&view).into();
        assert_eq!(summary.spec, "auth");
        assert_eq!(summary.status, SpecStatus::Implementing);
        assert_eq!(summary.ac_passed, 3);
        assert_eq!(summary.current_wave, Some(2));
    }
}
