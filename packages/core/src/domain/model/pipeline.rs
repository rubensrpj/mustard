//! `pipeline-state` schema — the shape of `.claude/.pipeline-states/*.json`.
//!
//! A pipeline-state file tracks one in-flight Mustard pipeline: which spec,
//! what phase, which wave, the task breakdown, and the model in use. Hooks
//! such as `model-routing-gate.js` and `close-gate.js` read it, and the
//! harness uses the newest file to tag events with the active spec.
//!
//! Derived from real files in `.claude/.pipeline-states/` (e.g.
//! `2026-05-18-b2-mustard-core-crate.json`) and the fields read by the JS
//! hooks. Like [`crate::domain::model::contract::HookInput`], [`PipelineState`] is
//! **lenient** — it keeps a `raw` catch-all so a new pipeline-state field
//! does not break deserialization.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A canonical pipeline phase.
///
/// Single source of truth: `.claude/refs/canonical-phases.md`. The sequence is
/// `ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE`, plus `COORDINATE` for
/// roadmap / multi-spec parents. `#[non_exhaustive]` because the phase
/// vocabulary is owned by that ref doc and may grow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
#[non_exhaustive]
pub enum Phase {
    /// Research the codebase: locate entities, map the change surface.
    Analyze,
    /// Write the spec: scope, waves, Acceptance Criteria (Full scope only).
    Plan,
    /// Implement the change across delegated agents.
    Execute,
    /// Inspect produced code before QA.
    Review,
    /// Run the spec's Acceptance Criteria commands and record pass/fail.
    Qa,
    /// Finalize: sync registry, move spec to done, commit.
    Close,
    /// Parent-level orchestration of a roadmap with multiple child specs.
    Coordinate,
}

/// The scope of a pipeline, which controls how many phases run.
///
/// Light scope skips `PLAN` (`ANALYZE → EXECUTE → REVIEW → QA → CLOSE`); Full
/// scope runs every phase. Specs in this repo use `full`; `light` / `medium`
/// are the other auto-detected scopes. `#[non_exhaustive]` for forward
/// compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Scope {
    /// 1-2 layers, small surface, known pattern — `PLAN` is skipped.
    Light,
    /// An intermediate scope between [`Scope::Light`] and [`Scope::Full`].
    Medium,
    /// 3+ layers or a new entity — every phase runs.
    Full,
}

/// One task in a pipeline's task breakdown (`tasks[]` array).
///
/// Each task names a wave, the agent role responsible, a free-form status,
/// and the ordered steps. `status` is kept a `String` rather than an enum:
/// pipeline-states use varied values (`"pending"`, `"in_progress"`,
/// `"done"`, …) and a strict enum would reject any new one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// Human-readable task / wave name.
    pub name: String,
    /// The agent role that owns this task, e.g. `"core"`, `"backend"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Free-form task status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Ordered steps that make up the task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
}

/// The full state of one in-flight pipeline.
///
/// Lenient: unmodelled fields land in [`PipelineState::raw`]. The JSON uses
/// camelCase keys (`specName`, `phaseName`, …); `#[serde(rename)]` maps them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineState {
    /// The spec this pipeline implements, e.g.
    /// `"2026-05-18-b2-mustard-core-crate"`.
    #[serde(default, rename = "specName", skip_serializing_if = "Option::is_none")]
    pub spec_name: Option<String>,

    /// Free-form pipeline status, e.g. `"implementing"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// The current canonical phase.
    #[serde(default, rename = "phaseName", skip_serializing_if = "Option::is_none")]
    pub phase: Option<Phase>,

    /// The pipeline scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,

    /// Model in use for this pipeline, e.g. `"opus"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Whether the pipeline is organised as a multi-wave plan.
    #[serde(default, rename = "isWavePlan")]
    pub is_wave_plan: bool,

    /// The wave currently in progress (`0` outside a wave plan).
    #[serde(default, rename = "currentWave")]
    pub current_wave: u32,

    /// Total number of planned waves.
    #[serde(default, rename = "totalWaves")]
    pub total_waves: u32,

    /// Waves already finished. Some pipeline-states record this as a count,
    /// so it stays an untyped [`Value`] rather than guessing a shape.
    #[serde(default, rename = "completedWaves", skip_serializing_if = "Value::is_null")]
    pub completed_waves: Value,

    /// The task / wave breakdown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<Task>,

    /// ISO-8601 timestamp of the last update.
    #[serde(default, rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,

    /// Every field of the original JSON, including unmodelled ones.
    #[serde(flatten)]
    pub raw: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real b2 pipeline-state file must round-trip into [`PipelineState`].
    #[test]
    fn deserializes_real_pipeline_state() {
        let raw = r#"{
            "specName": "2026-05-18-b2-mustard-core-crate",
            "status": "implementing",
            "phaseName": "EXECUTE",
            "currentWave": 1,
            "totalWaves": 4,
            "scope": "full",
            "model": "opus",
            "isWavePlan": false,
            "tasks": [
                {"name": "Wave 1 — model", "agent": "core", "status": "pending", "steps": ["a", "b"]}
            ],
            "updatedAt": "2026-05-19T01:17:00Z"
        }"#;
        let state: PipelineState = serde_json::from_str(raw).expect("parse pipeline-state");
        assert_eq!(state.phase, Some(Phase::Execute));
        assert_eq!(state.scope, Some(Scope::Full));
        assert_eq!(state.current_wave, 1);
        assert_eq!(state.total_waves, 4);
        assert_eq!(state.tasks.len(), 1);
        assert_eq!(state.tasks[0].agent.as_deref(), Some("core"));
    }

    /// An unmodelled field (`type`, read by `model-routing-gate.js`) must be
    /// reachable through `raw` without breaking the parse.
    #[test]
    fn pipeline_state_keeps_unknown_fields_in_raw() {
        let raw = r#"{"specName":"s","type":"feature","phaseName":"PLAN"}"#;
        let state: PipelineState = serde_json::from_str(raw).expect("lenient parse");
        assert_eq!(state.phase, Some(Phase::Plan));
        assert_eq!(state.raw["type"], serde_json::json!("feature"));
    }
}
