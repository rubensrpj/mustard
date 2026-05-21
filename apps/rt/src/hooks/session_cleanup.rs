//! `session_cleanup` — the SessionEnd state-cleanup module.
//!
//! ## Scope (b3 Wave 5, session family)
//!
//! Ports `session-cleanup.js` **alone** — a single concern with no sibling
//! hook to merge, kept as its own module so the registry wiring is one-to-one.
//! It triggers on `SessionEnd` and:
//!
//! 1. Archives stale `closed-followup` specs (best-effort, via the B4 script
//!    `complete-spec.js --archive-stale`).
//! 2. Removes terminal pipeline-state files (`completed`, `cancelled`, …) and
//!    states whose spec is already done.
//! 3. Removes the statusline git cache from the temp dir.
//! 4. Removes `.compact-state` files older than 24h.
//! 5. Removes a stale OTEL collector PID file.
//!
//! ## Contract shape
//!
//! Pure side effect — no verdict. `SessionCleanup` is an [`Observer`] only.
//!
//! ## OTEL collector note
//!
//! `session-cleanup.js` *stopped* the OTEL collector that `harness-init.js`
//! *spawned*. The b3 port of `harness-init` (in [`crate::hooks::session_start`])
//! deliberately does **not** spawn the collector — that subprocess depends on
//! a B4 script (out of bounds). With no Rust-side spawn there is nothing for
//! this module to stop; it still removes a stale `.otel-collector.pid` file so
//! a legacy JS-spawned collector from a pre-migration install is not orphaned
//! by a leftover PID file. The actual `process.kill` is not ported — there is
//! no portable, dependency-free signal API, and the file removal alone is the
//! correct cleanup once the Rust port owns SessionStart.

use crate::run::amend_finalize;
use mustard_core::economy::{self, sources::transcript, sources::IngestContext};
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::contract::{Ctx, HookInput, Observer, Trigger};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `.compact-state` files older than this are pruned — 24 hours.
const ONE_DAY_MS: u128 = 24 * 60 * 60 * 1000;

/// Terminal pipeline-state statuses — these files are removed on cleanup.
const TERMINAL_STATUSES: &[&str] = &["implemented", "completed", "validated", "cancelled"];

/// The SessionEnd state-cleanup module.
pub struct SessionCleanup;

/// Resolve the project dir for an invocation: the harness `cwd`, else `.`.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Current time as milliseconds since the Unix epoch.
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

/// Archive stale `closed-followup` specs via the B4 script `complete-spec.js`.
/// Best-effort: a missing script or a spawn error is silently ignored — parity
/// with the JS `if (fs.existsSync(...))` guard.
fn archive_stale_followups(cwd: &str) {
    let script = Path::new(cwd)
        .join(".claude")
        .join("scripts")
        .join("complete-spec.js");
    if !script.exists() {
        return;
    }
    for runtime in ["bun", "node"] {
        let spawned = Command::new(runtime)
            .arg(&script)
            .arg("--archive-stale")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            let _ = child.wait();
            return;
        }
    }
}

/// Read the `status` field of a pipeline-state JSON file.
fn state_status(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let obj: serde_json::Value = serde_json::from_str(&text).ok()?;
    obj.get("status")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The `specName` field of a pipeline-state JSON file.
fn state_spec_name(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let obj: serde_json::Value = serde_json::from_str(&text).ok()?;
    obj.get("specName")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// `true` if a spec is done — flat layout (wave-2 of
/// `2026-05-21-flatten-spec-layout-and-multi-collab`) reads the spec dir
/// directly under `.claude/spec/{name}/`, with no `active/` / `completed/`
/// buckets. Done means either the directory is gone or the `### Status:`
/// header in `spec.md` / `wave-plan.md` reads `completed` / `done`.
fn is_spec_done(claude_dir: &Path, spec_name: &str) -> bool {
    let spec_root = claude_dir.join("spec").join(spec_name);
    if !spec_root.exists() {
        // Spec deleted → treat as done.
        return true;
    }
    let wave_plan = spec_root.join("wave-plan.md");
    if wave_plan.exists() {
        return std::fs::read_to_string(&wave_plan)
            .ok()
            .map(|t| header_marks_done(&t))
            .unwrap_or(false);
    }
    let spec_file = spec_root.join("spec.md");
    if !spec_file.exists() {
        // Spec dir empty / spec.md absent → treat as done.
        return true;
    }
    std::fs::read_to_string(&spec_file)
        .ok()
        .map(|t| header_marks_done(&t))
        .unwrap_or(false)
}

/// `true` if the first 500 chars of a spec body contain a
/// `Status: completed|done` marker. Mirrors the JS `/Status:\s*(completed|done)\b/i`.
fn header_marks_done(content: &str) -> bool {
    let head: String = content.chars().take(500).collect();
    let lower = head.to_ascii_lowercase();
    for marker in ["completed", "done"] {
        if let Some(idx) = lower.find("status:") {
            let after = lower[idx + "status:".len()..].trim_start();
            if after.starts_with(marker) {
                let boundary_ok = after
                    .as_bytes()
                    .get(marker.len())
                    .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'));
                if boundary_ok {
                    return true;
                }
            }
        }
    }
    false
}

/// Remove terminal / orphaned pipeline-state files. Port of
/// `cleanPipelineStates` (`closed-followup` is intentionally non-terminal).
fn clean_pipeline_states(claude_dir: &Path) {
    let states_dir = claude_dir.join(".pipeline-states");
    if let Ok(entries) = std::fs::read_dir(&states_dir) {
        for entry in entries.filter_map(std::result::Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".json") {
                continue;
            }
            let path = entry.path();
            if let Some(status) = state_status(&path) {
                if TERMINAL_STATUSES.contains(&status.as_str()) {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
            }
            if let Some(spec) = state_spec_name(&path) {
                if is_spec_done(claude_dir, &spec) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        // Remove the directory when empty.
        let is_empty = std::fs::read_dir(&states_dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = std::fs::remove_dir(&states_dir);
        }
    }
    // Legacy single-file state.
    let legacy = claude_dir.join(".pipeline-state.json");
    if let Some(status) = state_status(&legacy) {
        if TERMINAL_STATUSES.contains(&status.as_str()) {
            let _ = std::fs::remove_file(&legacy);
        }
    }
}

/// Remove `.compact-state` files older than 24h; remove the dir when empty.
fn clean_compact_state(claude_dir: &Path) {
    let dir = claude_dir.join(".compact-state");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let now = now_millis();
    let mut remaining = 0;
    for entry in entries.filter_map(std::result::Result::ok) {
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            remaining += 1;
            continue;
        };
        let mtime_ms = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        if now.saturating_sub(mtime_ms) > ONE_DAY_MS {
            let _ = std::fs::remove_file(entry.path());
        } else {
            remaining += 1;
        }
    }
    if remaining == 0 {
        let _ = std::fs::remove_dir(&dir);
    }
}

/// Remove the statusline git cache from the temp dir.
fn clean_statusline_cache() {
    let cache = std::env::temp_dir().join("claude-statusline-git.json");
    let _ = std::fs::remove_file(cache);
}

/// Remove a stale OTEL collector PID file.
///
/// As of Wave 3 (economia-moat-unification) `session_start` *does* spawn the
/// collector itself, but the cleanup contract stays the same: removing the PID
/// file on `SessionEnd` lets a fresh `SessionStart` re-spawn a healthy
/// collector without a stale-PID false-positive in the idempotence check.
fn clean_otel_pid(claude_dir: &Path) {
    let pid_file = claude_dir.join(".harness").join(".otel-collector.pid");
    let _ = std::fs::remove_file(pid_file);
}

// ---------------------------------------------------------------------------
// Transcript ingest — Wave 3 (economia-moat-unification)
// ---------------------------------------------------------------------------

/// Env var Claude Code uses to point hooks at the active session's transcript.
/// When present we trust it verbatim; otherwise we fall back to the conventional
/// `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` layout.
const CLAUDE_TRANSCRIPT_PATH_ENV: &str = "CLAUDE_TRANSCRIPT_PATH";

// `home_dir` and `encode_cwd` live in `crate::util` since the post-Wave-2
// tactical bundle (b3 Wave 5 review follow-up): both `session_cleanup` and
// `transcript_watcher` resolved transcript paths and drift between two copies
// of the `:`-collapsing rule would silently mismatch the path Claude Code
// writes to. Imported here, not redefined.
use crate::util::{encode_cwd, home_dir};

/// Best-effort resolution of the session transcript path.
///
/// Priority:
/// 1. `CLAUDE_TRANSCRIPT_PATH` env var when set non-empty.
/// 2. `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` when both `home`
///    and `session_id` are available.
///
/// Returns `None` when neither path is resolvable (e.g. `HOME`/`USERPROFILE`
/// unset and the harness did not pass a session id).
fn resolve_transcript_path(cwd: &str, session_id: Option<&str>) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(CLAUDE_TRANSCRIPT_PATH_ENV) {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let session = session_id.filter(|s| !s.is_empty())?;
    let home = home_dir()?;
    let encoded = encode_cwd(cwd);
    Some(
        home.join(".claude")
            .join("projects")
            .join(encoded)
            .join(format!("{session}.jsonl")),
    )
}

/// Translate the session transcript JSONL into `ApiCostFrame`s and persist via
/// the W1 writer. Fail-open at every step.
fn ingest_session_transcript(cwd: &str, session_id: Option<&str>) {
    let Some(path) = resolve_transcript_path(cwd, session_id) else {
        return;
    };
    if !path.exists() {
        // A fresh session may never have written a transcript — silent skip.
        return;
    }
    let ctx = IngestContext {
        project_path: cwd.to_string(),
        session_id: session_id.map(str::to_string),
    };
    let frames = match transcript::ingest(&path, &ctx) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "session_cleanup: transcript::ingest failed for {}: {e}",
                path.display()
            );
            return;
        }
    };
    if frames.is_empty() {
        return;
    }
    let conn = match economy::store::open_for(cwd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("session_cleanup: economy::store::open_for failed: {e}");
            return;
        }
    };
    for frame in frames {
        if let Err(e) = economy::writer::record_api_cost(&conn, frame) {
            eprintln!("session_cleanup: record_api_cost failed: {e}");
            // Keep looping — a single bad row must not lose the rest.
        }
    }
}

impl Observer for SessionCleanup {
    /// On `SessionEnd`, clean stale state. Any other trigger is a no-op. Pure
    /// side effect — never panics, never affects a verdict.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::SessionEnd) {
            return;
        }
        let cwd = project_dir(input, ctx);
        let claude = Path::new(&cwd).join(".claude");

        // Finalize open amendment windows BEFORE other cleanup.
        if let Some(session_id) = input.session_id.as_deref().filter(|s| !s.is_empty()) {
            if let Ok(store) = SqliteEventStore::for_project(&cwd) {
                let _ = amend_finalize::run(session_id, &store);
            }
        }

        // Wave 3 (economia-moat-unification): ingest the session transcript
        // into the W1 economy writer BEFORE we wipe state. Pulls one
        // `ApiCostFrame` per assistant turn out of the Claude Code JSONL so the
        // `spans` table sees the cheapest cost signal we have.
        ingest_session_transcript(&cwd, input.session_id.as_deref());

        archive_stale_followups(&cwd);
        clean_pipeline_states(&claude);
        clean_statusline_cache();
        clean_compact_state(&claude);
        clean_otel_pid(&claude);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::SessionEnd),
        }
    }

    fn session_end_input() -> HookInput {
        HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            ..HookInput::default()
        }
    }

    /// Write a pipeline-state file.
    fn write_state(dir: &Path, name: &str, state: &Value) {
        let states = dir.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join(format!("{name}.json")), state.to_string()).unwrap();
    }

    use serde_json::Value;

    #[test]
    fn non_session_end_trigger_is_noop() {
        let dir = tempdir().unwrap();
        write_state(dir.path(), "done", &json!({ "status": "completed" }));
        let other = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PreToolUse),
        };
        SessionCleanup.observe(&session_end_input(), &other);
        // PreToolUse → cleanup did not run, the terminal state survives.
        assert!(dir
            .path()
            .join(".claude/.pipeline-states/done.json")
            .exists());
    }

    #[test]
    fn terminal_states_are_removed() {
        let dir = tempdir().unwrap();
        write_state(dir.path(), "finished", &json!({ "status": "completed" }));
        write_state(dir.path(), "active-one", &json!({ "status": "implementing" }));
        SessionCleanup.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        assert!(!dir
            .path()
            .join(".claude/.pipeline-states/finished.json")
            .exists());
        // Non-terminal state survives.
        assert!(dir
            .path()
            .join(".claude/.pipeline-states/active-one.json")
            .exists());
    }

    #[test]
    fn orphaned_state_of_completed_spec_is_removed() {
        let dir = tempdir().unwrap();
        write_state(
            dir.path(),
            "orphan",
            &json!({ "status": "implementing", "specName": "old-spec" }),
        );
        // Flat layout (wave-2): the spec dir is at .claude/spec/{name}/ with a
        // `### Status: completed` header. `is_spec_done` reads the header to
        // decide the state file is orphaned and removes it.
        let spec_dir = dir.path().join(".claude/spec/old-spec");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# old-spec\n### Status: completed\n",
        )
        .unwrap();
        SessionCleanup.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        assert!(!dir
            .path()
            .join(".claude/.pipeline-states/orphan.json")
            .exists());
    }

    #[test]
    fn old_compact_state_files_are_pruned() {
        let dir = tempdir().unwrap();
        let compact = dir.path().join(".claude/.compact-state");
        std::fs::create_dir_all(&compact).unwrap();
        let old = compact.join("old.txt");
        std::fs::write(&old, "snapshot").unwrap();
        // Backdate the file well past the 24h window.
        let two_days_ago = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
        let _ = filetime_set(&old, two_days_ago);
        SessionCleanup.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        assert!(!old.exists());
    }

    #[test]
    fn otel_pid_file_is_removed() {
        let dir = tempdir().unwrap();
        let harness = dir.path().join(".claude/.harness");
        std::fs::create_dir_all(&harness).unwrap();
        let pid = harness.join(".otel-collector.pid");
        std::fs::write(&pid, "12345").unwrap();
        SessionCleanup.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        assert!(!pid.exists());
    }

    #[test]
    fn observe_is_infallible_on_empty_project() {
        let dir = tempdir().unwrap();
        // No .claude dir at all — observe must not panic.
        SessionCleanup.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
    }

    /// Best-effort mtime backdating for the compact-state test. Uses
    /// `set_modified` via `std::fs::File` — `filetime` is not a dependency, so
    /// this re-opens the file and rewrites it with an older `SystemTime` is not
    /// possible directly; instead the test relies on the OS letting us set the
    /// mtime through a `File::set_modified`-style call. When unavailable the
    /// test still exercises the no-panic path.
    fn filetime_set(path: &Path, when: SystemTime) -> std::io::Result<()> {
        let file = std::fs::OpenOptions::new().write(true).open(path)?;
        file.set_modified(when)
    }
}
