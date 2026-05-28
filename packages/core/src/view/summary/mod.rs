//! Summary writer — the versionable `.summary.json` artefact.
//!
//! Every spec that goes through `/mustard:close` produces a
//! `{spec-dir}/.summary.json` file that is committed to git alongside the
//! spec itself. It captures the information that matters to *other* users
//! who clone the repo later: timeline, AC results, decisions, token economy,
//! and per-wave status — without requiring the raw NDJSON event streams that
//! live locally (`.events/*.ndjson`, not versioned).
//!
//! Schema design principles:
//! - `version: 1` (integer) is the first field; readers can branch on it.
//! - All timestamps are ISO-8601 UTC strings (same format as `meta.json`).
//! - Optional fields are `Option<…>` — callers set what they have.
//! - The struct derives `serde::{Serialize, Deserialize}` so tests can
//!   round-trip through JSON with a single call.
//!
//! ## Usage
//!
//! ```
//! use mustard_core::view::summary::{SpecSummaryDoc, writer};
//!
//! let doc = SpecSummaryDoc {
//!     version: 1,
//!     spec: "2026-05-26-no-sqlite".into(),
//!     title: "No SQLite".into(),
//!     ..Default::default()
//! };
//! // write to a tmpdir in tests:
//! # use std::path::Path;
//! # let spec_dir = std::env::temp_dir();
//! writer::write(&spec_dir, &doc).unwrap();
//! ```

pub mod writer;

use serde::{Deserialize, Serialize};

/// The root document persisted as `.summary.json` inside the spec directory.
///
/// `version: 1` — bump when the schema changes in a breaking way.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpecSummaryDoc {
    /// Schema version — always `1` for this revision.
    pub version: u32,

    /// Spec slug (directory name under `.claude/spec/`).
    pub spec: String,

    /// Human-readable title extracted from the spec's first `# Heading`.
    pub title: String,

    /// BCP-47 language code (`pt-BR` or `en-US`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,

    /// `didactic` or `technical`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,

    /// `light`, `medium`, or `full`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Last known lifecycle stage (`Plan`, `Execute`, `Review`, `QA`, `Close`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,

    /// Outcome at close time (`Completed`, `Cancelled`, `Superseded`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,

    /// Key timestamps — all optional since a summary may be written mid-flight.
    #[serde(default)]
    pub timeline: SummaryTimeline,

    /// Per-wave status snapshots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waves: Vec<WaveSummary>,

    /// Top-level acceptance criteria results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance_criteria: Vec<AcResult>,

    /// Non-obvious decisions recorded during the spec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Decision>,

    /// Aggregated token economy metrics.
    #[serde(default)]
    pub economy: EconomySummaryEntry,

    /// Aggregated telemetry (token + cost).
    #[serde(default)]
    pub telemetry: TelemetrySummaryEntry,

    /// Paths of every file touched during the spec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_affected: Vec<String>,
}

/// Key timestamps for the spec lifecycle.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryTimeline {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execute_started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qa_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
}

/// Status snapshot for a single wave.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WaveSummary {
    /// Wave number (1-based).
    pub n: u32,

    /// Wave role label (e.g. `core`, `rt`, `dashboard`, `mixed`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,

    /// One-sentence human summary of what the wave did.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,

    /// `completed`, `cancelled`, `skipped`, or `in_progress`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,

    /// Per-wave AC results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ac_results: Vec<AcResult>,

    /// Review decision (`approved`, `changes_requested`, `skipped`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,

    /// QA verdict (`pass`, `fail`, `skipped`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qa: Option<String>,

    /// Open concerns or caveats noted during this wave.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concerns: Vec<String>,
}

/// One acceptance criterion result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AcResult {
    /// AC identifier (e.g. `AC-1`, `AC-W3.2`).
    pub id: String,
    /// Whether the AC passed.
    pub pass: bool,
    /// The command that was run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Short note about the result (failure message, caveat, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A non-obvious design decision captured during the spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// ISO-8601 timestamp when the decision was made.
    pub at: String,
    /// One-line summary.
    pub summary: String,
    /// Optional link to an atomic markdown file in `.claude/knowledge/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_link: Option<String>,
}

/// Aggregated token savings from the economy layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EconomySummaryEntry {
    #[serde(default)]
    pub total_savings_tokens: i64,
    #[serde(default)]
    pub by_source: std::collections::HashMap<String, i64>,
}

/// Aggregated telemetry counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySummaryEntry {
    #[serde(default)]
    pub total_tokens: i64,
    #[serde(default)]
    pub total_cost_usd_micros: i64,
    #[serde(default)]
    pub by_model: std::collections::HashMap<String, i64>,
    #[serde(default)]
    pub by_agent: std::collections::HashMap<String, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_summary_doc_round_trips_json_with_version_field() {
        let doc = SpecSummaryDoc {
            version: 1,
            spec: "2026-05-26-no-sqlite-git-source-of-truth".into(),
            title: "No SQLite — Git como fonte de verdade".into(),
            lang: Some("pt-BR".into()),
            stage: Some("Close".into()),
            outcome: Some("Completed".into()),
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&doc).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // AC-W1.3: version field must be numeric
        assert_eq!(parsed["version"].as_u64(), Some(1));
        assert_eq!(parsed["spec"].as_str(), Some("2026-05-26-no-sqlite-git-source-of-truth"));

        // Round-trip back to struct
        let reconstructed: SpecSummaryDoc = serde_json::from_str(&json).unwrap();
        assert_eq!(reconstructed, doc);
    }

    #[test]
    fn empty_default_is_valid_json() {
        let doc = SpecSummaryDoc::default();
        let json = serde_json::to_string(&doc).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["version"].as_u64(), Some(0)); // default u32
    }
}
