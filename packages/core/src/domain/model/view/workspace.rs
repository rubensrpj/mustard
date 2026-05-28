//! [`WorkspaceSummary`] — the top-level shape for the Visão Geral page.
//!
//! Aggregates information across **all** active specs in the workspace.
//! `tokens_saved_today` is intentionally `Option<i64>`: the source (RTK,
//! prompt-economy events) may be unavailable on this host, and the UI
//! distinguishes "no data" from "zero" — a literal `0` would mislead.

use super::Phase;
#[allow(deprecated)] // SpecTrack still surfaces the legacy SpecStatus alongside the canonical state.
use super::SpecStatus;
use super::SpecState;
use serde::{Deserialize, Serialize};

/// One file + its count for "top files today" rolls-ups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileCount {
    /// Absolute or workspace-relative path, exactly as logged.
    pub path: String,
    /// Number of `tool.use` events that touched this path inside the window.
    pub count: u32,
}

/// State of a single phase segment in a [`SpecTrack`]. Three explicit values —
/// the UI never has to guess "is this past the current phase?".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentState {
    /// Phase happened.
    Completed,
    /// Phase is happening right now.
    Active,
    /// Phase has not started.
    Future,
}

/// One phase segment of a spec's pipeline trajectory. Five of these compose a
/// [`SpecTrack`] in canonical lifecycle order (`Analyze`..`Close`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseSegment {
    /// The phase this segment represents.
    pub phase: Phase,
    /// Render state.
    pub state: SegmentState,
}

/// One row of the "Sala de Operações multi-track" — one spec, one trajectory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(deprecated)] // carries the legacy `status` field until Wave 3 migrates SpecTrack to SpecState.
pub struct SpecTrack {
    /// Spec name.
    pub spec: String,
    /// Current status.
    #[deprecated(note = "Use the canonical `state` field; this legacy projection is kept for back-compat.")]
    pub status: SpecStatus,
    /// Canonical lifecycle state (Stage/Outcome/Flags) — the source of truth.
    /// Consumers should read this; `status` above is a derived legacy alias.
    pub state: SpecState,
    /// Phase the spec is currently in.
    pub current_phase: Option<Phase>,
    /// Current wave (1-based) when this is a wave plan.
    pub current_wave: Option<u32>,
    /// Total waves declared in the wave plan.
    pub total_waves: Option<u32>,
    /// Number of distinct active agent ids attached to this spec (counted
    /// from `agent.start` events that have no matching `agent.stop`).
    pub agents_active: u32,
    /// ISO-8601 of the most recent event scoped to this spec.
    pub last_event_at: Option<String>,
    /// Reason carried by the most recent `pipeline.pause` event, when the
    /// status is `Blocked`. None otherwise.
    pub blocked_reason: Option<String>,
    /// Five segments in lifecycle order: `[Analyze, Plan, Execute, Qa, Close]`.
    /// The state of each one is the projection's verdict for that phase.
    pub segments: Vec<PhaseSegment>,
}

/// Kind of workspace-wide alert surfaced in the Visão Geral.
///
/// `Ord`/`PartialOrd` are derived because the workspace projection groups
/// alerts in a `BTreeMap` keyed by `(spec, kind)` to deduplicate; the variant
/// order is implementation-detail (not user-visible).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceAlertKind {
    /// A pipeline is blocked (`pipeline.pause` event present, no resume yet).
    Blocked,
    /// A spec's most recent `qa.result.payload.overall` is `"fail"`.
    QaFail,
    /// A wave failed (`pipeline.wave.failed`).
    WaveFailed,
    /// A review returned `rejected` (`review.result.payload.verdict == "rejected"`).
    ReviewRejected,
    /// `verify-pipeline` returned non-zero exit recently.
    BuildBroken,
}

/// One row in the alerts column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAlert {
    /// Spec the alert is about.
    pub spec: String,
    /// Kind of alert.
    pub kind: WorkspaceAlertKind,
    /// Human-readable single-line message.
    pub message: String,
    /// ISO-8601 of the event that triggered the alert.
    pub ts: String,
}

/// The top-level Visão Geral payload.
///
/// `events_per_minute` is `f64` (continuous rate, averaged over the live
/// window), so the struct cannot derive `Eq` — `PartialEq` is enough for the
/// few equality checks that exist (tests).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    /// Events per minute, computed over the last 60s window of the event log.
    /// Float because the value is averaged over short windows.
    pub events_per_minute: f64,
    /// Number of specs whose status is `is_active()`.
    pub specs_active_count: u32,
    /// Tokens saved today across RTK, hooks, model routing, and prompt
    /// economy. **`None` when none of these data sources are populated** —
    /// distinguishes "no data" from "zero savings". The UI renders "—" in the
    /// first case and "0" in the second.
    pub tokens_saved_today: Option<i64>,
    /// One row per active or recently-closed spec, ordered by `last_event_at`
    /// descending.
    pub spec_tracks: Vec<SpecTrack>,
    /// Alerts that need user attention, ordered by `ts` descending.
    pub alerts: Vec<WorkspaceAlert>,
    /// Top files touched today, ordered by `count` descending. Capped at 10.
    pub top_files_today: Vec<FileCount>,
}

impl WorkspaceSummary {
    /// The empty workspace — what gets returned when the event store has no
    /// data at all. Distinct from `Err`: the workspace exists, it's just idle.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            events_per_minute: 0.0,
            specs_active_count: 0,
            tokens_saved_today: None,
            spec_tracks: Vec::new(),
            alerts: Vec::new(),
            top_files_today: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_summary_is_zeros_and_none() {
        let s = WorkspaceSummary::empty();
        assert!((s.events_per_minute - 0.0).abs() < f64::EPSILON);
        assert_eq!(s.specs_active_count, 0);
        assert!(s.tokens_saved_today.is_none());
        assert!(s.spec_tracks.is_empty());
        assert!(s.alerts.is_empty());
        assert!(s.top_files_today.is_empty());
    }

    #[test]
    fn segment_states_are_three_explicit_variants() {
        // Ensure no Default is implemented — we want explicit construction.
        let completed = PhaseSegment { phase: Phase::Plan, state: SegmentState::Completed };
        let active = PhaseSegment { phase: Phase::Execute, state: SegmentState::Active };
        let future = PhaseSegment { phase: Phase::Close, state: SegmentState::Future };
        assert_eq!(completed.state, SegmentState::Completed);
        assert_eq!(active.state, SegmentState::Active);
        assert_eq!(future.state, SegmentState::Future);
    }
}
