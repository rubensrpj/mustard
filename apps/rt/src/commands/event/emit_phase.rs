//! `mustard-rt run emit-phase` — a port of `scripts/emit-phase.js`.
//!
//! Records a `pipeline.phase` transition event from a SKILL. ANALYZE runs in
//! the parent context before any `pipeline-state` file exists, so the
//! `post_edit` pipeline-phase emitter never sees it — this is the only place
//! that knows ANALYZE started.
//!
//! The emitted event is shape-identical to what the `post_edit` module
//! produces (`event: "pipeline.phase"`, `payload: { from, to }`, `spec`), so
//! every downstream consumer treats both sources uniformly.
//!
//! Idempotency: the most recent `pipeline.phase` event for the same spec is
//! looked up from the per-spec NDJSON `.events/` dir; if its `to` already
//! equals the requested phase the emit is skipped. Events are written directly
//! to the NDJSON sink — no SQLite involved.

use crate::shared::context::{project_dir, session_id};
use crate::shared::events::writer_ndjson;
use crate::util::now_iso8601;
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use serde_json::json;
use std::path::Path;

/// Return the `to` phase of the most recent `pipeline.phase` event for `spec`
/// by reading the per-spec NDJSON `.events/` directory. Fail-open — a missing
/// events dir or any IO error yields `None` (the caller treats that as "phase
/// unknown").
///
/// This is the single source of truth for spec phase across the runtime; every
/// consumer that previously read `phaseName` from a pipeline-state JSON now
/// derives the phase through this helper instead.
#[must_use]
pub fn last_phase_for_spec(cwd: impl AsRef<Path>, spec: &str) -> Option<String> {
    let events_dir = ClaudePaths::for_project(cwd.as_ref())
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())
        .unwrap_or_else(|| {
            ClaudePaths::compose_unchecked(cwd.as_ref())
                .spec_dir()
                .join(spec)
                .join(".events")
        });
    let mut events = read_harness_events_from_ndjson_dir(&events_dir);
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    events
        .iter()
        .rev()
        .find(|e| e.event == "pipeline.phase" && e.spec.as_deref() == Some(spec))
        .and_then(|e| {
            e.payload
                .get("to")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
}

/// Run `mustard-rt run emit-phase --spec <name> --to <PHASE> [--from <PHASE>]`.
///
/// Fail-open for telemetry: any internal failure (NDJSON write) degrades to a
/// silent no-op. The **exception** is `--to CLOSE`, which runs the close-gate
/// sub-gates (debt/checklist/qa/build) inline before writing the event. A
/// strict gate failure prints the gate reason on stderr, leaves the event
/// un-written, and exits the process with status `1` — same user-visible
/// behavior as the legacy `close_gate` hook that fired on a pipeline-state
/// Write/Edit (the trigger that no longer exists post-Wave 2).
pub fn run(spec: &str, to: &str, from: Option<&str>) {
    let cwd = project_dir();

    // Idempotency: skip when the spec's latest phase already lands on `to`.
    let last = last_phase_for_spec(&cwd, spec);
    if last.as_deref() == Some(to) {
        return;
    }

    // CLOSE transition: run the close-gate sub-gates inline. A strict failure
    // blocks the transition (exit 1); fail-open on any infrastructure error.
    if to.eq_ignore_ascii_case("CLOSE") {
        if let Err(reason) = crate::hooks::write::close_gate::gate_close_for_spec(&cwd, spec) {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }

    // `from` defaults to the spec's last known phase (null when none).
    let from_phase = from.map(str::to_string).or(last);

    // Wave-5 (project-profiler) write-back: when a spec transitions OUT of
    // EXECUTE, gather every concept-node id the resolver cached during the
    // session and write them back into `spec.md` as `injected` backlinks.
    // Fail-open — any IO error is swallowed, telemetry stays best-effort.
    if from_phase
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case("EXECUTE"))
        && !to.eq_ignore_ascii_case("EXECUTE")
    {
        write_back_after_execute(spec);
    }

    let ts = now_iso8601();
    let sid = session_id();

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.clone(),
        session_id: sid.clone(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("emit-phase".to_string()),
            actor_type: None,
        },
        event: "pipeline.phase".to_string(),
        payload: json!({ "from": from_phase, "to": to }),
        spec: Some(spec.to_string()),
    };

    // Write directly to the NDJSON sink. `pipeline.phase` was previously routed
    // to SQLite via `route::emit` (the `pipeline.*` prefix match), but
    // this sub-spec migrates all phase emission to the pure-NDJSON path.
    let project = Path::new(&cwd);
    let kind = crate::shared::events::route::classify_kind(&event.event);
    let _ = writer_ndjson::write_event_with_ts(
        project,
        Some(spec),
        None,
        &sid,
        &event.event,
        kind,
        Some(event.wave),
        Some(&sid),
        Some("emit-phase"),
        None,
        &event.payload,
        Some(&ts),
    );
}

/// Resolve the spec.md path for `spec` under the active project, then write
/// the union of every cached resolver closure as `injected` backlinks. Fully
/// fail-open: a missing project dir, missing spec dir, empty cache, or any
/// IO failure degrades to a silent no-op. Telemetry must never block a phase
/// transition.
fn write_back_after_execute(spec: &str) {
    let project_root = std::path::PathBuf::from(project_dir());
    let Ok(spec_path) = ClaudePaths::for_project(&project_root)
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.spec_md_path())
    else {
        return;
    };
    if !spec_path.exists() {
        return;
    }
    let closure_ids = crate::commands::scan::resolve::collect_cached_closure_ids(&project_root);
    if closure_ids.is_empty() {
        return;
    }
    let _ = crate::commands::scan::graph::write_back_injected_edges(&spec_path, &closure_ids);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::events::writer_ndjson::write_event;
    use serde_json::json;
    use tempfile::tempdir;

    fn emit_phase_event(project: &std::path::Path, spec: &str, to: &str) {
        let payload = json!({ "from": null, "to": to });
        let _ = write_event(
            project,
            Some(spec),
            None,
            "s",
            "pipeline.phase",
            "pipeline",
            Some(0),
            Some("s"),
            Some("emit-phase"),
            None,
            &payload,
        );
    }

    #[test]
    fn last_phase_reads_the_freshest_event() {
        let dir = tempdir().unwrap();
        emit_phase_event(dir.path(), "demo", "ANALYZE");
        emit_phase_event(dir.path(), "demo", "PLAN");
        assert_eq!(last_phase_for_spec(dir.path(), "demo").as_deref(), Some("PLAN"));
        assert_eq!(last_phase_for_spec(dir.path(), "other"), None);
    }

    #[test]
    fn last_phase_empty_log_is_none() {
        let dir = tempdir().unwrap();
        assert_eq!(last_phase_for_spec(dir.path(), "demo"), None);
    }
}
