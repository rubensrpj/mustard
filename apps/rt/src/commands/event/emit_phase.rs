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
use mustard_core::time::now_iso8601;
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

/// What a phase emit did — the material of the ONE deterministic success line
/// [`run`] prints. `Recorded` carries the previous phase (`None` on a spec's
/// first transition); `AlreadyThere` is the idempotent short-circuit, which
/// must SAY so: printing nothing made "already in that phase" and "transition
/// recorded" indistinguishable from stdout.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PhaseOutcome {
    /// The transition was written; `from` is the phase it left.
    Recorded { from: Option<String> },
    /// The spec's latest phase already equals the requested one — no write.
    AlreadyThere,
}

/// The one deterministic success line for a phase emit — the same
/// `{ok, kind, spec}` shape [`super::emit_pipeline`] echoes (added there
/// because that emitter used to succeed in TOTAL silence), extended with the
/// transition itself: `from` (null on a first transition), `to`, and
/// `idempotent: true` on the short-circuit. No timestamp/session — run
/// outputs are byte-compared in gates; the NDJSON row carries those.
fn success_line(outcome: &PhaseOutcome, spec: &str, to: &str) -> serde_json::Value {
    match outcome {
        PhaseOutcome::Recorded { from } => json!({
            "ok": true, "kind": "pipeline.phase", "spec": spec, "from": from, "to": to,
        }),
        PhaseOutcome::AlreadyThere => json!({
            "ok": true, "kind": "pipeline.phase", "spec": spec, "from": to, "to": to,
            "idempotent": true,
        }),
    }
}

/// Run `mustard-rt run emit-phase --spec <name> --to <PHASE> [--from <PHASE>]`.
///
/// Fail-open for telemetry: any internal failure (NDJSON write) degrades to a
/// no-op. On success it prints the deterministic confirmation line
/// ([`success_line`]) — previous phase and new, and the idempotent
/// short-circuit says so instead of staying silent. The **exception** is
/// `--to CLOSE`, which runs the close-gate sub-gates (debt/checklist/qa/build)
/// inline before writing the event. A strict gate failure prints the gate
/// reason on stderr, leaves the event un-written, and exits the process with
/// status `1` — same user-visible behavior as the legacy `close_gate` hook
/// that fired on a pipeline-state Write/Edit (the trigger that no longer
/// exists post-Wave 2).
pub fn run(spec: &str, to: &str, from: Option<&str>) {
    match run_at(Path::new(&project_dir()), spec, to, from) {
        Ok(outcome) => println!("{}", success_line(&outcome, spec, to)),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}

/// Cwd-aware miolo of [`run`]: record the `pipeline.phase` transition against
/// an explicit `cwd` instead of the process-global project dir. Reused by
/// `plan-materialize` (composing PLAN entry in-process) — which is why the
/// confirmation PRINT lives in the CLI entry [`run`], never here: a composite
/// caller's own stdout contract must not grow an interleaved line. `Ok`
/// carries the [`PhaseOutcome`] the caller may render; `Err` carries the
/// close-gate reason for a blocked `--to CLOSE` (the CLI entry turns that into
/// the legacy stderr + exit 1, composite callers fold it into their report).
pub(crate) fn run_at(
    cwd: &Path,
    spec: &str,
    to: &str,
    from: Option<&str>,
) -> Result<PhaseOutcome, String> {
    // Idempotency: skip when the spec's latest phase already lands on `to`.
    let last = last_phase_for_spec(cwd, spec);
    if last.as_deref() == Some(to) {
        return Ok(PhaseOutcome::AlreadyThere);
    }

    // CLOSE transition: run the close-gate sub-gates inline. A strict failure
    // blocks the transition; fail-open on any infrastructure error.
    if to.eq_ignore_ascii_case("CLOSE") {
        crate::commands::pipeline::close_gates::gate_close_for_spec(&cwd.to_string_lossy(), spec)?;
    }

    // `from` defaults to the spec's last known phase (null when none).
    let from_phase = from.map(str::to_string).or(last);

    // Wave-5 (project-profiler) write-back: when a spec transitions OUT of
    // EXECUTE, gather every concept-node id the resolver cached during the
    // session and write them back into `spec.md` as `injected` backlinks.
    // Fail-open — any IO error is swallowed, telemetry stays best-effort.

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
        payload: json!({ "from": from_phase.clone(), "to": to }),
        spec: Some(spec.to_string()),
    };

    // Write directly to the NDJSON sink. `pipeline.phase` was previously routed
    // to SQLite via `route::emit` (the `pipeline.*` prefix match), but
    // this sub-spec migrates all phase emission to the pure-NDJSON path.
    let kind = crate::shared::events::route::classify_kind(&event.event);
    let _ = writer_ndjson::write_event_with_ts(
        cwd,
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
    Ok(PhaseOutcome::Recorded { from: from_phase })
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

    /// AC-10: a recorded transition confirms itself — the success line names
    /// the previous phase and the new one — and the idempotent short-circuit
    /// SAYS so instead of staying silent (stdout used to be empty either way,
    /// making "already there" and "recorded" indistinguishable).
    #[test]
    fn emit_phase_confirms_the_transition() {
        let dir = tempdir().unwrap();

        // First transition: no previous phase — `from` is honest null.
        let first = run_at(dir.path(), "demo", "ANALYZE", None).expect("first emit");
        assert_eq!(first, PhaseOutcome::Recorded { from: None });
        let line = success_line(&first, "demo", "ANALYZE");
        assert_eq!(line["ok"], json!(true));
        assert_eq!(line["kind"], json!("pipeline.phase"));
        assert_eq!(line["spec"], json!("demo"));
        assert_eq!(line["from"], json!(null), "first transition has no previous phase: {line}");
        assert_eq!(line["to"], json!("ANALYZE"));
        assert!(line.get("idempotent").is_none(), "a real transition is not idempotent: {line}");

        // Second transition: the previous phase is read back and named.
        let second = run_at(dir.path(), "demo", "PLAN", None).expect("second emit");
        assert_eq!(second, PhaseOutcome::Recorded { from: Some("ANALYZE".to_string()) });
        let line = success_line(&second, "demo", "PLAN");
        assert_eq!(line["from"], json!("ANALYZE"), "previous phase named: {line}");
        assert_eq!(line["to"], json!("PLAN"));

        // Idempotent short-circuit: already-in-that-phase says so.
        let third = run_at(dir.path(), "demo", "PLAN", None).expect("idempotent emit");
        assert_eq!(third, PhaseOutcome::AlreadyThere);
        let line = success_line(&third, "demo", "PLAN");
        assert_eq!(line["idempotent"], json!(true), "the short-circuit is named: {line}");
        assert_eq!(line["from"], json!("PLAN"));
        assert_eq!(line["to"], json!("PLAN"));
    }
}
