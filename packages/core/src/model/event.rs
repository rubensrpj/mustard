//! Harness event schema — the shape of one row in the harness event store
//! (`.claude/.harness/mustard.db`).
//!
//! The harness is an append-only event bus shared by every hook. Each event
//! is a record with a fixed *envelope* (schema version, timestamp, session,
//! wave, actor, event name) plus a free-form `payload` whose shape depends on
//! the event name.
//!
//! The `event` field is an arbitrary string and `payload` an arbitrary object,
//! so [`HarnessEvent`] keeps `event` a `String` and `payload` a
//! [`serde_json::Value`]: new event kinds from the harness must never break
//! deserialization.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Schema version currently emitted by the harness (`v` field).
///
/// Mirrors `SCHEMA_VERSION` in `_lib/harness-event.js`.
pub const SCHEMA_VERSION: u32 = 1;

/// The kind of actor that emitted an event.
///
/// In the JSON this is the `actor.kind` field. The harness emits `"hook"`
/// today; `"agent"` and `"orchestrator"` are reserved for the Rust port.
/// `#[non_exhaustive]` so adding a kind does not break consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ActorKind {
    /// Emitted from inside a lifecycle hook (the common case today).
    Hook,
    /// Emitted on behalf of a delegated agent.
    Agent,
    /// Emitted by the parent orchestrator context.
    Orchestrator,
    /// Emitted by the CLI or a standalone script.
    Cli,
}

/// Identifies who emitted an event (`actor` object in the JSON).
///
/// `kind` is always present; `id` and `actor_type` are optional and only
/// populated when the emitter supplies them (see `normalizeActor` in
/// `_lib/harness-event.js`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    /// The category of actor.
    pub kind: ActorKind,
    /// Optional emitter id, e.g. the hook name `"metrics-tracker"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Optional finer-grained actor type. Maps to the JSON `actor.type` field.
    #[serde(
        default,
        rename = "type",
        skip_serializing_if = "Option::is_none"
    )]
    pub actor_type: Option<String>,
}

/// One row of the harness event store — a single harness event.
///
/// The envelope fields (`v`, `ts`, `session_id`, `wave`, `actor`, `event`)
/// are stable; `payload` is event-specific and intentionally untyped so the
/// model never has to enumerate every event kind. `spec` is only written when
/// the harness can resolve an active spec.
///
/// Real event names observed in the wild: `session.start`, `tool.use`,
/// `decision`, `retry.attempt`, `pipeline.phase`. The emitter also documents
/// `agent.start`, `agent.stop`, `finding`, `lesson`, and `dispatch.failure`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessEvent {
    /// Schema version (`v`). See [`SCHEMA_VERSION`].
    pub v: u32,
    /// RFC-3339 / ISO-8601 timestamp string, as produced by JS `toISOString()`.
    pub ts: String,
    /// Session identifier grouping events from one Claude Code run.
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// Pipeline wave the event belongs to (`0` when outside a wave plan).
    #[serde(default)]
    pub wave: u32,
    /// Who emitted the event.
    pub actor: Actor,
    /// Event name, e.g. `"tool.use"`. Free-form: the harness may add new
    /// names without a schema bump.
    pub event: String,
    /// Event-specific data. Empty object when the emitter passes no payload.
    #[serde(default)]
    pub payload: Value,
    /// Active spec name, present only when the harness resolved one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
}

/// Alias kept for forward-compatibility with the spec's `model/event.rs`
/// listing, which names both `HookEvent` and `HarnessEvent`. The harness emits
/// a single unified envelope, so the two are the same type today; hook-only
/// consumers can refer to it as `HookEvent`.
pub type HookEvent = HarnessEvent;

// ---------------------------------------------------------------------------
// Pipeline event name constants
// ---------------------------------------------------------------------------

/// Records the scope and model selected for a pipeline run.
pub const EVENT_PIPELINE_SCOPE: &str = "pipeline.scope";
/// Records a lifecycle status transition (e.g. `active` → `closed`).
pub const EVENT_PIPELINE_STATUS: &str = "pipeline.status";
/// Records that an agent task was dispatched for a wave.
pub const EVENT_PIPELINE_TASK_DISPATCH: &str = "pipeline.task.dispatch";
/// Records that a dispatched agent task completed.
pub const EVENT_PIPELINE_TASK_COMPLETE: &str = "pipeline.task.complete";
/// Records that an entire wave finished.
pub const EVENT_PIPELINE_WAVE_COMPLETE: &str = "pipeline.wave.complete";
/// Records a task-dispatch failure (the agent could not be started).
pub const EVENT_PIPELINE_DISPATCH_FAILURE: &str = "pipeline.dispatch_failure";
/// Records a voluntary pipeline pause.
pub const EVENT_PIPELINE_PAUSE: &str = "pipeline.pause";
/// Records how a paused pipeline is being resumed.
pub const EVENT_PIPELINE_RESUME_MODE: &str = "pipeline.resume_mode";
/// Records that a pipeline run was fully closed: captures `closedAt` and the
/// set of files that were affected during the run.
pub const EVENT_PIPELINE_COMPLETE: &str = "pipeline.complete";
/// Records that an amendment window was opened for a closed pipeline.
pub const EVENT_PIPELINE_AMEND_OPEN: &str = "pipeline.amend_open";
/// Records a tool activity inside an open amendment window.
pub const EVENT_PIPELINE_AMEND_ACTIVITY: &str = "pipeline.amend_activity";
/// Records a user prompt intent observed during an amendment window.
pub const EVENT_PIPELINE_AMEND_INTENT: &str = "pipeline.amend_intent";
/// Records drift detected (edits outside the original pipeline file set).
pub const EVENT_PIPELINE_AMEND_DRIFT: &str = "pipeline.amend_drift";
/// Records that an amendment window was closed.
pub const EVENT_PIPELINE_AMEND_CLOSE: &str = "pipeline.amend_close";

// ---------------------------------------------------------------------------
// Typed payload structs — typed views over `HarnessEvent::payload: Value`.
//
// Each struct mirrors one of the pipeline event constants above. All optional
// fields use `Option<T>` and carry `#[serde(default)]` so unknown-field
// absence never fails deserialization. Do NOT touch `HarnessEvent` itself —
// these are helpers for callers that want a typed lens, not schema changes.
// ---------------------------------------------------------------------------

/// Payload for [`EVENT_PIPELINE_SCOPE`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineScopePayload {
    /// Pipeline scope token, e.g. `"full"` or `"wave"`.
    pub scope: String,
    /// Spec language override (e.g. `"pt"` or `"en"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Model routed to for this pipeline run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `true` when the spec uses a wave plan.
    ///
    /// Accepts both snake_case (`is_wave_plan`) and camelCase (`isWavePlan`)
    /// JSON keys: events emitted by the harness use the snake form, while
    /// historic NDJSON and the resume bootstrap layer write camelCase.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "isWavePlan")]
    pub is_wave_plan: Option<bool>,
    /// Total wave count declared in the spec (when wave-plan).
    ///
    /// Accepts both `total_waves` and `totalWaves` for the same reason as
    /// [`Self::is_wave_plan`].
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "totalWaves")]
    pub total_waves: Option<u32>,
}

/// Payload for [`EVENT_PIPELINE_STATUS`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineStatusPayload {
    /// Previous status (absent on the first recorded transition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// New status value.
    pub to: String,
}

/// Payload for [`EVENT_PIPELINE_TASK_DISPATCH`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineTaskDispatchPayload {
    /// Wave number the task belongs to (`None` outside a wave plan).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave: Option<u32>,
    /// Human-readable task name (matches the wave-plan heading).
    pub name: String,
    /// Agent sub-type used for this dispatch (e.g. `"general-purpose"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Role label (e.g. `"implement"` or `"plan"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Files listed in the wave's scope at dispatch time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    /// Retry attempt number (`0` on first attempt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<u32>,
}

/// Payload for [`EVENT_PIPELINE_TASK_COMPLETE`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineTaskCompletePayload {
    /// Wave number the task belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave: Option<u32>,
    /// Human-readable task name.
    pub name: String,
    /// Agent sub-type that ran the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Wall-clock task duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Files the agent reported as modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_modified: Option<Vec<String>>,
    /// Non-obvious architectural decisions recorded by the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decisions: Option<Vec<String>>,
    /// Escalation message, present when the agent requested human review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation: Option<String>,
}

/// Payload for [`EVENT_PIPELINE_WAVE_COMPLETE`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineWaveCompletePayload {
    /// Wave number that finished.
    pub wave: u32,
    /// Wall-clock wave duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Payload for [`EVENT_PIPELINE_DISPATCH_FAILURE`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineDispatchFailurePayload {
    /// Agent sub-type that could not be started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// One-line description of the task that failed to dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The prompt that was passed to the agent (truncated if large).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Human-readable reason the dispatch was rejected or failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// ISO-8601 timestamp of the failure (when the caller provides one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// Payload for [`EVENT_PIPELINE_PAUSE`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelinePausePayload {
    /// Human-readable reason the pipeline was paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Suggested next step for the human operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
}

/// Payload for [`EVENT_PIPELINE_RESUME_MODE`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineResumeModePayload {
    /// Resume mode selected (e.g. `"continue"`, `"rewave"`, `"abort"`).
    pub mode: String,
    /// Escalation context passed to the next wave, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation: Option<String>,
}

/// Payload for [`EVENT_PIPELINE_COMPLETE`].
///
/// All fields are optional and default to empty/None so that events written
/// by an older emitter (or with a partial payload) still deserialize cleanly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineCompletePayload {
    /// ISO-8601 timestamp at which the pipeline was closed.
    #[serde(default, rename = "closedAt", skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    /// Files touched during the pipeline run (union of harness events + git diff).
    #[serde(default, rename = "affectedFiles")]
    pub affected_files: Vec<String>,
}

/// Payload for [`EVENT_PIPELINE_AMEND_OPEN`].
///
/// Emitted when a new amendment window is opened for a closed pipeline.
/// `pipeline_file_set` and `subprojects` seed the window's known scope.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineAmendOpenPayload {
    /// Spec identifier that owns this amendment window.
    pub spec_id: String,
    /// Session in which the window was opened.
    pub session_id: String,
    /// ISO-8601 timestamp at which the pipeline was originally closed.
    pub closed_at: String,
    /// File paths touched during the closed pipeline run (the allowed edit set).
    pub pipeline_file_set: Vec<String>,
    /// Subproject identifiers active during the closed pipeline run.
    pub subprojects: Vec<String>,
}

/// Payload for [`EVENT_PIPELINE_AMEND_ACTIVITY`].
///
/// Emitted on each tool use observed inside an open amendment window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineAmendActivityPayload {
    /// Spec identifier for the active amendment window.
    pub spec_id: String,
    /// Session identifier for the active amendment window.
    pub session_id: String,
    /// Tool name (e.g. `"Write"`, `"Edit"`, `"Bash"`).
    pub tool: String,
    /// File path the tool operated on (may be empty for non-file tools).
    pub file_path: String,
    /// ISO-8601 timestamp of the activity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// Payload for [`EVENT_PIPELINE_AMEND_INTENT`].
///
/// Emitted when a user prompt is observed during an amendment window; used
/// to classify whether the amendment is in-scope or introduces new work.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineAmendIntentPayload {
    /// Spec identifier for the active amendment window.
    pub spec_id: String,
    /// Session identifier for the active amendment window.
    pub session_id: String,
    /// The raw user prompt text (may be truncated).
    pub prompt_text: String,
    /// ISO-8601 timestamp of the prompt submission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// Payload for [`EVENT_PIPELINE_AMEND_DRIFT`].
///
/// Emitted when the amendment window detects edits to paths outside the
/// original `pipeline_file_set`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineAmendDriftPayload {
    /// Spec identifier for the active amendment window.
    pub spec_id: String,
    /// Session identifier for the active amendment window.
    pub session_id: String,
    /// Paths that were edited but are not in `pipeline_file_set`.
    pub unrelated_paths: Vec<String>,
    /// The drift threshold (number of unrelated paths) that triggered this event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<u32>,
}

/// Payload for [`EVENT_PIPELINE_AMEND_CLOSE`].
///
/// Emitted when an amendment window is closed, recording the final outcome.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineAmendClosePayload {
    /// Spec identifier for the amendment window being closed.
    pub spec_id: String,
    /// Session identifier for the amendment window being closed.
    pub session_id: String,
    /// Final status: `"completed"`, `"abandoned"`, or `"expired"`.
    pub status: String,
    /// ISO-8601 timestamp at which the window was closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    /// Whether the build was green at close time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_verde: Option<bool>,
    /// Whether at least one drift event was emitted during the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift_emitted: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real `tool.use` event in the harness wire format must round-trip
    /// through [`HarnessEvent`] without loss.
    #[test]
    fn deserializes_real_tool_use_event() {
        let raw = r#"{"v":1,"ts":"2026-05-19T00:14:26.591Z","sessionId":"eb4a7c56-54fa-4a3a-80d0-0c5818c46be7","wave":0,"actor":{"kind":"hook","id":"metrics-tracker"},"event":"tool.use","payload":{"tool":"Read","phase":"PLAN","target":{"file":"C:\\Atiz\\mustard\\package.json"}},"spec":"2026-05-18-b1-monorepo-merge"}"#;
        let ev: HarnessEvent = serde_json::from_str(raw).expect("parse tool.use");
        assert_eq!(ev.v, SCHEMA_VERSION);
        assert_eq!(ev.event, "tool.use");
        assert_eq!(ev.actor.kind, ActorKind::Hook);
        assert_eq!(ev.actor.id.as_deref(), Some("metrics-tracker"));
        assert_eq!(ev.payload["tool"], serde_json::json!("Read"));
        assert_eq!(ev.spec.as_deref(), Some("2026-05-18-b1-monorepo-merge"));
    }

    /// An event without a `spec` field (e.g. before a spec is resolved) must
    /// still parse, with `spec` defaulting to `None`.
    #[test]
    fn spec_field_is_optional() {
        let raw = r#"{"v":1,"ts":"2026-05-19T00:13:55.691Z","sessionId":"s-1","wave":0,"actor":{"kind":"hook"},"event":"session.start","payload":{}}"#;
        let ev: HarnessEvent = serde_json::from_str(raw).expect("parse session.start");
        assert!(ev.spec.is_none());
        assert!(ev.actor.id.is_none());
    }
}
