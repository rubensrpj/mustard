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
