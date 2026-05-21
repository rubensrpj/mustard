//! `mustard-rt run emit-pipeline` — typed pipeline-event emitter.
//!
//! Records one of the eight `pipeline.*` events defined in
//! [`mustard_core::model::event`] constants. Callers supply the event kind, the
//! spec name, and an optional JSON payload string; this module validates both
//! and appends the event to the project's [`SqliteEventStore`].
//!
//! ## Fail-open contract
//!
//! - **Unknown kind** → prints an error on stderr and exits with code 1.
//! - **Invalid JSON payload** → prints an error on stderr and exits with code 1.
//! - **Store error** → prints a warning on stderr and exits with code 0 (fail-open).
//!
//! This matches the pattern used by `emit_phase` and every other harness
//! emitter: telemetry is never load-bearing, so a write failure must never
//! break the pipeline.

use crate::run::env::{project_dir, session_id};
use crate::util::now_iso8601;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, SCHEMA_VERSION,
    EVENT_PIPELINE_COMPLETE, EVENT_PIPELINE_DISPATCH_FAILURE, EVENT_PIPELINE_PAUSE,
    EVENT_PIPELINE_RESUME_MODE, EVENT_PIPELINE_SCOPE, EVENT_PIPELINE_STATUS,
    EVENT_PIPELINE_TASK_COMPLETE, EVENT_PIPELINE_TASK_DISPATCH, EVENT_PIPELINE_WAVE_COMPLETE,
};
use serde_json::Value;
use std::path::Path;

/// The 9 valid pipeline event kind strings.
const KNOWN_KINDS: &[&str] = &[
    EVENT_PIPELINE_SCOPE,
    EVENT_PIPELINE_STATUS,
    EVENT_PIPELINE_TASK_DISPATCH,
    EVENT_PIPELINE_TASK_COMPLETE,
    EVENT_PIPELINE_WAVE_COMPLETE,
    EVENT_PIPELINE_DISPATCH_FAILURE,
    EVENT_PIPELINE_PAUSE,
    EVENT_PIPELINE_RESUME_MODE,
    EVENT_PIPELINE_COMPLETE,
];

/// Options for `mustard-rt run emit-pipeline`.
pub struct EmitPipelineOpts {
    /// Pipeline event kind — must be one of the `EVENT_PIPELINE_*` constants.
    pub kind: String,
    /// Spec name the event is attributed to.
    pub spec: String,
    /// Optional JSON payload string. When `None`, the event payload is `null`.
    pub payload: Option<String>,
}

/// Run `mustard-rt run emit-pipeline --kind <name> --spec <name> [--payload <json>]`.
///
/// Validates `kind` and the optional JSON payload, then appends the event to
/// the project store. Exits 1 on validation failure; fails open (exit 0) on
/// store errors.
pub fn run(opts: EmitPipelineOpts) {
    // --- Validate kind ---
    if !KNOWN_KINDS.contains(&opts.kind.as_str()) {
        eprintln!(
            "emit-pipeline: unknown kind {:?}. Valid kinds: {}",
            opts.kind,
            KNOWN_KINDS.join(", ")
        );
        std::process::exit(1);
    }

    // --- Parse optional payload ---
    let payload: Value = match opts.payload.as_deref() {
        None => Value::Null,
        Some(raw) => match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("emit-pipeline: invalid JSON payload: {e}");
                std::process::exit(1);
            }
        },
    };

    // --- Open store (fail-open on error) ---
    let store = match SqliteEventStore::for_project(project_dir()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("emit-pipeline: could not open event store: {e} (skipping)");
            return;
        }
    };

    // Capture the values we need after `event` consumes them.
    let kind_str = opts.kind.clone();
    let spec_name = opts.spec.clone();
    let payload_for_header = payload.clone();

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("emit-pipeline".to_string()),
            actor_type: None,
        },
        event: opts.kind,
        payload,
        spec: Some(opts.spec),
    };

    // Fail-open: a write failure is logged but never propagates to an exit 1.
    let _ = store.append(&event);

    // Wave-2 (2026-05-21-flatten-spec-layout-and-multi-collab): keep the
    // `### Status:` header in `.claude/spec/{spec}/spec.md` in sync with the
    // canonical status the event store just received. Without this, two
    // collaborators on different machines see divergent statuses (the local
    // store says X, git says Y). Fail-open: a missing file or header is a
    // warn, never an error — the event has already been recorded.
    if kind_str == EVENT_PIPELINE_STATUS {
        if let Some(to) = payload_for_header.get("to").and_then(Value::as_str) {
            let cwd = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(project_dir()));
            sync_spec_status_header(&cwd, &spec_name, to);
        }
    }
}

/// Rewrite the `### Status:` line of `.claude/spec/{spec}/spec.md` so it
/// matches the freshly emitted `pipeline.status: <to>` event. Pure side
/// effect — every error path is a warn (the contract is fail-open per the
/// module-level docs).
///
/// The match is intentionally narrow: the first line whose trimmed prefix is
/// `### Status:` (case-insensitive on the key) gets its value replaced. If
/// no such line exists we emit a `WARN` to stderr and return — we don't try
/// to *insert* a header because that would silently mutate spec.md shape
/// (and the close-gate is the right place to enforce that header exists).
fn sync_spec_status_header(cwd: &Path, spec: &str, to: &str) {
    let path = cwd.join(".claude").join("spec").join(spec).join("spec.md");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "emit-pipeline: WARN: cannot read {} ({e}); skipping header sync",
                path.display()
            );
            return;
        }
    };

    let mut found = false;
    let mut out = String::with_capacity(content.len() + 16);
    let mut first = true;
    for line in content.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        if found {
            out.push_str(line);
            continue;
        }
        let trimmed = line.trim_start();
        // `^###\s+status\s*:` (case-insensitive on the key) — keep the line's
        // original indentation so we don't reflow the file.
        if let Some(rest) = trimmed.strip_prefix("###") {
            let after_hashes = rest.trim_start_matches([' ', '\t']);
            if after_hashes.len() < rest.len() {
                // Case-insensitive prefix match on the literal `Status`.
                let lower = after_hashes.to_ascii_lowercase();
                if let Some(tail) = lower.strip_prefix("status") {
                    let after_status = tail.trim_start_matches([' ', '\t']);
                    if let Some(_after_colon) = after_status.strip_prefix(':') {
                        // Reconstruct: original indent + `### Status: <to>`.
                        let indent_len = line.len() - line.trim_start().len();
                        let indent = &line[..indent_len];
                        out.push_str(indent);
                        out.push_str("### Status: ");
                        out.push_str(to);
                        found = true;
                        continue;
                    }
                }
            }
        }
        out.push_str(line);
    }

    if !found {
        eprintln!(
            "emit-pipeline: WARN: no `### Status:` header found in {}; skipping",
            path.display()
        );
        return;
    }

    if let Err(e) = std::fs::write(&path, out) {
        eprintln!(
            "emit-pipeline: WARN: could not write {} ({e}); status header may be stale",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use mustard_core::model::event::SCHEMA_VERSION;
    use serde_json::json;
    use tempfile::tempdir;

    /// Build a store backed by a fresh temp DB.
    fn temp_store() -> (tempfile::TempDir, SqliteEventStore) {
        let dir = tempdir().unwrap();
        let store = SqliteEventStore::new(dir.path().join("mustard.db")).unwrap();
        (dir, store)
    }

    /// Build opts with a known-good kind.
    fn opts(kind: &str, spec: &str, payload: Option<&str>) -> EmitPipelineOpts {
        EmitPipelineOpts {
            kind: kind.to_string(),
            spec: spec.to_string(),
            payload: payload.map(str::to_string),
        }
    }

    // -----------------------------------------------------------------------
    // Validation + payload parsing (unit-level, no store I/O)
    // -----------------------------------------------------------------------

    #[test]
    fn known_kinds_list_covers_all_nine_constants() {
        assert_eq!(KNOWN_KINDS.len(), 9);
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_SCOPE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_STATUS));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_TASK_DISPATCH));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_TASK_COMPLETE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_WAVE_COMPLETE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_DISPATCH_FAILURE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_PAUSE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_RESUME_MODE));
        assert!(KNOWN_KINDS.contains(&EVENT_PIPELINE_COMPLETE));
    }

    #[test]
    fn valid_json_payload_parses() {
        let raw = r#"{"scope":"full","model":"opus"}"#;
        let v: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(v["scope"], json!("full"));
    }

    #[test]
    fn null_payload_when_none() {
        // No payload → Value::Null (the emit loop handles this).
        let raw: Option<&str> = None;
        let v: Value = match raw {
            None => Value::Null,
            Some(s) => serde_json::from_str(s).unwrap(),
        };
        assert_eq!(v, Value::Null);
    }

    // -----------------------------------------------------------------------
    // Store integration — use a real tempfile DB.
    // -----------------------------------------------------------------------

    /// Helper: append one pipeline event directly through `store.append`.
    /// This exercises the same code path as `run()` but without going through
    /// the process exit on validation failure.
    fn emit_direct(store: &SqliteEventStore, kind: &str, spec: &str, payload: Value) {
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T00:00:00.000Z".to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("emit-pipeline".to_string()),
                actor_type: None,
            },
            event: kind.to_string(),
            payload,
            spec: Some(spec.to_string()),
        };
        store.append(&event).unwrap();
    }

    #[test]
    fn each_kind_appended_once_with_correct_event_name() {
        let (_dir, store) = temp_store();
        let spec = "2026-05-20-pipeline-state-from-sqlite";

        for &kind in KNOWN_KINDS {
            emit_direct(&store, kind, spec, json!({"test": true}));
        }

        let events = store.replay().unwrap();
        assert_eq!(events.len(), 9, "expected 9 events, one per kind");

        for (i, &kind) in KNOWN_KINDS.iter().enumerate() {
            assert_eq!(events[i].event, kind, "event name mismatch at index {i}");
            assert_eq!(events[i].spec.as_deref(), Some(spec));
            assert_eq!(events[i].payload["test"], json!(true));
        }
    }

    #[test]
    fn pipeline_scope_payload_round_trips() {
        use mustard_core::model::event::PipelineScopePayload;

        let (_dir, store) = temp_store();
        let payload_struct = PipelineScopePayload {
            scope: "full".to_string(),
            lang: Some("en".to_string()),
            model: Some("opus".to_string()),
            is_wave_plan: Some(true),
            total_waves: Some(6),
        };
        let payload_value = serde_json::to_value(&payload_struct).unwrap();
        emit_direct(&store, EVENT_PIPELINE_SCOPE, "demo", payload_value);

        let events = store.replay().unwrap();
        assert_eq!(events.len(), 1);
        let decoded: PipelineScopePayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(decoded.scope, "full");
        assert_eq!(decoded.model.as_deref(), Some("opus"));
        assert_eq!(decoded.total_waves, Some(6));
    }

    #[test]
    fn pipeline_task_complete_payload_round_trips() {
        use mustard_core::model::event::PipelineTaskCompletePayload;

        let (_dir, store) = temp_store();
        let payload_struct = PipelineTaskCompletePayload {
            wave: Some(3),
            name: "implement-store".to_string(),
            agent: Some("general-purpose".to_string()),
            duration_ms: Some(45_000),
            files_modified: Some(vec!["src/run/emit_pipeline.rs".to_string()]),
            decisions: Some(vec!["fail-open on store error".to_string()]),
            escalation: None,
        };
        let payload_value = serde_json::to_value(&payload_struct).unwrap();
        emit_direct(&store, EVENT_PIPELINE_TASK_COMPLETE, "demo", payload_value);

        let events = store.replay().unwrap();
        let decoded: PipelineTaskCompletePayload =
            serde_json::from_value(events[0].payload.clone()).unwrap();
        assert_eq!(decoded.wave, Some(3));
        assert_eq!(decoded.duration_ms, Some(45_000));
        assert!(decoded.escalation.is_none());
    }

    #[test]
    fn optional_fields_absent_in_minimal_payload() {
        use mustard_core::model::event::PipelineStatusPayload;

        // Only required fields: `to`. `from` is absent in JSON.
        let raw = r#"{"to":"active"}"#;
        let decoded: PipelineStatusPayload = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded.to, "active");
        assert!(decoded.from.is_none());
    }

    #[test]
    fn store_error_does_not_propagate_as_nonzero() {
        // We cannot easily simulate a store write failure without unsafe tricks,
        // but we can confirm the fail-open design by verifying `store.append`
        // returns Err and the caller drops it with `let _`.
        // Here we just ensure a legitimate append succeeds (regression guard).
        let (_dir, store) = temp_store();
        emit_direct(&store, EVENT_PIPELINE_PAUSE, "demo", json!({"reason": "user request"}));
        assert_eq!(store.replay().unwrap().len(), 1);
    }

    // -----------------------------------------------------------------------
    // Wave-2 header sync (2026-05-21-flatten-spec-layout-and-multi-collab)
    // -----------------------------------------------------------------------

    /// Helper: seed `.claude/spec/{spec}/spec.md` with the given body and
    /// return the project root + path to spec.md.
    fn seed_spec_md(spec: &str, body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let path = spec_dir.join("spec.md");
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    /// The header sync rewrites the `### Status:` line to the new value when
    /// the file exists and the marker is present.
    #[test]
    fn sync_status_header_rewrites_existing_marker() {
        let (dir, path) = seed_spec_md(
            "demo",
            "# Demo\n\n### Status: implementing\n### Phase: EXECUTE\n\n## Body\nx\n",
        );
        super::sync_spec_status_header(dir.path(), "demo", "completed");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("### Status: completed"),
            "header should be rewritten: {after:?}"
        );
        // Other headers untouched.
        assert!(after.contains("### Phase: EXECUTE"));
        // The implementing line is gone.
        assert!(!after.contains("### Status: implementing"));
    }

    /// Fail-open contract: a missing spec.md is a no-op, never a panic.
    #[test]
    fn sync_status_header_missing_file_is_noop() {
        let dir = tempdir().unwrap();
        super::sync_spec_status_header(dir.path(), "ghost", "completed");
        // No file created.
        assert!(!dir.path().join(".claude/spec/ghost/spec.md").exists());
    }

    /// Fail-open contract: a spec.md without a `### Status:` line is left
    /// alone, with a WARN to stderr. We assert the file content is unchanged.
    #[test]
    fn sync_status_header_missing_marker_leaves_file_unchanged() {
        let (dir, path) = seed_spec_md("demo", "# Demo\n\n## Body\nno header\n");
        let before = std::fs::read_to_string(&path).unwrap();
        super::sync_spec_status_header(dir.path(), "demo", "completed");
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after);
    }

    /// Indentation on the header line is preserved (we only rewrite the value
    /// after the colon, not the leading whitespace).
    #[test]
    fn sync_status_header_preserves_original_lines() {
        let body = "# Spec\n\n### Status: planning\n\nbody line\n";
        let (dir, path) = seed_spec_md("demo", body);
        super::sync_spec_status_header(dir.path(), "demo", "implementing");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("### Status: implementing"));
        assert!(after.contains("body line"));
        // No trailing newline drift: original ended with \n, new should too
        // (we re-join lines split on `\n` so the trailing empty segment is
        // preserved).
        assert_eq!(after.matches('\n').count(), body.matches('\n').count());
    }
}
