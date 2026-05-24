//! `mustard-rt run spec-link` — a port of `scripts/spec-link.js`.
//!
//! Links a child spec to a parent spec (parent/child epic hierarchy):
//!
//! 1. Emits a `spec.link` harness event.
//! 2. Updates `.pipeline-states/{parent}.json`: adds the child to
//!    `children_specs` (idempotent).
//! 3. Updates `.pipeline-states/{child}.json`: sets `parent_spec` (creating a
//!    placeholder when absent).
//!
//! Port note: the JS version shelled to `_lib/harness-event.js` to emit the
//! event. This port appends the event directly through `mustard_core`.

use crate::run::env::session_id;
use crate::util::now_iso8601;
use mustard_core::fs;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::path::Path;

/// Read a pipeline-state file, returning `None` on any error.
fn read_state(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write a pipeline-state file (pretty JSON + trailing newline). Fail-soft.
fn write_state(path: &Path, value: &Value) -> bool {
    match serde_json::to_string_pretty(value) {
        Ok(text) => fs::write_atomic(path, format!("{text}\n").as_bytes()).is_ok(),
        Err(_) => false,
    }
}

/// Emit a `spec.link` harness event. Best-effort.
///
/// **Spec attribution:** the event is attributed to the *child* spec — that is
/// the spec receiving the link (a follow-up, sub-feature, or addendum). The
/// parent shows up in the payload (`parent`, `reason`) so projections that
/// walk the lineage still have both names. Pre-2026-05-20 this event left
/// `spec = NULL`, which made `spec.link` rows invisible to projections that
/// filter by spec slug.
fn emit_link_event(cwd: &Path, parent: &str, child: &str, reason: &str) {
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("spec-link".to_string()),
            actor_type: None,
        },
        event: "spec.link".to_string(),
        payload: json!({ "parent": parent, "child": child, "reason": reason }),
        spec: Some(child.to_string()),
    };
    let _ = SqliteEventStore::for_project(cwd).and_then(|store| store.append(&ev));
}

/// Core link logic. Returns `true` when the link was applied (fail-open).
fn link_spec(cwd: &Path, parent: &str, child: &str, reason: &str) -> bool {
    let parent = parent.trim();
    let child = child.trim();
    if parent.is_empty() || child.is_empty() {
        eprintln!("[spec-link] warn: --parent and --child are required");
        return false;
    }

    emit_link_event(cwd, parent, child, reason);

    let states_dir = cwd.join(".claude").join(".pipeline-states");

    // Parent state — append the child to `children_specs` idempotently.
    let parent_file = states_dir.join(format!("{parent}.json"));
    let mut parent_state = read_state(&parent_file).unwrap_or_else(|| {
        json!({ "spec": parent, "parent_spec": Value::Null, "children_specs": [] })
    });
    if let Some(obj) = parent_state.as_object_mut() {
        if !obj.contains_key("parent_spec") {
            obj.insert("parent_spec".to_string(), Value::Null);
        }
        let children = obj
            .entry("children_specs")
            .or_insert_with(|| json!([]));
        if !children.is_array() {
            *children = json!([]);
        }
        if let Some(arr) = children.as_array_mut() {
            let present = arr.iter().any(|v| v.as_str() == Some(child));
            if !present {
                arr.push(json!(child));
            }
        }
    }
    write_state(&parent_file, &parent_state);

    // Child state — set `parent_spec`.
    let child_file = states_dir.join(format!("{child}.json"));
    let mut child_state = read_state(&child_file).unwrap_or_else(|| {
        json!({ "spec": child, "parent_spec": parent, "children_specs": [] })
    });
    if let Some(obj) = child_state.as_object_mut() {
        if !obj.get("children_specs").is_some_and(Value::is_array) {
            obj.insert("children_specs".to_string(), json!([]));
        }
        obj.insert("parent_spec".to_string(), json!(parent));
    }
    write_state(&child_file, &child_state);

    true
}

/// Dispatch `mustard-rt run spec-link`.
pub fn run(parent: Option<&str>, child: Option<&str>, reason: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let (Some(parent), Some(child)) = (parent, child) else {
        eprintln!("Usage: spec-link --parent <epic> --child <sub> --reason \"<text>\"");
        return;
    };
    let reason = reason.unwrap_or("");
    let ok = link_spec(&cwd, parent, child, reason);
    println!(
        "{}",
        json!({ "ok": ok, "parent": parent, "child": child, "reason": reason })
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn link_creates_and_updates_states() {
        let dir = tempdir().unwrap();
        assert!(link_spec(dir.path(), "epic", "child-1", "split"));
        let states = dir.path().join(".claude").join(".pipeline-states");
        let parent = read_state(&states.join("epic.json")).unwrap();
        assert_eq!(
            parent["children_specs"],
            json!(["child-1"])
        );
        let child = read_state(&states.join("child-1.json")).unwrap();
        assert_eq!(child["parent_spec"], json!("epic"));
    }

    #[test]
    fn link_is_idempotent() {
        let dir = tempdir().unwrap();
        link_spec(dir.path(), "epic", "child-1", "split");
        link_spec(dir.path(), "epic", "child-1", "split");
        let states = dir.path().join(".claude").join(".pipeline-states");
        let parent = read_state(&states.join("epic.json")).unwrap();
        assert_eq!(parent["children_specs"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn link_rejects_empty_args() {
        let dir = tempdir().unwrap();
        assert!(!link_spec(dir.path(), "", "child", "r"));
    }
}
