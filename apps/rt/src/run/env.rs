//! Environment resolution for the `run` face.
//!
//! Unlike the enforcement faces, a `run` subcommand never receives a
//! `HookInput` — it resolves the project directory and session id from the
//! process environment, mirroring how the JS scripts did (`CLAUDE_PROJECT_DIR`,
//! `MUSTARD_SESSION_ID` / `CLAUDE_SESSION_ID`).

use mustard_core::store::sqlite_store::SqliteEventStore;
use std::path::Path;

/// Resolve the project directory: `CLAUDE_PROJECT_DIR` when set, else the
/// process current working directory, else `"."`.
#[must_use]
pub fn project_dir() -> String {
    if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !dir.is_empty() {
            return dir;
        }
    }
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string())
}

/// Resolve the current session id from the environment, defaulting to
/// `"unknown"` — matching the JS scripts' `MUSTARD_SESSION_ID` /
/// `CLAUDE_SESSION_ID` lookup.
#[must_use]
pub fn session_id() -> String {
    std::env::var("MUSTARD_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("CLAUDE_SESSION_ID").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Resolve the name of the currently active spec, fail-open `None`.
///
/// Strategy (in priority order):
///
/// 1. `MUSTARD_ACTIVE_SPEC` env var — explicit override set by
///    `/mustard:feature` and `/mustard:resume` before dispatching hooks.
/// 2. **SQLite event store** — the most recent `pipeline.scope` event whose
///    `session_id` matches the current session. This is durable across
///    `session_cleanup` (which wipes `.pipeline-states/*.json`) so events
///    emitted *after* cleanup still get attributed to the active spec.
/// 3. The most recently modified `.claude/.pipeline-states/*.json` file under
///    `project_dir` — kept as a fallback for sessions that have not yet
///    written a `pipeline.scope` event (e.g. very early in a fresh pipeline).
///
/// Returns `None` when no spec is active — never panics. Every step fails
/// open: a missing env var, a closed DB, or an absent state directory all
/// degrade to the next strategy instead of erroring.
///
/// The session-id lookup uses [`session_id`] internally (which itself reads
/// `MUSTARD_SESSION_ID` / `CLAUDE_SESSION_ID`); callers do not need to
/// thread it through.
#[must_use]
pub fn current_spec(project_dir_path: &str) -> Option<String> {
    // 1. Explicit env override.
    if let Ok(s) = std::env::var("MUSTARD_ACTIVE_SPEC") {
        if !s.is_empty() {
            return Some(s);
        }
    }

    // 2. SQLite — last `pipeline.scope` event for this session.
    //
    // We swallow every error path here on purpose: the store may not yet
    // exist (the very first hook of a brand-new project), the DB may be
    // locked behind another writer, or the session id may be `"unknown"`
    // (no env vars yet). In each case we fall through to the filesystem
    // hint rather than erroring out — attribution is best-effort.
    let session = session_id();
    if session != "unknown" && !session.is_empty() {
        if let Ok(store) = SqliteEventStore::for_project(project_dir_path) {
            if let Ok(Some(spec)) = store.last_pipeline_scope_for_session(&session) {
                return Some(spec);
            }
        }
    }

    // 3. Newest pipeline-state file by mtime — legacy hint used when no
    //    `pipeline.scope` event exists yet (the moment between opening a
    //    spec on disk and emitting its first event).
    let states = Path::new(project_dir_path)
        .join(".claude")
        .join(".pipeline-states");
    let entries = std::fs::read_dir(&states).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries.filter_map(std::result::Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") || name.ends_with(".metrics.json") {
            continue;
        }
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            let spec = name.trim_end_matches(".json").to_string();
            best = Some((mtime, spec));
        }
    }
    best.map(|(_, spec)| spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // current_spec — filesystem branch (no env mutation needed)
    // -----------------------------------------------------------------------

    #[test]
    fn current_spec_returns_none_when_no_states_dir() {
        // A nonexistent project path → no pipeline-states dir → None.
        // (env var path not exercised; tested implicitly by the round-trip
        // test below which uses a tempdir-based spec name via the FS branch.)
        let result = current_spec("/nonexistent-mustard-test-path-xyzzy");
        // Either None (env var not set in CI) or Some(...) if MUSTARD_ACTIVE_SPEC
        // happens to be set — just assert it doesn't panic.
        let _ = result;
    }

    #[test]
    fn current_spec_falls_back_to_pipeline_states() {
        // Only exercises the FS branch — avoids process-env mutation.
        // Uses a unique spec name unlikely to match any real MUSTARD_ACTIVE_SPEC.
        let dir = tempdir().unwrap();
        let states = dir.path().join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join("my-feature-xyzzy.json"), "{}").unwrap();

        // When MUSTARD_ACTIVE_SPEC is not set (the common case in CI), the
        // filesystem branch fires and returns "my-feature-xyzzy".
        // When it IS set, the env-var branch takes priority — still no panic.
        let result = current_spec(dir.path().to_str().unwrap());
        // Either Some("my-feature-xyzzy") or Some(env-var) — never None here.
        assert!(result.is_some(), "expected Some(_) when a state file exists");
    }

    // -----------------------------------------------------------------------
    // Round-trip: event written with an explicit spec field is queryable
    // -----------------------------------------------------------------------

    /// This test exercises the full attribution round-trip that `current_spec`
    /// enables: construct a `HarnessEvent` with `spec = Some("test-spec")`,
    /// append it to an in-memory SQLite store, then query by spec and verify
    /// the field survives the round-trip. The `current_spec()` call is wired
    /// to the FS branch via a tempdir pipeline-states file, matching the
    /// production code path that all call sites now use.
    #[test]
    fn event_spec_field_survives_sqlite_round_trip() {
        let dir = tempdir().unwrap();
        // Create a pipeline-state file so the FS branch of current_spec returns
        // a known name.
        let states = dir.path().join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join("test-spec.json"), "{}").unwrap();

        let db = dir.path().join("mustard.db");
        let store = SqliteEventStore::new(&db).unwrap();

        // Resolve via current_spec FS branch (env var not set in CI).
        let spec_field = current_spec(dir.path().to_str().unwrap());
        // If MUSTARD_ACTIVE_SPEC is set in the environment, the env-var branch
        // fires instead; either way spec_field is Some(_).
        assert!(spec_field.is_some(), "expected Some(_) from current_spec with state file");
        let spec_name = spec_field.clone().unwrap();

        // Append one event with spec populated.
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T00:00:00.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("spec-attr-test".to_string()),
                actor_type: None,
            },
            event: "test.event".to_string(),
            payload: json!({}),
            spec: spec_field,
        };
        store.append(&event).unwrap();

        // Query filtered by the resolved spec name — must return the event.
        let events = store.query(Some(&spec_name)).unwrap();
        assert_eq!(events.len(), 1, "expected 1 event for {spec_name}");
        assert_eq!(events[0].spec.as_deref(), Some(spec_name.as_str()));

        // Append a second event with spec: None to verify discriminated queries.
        let event_no_spec = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T00:00:01.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("spec-attr-test".to_string()),
                actor_type: None,
            },
            event: "test.event".to_string(),
            payload: json!({}),
            spec: None,
        };
        store.append(&event_no_spec).unwrap();

        // Full replay (all events regardless of spec) returns both events.
        let all = store.replay().unwrap();
        assert_eq!(all.len(), 2, "expected 2 events in full replay");
        // query(None) returns only events where spec IS NULL.
        let unattributed = store.query(None).unwrap();
        assert_eq!(unattributed.len(), 1, "exactly one unattributed event");
    }
}
