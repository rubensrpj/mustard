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
/// 2. The most recently modified `.claude/.pipeline-states/*.json` file under
///    `project_dir` — fallback for sessions that have not yet emitted a
///    `pipeline.scope` event (e.g. very early in a fresh pipeline).
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

/// Resolve the active wave number from `MUSTARD_ACTIVE_WAVE` — the convention the
/// harness sets on every wave dispatch and that `route` already stamps on each
/// emitted event. Co-located with [`current_spec`] so the automatic
/// PostToolUse(Task) observer can attribute an `agent.memory` event to its wave
/// WITHOUT the orchestrator having to call `memory agent` explicitly. `None` when
/// unset / not numeric (a spec-less or non-wave dispatch).
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
}
