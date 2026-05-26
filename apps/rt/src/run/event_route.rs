//! Single event-routing layer for the W5 split (`pipeline.*` → SQLite,
//! everything else → per-spec NDJSON).
//!
//! ## Why one router
//!
//! The W5 split lands two stores for harness events:
//!
//! - **SQLite** (`pipeline_events` table in `mustard.db`) — the lean lifecycle
//!   index the dashboard reads by spec.
//! - **NDJSON** (`<spec>/[wave-N-{role}/]events/*.ndjson`) — the hot-path event
//!   log written by [`crate::run::event_writer_ndjson`].
//!
//! Before this module landed, every hook + run-face callsite that wanted to
//! emit a non-`pipeline.*` event funnelled through
//! [`mustard_core::store::event_store::EventSink::append`], which silently
//! dropped any non-`pipeline.*` event (the SQLite sink only handles lifecycle
//! events under W5). That left every `tool.use`, `agent.start`, `qa.result`,
//! `friction.*`, etc. event going nowhere.
//!
//! This module is the single switch: each callsite calls [`emit`] (or
//! [`emit_event`] / [`emit_event_with_wave_role`] for the typed-context
//! variants) and the router does the rest:
//!
//! 1. **`pipeline.*`** → keep the SQLite write path. The router opens the
//!    store via [`SqliteEventStore::for_project`] and calls [`EventSink::append`].
//! 2. **Everything else** → calls
//!    [`crate::run::event_writer_ndjson::write_event`] with the resolved spec /
//!    wave / session triple.
//!
//! Both paths are fail-open — the caller's tool execution is never blocked by
//! a telemetry failure (the SQLite append already swallows errors at every
//! existing callsite, and the NDJSON writer is already fail-open by design).
//!
//! ## Resolving spec + wave context
//!
//! The router resolves spec / wave / session like every other run-face
//! emitter: env vars first (`MUSTARD_ACTIVE_SPEC`, `MUSTARD_ACTIVE_WAVE`,
//! `MUSTARD_ACTIVE_WAVE_ROLE`, `MUSTARD_SESSION_ID` / `CLAUDE_SESSION_ID`),
//! then the SQLite `last_pipeline_scope_for_session` lookup, then the
//! filesystem `.pipeline-states/*.json` hint — see
//! [`crate::run::env::current_spec`]. The `HarnessEvent`'s own `spec` /
//! `session_id` / `wave` fields, when populated, are honoured first.

use crate::run::env::{current_spec, project_dir, session_id};
use crate::run::event_writer_ndjson;
use mustard_core::model::event::HarnessEvent;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use std::path::Path;

/// `true` when `event.event` is a lifecycle event that belongs in SQLite.
///
/// A lifecycle event is any name with the `pipeline.` prefix — the same
/// classification the W5 [`SqliteEventStore::append`] uses internally.
#[must_use]
pub fn is_pipeline_event(event_name: &str) -> bool {
    event_name.starts_with("pipeline.")
}

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

/// Route one [`HarnessEvent`] to the right sink (SQLite for `pipeline.*`,
/// NDJSON for everything else).
///
/// `project_dir_path` is the absolute project root — the canonical place to
/// resolve it is [`crate::run::env::project_dir`].
///
/// Returns `true` when the event landed somewhere (the SQLite append
/// succeeded, OR the NDJSON write returned a [`event_writer_ndjson::WriteOutcome`]).
/// Callers may ignore the return value: every error is swallowed — telemetry
/// is never load-bearing.
pub fn emit(project_dir_path: &str, event: &HarnessEvent) -> bool {
    if is_pipeline_event(&event.event) {
        return SqliteEventStore::for_project(project_dir_path)
            .and_then(|store| store.append(event))
            .is_ok();
    }

    // NDJSON path — resolve fields from the event first, then fall back to the
    // ambient process state.
    let project = Path::new(project_dir_path);
    let spec_owned = event.spec.clone().or_else(|| current_spec(project_dir_path));
    let spec = spec_owned.as_deref().filter(|s| !s.is_empty());

    let wave_role_owned = current_wave_role();
    let wave_role = wave_role_owned.as_deref();

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

    event_writer_ndjson::write_event_with_ts(
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
/// The vast majority of run-face emitters already shell out to
/// [`crate::run::env::project_dir`] before calling [`SqliteEventStore::for_project`];
/// this helper packages the common pattern. Marked `allow(dead_code)` until
/// the first short-form callsite picks it up — the explicit form
/// [`emit`]`(&project_dir, ev)` covers every site today.
#[allow(dead_code)]
pub fn emit_default(event: &HarnessEvent) -> bool {
    emit(&project_dir(), event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use mustard_core::ClaudePaths;
    use serde_json::json;
    use tempfile::tempdir;

    fn event(name: &str, spec: Option<&str>) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-24T00:00:00.000Z".to_string(),
            session_id: "s-route-test".to_string(),
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

    #[test]
    fn is_pipeline_event_matches_only_pipeline_prefix() {
        assert!(is_pipeline_event("pipeline.scope"));
        assert!(is_pipeline_event("pipeline.amend_open"));
        assert!(!is_pipeline_event("tool.use"));
        assert!(!is_pipeline_event("agent.start"));
    }

    /// Routing a non-pipeline event lands an NDJSON file under
    /// `<project>/.claude/spec/<spec>/.events/`, never touches SQLite.
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

        // SQLite must NOT have received the non-pipeline event.
        let db = paths.harness_dir().join("mustard.db");
        // The store may not exist at all (router didn't open it for tool.use);
        // if it does exist, no row should be present for `tool.use` either.
        if db.exists() {
            let store = SqliteEventStore::new(&db).unwrap();
            for ev in store.replay().unwrap() {
                assert_ne!(ev.event, "tool.use", "tool.use must not land in SQLite");
            }
        }
    }

    /// Routing a `pipeline.*` event lands SQLite (and never NDJSON).
    #[test]
    fn routes_pipeline_event_to_sqlite_not_ndjson() {
        let dir = tempdir().unwrap();
        let ok = emit(
            dir.path().to_str().unwrap(),
            &event("pipeline.scope", Some("pipe-spec")),
        );
        assert!(ok);

        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        // NDJSON dir must NOT have been created for a pipeline.* event.
        let events_dir = paths.for_spec("pipe-spec").unwrap().events_dir();
        assert!(
            !events_dir.exists(),
            "pipeline.* events should never land in NDJSON"
        );

        // SQLite must contain the event.
        let db = paths.harness_dir().join("mustard.db");
        assert!(db.exists(), "SQLite must have been opened");
        let store = SqliteEventStore::new(&db).unwrap();
        let events = store.replay().unwrap();
        assert!(
            events.iter().any(|e| e.event == "pipeline.scope"),
            "pipeline.scope must be present in SQLite replay"
        );
    }

    /// `classify_kind` covers session/scope/etc — kept as a pure-classifier
    /// test instead of an env-mutating fallback test (the `unsafe_code` lint is
    /// forbidden crate-wide, so we cannot temporarily remove env vars to force
    /// the session-fallback branch). The session-fallback path is exercised
    /// by `event_writer_ndjson::tests::event_dir_falls_back_to_session`, which
    /// targets the same code path one level lower without any env reads.
    #[test]
    fn classify_covers_remaining_families() {
        assert_eq!(classify_kind("subagent.start"), "other");
        assert_eq!(classify_kind("notification.echo"), "notification");
    }
}
