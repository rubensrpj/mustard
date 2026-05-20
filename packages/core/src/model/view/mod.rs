//! Typed ViewModels — the surface other crates render against.
//!
//! Each sub-module owns one cohesive shape so the project naturally honours
//! the Single Responsibility Principle: a change to "how we surface
//! acceptance criteria" touches `quality.rs` alone. Cross-cutting enums
//! (`Phase`, `Scope`) live here in `mod.rs` since multiple views
//! reference them.

mod filter;
mod quality;
mod spec;
mod timeline;
mod wave;
mod workspace;

pub use filter::{SpecFilter, SpecStatusFilter, TimeWindow};
pub use quality::{AcStatus, AcceptanceCriterion, QualityRollup};
pub use spec::{SpecStatus, SpecSummary, SpecView};
pub use timeline::{TimelineKind, TimelineNode};
pub use wave::{WaveStatus, WaveView};
pub use workspace::{
    FileCount, PhaseSegment, SegmentState, SpecTrack, WorkspaceAlert, WorkspaceAlertKind,
    WorkspaceSummary,
};

use serde::{Deserialize, Serialize};

/// The five canonical phases of a Mustard pipeline.
///
/// Ordered to match the lifecycle: ANALYZE → PLAN → EXECUTE → QA → CLOSE.
/// Serialized as lowercase strings to round-trip with the on-disk spec header
/// (`### Phase: analyze`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// ANALYZE — exploration before planning.
    Analyze,
    /// PLAN — drafting the spec and tasks.
    Plan,
    /// EXECUTE — running implementation waves.
    Execute,
    /// QA — running Acceptance Criteria.
    Qa,
    /// CLOSE — archival, registry sync, banner.
    Close,
}

impl Phase {
    /// All five phases in canonical order.
    #[must_use]
    pub const fn all() -> [Self; 5] {
        [Self::Analyze, Self::Plan, Self::Execute, Self::Qa, Self::Close]
    }

    /// Parse a free-form phase string — accepts upper/lower case and the
    /// punctuated form `pipeline.phase=PLAN`. Returns `None` for unknown
    /// values; callers fail open.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "analyze" => Some(Self::Analyze),
            "plan" => Some(Self::Plan),
            "execute" => Some(Self::Execute),
            "qa" => Some(Self::Qa),
            "close" => Some(Self::Close),
            _ => None,
        }
    }

    /// Position in the lifecycle (0..5). Useful for computing which segments
    /// of a timeline UI are completed vs future.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Analyze => 0,
            Self::Plan => 1,
            Self::Execute => 2,
            Self::Qa => 3,
            Self::Close => 4,
        }
    }
}

/// Scope of a pipeline run.
///
/// Drives whether the PLAN phase runs (skipped under `Light` and `Touch`) and
/// how strict the size gates are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Full scope — every phase runs, all gates strict.
    Full,
    /// Light scope — PLAN is skipped, gates relaxed.
    Light,
    /// Touch scope — single-file fix, no wave structure.
    Touch,
}

impl Scope {
    /// Parse a free-form scope string. Returns `None` for unknown values.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "light" => Some(Self::Light),
            "touch" => Some(Self::Touch),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_parse_round_trips_canonical_case() {
        for p in Phase::all() {
            let s = serde_json::to_string(&p).unwrap();
            // serde renames produce JSON strings, so we trim the quotes.
            let trimmed = s.trim_matches('"');
            assert_eq!(Phase::parse(trimmed), Some(p));
        }
    }

    #[test]
    fn phase_parse_accepts_upper_case_and_whitespace() {
        assert_eq!(Phase::parse("  PLAN  "), Some(Phase::Plan));
        assert_eq!(Phase::parse("Execute"), Some(Phase::Execute));
    }

    #[test]
    fn phase_parse_returns_none_for_garbage() {
        assert_eq!(Phase::parse("unknown"), None);
        assert_eq!(Phase::parse(""), None);
    }

    #[test]
    fn phase_index_is_lifecycle_order() {
        assert_eq!(Phase::Analyze.index(), 0);
        assert_eq!(Phase::Close.index(), 4);
        let indices: Vec<_> = Phase::all().iter().map(|p| p.index()).collect();
        assert_eq!(indices, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn scope_parse_lowercases() {
        assert_eq!(Scope::parse("FULL"), Some(Scope::Full));
        assert_eq!(Scope::parse("light"), Some(Scope::Light));
        assert_eq!(Scope::parse("Touch"), Some(Scope::Touch));
        assert_eq!(Scope::parse("medium"), None);
    }
}
