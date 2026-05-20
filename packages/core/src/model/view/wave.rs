//! [`WaveView`] — per-wave state inside a pipeline run.
//!
//! Waves are how Mustard parallelises EXECUTE: each one is a unit of work
//! dispatched to one or more agents. The status enum is the deliberate
//! 4-state model the dashboard wants — `Queued` (declared, not started),
//! `InProgress` (dispatched, no completion event yet), `Completed` (matching
//! `pipeline.wave.complete`), `Failed` (matching `pipeline.wave.failed` or a
//! fix-loop cap). No `Unknown` variant; an absent wave is just absent from
//! the returned `Vec<WaveView>`.

use serde::{Deserialize, Serialize};

/// Lifecycle status of a single wave inside a pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WaveStatus {
    /// Declared in the wave plan, no dispatch event yet.
    Queued,
    /// Dispatched (`pipeline.task.dispatch`), no matching completion yet.
    InProgress,
    /// Matching `pipeline.wave.complete` event present.
    Completed,
    /// Matching `pipeline.wave.failed` event or fix-loop cap hit.
    Failed,
}

impl WaveStatus {
    /// Whether the wave is in a state UIs render as "active" (animated dot,
    /// brand-yellow glow). `Queued` is also active in the sense that it's
    /// part of the current plan, but `InProgress` is the dispatch state.
    #[must_use]
    pub const fn is_running(self) -> bool {
        matches!(self, Self::InProgress)
    }
}

/// One wave row — what `SpecDrillDown > Ondas` lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveView {
    /// Wave number (1-based, matches the spec's `### Wave N` headings).
    pub wave: u32,
    /// Role tag from the wave plan (`api`, `ui`, `database`, …). None when
    /// the wave plan didn't declare one.
    pub role: Option<String>,
    /// Lifecycle status.
    pub status: WaveStatus,
    /// ISO-8601 of the first `pipeline.task.dispatch` event for this wave.
    pub started_at: Option<String>,
    /// ISO-8601 of `pipeline.wave.complete` (or `pipeline.wave.failed`).
    pub completed_at: Option<String>,
    /// `subagent_type` used by the dispatched agent.
    pub agent_type: Option<String>,
    /// Files modified by `pipeline.task.complete` events in this wave,
    /// deduplicated. Sorted for stable diffs.
    pub files_changed: Vec<String>,
    /// `completed_at - started_at` in milliseconds. None until both are set.
    pub duration_ms: Option<i64>,
}

impl WaveView {
    /// Construct a fresh wave row at `Queued` with empty collections.
    /// Folds populate the rest as events arrive.
    #[must_use]
    pub fn queued(wave: u32) -> Self {
        Self {
            wave,
            role: None,
            status: WaveStatus::Queued,
            started_at: None,
            completed_at: None,
            agent_type: None,
            files_changed: Vec::new(),
            duration_ms: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_default_has_no_timestamps() {
        let wv = WaveView::queued(2);
        assert_eq!(wv.wave, 2);
        assert_eq!(wv.status, WaveStatus::Queued);
        assert!(wv.started_at.is_none());
        assert!(wv.files_changed.is_empty());
    }

    #[test]
    fn is_running_only_matches_in_progress() {
        assert!(WaveStatus::InProgress.is_running());
        assert!(!WaveStatus::Queued.is_running());
        assert!(!WaveStatus::Completed.is_running());
        assert!(!WaveStatus::Failed.is_running());
    }
}
