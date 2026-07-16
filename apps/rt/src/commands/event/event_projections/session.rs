//! `session-summary` + `cross-session-timeline` projections. Extracted from
//! `event_projections` (F3 PERF-D split).

use mustard_core::ClaudePaths;
use mustard_core::io::fs;
use mustard_core::domain::model::event::HarnessEvent;
use serde_json::{json, Value};
use std::path::Path;

/// `buildSessionSummary` — roll-up over a whole session's events.
pub(super) fn build_session_summary(events: &[HarnessEvent]) -> Value {
    let mut session_id: Option<String> = None;
    let mut started_at: Option<String> = None;
    let mut ended_at: Option<String> = None;
    let mut agent_count = 0i64;
    let mut tool_count = 0i64;
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
        "findings": [],
        "decisions": decisions,
        "lessons": lessons,
        "hygiene": hygiene,
    })
}

/// `buildCrossSessionTimeline` — per-session summaries for the most-recent
/// `limit` files under `.harness/sessions/`, newest first by mtime. Each
/// summary is enriched with `epicInfo` for specs that have `children_specs`.
pub(super) fn build_cross_session_timeline(cwd: &Path, limit: usize) -> Value {
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
        // Enrich each spec that has children with epic metadata. Children come
        // from this session's `spec.link` events (the single source post-W4C),
        // not the `.pipeline-states/{spec}.json` sidecar, which is no longer
        // written.
        let mut epic_info = serde_json::Map::new();
        if let Some(specs) = summary.get("specs").and_then(Value::as_array).cloned() {
            for spec in specs.iter().filter_map(Value::as_str) {
                let mut child_set: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for ev in &events {
                    if ev.event != "spec.link" {
                        continue;
                    }
                    if ev.payload.get("parent").and_then(Value::as_str) != Some(spec) {
                        continue;
                    }
                    if let Some(child) = ev.payload.get("child").and_then(Value::as_str) {
                        child_set.insert(child.to_string());
                    }
                }
                let children: Vec<String> = child_set.into_iter().collect();
                if children.is_empty() {
                    continue;
                }
                // Phase per child derives from `pipeline.phase` events in
                // the session log (Wave-2 migration). A child that never
                // transitioned within this session is presumed not CLOSE.
                let closed = children
                    .iter()
                    .filter(|c| {
                        super::phase_from_events(&events, c)
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

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};

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
    fn session_summary_collects_specs_and_counts() {
        let events = vec![
            ev("agent.start", Some("a"), json!({})),
            ev("tool.use", Some("b"), json!({ "tool": "Edit" })),
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
    fn cross_session_timeline_empty_when_no_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let v = build_cross_session_timeline(dir.path(), 3);
        assert_eq!(v, json!([]));
    }
}
