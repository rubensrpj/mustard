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
use mustard_core::fs;
use mustard_core::ClaudePaths;
use mustard_core::model::view::{Phase, Stage};
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{
    HarnessEvent, EVENT_PIPELINE_COMPLETE, EVENT_PIPELINE_DISPATCH_FAILURE, EVENT_PIPELINE_PAUSE,
    EVENT_PIPELINE_RESUME_MODE, EVENT_PIPELINE_SCOPE, EVENT_PIPELINE_STATUS,
    EVENT_PIPELINE_TASK_COMPLETE, EVENT_PIPELINE_TASK_DISPATCH, EVENT_PIPELINE_WAVE_COMPLETE,
    PipelineCompletePayload, PipelineDispatchFailurePayload,
};
use mustard_core::projection::project_spec_view_with_header;
use serde::{Deserialize, Serialize};
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
    let mut hygiene: Vec<Value> = Vec::new();
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
            // hygiene.detected / hygiene.autoclose / hygiene.skipped — surfaced so
            // `--view session-summary` lists recent hygiene activity (AC-W5-5).
            k if k.starts_with("hygiene.") => {
                hygiene.push(serde_json::to_value(ev).unwrap_or(Value::Null));
            }
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
        "hygiene": hygiene,
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
    let states_dir = ClaudePaths::for_project(cwd)
        .map(|p| p.pipeline_states_dir())
        .unwrap_or_else(|_| cwd.to_path_buf());
    let read_state = |name: &str| -> Option<Value> {
        fs::read_to_string(states_dir.join(format!("{name}.json")))
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
            if min_ts.as_deref().is_none_or(|m| ev.ts.as_str() < m) {
                min_ts = Some(ev.ts.clone());
            }
            if max_ts.as_deref().is_none_or(|m| ev.ts.as_str() > m) {
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
    let sessions_dir = ClaudePaths::for_project(cwd)
        .map(|p| p.harness_dir().join("sessions"))
        .unwrap_or_else(|_| cwd.to_path_buf());
    let Ok(entries) = fs::read_dir(&sessions_dir) else {
        return json!([]);
    };
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = entries
        .into_iter()
        .filter(|e| e.path.extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .map(|e| {
            let mtime = fs::modified(&e.path).unwrap_or(std::time::UNIX_EPOCH);
            (e.path, mtime)
        })
        .collect();
    files.sort_by_key(|b| std::cmp::Reverse(b.1));
    files.truncate(limit);

    let states_dir = ClaudePaths::for_project(cwd)
        .map(|p| p.pipeline_states_dir())
        .unwrap_or_else(|_| cwd.to_path_buf());
    let mut results: Vec<Value> = Vec::new();
    for (file, _) in files {
        let Ok(raw) = fs::read_to_string(&file) else {
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
    serde_json::from_str(&fs::read_to_string(states_dir.join(format!("{name}.json"))).ok()?).ok()
}

/// `buildSpecTree` — the recursive parent/child spec hierarchy (max depth 3),
/// combining `spec.link` events with on-disk `.pipeline-states` files. Phase
/// per node derives from `pipeline.phase` events, not the JSON.
fn build_spec_tree(events: &[HarnessEvent], cwd: &Path, root_spec: &str) -> Value {
    let states_dir = ClaudePaths::for_project(cwd)
        .map(|p| p.pipeline_states_dir())
        .unwrap_or_else(|_| cwd.to_path_buf());
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
        if node.get("error").and_then(Value::as_str).is_some_and(|e| e.contains("cycle")) {
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
            .is_some_and(|t| t >= from_ms && t <= now_ms)
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


/// `buildActivePipelines` — specs that have at least one event (or a parseable
/// header on disk) and whose last `pipeline.stage` / `pipeline.phase` (event
/// stream) or `### Stage:` header (disk fallback) is not `Close`. Ordered by
/// `lastEventAt` descending. Specs with no activity in the last 30 days are
/// also excluded (defensive filter).
///
/// Two-pass algorithm:
/// 1. Fold the event stream exactly as before, populating `per_spec`.
/// 2. Glob `.claude/spec/*/spec.md` (+ `wave-plan.md` fallback). For every
///    spec present on disk but absent from the event-stream map, delegate to
///    `mustard_core::projection::project_spec_view_with_header` which parses
///    the `### Stage:` / `### Outcome:` header and emits a synthetic
///    `pipeline.status` event into the local SQLite store. The resulting
///    `SpecView` is merged into `per_spec` before the filter+sort step.
fn build_active_pipelines(events: &[HarnessEvent], cwd: &Path) -> Value {
    use std::collections::BTreeMap;

    let now_ms = crate::util::now_millis() as i64;
    // 30-day window in milliseconds.
    let cutoff_ms = now_ms - 30 * 86_400_000;

    // Per-spec accumulator: (last_event_ts, last_stage).
    let mut per_spec: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();

    // --- Pass 1: fold the event stream ---

    for ev in events {
        let Some(spec) = ev.spec.as_deref() else { continue };
        if ev.ts.is_empty() { continue; }

        let entry = per_spec.entry(spec.to_string()).or_insert_with(|| (ev.ts.clone(), None));
        // Track the maximum timestamp (ISO-8601 lexicographic order is correct for UTC).
        if ev.ts > entry.0 {
            entry.0.clone_from(&ev.ts);
        }
        // Track the last stage from `pipeline.stage` (Title Case) OR
        // `pipeline.phase` (UPPERCASE → Title Case). Whichever appears latest
        // in the event stream wins (lexicographic ts comparison is safe for UTC).
        match ev.event.as_str() {
            "pipeline.stage" => {
                if let Some(stage) = ev.payload.get("to").and_then(Value::as_str) {
                    entry.1 = Some(stage.to_string());
                }
            }
            "pipeline.phase" => {
                let raw = ev.payload.get("to")
                    .or_else(|| ev.payload.get("from"))
                    .and_then(Value::as_str);
                if let Some(r) = raw {
                    if let Some(p) = Phase::parse(r) {
                        entry.1 = Some(format!("{p:?}"));
                    }
                }
            }
            _ => {}
        }
    }

    // --- Pass 2: disk fallback for specs absent from the event stream ---
    //
    // Glob `.claude/spec/*/spec.md` + `wave-plan.md`. For each spec whose
    // directory name is not already in `per_spec`, call the canonical core
    // projection with the header fallback enabled. This covers the "git pull
    // brings a new spec from a teammate; no local event has been emitted yet"
    // case. Side-effect: a synthetic `pipeline.status` event is written to
    // the local SQLite store so subsequent reads are O(1).
    let spec_root = ClaudePaths::for_project(cwd)
        .map(|p| p.spec_dir())
        .unwrap_or_else(|_| cwd.to_path_buf());
    if let Ok(rd) = std::fs::read_dir(&spec_root) {
        // Open the local SQLite store once for the synthetic emit sink.
        // Fail-open: if the store cannot be opened, the fallback still runs
        // (without emitting) — we just pass `None` as the sink.
        let store_opt = SqliteEventStore::for_project(cwd).ok();

        for entry in rd.flatten() {
            let spec_dir = entry.path();
            if !spec_dir.is_dir() { continue; }

            let spec_name = match spec_dir.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Only process specs that have NO events — the event stream already
            // won for the others.
            if per_spec.contains_key(&spec_name) { continue; }

            // Resolve the spec.md path; accept wave-plan.md as a fallback.
            let spec_md = spec_dir.join("spec.md");
            let wave_plan_md = spec_dir.join("wave-plan.md");
            let spec_md_path = if spec_md.exists() {
                spec_md.clone()
            } else if wave_plan_md.exists() {
                wave_plan_md.clone()
            } else {
                continue; // no readable spec file — skip
            };

            // Delegate to the canonical core projection (parses header, emits
            // synthetic event). `events` is the full slice; the function
            // filters by spec_name internally.
            let view = project_spec_view_with_header(
                &spec_name,
                events,
                Some(spec_md_path.as_path()),
                store_opt.as_ref().map(|s| s as &dyn mustard_core::store::event_store::EventSink),
            );

            // Extract the stage string. Use Debug formatting to match the
            // phase-based path above (`format!("{p:?}")`): "Execute", "Plan", etc.
            let stage_str = format!("{:?}", view.state.stage);

            // Skip specs whose header says Close (non-active) or has no stage
            // signal (NoEvents maps to Stage::Plan — treat as active, include).
            // Stage::Close means terminal; exclude.
            if view.state.stage == Stage::Close {
                continue;
            }

            // Derive a `lastEventAt` sentinel from the spec.md mtime when the
            // header fallback fires (last_event_at == None from the projection).
            // This ensures the row has a sortable timestamp and passes the
            // 30-day cutoff filter. Falls back to now_iso8601 when mtime is
            // unavailable so the row is always included.
            let last_event_at = view.last_event_at.clone().unwrap_or_else(|| {
                // Use spec.md mtime as a proxy for "when the spec was written".
                std::fs::metadata(&spec_md_path)
                    .and_then(|m| m.modified())
                    .map_or_else(|_| crate::util::now_iso8601(), |mtime| {
                        use std::time::UNIX_EPOCH;
                        let secs = mtime
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs() as i64);
                        // Format as ISO-8601 seconds precision (same algorithm as
                        // `header_emit_timestamp` in mustard-core/projection/card.rs).
                        let days = secs.div_euclid(86_400);
                        let tod  = secs.rem_euclid(86_400);
                        let z = days + 719_468;
                        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
                        let doe = z - era * 146_097;
                        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
                        let y_raw = yoe + era * 400;
                        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
                        let mp  = (5 * doy + 2) / 153;
                        let d   = (doy - (153 * mp + 2) / 5 + 1) as u32;
                        let m   = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
                        let y   = if m <= 2 { y_raw + 1 } else { y_raw };
                        let h   = (tod / 3_600) as u32;
                        let mi  = ((tod % 3_600) / 60) as u32;
                        let s   = (tod % 60) as u32;
                        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
                    })
            });

            per_spec.insert(spec_name, (last_event_at, Some(stage_str)));
        }
    }

    let mut pipelines: Vec<Value> = per_spec
        .into_iter()
        .filter(|(_, (last_ts, last_stage))| {
            // Exclude specs that never emitted a pipeline.stage event (legacy/abandoned).
            if last_stage.is_none() {
                return false;
            }
            // Exclude specs whose last stage is Close.
            if last_stage.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("close")) {
                return false;
            }
            // Exclude specs with no activity in the last 30 days.
            let ts_ms = crate::run::complete_spec::parse_iso_millis(last_ts).unwrap_or(0);
            ts_ms >= cutoff_ms
        })
        .map(|(spec, (last_event_at, last_stage))| {
            json!({
                "spec": spec,
                "lastEventAt": last_event_at,
                "stage": last_stage,
            })
        })
        .collect();

    // Sort by lastEventAt descending (ISO-8601 lexicographic comparison is safe for UTC).
    pipelines.sort_by(|a, b| {
        let ta = a["lastEventAt"].as_str().unwrap_or("");
        let tb = b["lastEventAt"].as_str().unwrap_or("");
        tb.cmp(ta)
    });

    json!({ "pipelines": pipelines })
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
            let limit = wave.map_or(CROSS_SESSION_LIMIT, |w| w as usize);
            build_cross_session_timeline(cwd, limit)
        }
        "spec-tree" => match spec {
            Some(s) => build_spec_tree(&read_events(cwd), cwd, s),
            None => json!({ "error": "--spec is required for spec-tree view" }),
        },
        "pr-metrics" => {
            // `--wave` doubles as the optional `--days` window for this view.
            let days = wave.map_or(30, i64::from);
            build_pr_metrics(&read_events(cwd), cwd, days)
        }
        "active-pipelines" => build_active_pipelines(&read_events(cwd), cwd),
        other => json!({ "error": format!("Unknown view: {other}") }),
    }
}

/// Write the standalone HTML report wrapping the projection JSON.
///
/// Event-projection reports are *not* per-spec QA reports — they are
/// workspace-wide diagnostic views. The W2 cache reorg keeps them under
/// `<root>/.claude/.metrics/event-projections/` (rather than the legacy
/// `.qa-reports/` directory, which is now reserved for the per-spec
/// `spec/{name}/qa-report.{json,html}` pair).
fn write_html_report(cwd: &Path, view: &str, json_text: &str) -> Option<PathBuf> {
    let paths = ClaudePaths::for_project(cwd).ok()?;
    let dir = paths.metrics_dir().join("event-projections");
    fs::create_dir_all(&dir).ok()?;
    let mut report = Report::new(format!("Event Projection — {view}"), "harness event log view");
    report.pre_section("Projection", json_text);
    let path = dir.join(format!("event-projection-{view}.html"));
    fs::write_atomic(&path, report.render().as_bytes()).ok()?;
    Some(path)
}

/// Dispatch `mustard-rt run event-projections`.
pub fn run(view: Option<&str>, spec: Option<&str>, wave: Option<u32>, format: &str) {
    let Some(view) = view else {
        eprintln!("Usage: event-projections --view <name> [--spec <name>] [--wave <n>] [--format json|html]");
        eprintln!("Views: agent-visibility, pipeline-state, session-summary, epic-summary, cross-session-timeline, spec-tree, pr-metrics, active-pipelines");
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

// ---------------------------------------------------------------------------
// Typed pipeline-state projection — Wave 2 of 2026-05-19-pipeline-state-from-sqlite
// ---------------------------------------------------------------------------

/// A single task tracked inside a pipeline run. Built from `pipeline.task.dispatch`
/// and `pipeline.task.complete` event pairs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PipelineTaskView {
    /// Human-readable task name matching the wave-plan heading.
    pub name: String,
    /// Agent sub-type used for this dispatch (e.g. `"general-purpose"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Wave number the task belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave: Option<u32>,
    /// Role label (e.g. `"implement"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Files in scope at dispatch time, plus any reported as modified at complete.
    #[serde(default)]
    pub files: Vec<String>,
    /// `"pending"` until a matching `pipeline.task.complete` is seen, then `"completed"`.
    pub status: String,
    /// ISO-8601 timestamp of the dispatch event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched_at: Option<String>,
    /// ISO-8601 timestamp of the complete event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// Wall-clock task duration from the complete payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Typed view of a spec's pipeline state, derived entirely from the event log.
/// Mirrors the legacy `.pipeline-states/{spec}.json` shape. camelCase serde
/// renames match the dashboard's existing JSON consumers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PipelineStateView {
    /// Spec identifier.
    pub spec: String,
    /// Last `pipeline.status` value (`payload.to`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Pipeline scope token, e.g. `"full"` or `"wave"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Spec language override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Model routed to for this pipeline run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `true` when the spec uses a wave plan (from event or FS fallback).
    #[serde(default, rename = "isWavePlan", skip_serializing_if = "Option::is_none")]
    pub is_wave_plan: Option<bool>,
    /// Total wave count declared in the spec.
    #[serde(default, rename = "totalWaves", skip_serializing_if = "Option::is_none")]
    pub total_waves: Option<u32>,
    /// Next wave to dispatch: `max(completed_waves) + 1`, or `1`.
    #[serde(rename = "currentWave")]
    pub current_wave: u32,
    /// Wave numbers that have emitted a `pipeline.wave.complete` event, sorted ASC.
    #[serde(rename = "completedWaves")]
    pub completed_waves: Vec<u32>,
    /// Task views built from dispatch+complete pairs.
    pub tasks: Vec<PipelineTaskView>,
    /// Most recent `pipeline.dispatch_failure` payload, cleared if older than 10 min.
    #[serde(default, rename = "lastDispatchFailure", skip_serializing_if = "Option::is_none")]
    pub last_dispatch_failure: Option<PipelineDispatchFailurePayload>,
    /// ISO-8601 timestamp of the last `pipeline.pause` event.
    #[serde(default, rename = "pausedAt", skip_serializing_if = "Option::is_none")]
    pub paused_at: Option<String>,
    /// Human-readable pause reason.
    #[serde(default, rename = "pauseReason", skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    /// Resume mode selected (e.g. `"continue"`, `"rewrite"`, `"abort"`).
    #[serde(default, rename = "resumeMode", skip_serializing_if = "Option::is_none")]
    pub resume_mode: Option<String>,
    /// ISO-8601 timestamp at which the pipeline was closed (from `pipeline.complete`).
    /// Falls back to the last `pipeline.status` event's `ts` when the complete event
    /// is absent but status is `"closed-followup"`.
    #[serde(default, rename = "closedAt", skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    /// Files touched during the pipeline run (from `pipeline.complete`).
    #[serde(default, rename = "affectedFiles")]
    pub affected_files: Vec<String>,
}

/// Ten minutes in milliseconds — the stale-failure cutoff matching the `/resume` Step 0 contract.
const DISPATCH_FAILURE_TTL_MS: i64 = 10 * 60 * 1_000;

/// Derive a [`PipelineStateView`] for `spec` by folding its event stream.
///
/// Queries events ordered by `id ASC` (insertion order). Fail-open on
/// malformed payloads — a bad row is logged to stderr and skipped, never
/// panicked. Returns `None` when no events exist for the spec.
///
/// `spec_dir` is an optional filesystem path to the spec directory
/// (`.claude/spec/{spec}` — flat layout). When provided and `wave-plan.md` exists
/// there, `is_wave_plan` is set to `true` even if no `pipeline.scope` event
/// recorded it yet.
#[must_use]
pub fn pipeline_state_for_spec(
    store: &SqliteEventStore,
    spec: &str,
    spec_dir: Option<&std::path::Path>,
) -> Option<PipelineStateView> {
    let events = store.query(Some(spec)).unwrap_or_default();
    if events.is_empty() {
        return None;
    }

    let mut view = PipelineStateView {
        spec: spec.to_string(),
        current_wave: 1,
        ..Default::default()
    };

    // Raw dispatch-failure payload + its timestamp (cleared after fold if stale).
    let mut raw_failure: Option<(PipelineDispatchFailurePayload, Option<String>)> = None;
    // Timestamp of the most recent pipeline.status event — used as closed_at
    // fallback when status=="closed-followup" but no pipeline.complete exists.
    let mut last_status_ts: Option<String> = None;

    for ev in &events {
        match ev.event.as_str() {
            EVENT_PIPELINE_SCOPE => {
                // Lenient: missing fields default via #[serde(default)].
                match serde_json::from_value::<mustard_core::model::event::PipelineScopePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        view.scope = Some(p.scope);
                        if p.lang.is_some() { view.lang = p.lang; }
                        if p.model.is_some() { view.model = p.model; }
                        if p.is_wave_plan.is_some() { view.is_wave_plan = p.is_wave_plan; }
                        if p.total_waves.is_some() { view.total_waves = p.total_waves; }
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_SCOPE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_STATUS => {
                match serde_json::from_value::<mustard_core::model::event::PipelineStatusPayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        view.status = Some(p.to);
                        if !ev.ts.is_empty() {
                            last_status_ts = Some(ev.ts.clone());
                        }
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_STATUS} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_COMPLETE => {
                match serde_json::from_value::<PipelineCompletePayload>(ev.payload.clone()) {
                    Ok(p) => {
                        view.closed_at = p.closed_at;
                        view.affected_files = p.affected_files;
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_COMPLETE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_TASK_DISPATCH => {
                match serde_json::from_value::<mustard_core::model::event::PipelineTaskDispatchPayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        let task = find_or_insert_task(&mut view.tasks, &p.name);
                        task.agent = p.agent;
                        task.wave = p.wave;
                        task.role = p.role;
                        if let Some(files) = p.files {
                            for f in files {
                                if !task.files.contains(&f) {
                                    task.files.push(f);
                                }
                            }
                        }
                        task.dispatched_at = Some(ev.ts.clone());
                        task.status = "pending".to_string();
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_TASK_DISPATCH} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_TASK_COMPLETE => {
                match serde_json::from_value::<mustard_core::model::event::PipelineTaskCompletePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        let task = find_or_insert_task(&mut view.tasks, &p.name);
                        task.status = "completed".to_string();
                        task.completed_at = Some(ev.ts.clone());
                        task.duration_ms = p.duration_ms;
                        if let Some(modified) = p.files_modified {
                            for f in modified {
                                if !task.files.contains(&f) {
                                    task.files.push(f);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_TASK_COMPLETE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_WAVE_COMPLETE => {
                match serde_json::from_value::<mustard_core::model::event::PipelineWaveCompletePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        if !view.completed_waves.contains(&p.wave) {
                            view.completed_waves.push(p.wave);
                        }
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_WAVE_COMPLETE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_DISPATCH_FAILURE => {
                match serde_json::from_value::<PipelineDispatchFailurePayload>(ev.payload.clone()) {
                    Ok(p) => {
                        let at = p.at.clone().or_else(|| {
                            if ev.ts.is_empty() { None } else { Some(ev.ts.clone()) }
                        });
                        raw_failure = Some((p, at));
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_DISPATCH_FAILURE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_PAUSE => {
                match serde_json::from_value::<mustard_core::model::event::PipelinePausePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        // Use the event timestamp as the canonical pause time.
                        view.paused_at = if ev.ts.is_empty() { None } else { Some(ev.ts.clone()) };
                        view.pause_reason = p.reason;
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_PAUSE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_RESUME_MODE => {
                match serde_json::from_value::<mustard_core::model::event::PipelineResumeModePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => view.resume_mode = Some(p.mode),
                    Err(e) => {
                        eprintln!("[pipeline_state_for_spec] bad {EVENT_PIPELINE_RESUME_MODE} payload for {spec}: {e}");
                    }
                }
            }
            _ => {} // Unknown event kinds are silently skipped (fail-open).
        }
    }

    // Post-fold: sort and deduplicate completed_waves.
    view.completed_waves.sort_unstable();
    view.completed_waves.dedup();

    // current_wave = max completed wave + 1, defaulting to 1.
    view.current_wave = view
        .completed_waves
        .iter()
        .max()
        .map_or(1, |w| w + 1);

    // FS fallback for is_wave_plan — takes priority only if not already set by event.
    if view.is_wave_plan.is_none() {
        if let Some(dir) = spec_dir {
            if dir.join("wave-plan.md").exists() {
                view.is_wave_plan = Some(true);
            }
        }
    }

    // closed_at fallback: if status is "closed-followup" but no pipeline.complete
    // event was recorded (e.g. legacy or partially-migrated spec), use the
    // timestamp of the last pipeline.status event instead.
    if view.closed_at.is_none()
        && view.status.as_deref() == Some("closed-followup")
    {
        view.closed_at = last_status_ts;
    }

    // Stale dispatch-failure check: clear if older than 10 minutes.
    view.last_dispatch_failure = match raw_failure {
        None => None,
        Some((payload, Some(at_str))) => {
            let now_ms = i64::try_from(crate::util::now_millis()).unwrap_or(i64::MAX);
            let age_ms = crate::run::complete_spec::parse_iso_millis(&at_str)
                .map_or(0, |at_ms| now_ms - at_ms);
            if age_ms > DISPATCH_FAILURE_TTL_MS {
                None // stale — cleared per /resume Step 0 contract
            } else {
                Some(payload)
            }
        }
        Some((payload, None)) => Some(payload), // no timestamp → keep (fail-open)
    };

    Some(view)
}

/// Find a task by name, or push a new default one and return a mutable ref.
fn find_or_insert_task<'a>(tasks: &'a mut Vec<PipelineTaskView>, name: &str) -> &'a mut PipelineTaskView {
    // Linear search — task counts are small (< 50 per spec).
    if let Some(pos) = tasks.iter().position(|t| t.name == name) {
        return &mut tasks[pos];
    }
    tasks.push(PipelineTaskView {
        name: name.to_string(),
        status: "pending".to_string(),
        ..Default::default()
    });
    tasks.last_mut().expect("just pushed")
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
    fn session_summary_surfaces_hygiene_events() {
        // AC-W5-5: `--view session-summary` lists recent hygiene.* activity.
        let events = vec![
            ev("hygiene.detected", Some("a"), json!({ "reason": "stale" })),
            ev("hygiene.autoclose", Some("b"), json!({ "gate_result": { "build": "pass" } })),
            ev("hygiene.skipped", Some("c"), json!({ "blocker": "build_red" })),
            ev("tool.use", Some("a"), json!({ "tool": "Edit" })),
        ];
        let v = build_session_summary(&events);
        let hygiene = v["hygiene"].as_array().expect("hygiene array present");
        assert_eq!(hygiene.len(), 3, "all three hygiene.* kinds surfaced");
        assert_eq!(hygiene[0]["event"], json!("hygiene.detected"));
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

    // -----------------------------------------------------------------------
    // pipeline_state_for_spec tests — Wave 2 of 2026-05-19-pipeline-state-from-sqlite
    // -----------------------------------------------------------------------

    use mustard_core::store::event_store::EventSink;

    fn store_in_dir(dir: &std::path::Path) -> SqliteEventStore {
        SqliteEventStore::new(dir.join("mustard.db")).unwrap()
    }

    fn pipeline_ev(event: &str, spec: &str, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T10:00:00.000Z".to_string(),
            session_id: "s-pipeline".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: event.to_string(),
            payload,
            spec: Some(spec.to_string()),
        }
    }

    /// Test 1 — no events for spec → None.
    #[test]
    fn ps_no_events_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());
        assert!(pipeline_state_for_spec(&store, "ghost-spec", None).is_none());
    }

    /// Test 2 — scope + status events only → fields populated, tasks empty, current_wave=1.
    #[test]
    fn ps_scope_and_status_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());
        store.append(&pipeline_ev(
            EVENT_PIPELINE_SCOPE, "spec-a",
            json!({ "scope": "full", "lang": "en", "model": "claude-opus-4-5" }),
        )).unwrap();
        store.append(&pipeline_ev(
            EVENT_PIPELINE_STATUS, "spec-a",
            json!({ "to": "active" }),
        )).unwrap();

        let view = pipeline_state_for_spec(&store, "spec-a", None).unwrap();
        assert_eq!(view.scope.as_deref(), Some("full"));
        assert_eq!(view.lang.as_deref(), Some("en"));
        assert_eq!(view.model.as_deref(), Some("claude-opus-4-5"));
        assert_eq!(view.status.as_deref(), Some("active"));
        assert!(view.tasks.is_empty());
        assert_eq!(view.current_wave, 1);
        assert!(view.completed_waves.is_empty());
    }

    /// Test 3 — wave progression → completed_waves=[1,2], current_wave=3.
    #[test]
    fn ps_wave_progression() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());
        store.append(&pipeline_ev(
            EVENT_PIPELINE_WAVE_COMPLETE, "spec-b",
            json!({ "wave": 1 }),
        )).unwrap();
        store.append(&pipeline_ev(
            EVENT_PIPELINE_WAVE_COMPLETE, "spec-b",
            json!({ "wave": 2 }),
        )).unwrap();

        let view = pipeline_state_for_spec(&store, "spec-b", None).unwrap();
        assert_eq!(view.completed_waves, vec![1u32, 2u32]);
        assert_eq!(view.current_wave, 3);
    }

    /// Test 4 — task lifecycle: dispatch + complete → status=completed with timestamps.
    #[test]
    fn ps_task_lifecycle_dispatch_then_complete() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());

        let mut dispatch_ev = pipeline_ev(
            EVENT_PIPELINE_TASK_DISPATCH, "spec-c",
            json!({ "name": "implement-auth", "agent": "general-purpose", "wave": 1 }),
        );
        dispatch_ev.ts = "2026-05-20T10:00:00.000Z".to_string();
        store.append(&dispatch_ev).unwrap();

        let mut complete_ev = pipeline_ev(
            EVENT_PIPELINE_TASK_COMPLETE, "spec-c",
            json!({ "name": "implement-auth", "duration_ms": 5000 }),
        );
        complete_ev.ts = "2026-05-20T10:05:00.000Z".to_string();
        store.append(&complete_ev).unwrap();

        let view = pipeline_state_for_spec(&store, "spec-c", None).unwrap();
        assert_eq!(view.tasks.len(), 1);
        let task = &view.tasks[0];
        assert_eq!(task.name, "implement-auth");
        assert_eq!(task.status, "completed");
        assert_eq!(task.dispatched_at.as_deref(), Some("2026-05-20T10:00:00.000Z"));
        assert_eq!(task.completed_at.as_deref(), Some("2026-05-20T10:05:00.000Z"));
        assert_eq!(task.duration_ms, Some(5000));
    }

    /// Test 5 — conflicting status events → last wins.
    #[test]
    fn ps_last_status_wins() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());
        store.append(&pipeline_ev(
            EVENT_PIPELINE_STATUS, "spec-d",
            json!({ "to": "active" }),
        )).unwrap();
        store.append(&pipeline_ev(
            EVENT_PIPELINE_STATUS, "spec-d",
            json!({ "to": "completed" }),
        )).unwrap();

        let view = pipeline_state_for_spec(&store, "spec-d", None).unwrap();
        assert_eq!(view.status.as_deref(), Some("completed"));
    }

    /// Test 6 — stale dispatch_failure (>10 min old) → cleared in view.
    #[test]
    fn ps_stale_dispatch_failure_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());
        // Use a timestamp far in the past (2020-01-01) to guarantee staleness.
        store.append(&pipeline_ev(
            EVENT_PIPELINE_DISPATCH_FAILURE, "spec-e",
            json!({ "reason": "timeout", "at": "2020-01-01T00:00:00.000Z" }),
        )).unwrap();

        let view = pipeline_state_for_spec(&store, "spec-e", None).unwrap();
        assert!(view.last_dispatch_failure.is_none(), "stale failure should be cleared");
    }

    /// Test 7 — fresh dispatch_failure (<10 min old) → preserved in view.
    #[test]
    fn ps_fresh_dispatch_failure_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());

        // Use a recent timestamp relative to now. We compute "now - 30s" as a
        // known-good recent time by querying system time.
        let now_ms = crate::util::now_millis() as i64;
        let recent_secs = (now_ms / 1000) - 30; // 30 seconds ago
        let y = 1970u64 + (recent_secs as u64 / 31_536_000);
        // Rough but sufficient: just use a very recent ISO string close to now.
        // The most reliable approach: build from known-good recent milliseconds.
        // We use the `now_iso8601` helper from util to get the current time as the
        // failure timestamp — this guarantees it's always fresh.
        let recent_ts = crate::util::now_iso8601();
        let _ = y; // suppress warning

        store.append(&pipeline_ev(
            EVENT_PIPELINE_DISPATCH_FAILURE, "spec-f",
            json!({ "reason": "budget exceeded", "at": recent_ts }),
        )).unwrap();

        let view = pipeline_state_for_spec(&store, "spec-f", None).unwrap();
        assert!(view.last_dispatch_failure.is_some(), "fresh failure should be preserved");
        assert_eq!(
            view.last_dispatch_failure.as_ref().unwrap().reason.as_deref(),
            Some("budget exceeded"),
        );
    }

    /// Test — pipeline.complete sets closed_at and affected_files in the view.
    #[test]
    fn ps_pipeline_complete_sets_closed_at_and_files() {
        use mustard_core::model::event::EVENT_PIPELINE_COMPLETE;
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());
        store.append(&pipeline_ev(
            EVENT_PIPELINE_STATUS, "spec-complete",
            json!({ "to": "closed-followup" }),
        )).unwrap();
        store.append(&pipeline_ev(
            EVENT_PIPELINE_COMPLETE, "spec-complete",
            json!({
                "closedAt": "2026-05-20T12:00:00.000Z",
                "affectedFiles": ["src/foo.rs", "src/bar.rs"]
            }),
        )).unwrap();

        let view = pipeline_state_for_spec(&store, "spec-complete", None).unwrap();
        assert_eq!(view.status.as_deref(), Some("closed-followup"));
        assert_eq!(view.closed_at.as_deref(), Some("2026-05-20T12:00:00.000Z"));
        assert_eq!(view.affected_files, vec!["src/foo.rs", "src/bar.rs"]);
    }

    /// Test — closed_at fallback: status==closed-followup but no pipeline.complete event.
    #[test]
    fn ps_closed_at_falls_back_to_last_status_ts() {
        use mustard_core::model::event::EVENT_PIPELINE_COMPLETE;
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());
        // Emit a pipeline.status event with a known timestamp.
        let mut status_ev = pipeline_ev(
            EVENT_PIPELINE_STATUS, "spec-fallback",
            json!({ "to": "closed-followup" }),
        );
        status_ev.ts = "2026-05-20T09:30:00.000Z".to_string();
        store.append(&status_ev).unwrap();
        // No pipeline.complete event.

        let view = pipeline_state_for_spec(&store, "spec-fallback", None).unwrap();
        assert_eq!(view.status.as_deref(), Some("closed-followup"));
        // closed_at should fall back to the pipeline.status event's ts.
        assert_eq!(view.closed_at.as_deref(), Some("2026-05-20T09:30:00.000Z"));
        assert!(view.affected_files.is_empty(), "no affectedFiles when no complete event");

        // Sanity: if pipeline.complete IS present, it wins over the fallback.
        store.append(&pipeline_ev(
            EVENT_PIPELINE_COMPLETE, "spec-fallback",
            json!({ "closedAt": "2026-05-20T10:00:00.000Z", "affectedFiles": [] }),
        )).unwrap();
        let view2 = pipeline_state_for_spec(&store, "spec-fallback", None).unwrap();
        assert_eq!(view2.closed_at.as_deref(), Some("2026-05-20T10:00:00.000Z"));
    }

    /// Test 8 — is_wave_plan via FS when no pipeline.scope event present.
    #[test]
    fn ps_is_wave_plan_via_fs_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in_dir(dir.path());
        // Add any event so the spec is present.
        store.append(&pipeline_ev(
            EVENT_PIPELINE_STATUS, "spec-g",
            json!({ "to": "active" }),
        )).unwrap();

        // Create the spec dir with wave-plan.md.
        let spec_dir = dir.path().join("spec-dir");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("wave-plan.md"), "# Wave Plan\n").unwrap();

        let view = pipeline_state_for_spec(&store, "spec-g", Some(&spec_dir)).unwrap();
        assert_eq!(view.is_wave_plan, Some(true));
    }
}
