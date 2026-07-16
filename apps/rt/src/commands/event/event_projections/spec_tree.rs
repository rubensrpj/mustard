//! `spec-tree` projection. Extracted from `event_projections` (F3 PERF-D split).

use crate::util::json_io;
use mustard_core::ClaudePaths;
use mustard_core::domain::model::event::HarnessEvent;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// `spec-tree` recursion depth cap (`MAX_SPEC_TREE_DEPTH`).
const MAX_SPEC_TREE_DEPTH: u32 = 3;

/// Read a `.pipeline-states/<name>.json` file, `None` on any error.
fn read_state(states_dir: &Path, name: &str) -> Option<Value> {
    json_io::read_json(&states_dir.join(format!("{name}.json")))
}

/// `buildSpecTree` — the recursive parent/child spec hierarchy (max depth 3),
/// combining `spec.link` events with on-disk `.pipeline-states` files. Phase
/// per node derives from `pipeline.phase` events, not the JSON.
pub(super) fn build_spec_tree(events: &[HarnessEvent], cwd: &Path, root_spec: &str) -> Value {
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
    let phase = super::phase_from_events(events, spec);
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
}
