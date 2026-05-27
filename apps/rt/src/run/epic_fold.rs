//! `mustard-rt run epic-fold` — consolidate and compact harness events when
//! an epic completes.
//!
//! - `--detect` scans `.pipeline-states/*.json` and lists root specs whose
//!   children are all in phase `CLOSE` (and the root itself is not).
//! - `--epic <name>` folds one such epic: aggregates events for the epic + its
//!   children, emits an `epic.complete` event, writes an `epic-summary`
//!   knowledge entry (markdown), transitions the root to `CLOSE`, and emits
//!   an `epic.fold` tombstone.
//!
//! W4C migration: event aggregation reads per-spec NDJSON via
//! [`mustard_core::EventReader::stream`]; the `epic-summary` knowledge entry
//! is written as `.claude/knowledge/epic-{epic}.md` via
//! [`mustard_core::atomic_md::MarkdownStore`].
//!
//! Fail-open and idempotent.

use crate::util::now_iso8601;
use mustard_core::atomic_md::frontmatter::Frontmatter;
use mustard_core::atomic_md::{MarkdownDoc, MarkdownStore};
use mustard_core::fs;
use mustard_core::model::event::HarnessEvent;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};
use std::path::Path;

/// Read a JSON file, returning `None` on any error.
fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write a JSON value pretty-printed with a trailing newline. Fail-soft.
#[cfg(test)]
fn write_json(path: &Path, value: &Value) -> bool {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(value) {
        Ok(text) => std::fs::write(path, format!("{text}\n")).is_ok(),
        Err(_) => false,
    }
}

/// The uppercased phase of a pipeline-state object — derived from the
/// `pipeline.phase` event log keyed by the state's `spec` name. Falls back to
/// the legacy in-JSON `phase` field for backwards compatibility.
fn state_phase(state: &Value, cwd: &Path) -> String {
    if let Some(spec) = state.get("spec").and_then(Value::as_str) {
        if let Some(phase) = crate::run::emit_phase::last_phase_for_spec(cwd, spec) {
            return phase.to_uppercase();
        }
    }
    state
        .get("phase")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_uppercase()
}

/// Read every harness event for `spec` from its per-spec NDJSON sink.
fn read_events_for_spec(cwd: &Path, spec: &str) -> Vec<HarnessEvent> {
    let Ok(cp) = ClaudePaths::for_project(cwd) else {
        return Vec::new();
    };
    let Ok(sp) = cp.for_spec(spec) else {
        return Vec::new();
    };
    mustard_core::projection::read_harness_events_from_ndjson_dir(&sp.events_dir())
}

/// Append a harness event for the given epic via the NDJSON route. Best-effort.
fn emit_event(project_dir: &str, event: &str, payload: Value, spec: &str) {
    let ts = now_iso8601();
    let sid = crate::run::env::session_id();
    let kind = crate::run::event_route::classify_kind(event);
    let _ = crate::run::event_writer_ndjson::write_event_with_ts(
        Path::new(project_dir),
        Some(spec),
        None,
        &sid,
        event,
        kind,
        Some(0),
        Some(&sid),
        Some("epic-fold"),
        None,
        &payload,
        Some(&ts),
    );
}

/// Scan `.pipeline-states/*.json` for epics ready to fold.
fn detect_completed_epics(cwd: &Path) -> Vec<String> {
    let states_dir = ClaudePaths::for_project(cwd)
        .map(|p| p.pipeline_states_dir())
        .unwrap_or_else(|_| cwd.to_path_buf());
    let Ok(entries) = fs::read_dir(&states_dir) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let name = entry.file_name.clone();
        if !name.ends_with(".json") {
            continue;
        }
        let Some(state) = read_json(&entry.path) else {
            continue;
        };
        match state.get("parent_spec") {
            None | Some(Value::Null) => {}
            Some(_) => continue,
        }
        let children: Vec<&str> = state
            .get("children_specs")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if children.is_empty() {
            continue;
        }
        if state_phase(&state, cwd) == "CLOSE" {
            continue;
        }
        let all_closed = children.iter().all(|child| {
            let child_file = states_dir.join(format!("{child}.json"));
            read_json(&child_file)
                .is_some_and(|cs| state_phase(&cs, cwd) == "CLOSE")
        });
        if all_closed {
            let spec = state
                .get("spec")
                .and_then(Value::as_str)
                .map_or_else(|| name.trim_end_matches(".json").to_string(), str::to_string);
            candidates.push(spec);
        }
    }
    candidates
}

/// Write an `epic-summary` markdown file under `.claude/knowledge/`.
fn write_knowledge_entry(
    cwd: &Path,
    epic: &str,
    name: &str,
    description: &str,
    content: &str,
    children: &[String],
    concluded_at: &str,
) {
    let Ok(cp) = ClaudePaths::for_project(cwd) else {
        return;
    };
    let dir = cp.claude_dir().join("knowledge");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let dest = dir.join(format!("epic-{epic}.md"));
    let mut fm = Map::new();
    fm.insert("kind".into(), json!("epic-summary"));
    fm.insert("name".into(), json!(name));
    fm.insert("confidence".into(), json!(0.85));
    fm.insert("source".into(), json!("epic-fold"));
    fm.insert("concluded_at".into(), json!(concluded_at));
    fm.insert(
        "spec_children".into(),
        json!(children.iter().cloned().collect::<Vec<_>>()),
    );
    fm.insert("status".into(), json!("active"));
    let body = format!("{description}\n\n{content}\n");
    let doc = MarkdownDoc {
        path: dest.clone(),
        frontmatter: Some(Frontmatter(Value::Object(fm))),
        body,
    };
    let _ = MarkdownStore::write_atomic(&dest, &doc);
}

/// Fold an epic — returns `true` on success (or when already folded).
fn fold_epic(cwd: &Path, epic: &str) -> bool {
    if epic.is_empty() {
        eprintln!("[epic-fold] warn: --epic is required");
        return false;
    }
    let paths = ClaudePaths::for_project(cwd).ok();
    let states_dir = paths
        .as_ref()
        .map(ClaudePaths::pipeline_states_dir)
        .unwrap_or_else(|| cwd.to_path_buf());
    let epic_file = paths
        .as_ref()
        .map(|p| p.pipeline_state_file(epic))
        .unwrap_or_else(|| states_dir.join(format!("{epic}.json")));
    let Some(epic_state) = read_json(&epic_file) else {
        eprintln!("[epic-fold] warn: pipeline-state not found for epic \"{epic}\"");
        return false;
    };
    let children: Vec<String> = epic_state
        .get("children_specs")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();

    // Idempotency 1: root already CLOSE.
    if state_phase(&epic_state, cwd) == "CLOSE" {
        return true;
    }

    // Aggregate events for the epic + its children via per-spec NDJSON sinks.
    let mut all_events: Vec<HarnessEvent> = read_events_for_spec(cwd, epic);
    for child in &children {
        all_events.extend(read_events_for_spec(cwd, child));
    }

    // Idempotency 2: an `epic.complete` event already exists for this epic.
    let already_complete = all_events.iter().any(|e| {
        e.event == "epic.complete"
            && e.payload.get("epic").and_then(Value::as_str) == Some(epic)
    });
    if already_complete {
        emit_event(
            cwd.to_string_lossy().as_ref(),
            "pipeline.phase",
            json!({ "from": null, "to": "CLOSE" }),
            epic,
        );
        return true;
    }

    let spec_set: std::collections::BTreeSet<&str> = std::iter::once(epic)
        .chain(children.iter().map(String::as_str))
        .collect();
    let mut findings_count = 0usize;
    let mut decisions_count = 0usize;
    let mut lessons_count = 0usize;
    let mut tool_calls_total = 0usize;
    let mut agents_total = 0usize;
    let mut min_ts: Option<String> = None;
    let mut max_ts: Option<String> = None;
    let mut finding_events: Vec<&HarnessEvent> = Vec::new();

    for ev in &all_events {
        let Some(spec) = ev.spec.as_deref() else {
            continue;
        };
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
            "finding" => {
                findings_count += 1;
                finding_events.push(ev);
            }
            "decision" => decisions_count += 1,
            "lesson" => lessons_count += 1,
            "tool.use" => tool_calls_total += 1,
            "agent.start" => agents_total += 1,
            _ => {}
        }
    }

    let started_at = min_ts.clone().unwrap_or_else(now_iso8601);
    let ended_at = max_ts.clone().unwrap_or_else(now_iso8601);
    let duration_ms = match (
        min_ts.as_deref().and_then(crate::run::complete_spec::parse_iso_millis),
        max_ts.as_deref().and_then(crate::run::complete_spec::parse_iso_millis),
    ) {
        (Some(a), Some(b)) => (b - a).max(0),
        _ => 0,
    };

    finding_events.sort_by(|a, b| {
        let ca = a.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        let cb = b.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });
    let top3: Vec<&&HarnessEvent> = finding_events.iter().take(3).collect();

    emit_event(
        cwd.to_string_lossy().as_ref(),
        "epic.complete",
        json!({
            "epic": epic,
            "children": children,
            "findings_count": findings_count,
            "decisions_count": decisions_count,
            "lessons_count": lessons_count,
            "tool_calls_total": tool_calls_total,
            "agents_total": agents_total,
            "duration_ms": duration_ms,
            "started_at": started_at,
            "ended_at": ended_at,
        }),
        epic,
    );

    let finding_lines: Vec<String> = top3
        .iter()
        .enumerate()
        .map(|(i, fev)| {
            let content = fev.payload.get("content").and_then(Value::as_str).unwrap_or("");
            let conf = fev
                .payload
                .get("confidence")
                .and_then(Value::as_f64)
                .map_or_else(|| "?".to_string(), |c| format!("{c:.2}"));
            format!("{}. [conf={conf}] {content}", i + 1)
        })
        .collect();
    let mut content_parts: Vec<String> = Vec::new();
    if !finding_lines.is_empty() {
        content_parts.push(format!("Top findings:\n{}", finding_lines.join("\n")));
    }
    content_parts.push(format!("Decisions: {decisions_count}"));
    content_parts.push(format!("Lessons: {lessons_count}"));

    write_knowledge_entry(
        cwd,
        epic,
        epic,
        &format!(
            "Epic concluded with {} child spec(s): {}",
            children.len(),
            children.join(", ")
        ),
        &content_parts.join("\n\n"),
        &children,
        &ended_at,
    );

    emit_event(
        cwd.to_string_lossy().as_ref(),
        "pipeline.phase",
        json!({ "from": null, "to": "CLOSE" }),
        epic,
    );

    let mut compactable = vec![epic.to_string()];
    compactable.extend(children.iter().cloned());
    emit_event(
        cwd.to_string_lossy().as_ref(),
        "epic.fold",
        json!({
            "epic": epic,
            "compactable_specs": compactable,
            "folded_at": now_iso8601(),
        }),
        epic,
    );
    true
}

/// Dispatch `mustard-rt run epic-fold`.
pub fn run(detect: bool, epic: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    if detect {
        let epics = detect_completed_epics(&cwd);
        let out = json!({ "epics_ready": epics });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string()));
        return;
    }
    if let Some(epic) = epic {
        let ok = fold_epic(&cwd, epic);
        println!("{}", json!({ "ok": ok, "epic": epic }));
        return;
    }
    eprintln!("Usage:");
    eprintln!("  epic-fold --detect");
    eprintln!("  epic-fold --epic <name>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_state(states: &Path, name: &str, value: Value) {
        write_json(&states.join(format!("{name}.json")), &value);
    }

    #[test]
    fn detect_finds_epic_with_all_children_closed() {
        let dir = tempdir().unwrap();
        let states = dir.path().join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        write_state(
            &states,
            "epic",
            json!({ "spec": "epic", "parent_spec": null, "children_specs": ["c1"], "phase": "EXECUTE" }),
        );
        write_state(&states, "c1", json!({ "spec": "c1", "phase": "CLOSE" }));
        assert_eq!(detect_completed_epics(dir.path()), vec!["epic".to_string()]);
    }

    #[test]
    fn detect_skips_when_a_child_is_not_closed() {
        let dir = tempdir().unwrap();
        let states = dir.path().join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        write_state(
            &states,
            "epic",
            json!({ "spec": "epic", "parent_spec": null, "children_specs": ["c1"], "phase": "EXECUTE" }),
        );
        write_state(&states, "c1", json!({ "spec": "c1", "phase": "QA" }));
        assert!(detect_completed_epics(dir.path()).is_empty());
    }

    #[test]
    fn fold_missing_epic_returns_false() {
        let dir = tempdir().unwrap();
        assert!(!fold_epic(dir.path(), "ghost"));
    }

    #[test]
    fn state_phase_falls_back_to_json_field_when_no_events() {
        let dir = tempdir().unwrap();
        let state = serde_json::json!({ "spec": "epic-x", "phase": "EXECUTE" });
        assert_eq!(state_phase(&state, dir.path()), "EXECUTE");
    }
}
