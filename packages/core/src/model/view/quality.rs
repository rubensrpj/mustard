//! [`QualityRollup`] — per-spec acceptance criteria roll-up.
//!
//! Backs `SpecDrillDown > Qualidade` in the dashboard. Built from
//! `qa.result` events: each event carries a `criteria` array, the projection
//! folds the latest one per AC id.

use serde::{Deserialize, Serialize};

/// Per-AC status. `Pending` is the "no qa.result event yet" variant — never
/// a string `"unknown"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AcStatus {
    /// AC passed in the latest `qa.result`.
    Pass,
    /// AC failed (non-zero exit, or explicit `status: "fail"`).
    Fail,
    /// AC was skipped (e.g. the section was missing or marked skip in spec).
    Skip,
    /// No `qa.result` event has recorded this AC yet.
    Pending,
}

impl AcStatus {
    /// Parse the canonical strings used in `qa.result.payload.criteria[].status`.
    /// Accepts `"pass"`, `"fail"`, `"skip"`, `"pending"`, `"error"` (treated as fail).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pass" | "ok" | "success" => Some(Self::Pass),
            "fail" | "failed" | "error" => Some(Self::Fail),
            "skip" | "skipped" => Some(Self::Skip),
            "pending" | "queued" => Some(Self::Pending),
            _ => None,
        }
    }
}

/// One row in `SpecDrillDown > Qualidade`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    /// Identifier from the spec (`AC-1`, `AC-2`, …).
    pub id: String,
    /// Human-readable label (the line after `AC-N:` in the spec).
    pub label: String,
    /// Latest known status.
    pub status: AcStatus,
    /// Wave number the AC belongs to, when the projection can determine it.
    pub wave: Option<u32>,
    /// The shell command the AC runs. None when the spec did not declare one.
    pub command: Option<String>,
    /// ISO-8601 of the most recent `qa.result` event that touched this AC.
    pub last_run_at: Option<String>,
    /// Optional excerpt of the failing stderr, capped at ~200 chars by the
    /// projection. Used by the failure detail view; never displayed inline.
    pub fail_reason: Option<String>,
}

/// Aggregate roll-up: the counts + the criteria list. Built so a UI can show
/// a hero number ("3 / 5 passing") without iterating the full criteria array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityRollup {
    /// Number of criteria with status `Pass`.
    pub passed: u32,
    /// Total number of criteria in the spec.
    pub total: u32,
    /// Number with status `Fail`.
    pub failed: u32,
    /// Number with status `Skip`.
    pub skipped: u32,
    /// Number with status `Pending`.
    pub pending: u32,
    /// ISO-8601 of the most recent `qa.result` event across all criteria.
    pub last_run_at: Option<String>,
    /// Per-AC rows, ordered by spec id (`AC-1`, `AC-2`, …).
    pub criteria: Vec<AcceptanceCriterion>,
}

impl QualityRollup {
    /// Empty roll-up — what gets returned for a spec with no `qa.result`
    /// events. Distinct from `None`: the consumer knows the spec exists, just
    /// hasn't run QA yet.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            passed: 0,
            total: 0,
            failed: 0,
            skipped: 0,
            pending: 0,
            last_run_at: None,
            criteria: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac_status_parse_accepts_synonyms() {
        assert_eq!(AcStatus::parse("PASS"), Some(AcStatus::Pass));
        assert_eq!(AcStatus::parse("ok"), Some(AcStatus::Pass));
        assert_eq!(AcStatus::parse("error"), Some(AcStatus::Fail));
        assert_eq!(AcStatus::parse("skipped"), Some(AcStatus::Skip));
        assert_eq!(AcStatus::parse("queued"), Some(AcStatus::Pending));
        assert_eq!(AcStatus::parse("garbage"), None);
    }

    #[test]
    fn empty_rollup_has_zero_counters_and_no_timestamp() {
        let rollup = QualityRollup::empty();
        assert_eq!(rollup.total, 0);
        assert_eq!(rollup.passed, 0);
        assert!(rollup.criteria.is_empty());
        assert!(rollup.last_run_at.is_none());
    }
}
