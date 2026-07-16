//! `agent-visibility` projection: recent events of a wave, with `agent.stop`
//! summary truncation. Extracted from `event_projections` (F3 PERF-D split).

use mustard_core::domain::model::event::HarnessEvent;
use serde_json::{json, Value};

/// `agent.stop` summary truncation, matching `DEFAULT_AGENT_SUMMARY_CHARS`.
const AGENT_SUMMARY_CHARS: usize = 800;
/// Per-wave event cap, matching `DEFAULT_AGENT_EVENT_LIMIT`.
const AGENT_EVENT_LIMIT: usize = 40;

/// `buildAgentVisibility` — recent events of a wave. The `findings` key stays
/// in the shape as an always-empty array — the `finding` event lost its last
/// producer (phantom-reader sweep).
/// If `wave` is `None`, the max wave seen is used.
pub(super) fn build_agent_visibility(events: &[HarnessEvent], wave: Option<u32>) -> Value {
    let wave = wave.unwrap_or_else(|| events.iter().map(|e| e.wave).max().unwrap_or(0));

    let mut wave_events: Vec<Value> = Vec::new();
    for ev in events {
        if ev.wave == wave {
            wave_events.push(truncate_summary(ev));
        }
    }
    // Keep the most recent events within the limit.
    if wave_events.len() > AGENT_EVENT_LIMIT {
        wave_events.drain(..wave_events.len() - AGENT_EVENT_LIMIT);
    }
    json!({ "wave": wave, "events": wave_events, "findings": [] })
}

/// Truncate an `agent.stop` event's `payload.summary`, leaving others as-is.
fn truncate_summary(ev: &HarnessEvent) -> Value {
    let mut value = serde_json::to_value(ev).unwrap_or(Value::Null);
    if ev.event == "agent.stop" {
        if let Some(summary) = ev.payload.get("summary").and_then(Value::as_str) {
            if summary.chars().count() > AGENT_SUMMARY_CHARS {
                let cut: String = summary.chars().take(AGENT_SUMMARY_CHARS).collect();
                if let Some(p) = value.get_mut("payload").and_then(Value::as_object_mut) {
                    p.insert("summary".to_string(), json!(format!("{cut}…")));
                }
            }
        }
    }
    value
}

