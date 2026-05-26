//! `mustard-rt run epic-fold` — a port of `scripts/epic-fold.js`.
//!
//! Consolidates and compacts harness events when an epic completes.
//!
//! - `--detect` scans `.pipeline-states/*.json` and lists root specs whose
//!   children are all in phase `CLOSE` (and the root itself is not).
//! - `--epic <name>` folds one such epic: aggregates its events, emits an
//!   `epic.complete` event, writes an `epic-summary` knowledge entry,
//!   transitions the root to `CLOSE`, and emits an `epic.fold` tombstone.
//!
//! Fail-open and idempotent: a second fold of the same epic is a no-op (the
//! root is already `CLOSE`, or an `epic.complete` event already exists).
//!
//! Port note: the JS version shelled to `_lib/harness-event.js` to emit events
//! and to `memory.js` to write the knowledge entry. This port emits events
//! directly through `mustard_core` and writes the knowledge entry inline (the
//! same dedup logic as the `memory` knowledge subcommand, plus the epic-
//! specific `content` / `spec_children` / `concluded_at` fields).

use crate::run::memory::upsert_knowledge_pattern;
use crate::util::now_iso8601;
use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
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

/// The uppercased phase of a pipeline-state object — derived from the SQLite
/// `pipeline.phase` event log (Wave-2 migration) keyed by the state's `spec`
/// name. The pipeline-state JSON no longer carries phase. Falls back to the
/// legacy in-JSON `phase` field for backwards compatibility with state files
/// written before the migration; new code paths should not depend on it.
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

/// Append a harness event for the given epic. Best-effort.
///
/// W5: routed through [`crate::run::event_route::emit`] — `pipeline.*` epic
/// events (the bulk of what this emitter produces) land in SQLite; anything
/// else (test-only `epic.detected`-style events) lands in the per-spec NDJSON
/// sink. The `_store` arg is preserved for caller-shape parity and ignored.
#[allow(clippy::needless_pass_by_value)]
fn emit_event(_store: &SqliteEventStore, project_dir: &str, event: &str, payload: Value, spec: &str) {
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: crate::run::env::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("epic-fold".to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload,
        spec: Some(spec.to_string()),
    };
    let _ = crate::run::event_route::emit(project_dir, &ev);
}

/// Scan `.pipeline-states/*.json` for epics ready to fold:
/// a root spec (`parent_spec == null`) with children, all children in `CLOSE`,
/// and the root itself not yet `CLOSE`.
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
        // Must be a root spec — `parent_spec` explicitly null/absent.
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

/// Upsert an `epic-summary` entry into the `knowledge_patterns` SQLite table.
/// Pattern shape: `"name: description"` — the same format used by the
/// `memory knowledge` subcommand and consumed by session_start injection.
/// Fail-open: any SQLite error is silently discarded.
fn write_knowledge_entry(
    cwd: &Path,
    name: &str,
    description: &str,
    _content: &str,
    _children: &[String],
    _concluded_at: &str,
) {
    let Ok(store) = SqliteEventStore::for_project(cwd) else {
        return;
    };
    let db_path = store.path().to_path_buf();
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return;
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    let pattern = format!("{name}: {description}");
    let now = now_iso8601();
    let _ = upsert_knowledge_pattern(&conn, &pattern, 0.85, Some("epic-fold"), &now, &now);
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

    // Fail-open: a store that cannot be opened means no fold this run.
    let Ok(store) = SqliteEventStore::for_project(cwd) else {
        return false;
    };
    let events = store.replay().unwrap_or_default();

    // Idempotency 2: an `epic.complete` event already exists for this epic.
    let already_complete = events.iter().any(|e| {
        e.event == "epic.complete"
            && e.payload.get("epic").and_then(Value::as_str) == Some(epic)
    });
    if already_complete {
        // The canonical CLOSE marker is the `pipeline.phase` event; the
        // pipeline-state JSON is no longer written (Wave 6b migration).
        emit_event(
            &store,
            cwd.to_string_lossy().as_ref(),
            "pipeline.phase",
            json!({ "from": null, "to": "CLOSE" }),
            epic,
        );
        return true;
    }

    // Aggregate events for the epic + its children.
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

    for ev in &events {
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

    // Top 3 findings by descending confidence.
    finding_events.sort_by(|a, b| {
        let ca = a.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        let cb = b.payload.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });
    let top3: Vec<&&HarnessEvent> = finding_events.iter().take(3).collect();

    // Emit `epic.complete`.
    emit_event(
        &store,
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

    // Build the knowledge-entry content.
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
        &format!(
            "Epic concluded with {} child spec(s): {}",
            children.len(),
            children.join(", ")
        ),
        &content_parts.join("\n\n"),
        &children,
        &ended_at,
    );

    // Transition the root to CLOSE. The canonical marker is the
    // `pipeline.phase` event; the pipeline-state JSON is no longer written
    // (Wave 6b migration — phase lives in SQLite pipeline.phase events).
    emit_event(
        &store,
        cwd.to_string_lossy().as_ref(),
        "pipeline.phase",
        json!({ "from": null, "to": "CLOSE" }),
        epic,
    );

    // Emit the `epic.fold` tombstone.
    let mut compactable = vec![epic.to_string()];
    compactable.extend(children.iter().cloned());
    emit_event(
        &store,
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
    fn fold_transitions_root_to_close_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let states = dir.path().join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        write_state(
            &states,
            "epic",
            json!({ "spec": "epic", "parent_spec": null, "children_specs": [], "phase": "EXECUTE" }),
        );
        assert!(fold_epic(dir.path(), "epic"));
        let state = read_json(&states.join("epic.json")).unwrap();
        // After fold the canonical phase is the `pipeline.phase` event;
        // the in-JSON `phase` is the human-readable shape.
        assert_eq!(state_phase(&state, dir.path()), "CLOSE");
        // Second fold is a no-op success.
        assert!(fold_epic(dir.path(), "epic"));
    }

    #[test]
    fn fold_missing_epic_returns_false() {
        let dir = tempdir().unwrap();
        assert!(!fold_epic(dir.path(), "ghost"));
    }

    // --- Wave-3a: state_phase fail-open when no SQLite events exist ----------

    #[test]
    fn state_phase_falls_back_to_json_field_when_no_events() {
        // When the SQLite store has no events for the spec, `state_phase` falls
        // back to the in-JSON `phase` field (backward compat for legacy state
        // files). This is the fail-open path.
        let dir = tempdir().unwrap();
        let state = serde_json::json!({ "spec": "epic-x", "phase": "EXECUTE" });
        // No DB → last_phase_for_spec returns None → falls back to JSON field.
        assert_eq!(state_phase(&state, dir.path()), "EXECUTE");
    }

    #[test]
    fn state_phase_prefers_event_over_json_field() {
        use mustard_core::store::event_store::EventSink;
        use mustard_core::store::sqlite_store::SqliteEventStore;
        use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};

        let dir = tempdir().unwrap();
        let db_path = dir.path().join(".claude").join(".harness").join("mustard.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let store = SqliteEventStore::new(&db_path).unwrap();
        store
            .append(&HarnessEvent {
                v: SCHEMA_VERSION,
                ts: "2026-05-20T10:00:00.000Z".to_string(),
                session_id: "s1".to_string(),
                wave: 0,
                actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
                event: "pipeline.phase".to_string(),
                payload: serde_json::json!({ "from": "EXECUTE", "to": "QA" }),
                spec: Some("epic-y".to_string()),
            })
            .unwrap();

        // JSON says EXECUTE but the SQLite event says QA → event wins.
        let state = serde_json::json!({ "spec": "epic-y", "phase": "EXECUTE" });
        assert_eq!(state_phase(&state, dir.path()), "QA");
    }
}
