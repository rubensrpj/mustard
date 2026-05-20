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
//! looked up; if its `to` already equals the requested phase the emit is
//! skipped. The JS version shelled to `_lib/harness-event.js`; this port emits
//! directly through `mustard_core` instead.

use crate::run::env::{project_dir, session_id};
use crate::util::now_iso8601;
use mustard_core::io::event_store::EventSink;
use mustard_core::io::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;
use std::path::Path;

/// Return the `to` phase of the most recent `pipeline.phase` event for `spec`,
/// reading from an already-open [`SqliteEventStore`]. Fail-open — any replay
/// error yields `None` (the caller treats that as "phase unknown").
///
/// This is the single source of truth for spec phase across the runtime; every
/// consumer that previously read `phaseName` from a pipeline-state JSON now
/// derives the phase through this helper instead.
#[must_use]
pub fn last_phase_in_store(store: &SqliteEventStore, spec: &str) -> Option<String> {
    let events = store.replay().unwrap_or_default();
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

/// Convenience: open the project's SQLite store and look up the latest phase
/// for `spec`. Fail-open — a store-open error yields `None`.
#[must_use]
pub fn last_phase_for_spec(cwd: impl AsRef<Path>, spec: &str) -> Option<String> {
    let store = SqliteEventStore::for_project(cwd.as_ref()).ok()?;
    last_phase_in_store(&store, spec)
}

/// Run `mustard-rt run emit-phase --spec <name> --to <PHASE> [--from <PHASE>]`.
///
/// Fail-open for telemetry: any internal failure (db open, append) degrades to
/// a silent no-op. The **exception** is `--to CLOSE`, which runs the
/// close-gate sub-gates (debt/checklist/qa/build) inline before appending the
/// event. A strict gate failure prints the gate reason on stderr, leaves the
/// event un-appended, and exits the process with status `1` — same
/// user-visible behavior as the legacy `close_gate` hook that fired on a
/// pipeline-state Write/Edit (the trigger that no longer exists post-Wave 2).
pub fn run(spec: &str, to: &str, from: Option<&str>) {
    let Ok(store) = SqliteEventStore::for_project(project_dir()) else {
        return;
    };

    // Idempotency: skip when the spec's latest phase already lands on `to`.
    let last = last_phase_in_store(&store, spec);
    if last.as_deref() == Some(to) {
        return;
    }

    // CLOSE transition: run the close-gate sub-gates inline. A strict failure
    // blocks the transition (exit 1); fail-open on any infrastructure error.
    if to.eq_ignore_ascii_case("CLOSE") {
        if let Err(reason) = crate::hooks::close_gate::gate_close_for_spec(&project_dir(), spec) {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }

    // `from` defaults to the spec's last known phase (null when none).
    let from_phase = from.map(str::to_string).or(last);

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
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
    let _ = store.append(&event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn last_phase_reads_the_freshest_event() {
        let dir = tempdir().unwrap();
        let store = SqliteEventStore::new(dir.path().join("mustard.db")).unwrap();
        let mk = |to: &str| HarnessEvent {
            v: SCHEMA_VERSION,
            ts: now_iso8601(),
            session_id: "s".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: None,
                actor_type: None,
            },
            event: "pipeline.phase".to_string(),
            payload: json!({ "from": null, "to": to }),
            spec: Some("demo".to_string()),
        };
        store.append(&mk("ANALYZE")).unwrap();
        store.append(&mk("PLAN")).unwrap();
        assert_eq!(last_phase_in_store(&store, "demo").as_deref(), Some("PLAN"));
        assert_eq!(last_phase_in_store(&store, "other"), None);
    }

    #[test]
    fn last_phase_empty_log_is_none() {
        let dir = tempdir().unwrap();
        let store = SqliteEventStore::new(dir.path().join("mustard.db")).unwrap();
        assert_eq!(last_phase_in_store(&store, "demo"), None);
    }
}
