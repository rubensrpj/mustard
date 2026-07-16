//! Minimal W6-post fixture — the same 9 telemetry readers STUBBED, exactly
//! the fail-open regression the gate exists to catch: signatures kept, real
//! bodies replaced with `Vec::new()` / `Default::default()` / empty JSON
//! "until the next wave delivers the NDJSON reader".
//!
//! Data for `gate_regression_check`'s AC-A-1 replay test, never compiled.

use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn rtk_summary(_events: &[Value]) -> Value {
    // TODO(W7): real NDJSON reader lands next wave.
    json!({})
}

pub fn hook_fire_counts(_events: &[Value]) -> BTreeMap<String, u64> {
    Default::default()
}

pub fn routing_breakdown(_events: &[Value]) -> Value {
    json!({})
}

pub fn workflow_by_phase(_events: &[Value]) -> Value {
    json!({})
}

pub fn tool_breakdown(_events: &[Value]) -> BTreeMap<String, u64> {
    Default::default()
}

pub fn agent_activity(_events: &[Value]) -> Vec<Value> {
    Vec::new()
}

pub fn measured(_events: &[Value]) -> Value {
    json!({})
}

pub fn dashboard_prompt_economy(_events: &[Value]) -> Value {
    json!({})
}

pub fn dashboard_economy_summary(_events: &[Value]) -> Value {
    json!({})
}
