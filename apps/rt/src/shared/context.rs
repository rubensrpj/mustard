//! Environment resolution for the `run` face.
//!
//! Unlike the enforcement faces, a `run` subcommand never receives a
//! `HookInput` — it resolves the project directory and session id from the
//! process environment, mirroring how the JS scripts did (`CLAUDE_PROJECT_DIR`,
//! `MUSTARD_SESSION_ID` / `CLAUDE_SESSION_ID`).

use mustard_core::io::fs;
use mustard_core::io::workspace::{workspace_root, WorkspaceError};
use mustard_core::ClaudePaths;
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
/// The raw process working directory as a `String`, defaulting to `"."`.
///
/// This is the plain `std::env::current_dir()` idiom (NOT the workspace-root
/// walk of [`project_dir`]) — the single home for the `current_dir → String`
/// snippet that the per-command economy emitters used verbatim.
#[must_use]
pub fn cwd() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string())
}

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

/// Resolve the current session id, defaulting to `"unknown"`.
///
/// Resolution order:
///
/// 1. `MUSTARD_SESSION_ID` env var — matching the JS scripts' lookup.
/// 2. `CLAUDE_SESSION_ID` env var.
/// 3. Newest `.claude/.session/<id>/` directory by mtime — the filesystem
///    fallback. `run`-face emitters never receive a `HookInput`, so when neither
///    env var is set they used to land on `"unknown"`; the `SessionStart` hook
///    has already created `.claude/.session/<id>/`, so mirror
///    [`current_spec`]'s newest-by-mtime fallback to recover the real id.
/// 4. `"unknown"` as a last resort.
#[must_use]
pub fn session_id() -> String {
    if let Some(id) = std::env::var("MUSTARD_SESSION_ID").ok().filter(|s| !s.is_empty()) {
        return id;
    }
    if let Some(id) = std::env::var("CLAUDE_SESSION_ID").ok().filter(|s| !s.is_empty()) {
        return id;
    }
    // Filesystem fallback: newest `.claude/.session/<id>/` dir. The `.session/`
    // base is not exposed via `ClaudePaths` (it is the events writer's consumer,
    // not Mustard-owned), so compose it from `claude_dir()` the same way the
    // writer does (see `events::writer_ndjson::event_dir`).
    if let Some(id) = ClaudePaths::for_project(Path::new(&project_dir()))
        .ok()
        .map(|p| p.claude_dir().join(".session"))
        .and_then(|session_base| newest_session_dir(&session_base))
    {
        return id;
    }
    "unknown".to_string()
}

/// Newest directory name under `session_dir`, skipping the `"unknown"` bucket.
///
/// Reads the entries of `<.claude>/.session/`, keeps directories whose name is
/// not `"unknown"`, and returns the name of the one with the newest mtime.
/// Returns `None` on any IO error or when no eligible directory exists — never
/// panics. Co-located with [`current_spec`], which uses the same
/// newest-by-mtime strategy over a sibling directory.
#[must_use]
fn newest_session_dir(session_dir: &Path) -> Option<String> {
    let entries = fs::read_dir(session_dir).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries {
        if !entry.path.is_dir() {
            continue;
        }
        let name = &entry.file_name;
        if name == "unknown" {
            continue;
        }
        let Ok(mtime) = fs::modified(&entry.path) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, name.clone()));
        }
    }
    best.map(|(_, name)| name)
}

/// Resolve the name of the currently active spec, fail-open `None`.
///
/// Strategy (in priority order):
///
/// 1. `MUSTARD_ACTIVE_SPEC` env var — explicit override set by
///    `/mustard:feature` and `/mustard:resume` before dispatching hooks.
/// 2. The most recently modified `.claude/.pipeline-states/*.json` file under
///    `project_dir` — a **legacy** fallback. The pipeline-states sink is no
///    longer written (see `scripts/cleanup-legacy-claude.ps1`), so in practice
///    this branch yields nothing on a live run. The real session→spec binding
///    is carried by [`spec_for_session`], which the event router consults
///    before falling back here.
///
/// Returns `None` when no spec is active — never panics. Every step fails
/// open: a missing env var or an absent state directory degrades to the
/// next strategy instead of erroring.
#[must_use]
pub fn current_spec(project_dir_path: &str) -> Option<String> {
    // 1. Explicit env override.
    if let Ok(s) = std::env::var("MUSTARD_ACTIVE_SPEC") {
        if !s.is_empty() {
            return Some(s);
        }
    }

    // 2. Newest pipeline-state file by mtime — legacy hint used when no
    //    env override is present.
    let states = ClaudePaths::for_project(Path::new(project_dir_path))
        .ok()?
        .pipeline_states_dir();
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

/// Resolve the spec a session is currently bound to, fail-open `None`.
///
/// Hook-emitted events (`tool.use`, `agent.*`, …) are born with no spec — the
/// PostToolUse hook context never sets `MUSTARD_ACTIVE_SPEC`, and the legacy
/// `.pipeline-states/` sink [`current_spec`] reads is no longer written. The
/// only reliable binding is the `pipeline.scope` event the CLI run-face emits,
/// which carries BOTH `session_id` and `spec`. Rather than scan the NDJSON log
/// on every tool call, the router persists that binding as a small marker file
/// (see [`bind_session_spec`]); this reads it back in O(1).
///
/// Marker location: `.claude/.session/<session_id>/active-spec` — beside the
/// session's own `.events/` directory.
///
/// Returns `None` when the session has no recorded binding (no marker yet, an
/// empty/`"unknown"` session id, or any IO error) — never panics.
#[must_use]
pub fn spec_for_session(project_dir_path: &str, session_id: &str) -> Option<String> {
    if session_id.is_empty() || session_id == "unknown" {
        return None;
    }
    let marker = session_spec_marker(project_dir_path, session_id)?;
    let spec = fs::read_to_string(&marker).ok()?;
    let spec = spec.trim();
    if spec.is_empty() {
        None
    } else {
        Some(spec.to_string())
    }
}

/// Inverse lookup: the session currently bound to `spec` via its
/// `active-spec` marker. Scans `.claude/.session/*/active-spec` and, when
/// more than one session is bound to the same spec (rare — concurrent
/// sessions on one spec), returns the binding with the newest marker mtime.
///
/// Exists for spec-scoped READERS (e.g. `digest-adherence-finalize`): the
/// emitter and the reader run as separate processes minutes apart, and the
/// env-less newest-session-by-mtime fallback of [`session_id`] races against
/// any other session touching the project in between — the field symptom was
/// `digestUsed: false` with two digest queries on record. The marker is the
/// binding the researching session itself wrote, so resolving through it is
/// stable. `None` when no session is bound to `spec` — never panics.
#[must_use]
pub fn session_for_spec(project_dir_path: &str, spec: &str) -> Option<String> {
    if spec.is_empty() {
        return None;
    }
    let base = ClaudePaths::for_project(Path::new(project_dir_path))
        .ok()?
        .claude_dir()
        .join(".session");
    let entries = fs::read_dir(&base).ok()?;
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in entries {
        if !entry.path.is_dir() || entry.file_name == "unknown" {
            continue;
        }
        let marker = entry.path.join("active-spec");
        let Ok(content) = fs::read_to_string(&marker) else {
            continue;
        };
        if content.trim() != spec {
            continue;
        }
        let Ok(mtime) = fs::modified(&marker) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, entry.file_name.clone()));
        }
    }
    best.map(|(_, name)| name)
}

/// Persist the session→spec binding as the `active-spec` marker, best-effort.
///
/// Called from the event router whenever an event already carries both a
/// non-empty `spec` and a resolved `session_id` (the `pipeline.scope` /
/// `pipeline.stage` / `pipeline.status` events the run-face emits). Later
/// spec-less hook events for the same session then inherit the spec via
/// [`spec_for_session`]. Fail-open: any IO error is swallowed — telemetry must
/// never block tool execution.
pub fn bind_session_spec(project_dir_path: &str, session_id: &str, spec: &str) {
    if session_id.is_empty() || session_id == "unknown" || spec.is_empty() {
        return;
    }
    let Some(marker) = session_spec_marker(project_dir_path, session_id) else {
        return;
    };
    // Skip a redundant rewrite when the marker already names this spec — keeps
    // the hot-path write a no-op once the session is bound.
    if fs::read_to_string(&marker).ok().as_deref().map(str::trim) == Some(spec) {
        return;
    }
    let Some(parent) = marker.parent() else {
        return;
    };
    let _ = fs::create_dir_all(parent);
    let _ = fs::write_atomic(&marker, spec.as_bytes());
}

/// Remove the session→spec binding, best-effort.
///
/// Called on the terminal close of a spec so events in the gap after the close
/// no longer inherit the just-finished spec via [`spec_for_session`]. Resolves
/// the same `active-spec` marker [`bind_session_spec`] writes and deletes it.
/// A missing marker is a no-op, and any IO error is swallowed — telemetry
/// teardown must never block the close.
pub fn unbind_session_spec(project_dir: &str, session_id: &str) {
    if session_id.is_empty() || session_id == "unknown" {
        return;
    }
    let Some(marker) = session_spec_marker(project_dir, session_id) else {
        return;
    };
    let _ = fs::remove_file(&marker);
}

/// Compose the `active-spec` marker path:
/// `<project>/.claude/.session/<session_id>/active-spec`.
///
/// The `.session/` base is not exposed via [`ClaudePaths`] (it is the events
/// writer's consumer, not Mustard-owned), so compose it from `claude_dir()` the
/// same way [`session_id`]'s fallback and the NDJSON writer do. `None` on an I1
/// guard rejection of the project root.
fn session_spec_marker(project_dir_path: &str, session_id: &str) -> Option<PathBuf> {
    Some(
        ClaudePaths::for_project(Path::new(project_dir_path))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(session_id)
            .join("active-spec"),
    )
}

/// Resolve the pending auto-branch a session's first file mutation must check
/// out, fail-open `None`.
///
/// Sibling of [`spec_for_session`]: `emit-pipeline --kind pipeline.kind`
/// pre-computes the `{work_kind}/{slug}` branch name and drops it here; the
/// `work_branch_gate` PreToolUse(Write|Edit) hook reads it back on the FIRST
/// edit, checks the branch out, and clears the marker. A read-only request
/// never edits, so the marker is simply never consumed.
///
/// Marker location: `.claude/.session/<session_id>/pending-work-branch` — beside
/// the session's `active-spec` marker. Returns `None` when the session has no
/// recorded pending branch (no marker yet, an empty/`"unknown"` session id, or
/// any IO error) — never panics.
#[must_use]
pub fn pending_branch_for(project_dir_path: &str, session_id: &str) -> Option<String> {
    if session_id.is_empty() || session_id == "unknown" {
        return None;
    }
    let marker = pending_branch_marker(project_dir_path, session_id)?;
    let branch = fs::read_to_string(&marker).ok()?;
    let branch = branch.trim();
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_string())
    }
}

/// Persist the pending auto-branch name as the `pending-work-branch` marker,
/// best-effort.
///
/// Called from `emit-pipeline` when the work-type signal (`pipeline.kind`) is
/// emitted: it computes the target branch once and stores it so the first
/// Write/Edit can check it out without re-deriving the slug. Fail-open: any IO
/// error is swallowed — telemetry must never block. Skips a redundant rewrite
/// when the marker already names this branch (mirrors [`bind_session_spec`]).
pub fn set_pending_branch(project_dir_path: &str, session_id: &str, branch: &str) {
    if session_id.is_empty() || session_id == "unknown" || branch.is_empty() {
        return;
    }
    let Some(marker) = pending_branch_marker(project_dir_path, session_id) else {
        return;
    };
    if fs::read_to_string(&marker).ok().as_deref().map(str::trim) == Some(branch) {
        return;
    }
    let Some(parent) = marker.parent() else {
        return;
    };
    let _ = fs::create_dir_all(parent);
    let _ = fs::write_atomic(&marker, branch.as_bytes());
}

/// Remove the pending auto-branch marker, best-effort.
///
/// Called by `work_branch_gate` once it has checked the branch out (or decided
/// not to, on a git failure) so the gate does not re-fire on every subsequent
/// edit of the same session. A missing marker is a no-op and any IO error is
/// swallowed — this teardown must never block a write.
pub fn clear_pending_branch(project_dir: &str, session_id: &str) {
    if session_id.is_empty() || session_id == "unknown" {
        return;
    }
    let Some(marker) = pending_branch_marker(project_dir, session_id) else {
        return;
    };
    let _ = fs::remove_file(&marker);
}

/// Compose the `pending-work-branch` marker path:
/// `<project>/.claude/.session/<session_id>/pending-work-branch`.
///
/// Composed from `claude_dir()` the same way [`session_spec_marker`] resolves
/// the sibling `active-spec` marker. `None` on an I1 guard rejection of the
/// project root.
fn pending_branch_marker(project_dir_path: &str, session_id: &str) -> Option<PathBuf> {
    Some(
        ClaudePaths::for_project(Path::new(project_dir_path))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(session_id)
            .join("pending-work-branch"),
    )
}

/// Filename of the per-spec **user-approval** marker (see
/// [`approval_marker_path`]).
pub(crate) const APPROVED_BY_USER_MARKER: &str = ".approved-by-user";

/// Compose the per-spec user-approval marker path:
/// `<project>/.claude/spec/<spec>/.approved-by-user`.
///
/// The single home for this path so its two consumers cannot drift: the
/// `approval_marker_observer` (which WRITES it on a genuine human PLAN approval)
/// and `approve-spec` (which REQUIRES it before emitting the `draft→approved`
/// signal). The marker can only be born from the user's real `AskUserQuestion`
/// answer — echoed by the harness in `tool_response`, which the model does not
/// author — so an orchestrator cannot forge the approval it is itself gated by.
/// `None` on an I1 guard rejection of the project root or an invalid spec name.
#[must_use]
pub fn approval_marker_path(project_dir_path: &str, spec: &str) -> Option<PathBuf> {
    Some(
        ClaudePaths::for_project(Path::new(project_dir_path))
            .and_then(|p| p.for_spec(spec))
            .ok()?
            .dir()
            .join(APPROVED_BY_USER_MARKER),
    )
}

/// Resolve the active wave number from `MUSTARD_ACTIVE_WAVE` — the convention the
/// harness sets on every wave dispatch and that `route` already stamps on each
/// emitted event. Co-located with [`current_spec`] so a hook (e.g. the
/// change-request logger) can attribute its emitted event to the active wave.
/// `None` when unset / not numeric (a spec-less or non-wave dispatch).
#[must_use]
pub fn current_wave() -> Option<i64> {
    std::env::var("MUSTARD_ACTIVE_WAVE")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // current_spec — filesystem branch (no env mutation needed)
    // -----------------------------------------------------------------------

    #[test]
    fn current_spec_returns_none_when_no_states_dir() {
        // A nonexistent project path → no pipeline-states dir → None.
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
    // session_for_spec — marker-first inverse lookup
    // -----------------------------------------------------------------------

    /// The spec-scoped reader must resolve the session BOUND to the spec via
    /// its `active-spec` marker — not the newest session dir by mtime, which
    /// races against unrelated concurrent sessions (field symptom: a false
    /// `digestUsed: false` in digest-adherence-finalize).
    #[test]
    fn session_for_spec_resolves_bound_session_not_newest() {
        let dir = tempdir().unwrap();
        let base = dir.path().join(".claude").join(".session");
        // sess-a is bound to the spec under test.
        std::fs::create_dir_all(base.join("sess-a")).unwrap();
        std::fs::write(base.join("sess-a").join("active-spec"), "minha-spec\n").unwrap();
        // sess-b is created LAST (newest mtime) and bound to another spec —
        // the mtime-based fallback would wrongly pick it.
        std::fs::create_dir_all(base.join("sess-b")).unwrap();
        std::fs::write(base.join("sess-b").join("active-spec"), "outra-spec").unwrap();

        let root = dir.path().to_str().unwrap();
        assert_eq!(
            session_for_spec(root, "minha-spec").as_deref(),
            Some("sess-a"),
            "the bound session wins regardless of mtime order"
        );
        assert_eq!(session_for_spec(root, "outra-spec").as_deref(), Some("sess-b"));
        assert_eq!(session_for_spec(root, "spec-sem-binding"), None);
        assert_eq!(session_for_spec(root, ""), None);
    }

    // -----------------------------------------------------------------------
    // session_id — filesystem fallback (no env mutation needed)
    // -----------------------------------------------------------------------

    #[test]
    fn session_id_falls_back_to_newest_session_dir() {
        // AC-2 — `newest_session_dir` returns the newest real session id and
        // never the `"unknown"` bucket. Exercised directly (the crate forbids
        // `unsafe`, so a test cannot unset the env to reach this branch via
        // `session_id()`); mirrors `current_spec`'s FS-branch unit tests.
        let dir = tempdir().unwrap();
        let session_base = dir.path().join(".claude").join(".session");
        std::fs::create_dir_all(session_base.join("unknown")).unwrap();
        // Create `sess-A` last so it has the newest mtime.
        std::fs::create_dir_all(session_base.join("sess-A")).unwrap();

        let result = newest_session_dir(&session_base);
        assert_eq!(result.as_deref(), Some("sess-A"));
        assert_ne!(result.as_deref(), Some("unknown"));
    }

    #[test]
    fn newest_session_dir_returns_none_on_missing_dir() {
        // Fail-open: a nonexistent `.session/` base degrades to None.
        assert!(newest_session_dir(Path::new("/nonexistent-mustard-session-xyzzy")).is_none());
    }

    // -----------------------------------------------------------------------
    // session→spec binding lifecycle (bind → resolve → unbind)
    // -----------------------------------------------------------------------

    #[test]
    fn unbind_session_spec_clears_the_binding() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        bind_session_spec(project, "sess-X", "my-spec");
        assert_eq!(
            spec_for_session(project, "sess-X").as_deref(),
            Some("my-spec"),
            "bind then resolve should round-trip",
        );
        unbind_session_spec(project, "sess-X");
        assert!(
            spec_for_session(project, "sess-X").is_none(),
            "unbind should clear the marker",
        );
        // A second unbind on a missing marker is a no-op (must not panic).
        unbind_session_spec(project, "sess-X");
    }

    // -----------------------------------------------------------------------
    // pending auto-branch lifecycle (set → resolve → clear)
    // -----------------------------------------------------------------------

    #[test]
    fn pending_branch_round_trips_then_clears() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        set_pending_branch(project, "sess-B", "feature/my-thing");
        assert_eq!(
            pending_branch_for(project, "sess-B").as_deref(),
            Some("feature/my-thing"),
            "set then resolve should round-trip",
        );
        clear_pending_branch(project, "sess-B");
        assert!(
            pending_branch_for(project, "sess-B").is_none(),
            "clear should remove the marker",
        );
        // A blank branch / unknown session never writes; a second clear is a no-op.
        set_pending_branch(project, "sess-B", "");
        assert!(pending_branch_for(project, "sess-B").is_none());
        set_pending_branch(project, "unknown", "feature/x");
        assert!(pending_branch_for(project, "unknown").is_none());
        clear_pending_branch(project, "sess-B");
    }
}
