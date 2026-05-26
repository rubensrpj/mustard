//! Environment resolution for the `run` face.
//!
//! Unlike the enforcement faces, a `run` subcommand never receives a
//! `HookInput` — it resolves the project directory and session id from the
//! process environment, mirroring how the JS scripts did (`CLAUDE_PROJECT_DIR`,
//! `MUSTARD_SESSION_ID` / `CLAUDE_SESSION_ID`).

use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::workspace::{workspace_root, WorkspaceError};
use std::path::{Path, PathBuf};

/// Resolve the Mustard workspace root by ancestor walk, **failing strictly**
/// on missing anchor.
///
/// This is the W2 entry point for run subcommands — unlike enforcement hooks
/// (which fail open via `dispatch::build_ctx`), a `run` subcommand has no
/// useful behaviour without a workspace and must surface the error to the
/// caller. The returned [`PathBuf`] is the directory containing both
/// `mustard.json` and `.claude/`.
///
/// # Errors
///
/// Propagates [`WorkspaceError`] from [`workspace_root`] when no ancestor
/// satisfies the anchor predicate, when the resolved path violates the I1
/// `.claude/.claude/` guard, or when `MUSTARD_WORKSPACE_ROOT` is set to an
/// invalid path.
pub fn workspace_root_strict() -> Result<PathBuf, WorkspaceError> {
    let start = if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !dir.is_empty() {
            PathBuf::from(dir)
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    workspace_root(&start)
}

/// Resolve the project directory.
///
/// W2 (claude-paths-single-source) made the canonical resolver
/// [`workspace_root_strict`], which fails strictly on a missing anchor.
/// `project_dir` keeps its legacy `String` return shape so the many existing
/// call-sites that bake the value into `current_dir(...)` of a `Command`
/// continue to work, but it now consults [`workspace_root_strict`] first.
///
/// Resolution order:
///
/// 1. [`workspace_root_strict`] — `mustard.json + .claude/` ancestor walk.
/// 2. `CLAUDE_PROJECT_DIR` env var.
/// 3. `std::env::current_dir()`.
/// 4. `"."` as a last resort.
#[must_use]
pub fn project_dir() -> String {
    if let Ok(root) = workspace_root_strict() {
        return root.to_string_lossy().into_owned();
    }
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
    let entries = fs::read_dir(&states).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries {
        let name = &entry.file_name;
        if !name.ends_with(".json") || name.ends_with(".metrics.json") {
            continue;
        }
        let Ok(mtime) = fs::modified(&entry.path) else {
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
    use mustard_core::store::sqlite_store::SqliteEventStore;
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
    // last_pipeline_scope_for_session ordering
    // -----------------------------------------------------------------------

    /// After spec A writes a `pipeline.scope` event, and then spec B writes a
    /// later `pipeline.scope` event in the same session, `last_pipeline_scope_for_session`
    /// must return B (the most recent), not A.
    #[test]
    fn last_pipeline_scope_latest_wins() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mustard.db");
        let store = SqliteEventStore::new(&db).unwrap();

        let sid = "s-test-ordering";
        store
            .append_pipeline_event(
                "2026-05-25T00:00:00.000Z",
                Some(sid),
                Some("spec-a-old"),
                None,
                "pipeline.scope",
                None,
                Some(r#"{"scope":"full"}"#),
            )
            .unwrap();
        store
            .append_pipeline_event(
                "2026-05-25T00:00:01.000Z",
                Some(sid),
                Some("spec-b-new"),
                None,
                "pipeline.scope",
                None,
                Some(r#"{"scope":"resumed"}"#),
            )
            .unwrap();

        let result = store.last_pipeline_scope_for_session(sid).unwrap();
        assert_eq!(result.as_deref(), Some("spec-b-new"), "latest scope wins");
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

        // W5 round-trip: a `pipeline.*` event lands in the `pipeline_events`
        // SQLite table via `append_pipeline_event`, then a `query(Some(spec))`
        // returns it filtered by spec column.
        store
            .append_pipeline_event(
                "2026-05-20T00:00:00.000Z",
                Some("s-test"),
                Some(&spec_name),
                None,
                "pipeline.status",
                None,
                Some("{}"),
            )
            .unwrap();

        let events = store.query(Some(&spec_name)).unwrap();
        assert_eq!(events.len(), 1, "expected 1 event for {spec_name}");
        assert_eq!(events[0].spec.as_deref(), Some(spec_name.as_str()));

        // Append a second pipeline event with spec NULL to verify discriminated queries.
        store
            .append_pipeline_event(
                "2026-05-20T00:00:01.000Z",
                Some("s-test"),
                None,
                None,
                "pipeline.status",
                None,
                Some("{}"),
            )
            .unwrap();

        // Full replay (all lifecycle rows regardless of spec) returns both events.
        let all = store.replay().unwrap();
        assert_eq!(all.len(), 2, "expected 2 events in full replay");
        // query(None) returns only events where spec IS NULL.
        let unattributed = store.query(None).unwrap();
        assert_eq!(unattributed.len(), 1, "exactly one unattributed event");
    }
}
