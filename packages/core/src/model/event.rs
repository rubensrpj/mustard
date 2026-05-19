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
