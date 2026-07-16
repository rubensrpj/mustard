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
/// Records that a wave began executing (its first subagent started). The
/// counterpart to [`EVENT_PIPELINE_WAVE_COMPLETE`]: it lets a projection mark a
/// wave `InProgress` from an explicit signal rather than inferring it from a
/// `pipeline.task.dispatch`. Carries the same `{wave: <n>}` correlation as the
/// completion event.
pub const EVENT_PIPELINE_WAVE_START: &str = "pipeline.wave.start";
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
/// Records the KIND of work a run was routed to (`feature` / `bugfix` /
/// `task` / `tactical-fix`) together with its scope. Emitted as a deterministic
/// side-effect even on the lean paths (`task`, `bugfix` fast-path) that skip the
/// full pipeline ceremony, so the dashboard can separate work by type and keep
/// the narrative of what was requested.
pub const EVENT_PIPELINE_KIND: &str = "pipeline.kind";
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
// Checklist event name constants
// ---------------------------------------------------------------------------

/// Records that one trackable checklist item was marked done (`- [ ]` →
/// `- [x]`). Follows the `qa.result` pattern: the name constant + typed
/// payload ([`ChecklistItemMarkedPayload`]) live here; emission is wired by
/// the rt-side auto-mark hook / `mark-checklist-item` (Wave 2).
pub const EVENT_CHECKLIST_ITEM_MARKED: &str = "checklist.item.marked";

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

impl PipelineCompletePayload {
    /// Deserialize from a raw [`Value`], tolerating a `null` / absent payload.
    ///
    /// A bare `pipeline.complete` emitted without a `--payload` lands as
    /// [`Value::Null`], which serde rejects when targeting a struct (`invalid
    /// type: null, expected struct`). A null / absent payload is semantically
    /// "an empty completion", so it maps to the all-default payload rather than
    /// an error. Any other JSON value goes through the normal lenient
    /// deserialiser (every field is `#[serde(default)]`).
    ///
    /// # Errors
    ///
    /// Returns the [`serde_json::Error`] only for a *malformed* non-null payload
    /// (e.g. a wrong-typed field), never for a null/absent one.
    pub fn from_value_lenient(value: Value) -> Result<Self, serde_json::Error> {
        if value.is_null() {
            return Ok(Self::default());
        }
        serde_json::from_value(value)
    }
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

/// Payload for [`EVENT_CHECKLIST_ITEM_MARKED`].
///
/// Correlates one marked checklist item back to its spec + wave so a
/// projection can fold per-wave progress (N done / M total) without re-reading
/// `meta.json`. `spec` and `wave` repeat the envelope's correlation fields on
/// purpose — payloads stay self-contained for consumers that index by payload
/// only (mirroring `qa.result`, whose payload also carries the verdict in
/// full).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChecklistItemMarkedPayload {
    /// Spec slug that owns the checklist.
    pub spec: String,
    /// Wave number the item belongs to (`0` outside a wave plan).
    #[serde(default)]
    pub wave: u32,
    /// Checklist item label, exactly as it appears in the spec's checklist.
    pub item: String,
    /// Auto-mark anchor path (the ` → <path>` target), when the item has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
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

    /// A bare `pipeline.complete` carries a `null` payload (no `--payload`
    /// supplied). [`PipelineCompletePayload::from_value_lenient`] must treat it
    /// as the all-default completion rather than the serde error
    /// `invalid type: null, expected struct PipelineCompletePayload`.
    #[test]
    fn pipeline_complete_payload_lenient_accepts_null() {
        let p = PipelineCompletePayload::from_value_lenient(Value::Null)
            .expect("null payload maps to default");
        assert!(p.closed_at.is_none());
        assert!(p.affected_files.is_empty());
    }

    /// The lenient deserialiser still reads a populated object payload.
    #[test]
    fn pipeline_complete_payload_lenient_reads_object() {
        let v = serde_json::json!({
            "closedAt": "2026-06-01T00:00:00Z",
            "affectedFiles": ["a.rs", "b.rs"],
        });
        let p = PipelineCompletePayload::from_value_lenient(v).expect("object payload parses");
        assert_eq!(p.closed_at.as_deref(), Some("2026-06-01T00:00:00Z"));
        assert_eq!(p.affected_files, vec!["a.rs".to_string(), "b.rs".to_string()]);
    }

    /// A genuinely malformed (wrong-typed) payload still surfaces an error —
    /// leniency covers null/absent only, not type mismatches.
    #[test]
    fn pipeline_complete_payload_lenient_rejects_wrong_type() {
        let v = serde_json::json!({ "affectedFiles": "not-an-array" });
        assert!(PipelineCompletePayload::from_value_lenient(v).is_err());
    }

    /// A `checklist.item.marked` payload round-trips through the typed lens,
    /// and the optional `path` is elided when absent.
    #[test]
    fn checklist_item_marked_payload_round_trips() {
        let p = ChecklistItemMarkedPayload {
            spec: "checklist-progresso-por-onda".into(),
            wave: 2,
            item: "wire the handler".into(),
            path: Some("src/handler.rs".into()),
        };
        let text = serde_json::to_string(&p).expect("serializes");
        let back: ChecklistItemMarkedPayload = serde_json::from_str(&text).expect("parses");
        assert_eq!(back.spec, "checklist-progresso-por-onda");
        assert_eq!(back.wave, 2);
        assert_eq!(back.item, "wire the handler");
        assert_eq!(back.path.as_deref(), Some("src/handler.rs"));
        // No-path form: the key is elided, and a payload missing `wave` /
        // `path` still deserialises (defaults: wave 0, no anchor).
        let plain = ChecklistItemMarkedPayload { path: None, ..p };
        assert!(!serde_json::to_string(&plain).expect("serializes").contains("\"path\""));
        let sparse: ChecklistItemMarkedPayload =
            serde_json::from_str(r#"{"spec":"s","item":"t"}"#).expect("parses");
        assert_eq!(sparse.wave, 0);
        assert!(sparse.path.is_none());
    }

    /// The full event envelope carries the new name + payload like any other
    /// harness event — `event` stays a free-form string, so no schema bump.
    #[test]
    fn checklist_item_marked_event_parses_in_envelope() {
        let raw = r#"{"v":1,"ts":"2026-06-10T12:00:00.000Z","sessionId":"s-1","wave":2,"actor":{"kind":"hook","id":"checklist-auto-mark"},"event":"checklist.item.marked","payload":{"spec":"demo","wave":2,"item":"T1","path":"src/lib.rs"},"spec":"demo"}"#;
        let ev: HarnessEvent = serde_json::from_str(raw).expect("parse checklist.item.marked");
        assert_eq!(ev.event, EVENT_CHECKLIST_ITEM_MARKED);
        let p: ChecklistItemMarkedPayload =
            serde_json::from_value(ev.payload).expect("typed payload");
        assert_eq!(p.item, "T1");
        assert_eq!(p.wave, 2);
    }
}
