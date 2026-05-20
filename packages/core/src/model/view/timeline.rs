//! [`TimelineNode`] — one entry in a per-spec timeline.
//!
//! Backs `SpecDrillDown > Timeline`. Classified by `kind` so the UI can pick
//! an icon / colour without reparsing the raw event name.

use super::Phase;
use serde::{Deserialize, Serialize};

/// Coarse classification of an event for timeline rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimelineKind {
    /// `pipeline.scope` — spec was bootstrapped.
    Scope,
    /// `pipeline.phase` — phase transition.
    Phase,
    /// `pipeline.status` — status transition.
    Status,
    /// `pipeline.task.dispatch` / `pipeline.task.complete`.
    Task,
    /// `pipeline.wave.complete` / `pipeline.wave.failed`.
    Wave,
    /// `qa.result` — QA gate result.
    Qa,
    /// `review.result` — review gate result.
    Review,
    /// `agent.start` / `agent.stop`.
    Agent,
    /// `tool.use`.
    Tool,
    /// `decision` / `lesson` — knowledge writes.
    Decision,
    /// Anything we don't classify above. The raw event string still survives
    /// in `payload_summary` so the UI can render it verbatim.
    Other,
}

impl TimelineKind {
    /// Classify a raw event name into a coarse kind.
    #[must_use]
    pub fn classify(event: &str) -> Self {
        match event {
            "pipeline.scope" => Self::Scope,
            "pipeline.phase" => Self::Phase,
            "pipeline.status" | "pipeline.complete" => Self::Status,
            "pipeline.task.dispatch" | "pipeline.task.complete" => Self::Task,
            "pipeline.wave.complete" | "pipeline.wave.failed" => Self::Wave,
            "qa.result" => Self::Qa,
            "review.result" | "review.start" | "review.complete" => Self::Review,
            "agent.start" | "agent.stop" => Self::Agent,
            "tool.use" => Self::Tool,
            "decision" | "lesson" => Self::Decision,
            _ => Self::Other,
        }
    }
}

/// One timeline row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineNode {
    /// ISO-8601 of the underlying event.
    pub ts: String,
    /// Coarse classification.
    pub kind: TimelineKind,
    /// Short human-readable label (`"EXECUTE phase started"`,
    /// `"Wave 2 completed"`, …). Built by the projection from the event +
    /// payload.
    pub label: String,
    /// The phase tag carried by the event, when applicable (`pipeline.phase`
    /// events surface their target phase here).
    pub phase: Option<Phase>,
    /// The wave number carried by the event, when applicable.
    pub wave: Option<u32>,
    /// Single-line summary of the payload — typically the most important
    /// field rendered inline ("agent: dashboard-impl", "files: 3", …).
    /// Capped at ~120 chars by the projection.
    pub payload_summary: String,
    /// Original event kind, kept verbatim for filtering and search.
    pub raw_event: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_maps_known_events() {
        assert_eq!(TimelineKind::classify("pipeline.scope"), TimelineKind::Scope);
        assert_eq!(TimelineKind::classify("pipeline.phase"), TimelineKind::Phase);
        assert_eq!(TimelineKind::classify("pipeline.wave.complete"), TimelineKind::Wave);
        assert_eq!(TimelineKind::classify("qa.result"), TimelineKind::Qa);
        assert_eq!(TimelineKind::classify("agent.start"), TimelineKind::Agent);
        assert_eq!(TimelineKind::classify("tool.use"), TimelineKind::Tool);
        assert_eq!(TimelineKind::classify("decision"), TimelineKind::Decision);
    }

    #[test]
    fn classify_unknown_falls_back_to_other() {
        assert_eq!(TimelineKind::classify("spec.link"), TimelineKind::Other);
        assert_eq!(TimelineKind::classify(""), TimelineKind::Other);
    }
}
