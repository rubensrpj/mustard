//! `epic-summary` projection. Extracted from `event_projections` (F3 PERF-D split).

use mustard_core::domain::model::event::HarnessEvent;
use serde_json::{json, Value};
use std::path::Path;

/// `buildEpicSummary` — derive a summary view for an epic and its children.
///
/// Children are reconstructed from `spec.link` events (`{ parent, child }`) —
/// the single source of truth post-W4C. The legacy
/// `.pipeline-states/{epic}.json` `children_specs` array is no longer written
/// (the retired `spec-link` command emitted only the NDJSON event), so this
/// derives the edge from the stream like [`build_spec_tree`] does.
pub(super) fn build_epic_summary(events: &[HarnessEvent], cwd: &Path, epic: &str) -> Value {
    let _ = cwd;
    let mut children_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for ev in events {
        if ev.event != "spec.link" {
            continue;
        }
        if ev.payload.get("parent").and_then(Value::as_str) != Some(epic) {
            continue;
        }
        if let Some(child) = ev.payload.get("child").and_then(Value::as_str) {
            children_set.insert(child.to_string());
        }
    }
    let children: Vec<String> = children_set.into_iter().collect();

    let children_info: Vec<Value> = children
        .iter()
        .map(|c| {
            // Phase derives from `pipeline.phase` events, not pipeline-state
            // JSON (Wave-2 migration).
            let phase = super::phase_from_events(events, c);
            json!({ "spec": c, "phase": phase })
        })
        .collect();

    let root_phase = super::phase_from_events(events, epic)
        .unwrap_or_default()
        .to_uppercase();

    let mut spec_set: std::collections::BTreeSet<&str> = children.iter().map(String::as_str).collect();
    spec_set.insert(epic);

    let mut decisions: Vec<Value> = Vec::new();
    let mut lessons: Vec<Value> = Vec::new();
    let (mut tool_calls, mut agents) = (0i64, 0i64);
    let (mut min_ts, mut max_ts): (Option<String>, Option<String>) = (None, None);
    let mut folded = root_phase == "CLOSE";

    for ev in events {
        if ev.event == "epic.fold"
            && ev.payload.get("epic").and_then(Value::as_str) == Some(epic)
        {
            folded = true;
        }
        let Some(spec) = ev.spec.as_deref() else { continue };
        if !spec_set.contains(spec) {
            continue;
        }
        if !ev.ts.is_empty() {
            if min_ts.as_deref().is_none_or(|m| ev.ts.as_str() < m) {
                min_ts = Some(ev.ts.clone());
            }
            if max_ts.as_deref().is_none_or(|m| ev.ts.as_str() > m) {
                max_ts = Some(ev.ts.clone());
            }
        }
        match ev.event.as_str() {
            "decision" => decisions.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "lesson" => lessons.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "tool.use" => tool_calls += 1,
            "agent.start" => agents += 1,
            _ => {}
        }
    }
    let duration_ms = match (
        min_ts.as_deref().and_then(mustard_core::time::parse_iso_millis),
        max_ts.as_deref().and_then(mustard_core::time::parse_iso_millis),
    ) {
        (Some(a), Some(b)) => (b - a).max(0),
        _ => 0,
    };
    json!({
        "epic": epic,
        "children": children_info,
        "findings": [],
        "decisions": decisions,
        "lessons": lessons,
        "metrics": {
            "toolCallsTotal": tool_calls,
            "agentsTotal": agents,
            "durationMs": duration_ms,
            "startedAt": min_ts,
            "endedAt": max_ts,
        },
        "folded": folded,
    })
}

