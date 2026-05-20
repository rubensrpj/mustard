//! `mustard-rt run memory` — a port of `scripts/memory.js`.
//!
//! A unified persistence CLI with three subcommands:
//!
//! - `agent`     → `.claude/.agent-memory/`        (rolling cap 20)
//! - `decision`  → `.claude/memory/{decisions,lessons}.json` (cap 50)
//! - `knowledge` → `.claude/knowledge.json`        (cap 200 / 80 per type)
//!
//! Input JSON arrives either via `--json '<JSON>'` (the Windows-friendly form)
//! or piped on stdin (the POSIX fallback). Exit is always `0` (fail-open).
//!
//! Port note: the `decision` subcommand emits a `decision` / `lesson` harness
//! event. The JS version shelled to `_lib/harness-event.js`; this port appends
//! the event directly through `mustard_core`.

use crate::run::env::{project_dir, session_id};
use crate::util::{now_iso8601, now_millis};
use mustard_core::io::event_store::EventSink;
use mustard_core::io::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

/// `.agent-memory/` rolling cap.
const AGENT_CAP: usize = 20;
/// `decisions.json` / `lessons.json` cap.
const DECISION_CAP: usize = 50;
/// `knowledge.json` global cap.
const KNOWLEDGE_CAP: usize = 200;
/// `knowledge.json` per-type cap.
const KNOWLEDGE_CAP_CAT: usize = 80;

/// Read the input JSON text — `--json <text>` argument, else piped stdin.
fn read_input(json_arg: Option<&str>) -> String {
    if let Some(text) = json_arg {
        return text.to_string();
    }
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

/// Resolve the project dir for an input: its `cwd` field, else the env default.
fn input_cwd(input: &Value) -> String {
    input
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(project_dir)
}

/// Truncate `text` to `max_len`, preferring a sentence boundary.
fn truncate_summary(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let slice: String = text.chars().take(max_len).collect();
    let boundary = ['.', '!', '?']
        .iter()
        .filter_map(|c| slice.rfind(*c))
        .max();
    match boundary {
        Some(b) => text.chars().take(b + 1).collect(),
        None => {
            let kept: String = text.chars().take(max_len.saturating_sub(3)).collect();
            format!("{kept}...")
        }
    }
}

/// Resolve the 8-char session prefix for an `agent` entry — the first
/// `.agent-state/*.json` `session_id`, else the OS process id.
fn resolve_session_prefix(project_dir: &Path) -> String {
    let state_dir = project_dir.join(".claude").join(".agent-state");
    if let Ok(entries) = std::fs::read_dir(&state_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") || name == "_queue.json" {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(sid) = v.get("session_id").and_then(Value::as_str) {
                        if !sid.is_empty() {
                            return sid.chars().take(8).collect();
                        }
                    }
                }
            }
        }
    }
    std::process::id().to_string()
}

/// `agent` subcommand — write an agent-memory entry, prune to [`AGENT_CAP`].
fn run_agent(input: &Value) {
    let project_dir = input_cwd(input);
    let project_dir = Path::new(&project_dir);
    let mem_dir = project_dir.join(".claude").join(".agent-memory");
    if std::fs::create_dir_all(&mem_dir).is_err() {
        return;
    }

    let session8 = resolve_session_prefix(project_dir);
    let agent_type = input
        .get("agent_type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let id = format!("{session8}-{agent_type}-{}", now_millis());
    let filename = format!("{id}.json");
    let timestamp = now_iso8601();
    let raw_summary = input
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let summary = truncate_summary(&raw_summary, 300);
    let wave = input.get("wave").and_then(Value::as_i64);
    let pipeline = input
        .get("pipeline")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let entry = json!({
        "v": 1,
        "id": id,
        "session": session8,
        "agent_type": agent_type,
        "wave": wave,
        "pipeline": pipeline,
        "timestamp": timestamp,
        "summary": summary,
        "details": input.get("details").cloned().unwrap_or_else(|| json!({})),
    });
    if let Ok(text) = serde_json::to_string_pretty(&entry) {
        let _ = std::fs::write(mem_dir.join(&filename), text);
    }

    // Rolling index.
    let index_path = mem_dir.join("_index.json");
    let mut index: Vec<Value> = std::fs::read_to_string(&index_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default();
    index.push(json!({
        "id": id,
        "file": filename,
        "agent_type": agent_type,
        "wave": wave,
        "pipeline": pipeline,
        "summary": summary,
        "timestamp": timestamp,
    }));
    if index.len() > AGENT_CAP {
        let excess = index.len() - AGENT_CAP;
        for old in index.drain(..excess) {
            if let Some(f) = old.get("file").and_then(Value::as_str) {
                let _ = std::fs::remove_file(mem_dir.join(f));
            }
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&index) {
        let _ = std::fs::write(&index_path, text);
    }
}

/// Append a `decision` / `lesson` harness event for the `decision` subcommand.
fn emit_decision_event(entry_type: &str, content: &str, context: &str, source: &str, dir: &str) {
    let head: String = content.chars().take(200).collect();
    let (event, payload) = if entry_type == "decision" {
        (
            "decision",
            json!({
                "title": head,
                "rationale": if context.is_empty() { Value::Null } else { json!(context) },
            }),
        )
    } else {
        (
            "lesson",
            json!({
                "trigger": if source.is_empty() { Value::Null } else { json!(source) },
                "takeaway": head,
            }),
        )
    };
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("memory".to_string()),
            actor_type: None,
        },
        event: event.to_string(),
        payload,
        spec: None,
    };
    let _ = SqliteEventStore::for_project(dir).and_then(|store| store.append(&ev));
}

/// `decision` subcommand — append to `decisions.json` / `lessons.json`.
fn run_decision(input: &Value) {
    let entry_type = input
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if entry_type != "decision" && entry_type != "lesson" {
        eprintln!(
            "[memory] decision: invalid type \"{entry_type}\" — must be \"decision\" or \"lesson\""
        );
        return;
    }
    let project_dir = input_cwd(input);
    let mem_dir = Path::new(&project_dir).join(".claude").join("memory");
    if std::fs::create_dir_all(&mem_dir).is_err() {
        return;
    }
    let file_name = if entry_type == "decision" {
        "decisions.json"
    } else {
        "lessons.json"
    };
    let file_path = mem_dir.join(file_name);

    let mut data = std::fs::read_to_string(&file_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v.get("entries").map(Value::is_array).unwrap_or(false))
        .unwrap_or_else(|| json!({ "entries": [] }));

    let content = input.get("content").and_then(Value::as_str).unwrap_or("").to_string();
    let context = input.get("context").and_then(Value::as_str).unwrap_or("").to_string();
    let source = input.get("source").and_then(Value::as_str).unwrap_or("").to_string();
    let entry = json!({
        "id": format!("{entry_type}-{}", now_millis()),
        "timestamp": now_iso8601(),
        "content": content,
        "source": source,
        "context": context,
    });

    if let Some(entries) = data.get_mut("entries").and_then(Value::as_array_mut) {
        entries.push(entry);
        if entries.len() > DECISION_CAP {
            let excess = entries.len() - DECISION_CAP;
            entries.drain(..excess);
        }
    }

    emit_decision_event(&entry_type, &content, &context, &source, &project_dir);

    if let Ok(text) = serde_json::to_string_pretty(&data) {
        let _ = std::fs::write(&file_path, format!("{text}\n"));
    }
}

/// `knowledge` subcommand — upsert into `knowledge.json` with dedup + pruning.
fn run_knowledge(input: &Value) {
    let cwd = input_cwd(input);
    let kb_path = Path::new(&cwd).join(".claude").join("knowledge.json");

    let mut kb = std::fs::read_to_string(&kb_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| json!({ "version": 1, "entries": [] }));
    if kb.get("entries").is_none() {
        if let Some(obj) = kb.as_object_mut() {
            obj.insert("entries".to_string(), json!([]));
        }
    }

    let entry_type = input.get("type").and_then(Value::as_str).unwrap_or("pattern").to_string();
    let name = input.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let description = input
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let source = input.get("source").and_then(Value::as_str).unwrap_or("unknown").to_string();
    let tags = input.get("tags").cloned().filter(Value::is_array).unwrap_or_else(|| json!([]));
    let initial_confidence = input
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|&c| (0.0..=1.0).contains(&c))
        .unwrap_or(0.3);

    if name.is_empty() || description.is_empty() {
        eprintln!("[memory] knowledge: missing name or description");
        return;
    }

    let timestamp = now_iso8601();
    // `entries` was ensured to be an array above; bail fail-open if not.
    let Some(entries) = kb.get_mut("entries").and_then(Value::as_array_mut) else {
        return;
    };

    let existing_idx = entries.iter().position(|e| {
        e.get("name").and_then(Value::as_str) == Some(name.as_str())
            && e.get("type").and_then(Value::as_str) == Some(entry_type.as_str())
    });

    if let Some(idx) = existing_idx {
        let e = &mut entries[idx];
        if let Some(obj) = e.as_object_mut() {
            let prev_occ = obj
                .get("occurrences")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let occ = prev_occ + 1;
            obj.insert("description".to_string(), json!(description));
            obj.insert("source".to_string(), json!(source));
            obj.insert("tags".to_string(), tags);
            obj.insert("updatedAt".to_string(), json!(timestamp));
            obj.insert("occurrences".to_string(), json!(occ));
            obj.insert(
                "confidence".to_string(),
                json!(f64::min(1.0, 0.3 + (occ as f64) * 0.1)),
            );
            obj.insert("lastSeen".to_string(), json!(timestamp));
        }
    } else {
        entries.push(json!({
            "id": format!("{entry_type}-{}", now_millis()),
            "type": entry_type,
            "name": name,
            "description": description,
            "source": source,
            "tags": tags,
            "confidence": initial_confidence,
            "occurrences": 1,
            "createdAt": timestamp,
            "updatedAt": timestamp,
            "lastSeen": timestamp,
            "verifiedAt": null,
            "sourceFiles": [],
        }));
    }

    prune_knowledge(entries);

    if let Some(parent) = kb_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&kb) {
        let _ = std::fs::write(&kb_path, text);
    }
}

/// Sort key for a knowledge entry — `updatedAt` else `createdAt`.
fn entry_sort_key(e: &Value) -> String {
    e.get("updatedAt")
        .and_then(Value::as_str)
        .or_else(|| e.get("createdAt").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

/// Prune `entries` per type to [`KNOWLEDGE_CAP_CAT`] then globally to
/// [`KNOWLEDGE_CAP`], newest first.
fn prune_knowledge(entries: &mut Vec<Value>) {
    use std::collections::BTreeMap;
    let mut by_type: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for e in entries.drain(..) {
        let t = e.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        by_type.entry(t).or_default().push(e);
    }
    let mut pruned: Vec<Value> = Vec::new();
    for (_, mut group) in by_type {
        group.sort_by(|a, b| entry_sort_key(b).cmp(&entry_sort_key(a)));
        group.truncate(KNOWLEDGE_CAP_CAT);
        pruned.extend(group);
    }
    pruned.sort_by(|a, b| entry_sort_key(b).cmp(&entry_sort_key(a)));
    pruned.truncate(KNOWLEDGE_CAP);
    *entries = pruned;
}

/// Dispatch `mustard-rt run memory <subcommand>`.
pub fn run(subcommand: &str, json_arg: Option<&str>) {
    if !matches!(subcommand, "agent" | "decision" | "knowledge") {
        println!("Usage: memory <agent|decision|knowledge> [--json '<JSON>']");
        return;
    }
    let raw = read_input(json_arg);
    let input: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("[memory] Failed to parse input JSON: {err}");
            return;
        }
    };
    match subcommand {
        "agent" => run_agent(&input),
        "decision" => run_decision(&input),
        "knowledge" => run_knowledge(&input),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn truncate_summary_prefers_sentence_boundary() {
        let text = "First sentence. Second sentence that is quite long indeed.";
        let out = truncate_summary(text, 20);
        assert!(out.ends_with('.'));
        assert!(out.len() <= text.len());
    }

    #[test]
    fn agent_writes_entry_and_index() {
        let dir = tempdir().unwrap();
        let input = json!({
            "cwd": dir.path().to_string_lossy(),
            "agent_type": "backend",
            "wave": 2,
            "pipeline": "demo",
            "summary": "did the thing",
        });
        run_agent(&input);
        let index = dir.path().join(".claude").join(".agent-memory").join("_index.json");
        let parsed: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(index).unwrap()).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["agent_type"], json!("backend"));
    }

    #[test]
    fn decision_appends_to_decisions_json() {
        let dir = tempdir().unwrap();
        let input = json!({
            "cwd": dir.path().to_string_lossy(),
            "type": "decision",
            "content": "chose X over Y",
            "source": "spec-1",
        });
        run_decision(&input);
        let p = dir.path().join(".claude").join("memory").join("decisions.json");
        let data: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert_eq!(data["entries"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn knowledge_upsert_bumps_occurrences() {
        let dir = tempdir().unwrap();
        let mk = || json!({
            "cwd": dir.path().to_string_lossy(),
            "type": "pattern",
            "name": "repo-pattern",
            "description": "use a repository",
        });
        run_knowledge(&mk());
        run_knowledge(&mk());
        let p = dir.path().join(".claude").join("knowledge.json");
        let kb: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let entries = kb["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["occurrences"], json!(2));
    }

    #[test]
    fn prune_knowledge_caps_per_type() {
        let mut entries: Vec<Value> = (0..KNOWLEDGE_CAP_CAT + 10)
            .map(|i| json!({ "type": "pattern", "name": format!("p{i}"), "createdAt": format!("2026-01-{:02}", i % 28 + 1) }))
            .collect();
        prune_knowledge(&mut entries);
        assert_eq!(entries.len(), KNOWLEDGE_CAP_CAT);
    }
}
