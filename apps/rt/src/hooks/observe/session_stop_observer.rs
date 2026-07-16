//! `session_stop_observer` — `Stop` lifecycle observer (W9.T9.2).
//!
//! The harness fires `Stop` when the **main orchestrator** finishes a turn
//! (and on an explicit interrupt). This observer keeps the **anti-spam
//! marker**: it touches `.claude/.harness/.last-stop` so the Stop-adjacent
//! double-fire bookkeeping other logic relies on keeps working (a 5-minute
//! window absorbs a double-fire).
//!
//! The orchestrator `<MEMORY>` knowledge capture that used to live here was
//! retired with the Mustard knowledge store — durable prose knowledge is
//! Claude Code's native auto-memory now, and decision/lesson capture happens
//! as `decision`/`lesson` events emitted by `/close` via `run emit-event`.
//!
//! ## Scope
//!
//! Registered ONLY for [`Trigger::Stop`](mustard_core::domain::model::contract::Trigger::Stop)
//! (the main session), and additionally skips any input the harness marks as
//! a subagent ([`HookInput::is_subagent`]) — a subagent stop must not advance
//! the main session's anti-spam window.
//!
//! ## Fail-open
//!
//! Pure [`Observer`] — never blocks. Every IO step degrades to a no-op on
//! error; no `unwrap`/`expect` outside tests.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Anti-spam window between consecutive Stop fires. Five minutes — long enough
/// to absorb a double-fire, short enough that two distinct interrupts inside the
/// same long-running task each advance the window.
const STOP_ANTISPAM_SECS: u64 = 5 * 60;

/// The `Stop` lifecycle observer.
pub struct SessionStopObserver;

/// Path to the anti-spam marker file under the project's harness directory.
fn marker_path(cwd: &str) -> PathBuf {
    ClaudePaths::for_project(cwd)
        .map(|p| p.harness_dir().join(".last-stop"))
        .unwrap_or_default()
}

/// `true` when the previous Stop fired less than [`STOP_ANTISPAM_SECS`] ago.
fn recently_stopped(marker: &Path, now: SystemTime) -> bool {
    let Ok(modified) = fs::modified(marker) else {
        return false;
    };
    let Ok(elapsed) = now.duration_since(modified) else {
        // Clock skew → treat as recent (fail closed against spam).
        return true;
    };
    elapsed < Duration::from_secs(STOP_ANTISPAM_SECS)
}

/// Persist the marker file (best-effort; missing dir → create).
fn touch_marker(marker: &Path) {
    if let Some(parent) = marker.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write_atomic(marker, b"");
}

impl Observer for SessionStopObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        // Belt-and-braces: this observer is registered for `Trigger::Stop`
        // (main session) only — a subagent stop never advances the window.
        if input.is_subagent() {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        let now = SystemTime::now();

        // Anti-spam — bail if the previous Stop fired inside the window.
        let marker = marker_path(&cwd);
        if recently_stopped(&marker, now) {
            return;
        }
        touch_marker(&marker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::Trigger;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::Stop),
            workspace_root: None,
        }
    }

    fn stop_input(session: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some(session.to_string()),
            ..HookInput::default()
        }
    }

    #[test]
    fn stop_touches_anti_spam_marker() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/.harness")).unwrap();
        SessionStopObserver.observe(&stop_input("s-stop"), &ctx(project));
        assert!(marker_path(project).exists(), "marker should be touched");
    }

    #[test]
    fn antispam_skips_second_stop_inside_window() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        SessionStopObserver.observe(&stop_input("s-1"), &ctx(project));
        let first = fs::modified(&marker_path(project)).unwrap();
        SessionStopObserver.observe(&stop_input("s-1"), &ctx(project));
        let second = fs::modified(&marker_path(project)).unwrap();
        assert_eq!(first, second, "second Stop inside the window is a no-op");
    }

    #[test]
    fn subagent_stop_input_never_touches_the_marker() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let mut input = stop_input("s-sub");
        input.agent_id = Some("explore-42".to_string());
        SessionStopObserver.observe(&input, &ctx(project.to_str().unwrap()));
        assert!(
            !marker_path(project.to_str().unwrap()).exists(),
            "subagent input is skipped"
        );
    }
}
