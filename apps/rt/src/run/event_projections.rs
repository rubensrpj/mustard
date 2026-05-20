//! `mustard-rt run event-projections` — a port of `scripts/event-projections.js`.
//!
//! Read-only projections over the harness event log
//! (`.claude/.harness/mustard.db`). Each view derives a JSON document from
//! the replayed events; the CLI prints it to stdout. Exit `0` always
//! (fail-open).
//!
//! Views ported: `agent-visibility`, `pipeline-state`, `session-summary`,
//! `epic-summary`, `cross-session-timeline`, `spec-tree`, `pr-metrics`. The JS
//! `buildSlopeReport` projection is **deliberately not ported** — B3 deleted
//! the `duplication.warn` / `convention.warn` hooks that fed it, so nothing
//! emits those events anymore (b4 spec, dead-code removal). An unknown
//! `--view` returns `{ "error": ... }`.
//!
//! `--format json` (default) prints the projection. `--format html` wraps the
//! same JSON in a standalone HTML page and prints its path on stderr.

use crate::report::Report;
use mustard_core::io::sqlite_store::SqliteEventStore;
use mustard_core::model::event::HarnessEvent;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `agent.stop` summary truncation, matching `DEFAULT_AGENT_SUMMARY_CHARS`.
const AGENT_SUMMARY_CHARS: usize = 800;
/// Finding-confidence floor, matching `DEFAULT_FINDING_CONFIDENCE`.
const FINDING_CONFIDENCE: f64 = 0.7;
/// Per-wave event cap, matching `DEFAULT_AGENT_EVENT_LIMIT`.
const AGENT_EVENT_LIMIT: usize = 40;

/// Replay the harness event log under `cwd`.
fn read_events(cwd: &Path) -> Vec<HarnessEvent> {
    SqliteEventStore::for_project(cwd)
        .and_then(|store| store.replay())
        .unwrap_or_default()
}

/// `buildAgentVisibility` — recent events of a wave plus high-confidence
/// findings. If `wave` is `None`, the max wave seen is used.
fn build_agent_visibility(events: &[HarnessEvent], wave: Option<u32>) -> Value {
    let wave = wave.unwrap_or_else(|| events.iter().map(|e| e.wave).max().unwrap_or(0));

    let mut wave_events: Vec<Value> = Vec::new();
    let mut findings: Vec<&HarnessEvent> = Vec::new();
    for ev in events {
        if ev.wave == wave {
            wave_events.push(truncate_summary(ev));
        }
        if ev.event == "finding" {
            let conf = ev.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
            if conf >= FINDING_CONFIDENCE {
                findings.push(ev);
            }
        }
    }
    // Sort findings: confidence desc, then ts desc.
    findings.sort_by(|a, b| {
        let ca = a.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        let cb = b.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        cb.partial_cmp(&ca)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.ts.cmp(&a.ts))
    });
    // Dedup findings by the first 60 chars of normalised content.
    let mut seen = std::collections::HashSet::new();
    let mut deduped: Vec<Value> = Vec::new();
    for f in findings {
        let content = f.payload.get("content").and_then(Value::as_str).unwrap_or("");
        let key: String = content
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(60)
            .collect();
        if seen.insert(key) {
            deduped.push(serde_json::to_value(f).unwrap_or(Value::Null));
        }
    }
    // Keep the most recent events within the limit.
    if wave_events.len() > AGENT_EVENT_LIMIT {
        wave_events.drain(..wave_events.len() - AGENT_EVENT_LIMIT);
    }
    json!({ "wave": wave, "events": wave_events, "findings": deduped })
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

/// `buildPipelineState` — current phase + dispatch failures + metrics.
fn build_pipeline_state(events: &[HarnessEvent], spec: Option<&str>) -> Value {
    let mut phase: Option<String> = None;
    let mut last_event_at: Option<String> = None;
    let mut started_at: Option<String> = None;
    let mut dispatch_failures: Vec<Value> = Vec::new();
    let mut decisions: Vec<Value> = Vec::new();
    let mut lessons: Vec<Value> = Vec::new();
    let mut api_calls = 0i64;
    let mut tool_breakdown: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut agent_count = 0i64;
    let mut failures_by_phase: serde_json::Map<String, Value> = serde_json::Map::new();

    for ev in events {
        if let Some(s) = spec {
            if ev.spec.as_deref() != Some(s) {
                continue;
            }
        }
        if !ev.ts.is_empty() {
            if started_at.is_none() {
                started_at = Some(ev.ts.clone());
            }
            last_event_at = Some(ev.ts.clone());
        }
        match ev.event.as_str() {
            "pipeline.phase" => {
                if let Some(to) = ev.payload.get("to").and_then(Value::as_str) {
                    phase = Some(to.to_string());
                } else if let Some(from) = ev.payload.get("from").and_then(Value::as_str) {
                    phase = Some(from.to_string());
                }
            }
            "dispatch.failure" => {
                dispatch_failures.push(serde_json::to_value(ev).unwrap_or(Value::Null));
                let ph = ev
                    .payload
                    .get("phase")
                    .and_then(Value::as_str)
                    .unwrap_or("UNKNOWN")
                    .to_string();
                let n = failures_by_phase.get(&ph).and_then(Value::as_i64).unwrap_or(0);
                failures_by_phase.insert(ph, json!(n + 1));
            }
            "decision" => decisions.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "lesson" => lessons.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "tool.use" => {
                let tool = ev.payload.get("tool").and_then(Value::as_str).unwrap_or("unknown");
                if tool != "Read" {
                    api_calls += 1;
                    let n = tool_breakdown.get(tool).and_then(Value::as_i64).unwrap_or(0);
                    tool_breakdown.insert(tool.to_string(), json!(n + 1));
                }
            }
            "agent.start" => agent_count += 1,
            _ => {}
        }
    }

    json!({
        "spec": spec,
        "phase": phase,
        "lastEventAt": last_event_at,
        "dispatchFailures": dispatch_failures,
        "decisions": decisions,
        "lessons": lessons,
        "metrics": {
            "apiCalls": api_calls,
            "toolBreakdown": tool_breakdown,
            "retries": failures_by_phase.values().filter_map(Value::as_i64).sum::<i64>(),
            "agentCount": agent_count,
            "startedAt": started_at,
            "dispatchFailuresByPhase": failures_by_phase,
        },
    })
}

/// `buildSessionSummary` — roll-up over a whole session's events.
fn build_session_summary(events: &[HarnessEvent]) -> Value {
    let mut session_id: Option<String> = None;
    let mut started_at: Option<String> = None;
    let mut ended_at: Option<String> = None;
    let mut agent_count = 0i64;
    let mut tool_count = 0i64;
    let mut findings: Vec<Value> = Vec::new();
    let mut decisions: Vec<Value> = Vec::new();
    let mut lessons: Vec<Value> = Vec::new();
    let mut specs = std::collections::BTreeSet::new();

    for ev in events {
        if session_id.is_none() && !ev.session_id.is_empty() {
            session_id = Some(ev.session_id.clone());
        }
        if !ev.ts.is_empty() {
            if started_at.is_none() {
                started_at = Some(ev.ts.clone());
            }
            ended_at = Some(ev.ts.clone());
        }
        if let Some(s) = &ev.spec {
            specs.insert(s.clone());
        }
        match ev.event.as_str() {
            "agent.start" => agent_count += 1,
            "tool.use" => tool_count += 1,
            "finding" => findings.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "decision" => decisions.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "lesson" => lessons.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            _ => {}
        }
    }
    json!({
        "sessionId": session_id,
        "startedAt": started_at,
        "endedAt": ended_at,
        "agentCount": agent_count,
        "toolCount": tool_count,
        "specs": specs.into_iter().collect::<Vec<_>>(),
        "findings": findings,
        "decisions": decisions,
        "lessons": lessons,
    })
}

/// Derive the latest phase for `spec` from a replayed event list. The single
/// source of truth post-`pipeline.phase` migration — phase no longer lives in
/// pipeline-state JSON. Returns the `to` field of the most recent
/// `pipeline.phase` event for the spec, or `None` if none exists.
fn phase_from_events(events: &[HarnessEvent], spec: &str) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|e| e.event == "pipeline.phase" && e.spec.as_deref() == Some(spec))
        .and_then(|e| {
            e.payload
                .get("to")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

/// `buildEpicSummary` — derive a summary view for an epic and its children.
fn build_epic_summary(events: &[HarnessEvent], cwd: &Path, epic: &str) -> Value {
    let states_dir = cwd.join(".claude").join(".pipeline-states");
    let read_state = |name: &str| -> Option<Value> {
        std::fs::read_to_string(states_dir.join(format!("{name}.json")))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
    };
    let root_state = read_state(epic);
    let children: Vec<String> = root_state
        .as_ref()
        .and_then(|s| s.get("children_specs"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();

    let children_info: Vec<Value> = children
        .iter()
        .map(|c| {
            // Phase derives from `pipeline.phase` events, not pipeline-state
            // JSON (Wave-2 migration).
            let phase = phase_from_events(events, c);
            json!({ "spec": c, "phase": phase })
        })
        .collect();

    let root_phase = phase_from_events(events, epic)
        .unwrap_or_default()
        .to_uppercase();

    let mut spec_set: std::collections::BTreeSet<&str> = children.iter().map(String::as_str).collect();
    spec_set.insert(epic);

    let mut findings: Vec<Value> = Vec::new();
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
            if min_ts.as_deref().map(|m| ev.ts.as_str() < m).unwrap_or(true) {
                min_ts = Some(ev.ts.clone());
            }
            if max_ts.as_deref().map(|m| ev.ts.as_str() > m).unwrap_or(true) {
                max_ts = Some(ev.ts.clone());
            }
        }
        match ev.event.as_str() {
            "finding" => findings.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "decision" => decisions.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "lesson" => lessons.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "tool.use" => tool_calls += 1,
            "agent.start" => agents += 1,
            _ => {}
        }
    }
    let duration_ms = match (
        min_ts.as_deref().and_then(crate::run::complete_spec::parse_iso_millis),
        max_ts.as_deref().and_then(crate::run::complete_spec::parse_iso_millis),
    ) {
        (Some(a), Some(b)) => (b - a).max(0),
        _ => 0,
    };
    json!({
        "epic": epic,
        "children": children_info,
        "findings": findings,
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

/// Default `cross-session-timeline` session limit (`DEFAULT_CROSS_SESSION_LIMIT`).
const CROSS_SESSION_LIMIT: usize = 3;
/// `spec-tree` recursion depth cap (`MAX_SPEC_TREE_DEPTH`).
const MAX_SPEC_TREE_DEPTH: u32 = 3;

/// `buildCrossSessionTimeline` — per-session summaries for the most-recent
/// `limit` files under `.harness/sessions/`, newest first by mtime. Each
/// summary is enriched with `epicInfo` for specs that have `children_specs`.
fn build_cross_session_timeline(cwd: &Path, limit: usize) -> Value {
    let sessions_dir = cwd.join(".claude").join(".harness").join("sessions");
    let Ok(entries) = std::fs::read_dir(&sessions_dir) else {
        return json!([]);
    };
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .map(|e| {
            let mtime = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (e.path(), mtime)
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files.truncate(limit);

    let states_dir = cwd.join(".claude").join(".pipeline-states");
    let mut results: Vec<Value> = Vec::new();
    for (file, _) in files {
        let Ok(raw) = std::fs::read_to_string(&file) else {
            continue;
        };
        let events: Vec<HarnessEvent> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let mut summary = build_session_summary(&events);
        // Enrich each spec that has children with epic metadata.
        let mut epic_info = serde_json::Map::new();
        if let Some(specs) = summary.get("specs").and_then(Value::as_array).cloned() {
            for spec in specs.iter().filter_map(Value::as_str) {
                let Some(state) = read_state(&states_dir, spec) else {
                    continue;
                };
                let children: Vec<String> = state
                    .get("children_specs")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default();
                if children.is_empty() {
                    continue;
                }
                // Phase per child derives from `pipeline.phase` events in
                // the session log (Wave-2 migration). A child that never
                // transitioned within this session is presumed not CLOSE.
                let closed = children
                    .iter()
                    .filter(|c| {
                        phase_from_events(&events, c)
                            .map(|p| p.to_uppercase())
                            .as_deref()
                            == Some("CLOSE")
                    })
                    .count();
                epic_info.insert(
                    spec.to_string(),
                    json!({ "total": children.len(), "closed": closed, "children": children }),
                );
            }
        }
        if let Some(obj) = summary.as_object_mut() {
            obj.insert("file".to_string(), json!(file.to_string_lossy()));
            obj.insert("epicInfo".to_string(), Value::Object(epic_info));
        }
        results.push(summary);
    }
    Value::Array(results)
}

/// Read a `.pipeline-states/<name>.json` file, `None` on any error.
fn read_state(states_dir: &Path, name: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(states_dir.join(format!("{name}.json"))).ok()?).ok()
}

/// `buildSpecTree` — the recursive parent/child spec hierarchy (max depth 3),
/// combining `spec.link` events with on-disk `.pipeline-states` files. Phase
/// per node derives from `pipeline.phase` events, not the JSON.
fn build_spec_tree(events: &[HarnessEvent], cwd: &Path, root_spec: &str) -> Value {
    let states_dir = cwd.join(".claude").join(".pipeline-states");
    // parent → children, child → parent — from spec.link events.
    let mut link_children: std::collections::BTreeMap<String, BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut link_parent: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for ev in events {
        if ev.event != "spec.link" {
            continue;
        }
        let parent = ev.payload.get("parent").and_then(Value::as_str);
        let child = ev.payload.get("child").and_then(Value::as_str);
        if let (Some(p), Some(c)) = (parent, child) {
            link_children.entry(p.to_string()).or_default().insert(c.to_string());
            link_parent.insert(c.to_string(), p.to_string());
        }
    }
    // Root must exist on disk or in events.
    if read_state(&states_dir, root_spec).is_none()
        && !link_children.contains_key(root_spec)
        && !link_parent.contains_key(root_spec)
    {
        return json!({ "error": "spec not found" });
    }
    build_spec_node(events, &states_dir, &link_children, &link_parent, root_spec, 1, &BTreeSet::new())
}

/// Build one `spec-tree` node, recursing into children. Detects cycles. Phase
/// per node derives from `pipeline.phase` events (Wave-2 migration); the JSON
/// state file is still consulted for `children_specs` / `parent_spec` shape.
fn build_spec_node(
    events: &[HarnessEvent],
    states_dir: &Path,
    link_children: &std::collections::BTreeMap<String, BTreeSet<String>>,
    link_parent: &std::collections::BTreeMap<String, String>,
    spec: &str,
    depth: u32,
    ancestors: &BTreeSet<String>,
) -> Value {
    if depth > MAX_SPEC_TREE_DEPTH {
        return json!({ "spec": spec, "phase": Value::Null, "truncated": true, "children": [] });
    }
    if ancestors.contains(spec) {
        return json!({ "error": "cycle-detected", "cycle_member": spec });
    }
    let state = read_state(states_dir, spec);
    let phase = phase_from_events(events, spec);
    let parent_spec = state
        .as_ref()
        .and_then(|s| s.get("parent_spec").and_then(Value::as_str))
        .map(str::to_string)
        .or_else(|| link_parent.get(spec).cloned());

    let mut children_set: BTreeSet<String> = BTreeSet::new();
    if let Some(arr) = state.as_ref().and_then(|s| s.get("children_specs")).and_then(Value::as_array) {
        children_set.extend(arr.iter().filter_map(Value::as_str).map(str::to_string));
    }
    if let Some(linked) = link_children.get(spec) {
        children_set.extend(linked.iter().cloned());
    }

    let mut new_ancestors = ancestors.clone();
    new_ancestors.insert(spec.to_string());
    let mut children: Vec<Value> = Vec::new();
    for child in &children_set {
        let node = build_spec_node(events, states_dir, link_children, link_parent, child, depth + 1, &new_ancestors);
        if node.get("error").and_then(Value::as_str).map(|e| e.contains("cycle")).unwrap_or(false) {
            return json!({ "error": "cycle-detected", "parent": spec, "child": child });
        }
        children.push(node);
    }
    let mut node = serde_json::Map::new();
    node.insert("spec".to_string(), json!(spec));
    node.insert("phase".to_string(), json!(phase));
    node.insert("children".to_string(), Value::Array(children));
    if let Some(p) = parent_spec {
        node.insert("parent_spec".to_string(), json!(p));
    }
    Value::Object(node)
}

/// `buildPRMetrics` — DORA-style metrics from `pr.opened` / `pr.merged` /
/// `review.start` / `review.complete` events within the last `days`.
fn build_pr_metrics(events: &[HarnessEvent], cwd: &Path, days: i64) -> Value {
    let _ = cwd;
    let now_ms = crate::util::now_millis() as i64;
    let from_ms = now_ms - days * 86_400_000;
    let in_window = |ts: &str| -> bool {
        crate::run::complete_spec::parse_iso_millis(ts)
            .map(|t| t >= from_ms && t <= now_ms)
            .unwrap_or(false)
    };
    let pair_key = |ev: &HarnessEvent| -> Option<String> {
        ev.payload
            .get("spec")
            .or_else(|| ev.payload.get("branch"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    let (mut opened, mut merged, mut review_start, mut review_complete): (
        Vec<&HarnessEvent>,
        Vec<&HarnessEvent>,
        Vec<&HarnessEvent>,
        Vec<&HarnessEvent>,
    ) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for ev in events {
        if ev.ts.is_empty() || !in_window(&ev.ts) {
            continue;
        }
        match ev.event.as_str() {
            "pr.opened" => opened.push(ev),
            "pr.merged" => merged.push(ev),
            "review.start" => review_start.push(ev),
            "review.complete" => review_complete.push(ev),
            _ => {}
        }
    }

    // Pair opened → merged (earliest opener first; one merge per opener).
    let pair_durations = |starts: &mut Vec<&HarnessEvent>, ends: &[&HarnessEvent]| -> Vec<i64> {
        starts.sort_by(|a, b| a.ts.cmp(&b.ts));
        let mut sorted_ends: Vec<&HarnessEvent> = ends.to_vec();
        sorted_ends.sort_by(|a, b| a.ts.cmp(&b.ts));
        let mut used = vec![false; sorted_ends.len()];
        let mut durations = Vec::new();
        for s in starts.iter() {
            let Some(key) = pair_key(s) else { continue };
            let Some(s_ms) = crate::run::complete_spec::parse_iso_millis(&s.ts) else {
                continue;
            };
            for (i, e) in sorted_ends.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let Some(e_ms) = crate::run::complete_spec::parse_iso_millis(&e.ts) else {
                    continue;
                };
                if e_ms < s_ms || pair_key(e) != Some(key.clone()) {
                    continue;
                }
                durations.push(e_ms - s_ms);
                used[i] = true;
                break;
            }
        }
        durations
    };
    let lead_times = pair_durations(&mut opened, &merged);
    let review_times = pair_durations(&mut review_start, &review_complete);
    let sizes: Vec<i64> = opened
        .iter()
        .filter_map(|e| e.payload.get("linesChanged").and_then(Value::as_i64))
        .filter(|n| *n > 0)
        .collect();

    let stat = |arr: &[i64]| -> Value {
        if arr.is_empty() {
            return json!({ "count": 0, "p50": Value::Null, "p90": Value::Null, "max": Value::Null });
        }
        let mut sorted = arr.to_vec();
        sorted.sort_unstable();
        let pct = |p: usize| -> i64 {
            let idx = ((p as f64 / 100.0) * sorted.len() as f64).floor() as usize;
            sorted[idx.min(sorted.len() - 1)]
        };
        json!({
            "count": sorted.len(),
            "p50": pct(50),
            "p90": pct(90),
            "max": *sorted.last().unwrap_or(&0),
        })
    };
    let bucket_by_day = |arr: &[&HarnessEvent]| -> Value {
        let mut map: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for e in arr {
            let day: String = e.ts.chars().take(10).collect();
            if !day.is_empty() {
                *map.entry(day).or_insert(0) += 1;
            }
        }
        Value::Array(
            map.into_iter()
                .map(|(date, count)| json!({ "date": date, "count": count }))
                .collect(),
        )
    };

    json!({
        "window": { "days": days },
        "totals": {
            "opened": opened.len(),
            "merged": merged.len(),
            "reviewsStarted": review_start.len(),
            "reviewsCompleted": review_complete.len(),
        },
        "leadTimeMs": stat(&lead_times),
        "reviewTimeMs": stat(&review_times),
        "prSize": stat(&sizes),
        "openedByDay": bucket_by_day(&opened),
        "mergedByDay": bucket_by_day(&merged),
    })
}

/// Compute the projection for a `--view`.
fn project(cwd: &Path, view: &str, spec: Option<&str>, wave: Option<u32>) -> Value {
    match view {
        "agent-visibility" => build_agent_visibility(&read_events(cwd), wave),
        "pipeline-state" => build_pipeline_state(&read_events(cwd), spec),
        "session-summary" => build_session_summary(&read_events(cwd)),
        "epic-summary" => match spec {
            Some(s) => build_epic_summary(&read_events(cwd), cwd, s),
            None => json!({ "error": "--spec is required for epic-summary view" }),
        },
        "cross-session-timeline" => {
            // `--wave` doubles as the optional `--limit` for this view.
            let limit = wave.map(|w| w as usize).unwrap_or(CROSS_SESSION_LIMIT);
            build_cross_session_timeline(cwd, limit)
        }
        "spec-tree" => match spec {
            Some(s) => build_spec_tree(&read_events(cwd), cwd, s),
            None => json!({ "error": "--spec is required for spec-tree view" }),
        },
        "pr-metrics" => {
            // `--wave` doubles as the optional `--days` window for this view.
            let days = wave.map(i64::from).unwrap_or(30);
            build_pr_metrics(&read_events(cwd), cwd, days)
        }
        other => json!({ "error": format!("Unknown view: {other}") }),
    }
}

/// Write the standalone HTML report wrapping the projection JSON.
fn write_html_report(cwd: &Path, view: &str, json_text: &str) -> Option<PathBuf> {
    let dir = cwd.join(".claude").join(".qa-reports");
    std::fs::create_dir_all(&dir).ok()?;
    let mut report = Report::new(format!("Event Projection — {view}"), "harness event log view");
    report.pre_section("Projection", json_text);
    let path = dir.join(format!("event-projection-{view}.html"));
    std::fs::write(&path, report.render()).ok()?;
    Some(path)
}

/// Dispatch `mustard-rt run event-projections`.
pub fn run(view: Option<&str>, spec: Option<&str>, wave: Option<u32>, format: &str) {
    let Some(view) = view else {
        eprintln!("Usage: event-projections --view <name> [--spec <name>] [--wave <n>] [--format json|html]");
        eprintln!("Views: agent-visibility, pipeline-state, session-summary, epic-summary, cross-session-timeline, spec-tree, pr-metrics");
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = project(&cwd, view, spec, wave);
    let json_text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());

    if format == "html" {
        match write_html_report(&cwd, view, &json_text) {
            Some(path) => eprintln!("[event-projections] HTML report: {}", path.display()),
            None => eprintln!("[event-projections] WARN: could not write HTML report"),
        }
    }
    println!("{json_text}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::model::event::{Actor, ActorKind, SCHEMA_VERSION};

    fn ev(event: &str, spec: Option<&str>, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-19T00:00:00.000Z".to_string(),
            session_id: "s1".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: event.to_string(),
            payload,
            spec: spec.map(str::to_string),
        }
    }

    #[test]
    fn pipeline_state_counts_tool_use_and_phase() {
        let events = vec![
            ev("pipeline.phase", Some("demo"), json!({ "to": "EXECUTE" })),
            ev("tool.use", Some("demo"), json!({ "tool": "Edit" })),
            ev("tool.use", Some("demo"), json!({ "tool": "Read" })),
        ];
        let v = build_pipeline_state(&events, Some("demo"));
        assert_eq!(v["phase"], json!("EXECUTE"));
        // Read is excluded from apiCalls.
        assert_eq!(v["metrics"]["apiCalls"], json!(1));
    }

    #[test]
    fn session_summary_collects_specs_and_counts() {
        let events = vec![
            ev("agent.start", Some("a"), json!({})),
            ev("finding", Some("b"), json!({ "content": "x" })),
        ];
        let v = build_session_summary(&events);
        assert_eq!(v["agentCount"], json!(1));
        assert_eq!(v["specs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn unknown_view_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let v = project(dir.path(), "slope-report", None, None);
        assert!(v.get("error").is_some());
    }

    #[test]
    fn spec_tree_builds_parent_child() {
        let events = vec![ev(
            "spec.link",
            None,
            json!({ "parent": "epic-a", "child": "child-b" }),
        )];
        let dir = tempfile::tempdir().unwrap();
        let tree = build_spec_tree(&events, dir.path(), "epic-a");
        assert_eq!(tree["spec"], json!("epic-a"));
        let children = tree["children"].as_array().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["spec"], json!("child-b"));
    }

    #[test]
    fn spec_tree_unknown_root_errors() {
        let dir = tempfile::tempdir().unwrap();
        let tree = build_spec_tree(&[], dir.path(), "ghost");
        assert_eq!(tree["error"], json!("spec not found"));
    }

    #[test]
    fn pr_metrics_pairs_lead_time() {
        let events = vec![
            ev("pr.opened", None, json!({ "spec": "auth", "linesChanged": 40 })),
            {
                let mut e = ev("pr.merged", None, json!({ "spec": "auth" }));
                e.ts = "2026-05-19T01:00:00.000Z".to_string();
                e
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        let m = build_pr_metrics(&events, dir.path(), 30);
        assert_eq!(m["totals"]["opened"], json!(1));
        assert_eq!(m["totals"]["merged"], json!(1));
        assert_eq!(m["leadTimeMs"]["count"], json!(1));
        assert_eq!(m["prSize"]["count"], json!(1));
    }

    #[test]
    fn cross_session_timeline_empty_when_no_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let v = build_cross_session_timeline(dir.path(), 3);
        assert_eq!(v, json!([]));
    }
}
