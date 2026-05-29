//! `mustard-rt run spec-link` — link a child spec to a parent spec
//! (parent/child epic hierarchy).
//!
//! Emits a `spec.link` harness event (`{ parent, child, reason }`) attributed
//! to the child spec. That NDJSON event — together with the child's
//! `### Parent:` spec-md header — is the **single source of truth** for the
//! parent→child edge: `epic-fold`, `spec-children`, `spec-tree` and
//! `epic-summary` all reconstruct lineage from `spec.link` events.
//!
//! F4-f: the legacy `.pipeline-states/{parent,child}.json` sidecar writes
//! (`children_specs` / `parent_spec`) were removed. Nothing in the runtime
//! reads them post-W4C — every consumer now derives the edge from the event
//! stream — so writing them was dead duplication of state.

use crate::shared::context::session_id;
use mustard_core::time::now_iso8601;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;
use std::path::Path;

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
    // `spec.link` is non-pipeline → per-spec NDJSON via the W5 router.
    let _ = crate::shared::events::route::emit(cwd.to_string_lossy().as_ref(), &ev);
}

/// Core link logic. Returns `true` when the link was applied (fail-open).
///
/// The link is recorded **only** as a `spec.link` NDJSON event — the parent→
/// child edge is reconstructed from the event stream by every consumer. No
/// `.pipeline-states` sidecar is written (F4-f: that was dead duplicate state).
fn link_spec(cwd: &Path, parent: &str, child: &str, reason: &str) -> bool {
    let parent = parent.trim();
    let child = child.trim();
    if parent.is_empty() || child.is_empty() {
        eprintln!("[spec-link] warn: --parent and --child are required");
        return false;
    }

    emit_link_event(cwd, parent, child, reason);
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
    use mustard_core::ClaudePaths;
    use mustard_core::view::projection::read_workspace_events;
    use tempfile::tempdir;

    #[test]
    fn link_emits_event_and_writes_no_sidecar() {
        let dir = tempdir().unwrap();
        assert!(link_spec(dir.path(), "epic", "child-1", "split"));

        // The edge lives ONLY as a `spec.link` event — no pipeline-state sidecar.
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        assert!(
            !paths.pipeline_state_file("epic").exists(),
            "parent sidecar must not be written"
        );
        assert!(
            !paths.pipeline_state_file("child-1").exists(),
            "child sidecar must not be written"
        );

        let events = read_workspace_events(dir.path());
        let link = events
            .iter()
            .find(|e| e.event == "spec.link")
            .expect("a spec.link event must be emitted");
        assert_eq!(link.payload.get("parent").and_then(|v| v.as_str()), Some("epic"));
        assert_eq!(link.payload.get("child").and_then(|v| v.as_str()), Some("child-1"));
        // Attributed to the child spec (see `emit_link_event`).
        assert_eq!(link.spec.as_deref(), Some("child-1"));
    }

    #[test]
    fn link_rejects_empty_args() {
        let dir = tempdir().unwrap();
        assert!(!link_spec(dir.path(), "", "child", "r"));
    }
}
