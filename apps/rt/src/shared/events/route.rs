//! Single event-routing layer — every harness event lands in per-spec NDJSON.
//!
//! ## Why one router
//!
//! Originally the W5 split kept lifecycle (`pipeline.*`) events in SQLite and
//! everything else in NDJSON. The W6–W8 migration of
//! `2026-05-26-no-sqlite-git-source-of-truth` collapsed both stores into the
//! single NDJSON sink under `<spec>/[wave-N-{role}/]events/*.ndjson`, written
//! by [`crate::shared::events::writer_ndjson`].
//!
//! Before this module landed, every hook + run-face callsite emitting a
//! non-`pipeline.*` event funnelled through the old SQLite `EventSink`
//! variant, which silently dropped non-`pipeline.*` events. That left every
//! `tool.use`, `agent.start`, `qa.result`, `friction.*`, etc. event going
//! nowhere.
//!
//! This module is the single switch. Each callsite calls [`emit`] (or
//! [`emit_event`] / [`emit_event_with_wave_role`] for the typed-context
//! variants) and the router classifies + dispatches:
//!
//! 1. **Every event** → [`crate::shared::events::writer_ndjson::write_event_with_ts`]
//!    with the resolved spec / wave / session triple.
//! 2. **`pipeline.*`** is still recognised by [`classify_kind`] so the
//!    `kind` column carries `"pipeline"` — but the destination is the same
//!    NDJSON sink as every other event. No SQLite append path remains.
//!
//! All paths are fail-open — the caller's tool execution is never blocked by
//! a telemetry failure (the NDJSON writer is fail-open by design).
//!
//! ## Resolving spec + wave context
//!
//! The router resolves the session id first, then the spec, because a
//! spec-less event inherits the spec its session is bound to:
//!
//! - **session**: `HarnessEvent.session_id` → env (`MUSTARD_SESSION_ID` /
//!   `CLAUDE_SESSION_ID`) → newest `.claude/.session/<id>/` by mtime
//!   ([`crate::shared::context::session_id`]).
//! - **spec**: `HarnessEvent.spec` → env / legacy `.pipeline-states`
//!   ([`crate::shared::context::current_spec`]) → the session→spec marker
//!   ([`crate::shared::context::spec_for_session`]). The marker is written
//!   HERE whenever an event arrives carrying BOTH a spec and a session (the
//!   `pipeline.scope` / `pipeline.stage` / `pipeline.status` events the
//!   run-face emits), so subsequent spec-less hook heartbeats
//!   (`tool.use`, `agent.*`) attribute to the running spec instead of
//!   landing unattributed under `.session/<id>/`.
//! - **wave**: `HarnessEvent.wave` → `MUSTARD_ACTIVE_WAVE`.

use crate::shared::context::{
    bind_session_spec, current_spec, project_dir, session_id, spec_for_session,
};
use crate::shared::events::writer_ndjson;
use mustard_core::domain::model::event::HarnessEvent;
use std::path::Path;

/// Classify an `event_name` into a [`event_writer_ndjson`] `kind` string.
///
/// `kind` is the short logical bucket the dashboard reads to colour rows
/// without re-parsing the event name. The classification mirrors how the
/// timeline UI groups events. Unknown names fall back to `"other"`.
#[must_use]
pub fn classify_kind(event_name: &str) -> &'static str {
    if event_name.starts_with("pipeline.") {
        "pipeline"
    } else if event_name.starts_with("tool.") {
        "tool"
    } else if event_name.starts_with("agent.") {
        "agent"
    } else if event_name.starts_with("analyze.") {
        "analyze"
    } else if event_name.starts_with("qa.") {
        "qa"
    } else if event_name.starts_with("knowledge.")
        || event_name == "decision"
        || event_name == "finding"
        || event_name == "lesson"
    {
        "knowledge"
    } else if event_name.starts_with("friction.") || event_name == "retry.attempt" {
        "friction"
    } else if event_name.starts_with("notification.") {
        "notification"
    } else if event_name.starts_with("session.") {
        "session"
    } else if event_name.starts_with("hygiene.") {
        "hygiene"
    } else if event_name.starts_with("review.") {
        "review"
    } else if event_name.starts_with("boundary.") {
        "boundary"
    } else if event_name.starts_with("spec.")
        || event_name.starts_with("worktree.")
    {
        "scope"
    } else {
        "other"
    }
}

/// Resolve the wave-role segment for the NDJSON path from the environment.
///
/// Returns `Some("wave-N-{role}")` when both `MUSTARD_ACTIVE_WAVE` and
/// `MUSTARD_ACTIVE_WAVE_ROLE` are set; `None` otherwise (the event then lands
/// directly under `<spec>/events/` instead of inside a wave subdir).
#[must_use]
pub fn current_wave_role() -> Option<String> {
    let wave = std::env::var("MUSTARD_ACTIVE_WAVE")
        .ok()
        .filter(|s| !s.is_empty())?;
    let role = std::env::var("MUSTARD_ACTIVE_WAVE_ROLE")
        .ok()
        .filter(|s| !s.is_empty())?;
    Some(format!("wave-{wave}-{role}"))
}

/// Parse `MUSTARD_ACTIVE_WAVE` into a `u32` for the NDJSON record's `wave`
/// column. `None` when unset / not numeric.
fn current_wave_number() -> Option<u32> {
    std::env::var("MUSTARD_ACTIVE_WAVE")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
}

/// Route one [`HarnessEvent`] to the NDJSON sink.
///
/// `project_dir_path` is the absolute project root — the canonical place to
/// resolve it is [`crate::shared::context::project_dir`].
///
/// Returns `true` when the NDJSON write succeeded.
/// Callers may ignore the return value: every error is swallowed — telemetry
/// is never load-bearing.
pub fn emit(project_dir_path: &str, event: &HarnessEvent) -> bool {
    // All events (including `pipeline.*`) are now routed to NDJSON.
    // The `classify_kind` classifier stamps the row's `kind` column.
    let project = Path::new(project_dir_path);

    // Resolve the session id BEFORE the spec: an event that lacks a spec
    // inherits it from the session's recorded `pipeline.scope` binding, so we
    // need the session id in hand first.
    let session_id_owned = if event.session_id.is_empty() || event.session_id == "unknown" {
        let resolved = session_id();
        if resolved == "unknown" {
            None
        } else {
            Some(resolved)
        }
    } else {
        Some(event.session_id.clone())
    };
    let session_slug = session_id_owned.clone().unwrap_or_else(|| "unknown".to_string());
    let session_id_ref = session_id_owned.as_deref();

    // Spec resolution chain:
    //   event.spec → current_spec (env / legacy pipeline-states) →
    //   the session→spec marker the run-face's `pipeline.scope` events leave.
    // The marker step is what lets a spec-less `tool.use` heartbeat inherit the
    // spec its session is executing under.
    let spec_owned = event
        .spec
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| current_spec(project_dir_path))
        .or_else(|| session_id_ref.and_then(|sid| spec_for_session(project_dir_path, sid)));
    let spec = spec_owned.as_deref().filter(|s| !s.is_empty());

    // When THIS event already carries both a spec and a session id (the
    // `pipeline.scope` / `pipeline.stage` / `pipeline.status` events the
    // run-face emits), persist the binding so later spec-less hook events for
    // the same session can inherit it via the chain above. Fail-open.
    if let (Some(s), Some(sid)) = (spec, session_id_ref) {
        bind_session_spec(project_dir_path, sid, s);
    }

    let wave_role_owned = current_wave_role();
    let wave_role = wave_role_owned.as_deref();

    let wave_num = if event.wave > 0 {
        Some(event.wave)
    } else {
        current_wave_number()
    };

    let actor_id = event.actor.id.as_deref();
    let kind = classify_kind(&event.event);

    // Honour the caller's pre-stamped `ts` so consumer-side filters (MCP
    // `since`, `metrics wave-status` duration) reflect when the event
    // logically occurred, not when the router happened to flush it. Empty
    // strings fall back to wall-clock time in [`write_event_with_ts`].
    let ts_override = if event.ts.is_empty() {
        None
    } else {
        Some(event.ts.as_str())
    };

    writer_ndjson::write_event_with_ts(
        project,
        spec,
        wave_role,
        &session_slug,
        &event.event,
        kind,
        wave_num,
        session_id_ref,
        actor_id,
        None,
        &event.payload,
        ts_override,
    )
    .is_some()
}

/// Convenience wrapper that resolves the project dir for the caller.
///
/// The vast majority of run-face emitters already call
/// [`crate::shared::context::project_dir`] before routing through [`emit`]; this
/// helper packages the common pattern. Marked `allow(dead_code)` until the
/// first short-form callsite picks it up — the explicit form
/// [`emit`]`(&project_dir, ev)` covers every site today.
#[allow(dead_code)]
pub fn emit_default(event: &HarnessEvent) -> bool {
    emit(&project_dir(), event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use mustard_core::ClaudePaths;
    use serde_json::json;
    use tempfile::tempdir;

    fn event(name: &str, spec: Option<&str>) -> HarnessEvent {
        event_for_session(name, spec, "s-route-test")
    }

    fn event_for_session(name: &str, spec: Option<&str>, session: &str) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-24T00:00:00.000Z".to_string(),
            session_id: session.to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("router-test".to_string()),
                actor_type: None,
            },
            event: name.to_string(),
            payload: json!({"k": "v"}),
            spec: spec.map(ToString::to_string),
        }
    }

    #[test]
    fn classifies_known_event_families() {
        assert_eq!(classify_kind("tool.use"), "tool");
        assert_eq!(classify_kind("agent.start"), "agent");
        assert_eq!(classify_kind("qa.result"), "qa");
        assert_eq!(classify_kind("retry.attempt"), "friction");
        assert_eq!(classify_kind("knowledge.captured"), "knowledge");
        assert_eq!(classify_kind("session.start"), "session");
        assert_eq!(classify_kind("pipeline.scope"), "pipeline");
        assert_eq!(classify_kind("review.result"), "review");
        assert_eq!(classify_kind("hygiene.spec.archived"), "hygiene");
        assert_eq!(classify_kind("boundary.expansion"), "boundary");
        assert_eq!(classify_kind("spec.link"), "scope");
        assert_eq!(classify_kind("worktree.gc.run"), "scope");
        assert_eq!(classify_kind("totally.unknown"), "other");
    }

    /// Routing a `tool.use` event lands an NDJSON file under
    /// `<project>/.claude/spec/<spec>/.events/`.
    #[test]
    fn routes_tool_event_to_ndjson_under_spec_dir() {
        let dir = tempdir().unwrap();
        let ok = emit(
            dir.path().to_str().unwrap(),
            &event("tool.use", Some("router-spec")),
        );
        assert!(ok, "router should return true on success");

        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let events_dir = paths.for_spec("router-spec").unwrap().events_dir();
        assert!(events_dir.exists(), "NDJSON .events dir must exist");
        let files: Vec<_> = std::fs::read_dir(&events_dir).unwrap().collect();
        assert!(!files.is_empty(), "expected at least one NDJSON file");
    }

    /// Routing a `pipeline.*` event also lands in NDJSON (W2A: all events → NDJSON).
    #[test]
    fn routes_pipeline_event_to_ndjson() {
        let dir = tempdir().unwrap();
        let ok = emit(
            dir.path().to_str().unwrap(),
            &event("pipeline.scope", Some("pipe-spec")),
        );
        assert!(ok, "pipeline.scope should land in NDJSON");

        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let events_dir = paths.for_spec("pipe-spec").unwrap().events_dir();
        assert!(events_dir.exists(), "NDJSON .events dir must exist for pipeline.* too");
        let files: Vec<_> = std::fs::read_dir(&events_dir).unwrap().collect();
        assert!(!files.is_empty(), "expected at least one NDJSON file for pipeline.scope");
    }

    /// The session→spec binding fix: a `pipeline.scope` carrying (session=S,
    /// spec=X) leaves a marker so a later spec-LESS `tool.use` carrying only
    /// session=S inherits `spec=X` — it attributes to the running spec instead
    /// of falling through unattributed under `.session/<id>/`.
    #[test]
    fn tool_use_inherits_spec_from_session_pipeline_scope() {
        // Skip the spec-attribution assertion if the ambient env pins a spec —
        // `current_spec` would win the chain before the marker step and the
        // crate forbids `unsafe`, so a test cannot clear the env var. The
        // marker-write + read is still exercised below regardless.
        let env_spec = std::env::var("MUSTARD_ACTIVE_SPEC").ok().filter(|s| !s.is_empty());

        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let session = "s-bind-test";
        let spec = "binding-spec-xyzzy";

        // 1. run-face emits `pipeline.scope` carrying BOTH session + spec.
        assert!(emit(project, &event_for_session("pipeline.scope", Some(spec), session)));

        // The router persisted the binding marker.
        assert_eq!(
            crate::shared::context::spec_for_session(project, session).as_deref(),
            Some(spec),
            "pipeline.scope must persist the session→spec marker"
        );

        // 2. a spec-LESS `tool.use` for the same session.
        assert!(emit(project, &event_for_session("tool.use", None, session)));

        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let spec_events = paths.for_spec(spec).unwrap().events_dir();

        if env_spec.is_some() {
            // Ambient MUSTARD_ACTIVE_SPEC pre-empts the marker — only assert the
            // marker round-trip (done above), not where the tool.use landed.
            return;
        }

        // The spec-less tool.use inherited spec=X via the session marker: it
        // landed under the spec's `.events/` with `spec=X` in the record.
        assert!(spec_events.exists(), "tool.use must land under the bound spec's .events/");
        let mut found_spec = false;
        for f in std::fs::read_dir(&spec_events).unwrap() {
            let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
            for line in body.lines() {
                let rec: serde_json::Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if rec["event"] == "tool.use" {
                    assert_eq!(rec["spec"], spec, "routed tool.use must carry spec=X");
                    assert_eq!(rec["session_id"], session);
                    found_spec = true;
                }
            }
        }
        assert!(found_spec, "tool.use NDJSON line must exist under the bound spec");
    }

    /// `classify_kind` covers session/scope/etc — kept as a pure-classifier
    /// test instead of an env-mutating fallback test (the `unsafe_code` lint is
    /// forbidden crate-wide, so we cannot temporarily remove env vars to force
    /// the session-fallback branch). The session-fallback path is exercised
    /// by `writer_ndjson::tests::event_dir_falls_back_to_session`, which
    /// targets the same code path one level lower without any env reads.
    #[test]
    fn classify_covers_remaining_families() {
        assert_eq!(classify_kind("subagent.start"), "other");
        assert_eq!(classify_kind("notification.echo"), "notification");
    }

    /// The digest-adherence events (`analyze.digest.used` marker emitted by
    /// `feature::run`, `analyze.digest.summary` emitted by
    /// `digest-adherence-finalize`) classify under the `analyze` kind — never
    /// `other`, so the dashboard can bucket them without name-parsing.
    #[test]
    fn classify_analyze_family_as_analyze() {
        assert_eq!(classify_kind("analyze.digest.used"), "analyze");
        assert_eq!(classify_kind("analyze.digest.summary"), "analyze");
    }
}
