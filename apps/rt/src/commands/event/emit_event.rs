//! `mustard-rt run emit-event` — a generic harness-event emitter.
//!
//! `emit-phase` records one fixed event shape (`pipeline.phase`). This is its
//! general counterpart: it appends an arbitrary named event to the harness bus
//! with a caller-supplied payload, replacing the inline `node -e` snippets that
//! commands used to shell out to.
//!
//! The payload is built from repeated `--payload key=value` arguments. Each
//! value is parsed as JSON when it parses (so `--payload count=3` lands an
//! integer); otherwise it is kept as a string. Like every emitter here it is
//! fail-open — a write failure degrades to a no-op so telemetry never breaks a
//! command.

use crate::shared::context::{current_spec, project_dir, session_id};
use crate::util::now_iso8601;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{Map, Value};

/// Parse a `key=value` payload argument into a `(key, Value)` pair.
///
/// The value is interpreted as JSON when it parses cleanly (numbers, booleans,
/// `null`, quoted strings, arrays, objects); a bare unparseable token is kept
/// verbatim as a JSON string. An argument with no `=` is skipped.
fn parse_payload_pair(arg: &str) -> Option<(String, Value)> {
    let eq = arg.find('=')?;
    let key = arg[..eq].trim();
    if key.is_empty() {
        return None;
    }
    let raw = &arg[eq + 1..];
    let value = serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()));
    Some((key.to_string(), value))
}

/// Build the payload object from the repeated `--payload key=value` args.
fn build_payload(pairs: &[String]) -> Value {
    let mut map = Map::new();
    for pair in pairs {
        if let Some((key, value)) = parse_payload_pair(pair) {
            map.insert(key, value);
        }
    }
    Value::Object(map)
}

/// Run `mustard-rt run emit-event --event <name> [--payload k=v]... [--spec <s>]`.
///
/// Fail-open: a missing `--event` prints usage and returns; any append failure
/// is swallowed.
///
/// **Spec attribution:** when `--spec` is omitted the caller still gets the
/// best-available attribution via [`current_spec`] (env var → SQLite scope
/// lookup → filesystem hint). This closes the orphan-event hole the
/// 2026-05-20 audit found: pipeline commands that emitted ad-hoc events
/// without passing `--spec` used to write `events.spec = NULL`, which
/// projections then filtered out.
pub fn run(event: Option<&str>, payload: &[String], spec: Option<&str>, wave: u32) {
    let Some(event) = event.filter(|e| !e.is_empty()) else {
        eprintln!("Usage: emit-event --event <name> [--payload key=value]... [--spec <name>] [--wave <n>]");
        return;
    };

    let dir = project_dir();
    let resolved_spec = spec
        .map(str::to_string)
        .or_else(|| current_spec(&dir));

    let harness_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("emit-event".to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload: build_payload(payload),
        spec: resolved_spec,
    };
    // W5: route through the central classifier — `pipeline.*` events still
    // land in SQLite, anything else (the bulk of `emit-event` users) goes to
    // the per-spec NDJSON sink.
    let _ = crate::shared::events::route::emit(&dir, &harness_event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_payload_pair_keeps_strings_and_parses_json() {
        assert_eq!(
            parse_payload_pair("target=42"),
            Some(("target".to_string(), json!(42)))
        );
        assert_eq!(
            parse_payload_pair("target=https://x/pull/1"),
            Some(("target".to_string(), json!("https://x/pull/1")))
        );
        assert_eq!(
            parse_payload_pair("done=true"),
            Some(("done".to_string(), json!(true)))
        );
        assert_eq!(parse_payload_pair("no-equals"), None);
        assert_eq!(parse_payload_pair("=value"), None);
    }

    #[test]
    fn build_payload_collects_all_pairs() {
        let pairs = vec!["spec=auth".to_string(), "target=7".to_string()];
        let payload = build_payload(&pairs);
        assert_eq!(payload["spec"], json!("auth"));
        assert_eq!(payload["target"], json!(7));
    }

    #[test]
    fn build_payload_empty_is_empty_object() {
        assert_eq!(build_payload(&[]), json!({}));
    }
}
