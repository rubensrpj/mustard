//! `session_stop_observer` — `Stop` lifecycle observer (W9.T9.2).
//!
//! The harness fires `Stop` when the user interrupts the session (Ctrl+C or an
//! explicit `/stop`). We treat that as a soft signal: if there has been a
//! recent file edit in this project, the user almost certainly walked away mid
//! task — so we persist a single `agent_memory` row with a coarse
//! `"interrupted at wave N"` summary. The row gives the next session something
//! to surface in `pre_compact` / `session_start` so the user is reminded where
//! they left off.
//!
//! ## Anti-spam
//!
//! A flapping Stop (user mashes Ctrl+C, or the harness double-fires) would
//! pollute `agent_memory` with duplicates. We persist a marker file
//! `<project>/.claude/.harness/.last-stop` and skip the insert when the
//! previous Stop was less than [`STOP_ANTISPAM_SECS`] (300 s = 5 min) ago.
//!
//! ## Fail-open
//!
//! Pure [`Observer`] — never blocks. Every IO step (marker read/write, DB
//! open, insert) degrades to a no-op on error.

use serde_json::json;
use mustard_core::domain::model::event::ActorKind;
use crate::shared::events::economy;
use mustard_core::io::fs;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Anti-spam window between consecutive Stop captures. Five minutes — long
/// enough to absorb a double-fire, short enough that two distinct interrupts
/// inside the same long-running task still each leave a row.
const STOP_ANTISPAM_SECS: u64 = 5 * 60;

/// Edit-recency window: only persist when at least one tracked path under the
/// project tree was modified inside this window. Mirrors the rough definition
/// of "the user was actively editing".
const EDIT_RECENCY_SECS: u64 = 15 * 60;

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

/// `true` when there is evidence of recent harness activity — a harness event
/// written inside the edit-recency window.
///
/// Every harness event now appends to a per-spec or per-session
/// `.events/*.ndjson` file (the SQLite-WAL probe was retired with `mustard.db`).
/// We take the newest mtime among those files as a coarse "the harness wrote
/// something recently" signal without maintaining a dedicated marker. Falls back
/// to `false` when there are no event files (a brand-new project on first Stop).
fn recent_edit(cwd: &str, now: SystemTime) -> bool {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return false;
    };
    let Some(modified) = newest_event_write(&paths) else {
        return false;
    };
    match now.duration_since(modified) {
        Ok(elapsed) => elapsed < Duration::from_secs(EDIT_RECENCY_SECS),
        // Clock skew → treat as recent (fail closed, like `recently_stopped`).
        Err(_) => true,
    }
}

/// Newest mtime among the harness's NDJSON event files, or `None` when there are
/// none. Scans the per-spec (`<root>/.claude/spec/*/.events/`) and per-session
/// (`<root>/.claude/.session/*/.events/`) sinks — the two places `route::emit`
/// lands events after the SQLite retirement.
fn newest_event_write(paths: &ClaudePaths) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    let roots = [paths.spec_dir(), paths.claude_dir().join(".session")];
    for root in roots {
        let Ok(dirs) = fs::read_dir(&root) else {
            continue;
        };
        for d in dirs {
            if !d.is_dir {
                continue;
            }
            let events_dir = d.path.join(".events");
            let Ok(entries) = fs::read_dir(&events_dir) else {
                continue;
            };
            for e in entries {
                let is_ndjson = Path::new(&e.file_name)
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("ndjson"));
                if !is_ndjson {
                    continue;
                }
                if let Ok(m) = fs::modified(&e.path) {
                    if newest.map_or(true, |n| m > n) {
                        newest = Some(m);
                    }
                }
            }
        }
    }
    newest
}

/// Build the `interrupted at wave N` summary line for a given wave token.
/// Empty / missing → `"?"`.
fn format_summary(wave: Option<&str>) -> String {
    let w = wave
        .filter(|s| !s.is_empty())
        .unwrap_or("?");
    format!("interrupted at wave {w}")
}

/// Resolve the active wave from `MUSTARD_ACTIVE_WAVE` and build the summary.
/// Falls back to `"?"` when unset.
fn build_summary() -> String {
    let wave = std::env::var("MUSTARD_ACTIVE_WAVE").ok();
    format_summary(wave.as_deref())
}

/// Persist the interrupted-at row via the W7 helper. Fail-open at every step.
fn persist_interrupted(cwd: &str, summary: &str, session_id: Option<&str>) {
    // W4B migration: persist as `.claude/memory/agent/{slug}.md` via the
    // shared helper (no SQLite).
    let spec = crate::shared::context::current_spec(cwd);
    let wave_num: Option<i64> = std::env::var("MUSTARD_ACTIVE_WAVE")
        .ok()
        .and_then(|s| s.parse::<i64>().ok());
    let role = std::env::var("MUSTARD_ACTIVE_WAVE_ROLE")
        .ok()
        .filter(|s| !s.is_empty());
    let _ = crate::commands::knowledge::memory::persist_agent_memory_md(
        cwd,
        session_id,
        spec.as_deref(),
        wave_num,
        role.as_deref(),
        summary,
        None,
        0.7,
        Some("active"),
    );
}

/// Emit `pipeline.economy.operation.invoked` for the capture. Fail-open.

impl Observer for SessionStopObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let cwd = ctx.project_dir_or_cwd(input);
        let now = SystemTime::now();

        // Anti-spam — bail if the previous Stop fired inside the window.
        let marker = marker_path(&cwd);
        if recently_stopped(&marker, now) {
            return;
        }

        // Only persist when there is an edit recent enough to call this an
        // "in-progress" interruption. A bare Stop with no recent edit is
        // almost certainly the user closing a passive read session — no row.
        if !recent_edit(&cwd, now) {
            // Still touch the marker so we don't re-evaluate every Stop in a
            // rapid sequence — same anti-spam contract.
            touch_marker(&marker);
            return;
        }

        let summary = build_summary();
        let session_id = input
            .session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        persist_interrupted(&cwd, &summary, session_id.as_deref());
        touch_marker(&marker);
        economy::emit(&cwd, ActorKind::Hook, "stop", "pipeline.economy.operation.invoked", None, json!({"operation": "stop.persist_interrupted", "duration_ms": 0, "tokens_used": 0}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::Trigger;
    use tempfile::tempdir;

    fn input() -> HookInput {
        HookInput {
            hook_event_name: Some("Stop".to_string()),
            session_id: Some("s-stop".to_string()),
            ..HookInput::default()
        }
    }

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::Stop),
            workspace_root: None,
        }
    }

    #[test]
    fn no_recent_edit_skips_insert_but_touches_marker() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/.harness")).unwrap();
        SessionStopObserver.observe(&input(), &ctx(project));
        assert!(marker_path(project).exists(), "marker should be touched");
        // No event files → no recent edit → no interrupted-memory row was written.
        assert!(!dir.path().join(".claude/memory/agent").exists(), "no memory row");
    }

    #[test]
    fn antispam_skips_second_stop_inside_window() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        // Plant a fresh NDJSON event so the recent-edit probe fires (the SQLite
        // WAL probe was retired — recency now reads the `.events/*.ndjson` sink).
        let events = dir.path().join(".claude/.session/s-1/.events");
        std::fs::create_dir_all(&events).unwrap();
        fs::write_atomic(&events.join("now.ndjson"), b"{}\n").unwrap();
        SessionStopObserver.observe(&input(), &ctx(project));
        let first_modified = fs::modified(&marker_path(project)).unwrap();
        // Second invocation — must be a no-op (marker mtime unchanged).
        SessionStopObserver.observe(&input(), &ctx(project));
        let second_modified = fs::modified(&marker_path(project)).unwrap();
        assert_eq!(first_modified, second_modified);
    }

    #[test]
    fn format_summary_uses_wave_when_present() {
        assert_eq!(format_summary(Some("9")), "interrupted at wave 9");
        assert_eq!(format_summary(None), "interrupted at wave ?");
        assert_eq!(format_summary(Some("")), "interrupted at wave ?");
    }
}
