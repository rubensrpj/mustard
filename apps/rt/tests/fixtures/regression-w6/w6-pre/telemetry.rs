//! Minimal W6-pre fixture — the 9 telemetry readers with small REAL bodies.
//!
//! Data for `gate_regression_check`'s AC-A-1 replay test
//! (`wave_7_review_w6_fixture_triggers_three_of_four_moments`), never
//! compiled. Each function carries enough real lines that the W6 stubbing
//! (see `w6-post/telemetry.rs`) shrinks it past `LINE_CHANGE_THRESHOLD`.

use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn rtk_summary(events: &[Value]) -> Value {
    let mut saved = 0i64;
    let mut commands = 0i64;
    for ev in events {
        if ev.get("event").and_then(Value::as_str) != Some("rtk.savings") {
            continue;
        }
        let payload = ev.get("payload").cloned().unwrap_or(Value::Null);
        saved += payload.get("saved").and_then(Value::as_i64).unwrap_or(0);
        commands += 1;
    }
    json!({ "saved": saved, "commands": commands })
}

pub fn hook_fire_counts(events: &[Value]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for ev in events {
        let Some(actor) = ev.get("actor").and_then(|a| a.get("id")) else {
            continue;
        };
        let Some(name) = actor.as_str() else {
            continue;
        };
        *counts.entry(name.to_string()).or_insert(0) += 1;
    }
    counts
}

pub fn routing_breakdown(events: &[Value]) -> Value {
    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    for ev in events {
        let kind = ev
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("other")
            .to_string();
        *by_kind.entry(kind).or_insert(0) += 1;
    }
    serde_json::to_value(by_kind).unwrap_or(Value::Null)
}

pub fn workflow_by_phase(events: &[Value]) -> Value {
    let mut phases: BTreeMap<String, u64> = BTreeMap::new();
    for ev in events {
        if ev.get("event").and_then(Value::as_str) != Some("pipeline.phase") {
            continue;
        }
        let to = ev
            .get("payload")
            .and_then(|p| p.get("to"))
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN");
        *phases.entry(to.to_string()).or_insert(0) += 1;
    }
    serde_json::to_value(phases).unwrap_or(Value::Null)
}

pub fn tool_breakdown(events: &[Value]) -> BTreeMap<String, u64> {
    let mut tools: BTreeMap<String, u64> = BTreeMap::new();
    for ev in events {
        if ev.get("event").and_then(Value::as_str) != Some("tool.use") {
            continue;
        }
        let tool = ev
            .get("payload")
            .and_then(|p| p.get("tool"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        *tools.entry(tool.to_string()).or_insert(0) += 1;
    }
    tools
}

pub fn agent_activity(events: &[Value]) -> Vec<Value> {
    let mut rows: Vec<Value> = Vec::new();
    for ev in events {
        let name = ev.get("event").and_then(Value::as_str).unwrap_or("");
        if name != "agent.start" && name != "agent.stop" {
            continue;
        }
        rows.push(json!({
            "event": name,
            "ts": ev.get("ts").cloned().unwrap_or(Value::Null),
            "actor": ev.get("actor").cloned().unwrap_or(Value::Null),
        }));
    }
    rows
}

pub fn measured(events: &[Value]) -> Value {
    let mut input_tokens = 0i64;
    let mut output_tokens = 0i64;
    for ev in events {
        if ev.get("event").and_then(Value::as_str) != Some("pipeline.telemetry.metric") {
            continue;
        }
        let payload = ev.get("payload").cloned().unwrap_or(Value::Null);
        input_tokens += payload.get("input").and_then(Value::as_i64).unwrap_or(0);
        output_tokens += payload.get("output").and_then(Value::as_i64).unwrap_or(0);
    }
    json!({ "inputTokens": input_tokens, "outputTokens": output_tokens })
}

pub fn dashboard_prompt_economy(events: &[Value]) -> Value {
    let mut baseline = 0i64;
    let mut optimized = 0i64;
    for ev in events {
        if ev.get("event").and_then(Value::as_str) != Some("prompt.economy") {
            continue;
        }
        let payload = ev.get("payload").cloned().unwrap_or(Value::Null);
        baseline += payload.get("baseline").and_then(Value::as_i64).unwrap_or(0);
        optimized += payload.get("optimized").and_then(Value::as_i64).unwrap_or(0);
    }
    json!({ "baseline": baseline, "optimized": optimized, "savedPct": pct(baseline, optimized) })
}

pub fn dashboard_economy_summary(events: &[Value]) -> Value {
    let rtk = rtk_summary(events);
    let prompts = dashboard_prompt_economy(events);
    let tools = tool_breakdown(events);
    let tool_total: u64 = tools.values().sum();
    json!({
        "rtk": rtk,
        "prompts": prompts,
        "toolCalls": tool_total,
    })
}

fn pct(baseline: i64, optimized: i64) -> i64 {
    if baseline <= 0 {
        return 0;
    }
    ((baseline - optimized) * 100) / baseline
}
