//! `session_cleanup` — the `SessionEnd` state-cleanup module.
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
//! 6. Prunes telemetry rows (`run_usage`/`usage_totals`) older than the
//!    retention window from `.harness/telemetry.db`.
//!
//! ## Contract shape
//!
//! Pure side effect — no verdict. `SessionCleanup` is an [`Observer`] only.
//!
//! ## OTEL collector note
//!
//! `session_start` spawns the OTEL collector (in
//! [`crate::hooks::session_start`]); this module tears it down on `SessionEnd`.
//! Because there is one collector per machine on the OTLP port, [`clean_otel_pid`]
//! now **kills** the process whose PID is in `.otel-collector.pid` before
//! removing the file — leaving it alive would let the next project's telemetry
//! bind to this project's lingering listener (cross-project contamination).
//! The kill is signal-free (subprocess `taskkill`/`kill`, the crate forbids
//! `unsafe`) and fail-open: a dead PID or a missing kill binary degrades to a
//! warning and the PID file is still removed.

use crate::run::amend_finalize;
use mustard_core::economy::{
    self, sources::rtk as rtk_source, sources::transcript, sources::IngestContext,
};
use mustard_core::fs;
use mustard_core::spec;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::ClaudePaths;
use mustard_core::model::contract::{Ctx, HookInput, Observer, Trigger};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `.compact-state` files older than this are pruned — 24 hours.
const ONE_DAY_MS: u128 = 24 * 60 * 60 * 1000;

/// Telemetry retention window — `run_usage`/`usage_totals` rows older than this
/// many days are pruned on `SessionEnd`. Fail-open: pruning never aborts cleanup.
const TELEMETRY_RETENTION_DAYS: i64 = 90;

/// Terminal pipeline-state statuses — these files are removed on cleanup.
const TERMINAL_STATUSES: &[&str] = &["implemented", "completed", "validated", "cancelled"];

/// The `SessionEnd` state-cleanup module.
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
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let script = paths.claude_dir().join("scripts").join("complete-spec.js");
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
    let text = fs::read_to_string(path).ok()?;
    let obj: serde_json::Value = serde_json::from_str(&text).ok()?;
    obj.get("status")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The `specName` field of a pipeline-state JSON file.
fn state_spec_name(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
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
    if fs::exists(&wave_plan) {
        return fs::read_to_string(&wave_plan)
            .ok()
            .is_some_and(|t| header_marks_done(&t));
    }
    let spec_file = spec_root.join("spec.md");
    if !fs::exists(&spec_file) {
        // Spec dir empty / spec.md absent → treat as done.
        return true;
    }
    fs::read_to_string(&spec_file)
        .ok()
        .is_some_and(|t| header_marks_done(&t))
}

/// `true` when a spec's lifecycle header resolves to the terminal `Completed`
/// outcome. Delegates to the canonical [`mustard_core::spec`] parser, so
/// the new `### Stage:`/`### Outcome:` header and every legacy `### Status:`
/// shape (`completed`/`done`/`closed`) are recognised. Fail-open: an
/// unparseable header is treated as not-done (the spec stays, its state file is
/// not reaped).
fn header_marks_done(content: &str) -> bool {
    spec::parse_state(content)
        .is_some_and(|s| s.outcome == mustard_core::Outcome::Completed)
}

/// Remove terminal / orphaned pipeline-state files. Port of
/// `cleanPipelineStates` (`closed-followup` is intentionally non-terminal).
fn clean_pipeline_states(claude_dir: &Path) {
    let states_dir = claude_dir
        .parent()
        .filter(|_| claude_dir.file_name().and_then(|s| s.to_str()) == Some(".claude"))
        .and_then(|root| ClaudePaths::for_project(root).ok())
        .map(|p| p.pipeline_states_dir());
    let Some(states_dir) = states_dir else {
        return;
    };
    if let Ok(entries) = fs::read_dir(&states_dir) {
        for entry in entries {
            if !std::path::Path::new(&entry.file_name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
                continue;
            }
            let path = &entry.path;
            if let Some(status) = state_status(path) {
                if TERMINAL_STATUSES.contains(&status.as_str()) {
                    let _ = fs::remove_file(path);
                    continue;
                }
            }
            if let Some(spec) = state_spec_name(path) {
                if is_spec_done(claude_dir, &spec) {
                    let _ = fs::remove_file(path);
                }
            }
        }
        // Remove the directory when empty.
        let is_empty = fs::read_dir(&states_dir)
            .is_ok_and(|d| d.is_empty());
        if is_empty {
            // std::fs::remove_dir has no facade equivalent — one-off use is fine.
            let _ = std::fs::remove_dir(&states_dir);
        }
    }
    // Legacy single-file state.
    let legacy = claude_dir.join(".pipeline-state.json");
    if let Some(status) = state_status(&legacy) {
        if TERMINAL_STATUSES.contains(&status.as_str()) {
            let _ = fs::remove_file(&legacy);
        }
    }
}

/// Remove `.compact-state` files older than 24h; remove the dir when empty.
fn clean_compact_state(claude_dir: &Path) {
    let dir = claude_dir.join(".compact-state");
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let now = now_millis();
    let mut remaining = 0;
    for entry in entries {
        let Ok(modified) = fs::modified(&entry.path) else {
            remaining += 1;
            continue;
        };
        let mtime_ms = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        if now.saturating_sub(mtime_ms) > ONE_DAY_MS {
            let _ = fs::remove_file(&entry.path);
        } else {
            remaining += 1;
        }
    }
    if remaining == 0 {
        // std::fs::remove_dir has no facade equivalent — one-off use is fine.
        let _ = std::fs::remove_dir(&dir);
    }
}

/// Remove the statusline git cache from the temp dir.
fn clean_statusline_cache() {
    let cache = std::env::temp_dir().join("claude-statusline-git.json");
    let _ = fs::remove_file(&cache);
}

/// Kill this project's OTEL collector (if any) and remove its PID file.
///
/// As of the cross-project telemetry-contamination fix, `SessionEnd` must
/// actually terminate the collector process — not merely drop the PID file.
/// There is one collector per machine on the OTLP port; leaving project A's
/// collector alive when the user moves to project B means B's telemetry binds
/// to A's lingering listener and lands in A's `telemetry.db`. Killing on
/// SessionEnd guarantees the live collector always belongs to the active
/// project. Fail-open: a kill failure (process already gone, no `taskkill`/
/// `kill` on PATH) degrades to a warning and we still remove the PID file.
fn clean_otel_pid(claude_dir: &Path) {
    let harness_dir = claude_dir
        .parent()
        .filter(|_| claude_dir.file_name().and_then(|s| s.to_str()) == Some(".claude"))
        .and_then(|root| ClaudePaths::for_project(root).ok())
        .map(|p| p.harness_dir());
    let Some(harness_dir) = harness_dir else {
        return;
    };
    let pid_file = harness_dir.join(".otel-collector.pid");
    if let Some(pid) = read_pid(&pid_file) {
        kill_pid(pid);
    }
    let _ = fs::remove_file(&pid_file);
}

/// Read a PID from `path`. Returns `None` for any IO/parse failure.
fn read_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Best-effort, signal-free process termination via a subprocess (the crate
/// forbids `unsafe`, so no raw signal API). `cmd /C taskkill /F /PID` on
/// Windows; `sh -c kill` on POSIX. Fail-open: any error is dropped — telemetry
/// teardown must never abort session cleanup.
fn kill_pid(pid: u32) {
    let _ = spawn_kill(pid);
}

/// Spawn the platform kill command for `pid`, waiting for it to complete.
fn spawn_kill(pid: u32) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", &format!("taskkill /F /PID {pid}")]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", &format!("kill {pid}")]);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|_| ())
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
    // Wave 2 (telemetry-separation): `record_api_cost` now writes `run_usage`
    // in telemetry.db, so open the dedicated telemetry store (not mustard.db).
    let store = match mustard_core::telemetry::TelemetryStore::for_project(cwd) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("session_cleanup: TelemetryStore::for_project failed: {e}");
            return;
        }
    };
    for frame in frames {
        if let Err(e) = economy::writer::record_api_cost(store.conn(), frame) {
            eprintln!("session_cleanup: record_api_cost failed: {e}");
            // Keep looping — a single bad row must not lose the rest.
        }
    }
}

/// Pull every `rtk gain --json` rewrite into the W1 `savings_records` table
/// once per session.
///
/// Mirrors [`crate::run::rtk_gain`]'s own `persist_savings()` — same
/// [`IngestContext`], same fail-open `eprintln!` blocks, same write loop via
/// [`economy::writer::record_savings`]. We re-use the shared `rtk_source`
/// adapter rather than duplicating the JSON-parsing logic.
///
/// Fail-open: a missing `rtk` binary, an empty record set, a connection
/// failure, or a row insert error each degrade to an `eprintln!` + continue.
/// SessionEnd cleanup must never abort because the RTK ledger could not be
/// drained.
fn ingest_rtk_savings(cwd: &str, session_id: Option<&str>) {
    let ctx = IngestContext {
        project_path: cwd.to_string(),
        session_id: session_id.map(str::to_string),
    };

    let records = match rtk_source::ingest(&ctx) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("session_cleanup: rtk_source::ingest failed ({e}); skipping persist");
            return;
        }
    };
    if records.is_empty() {
        return;
    }

    let conn = match economy::store::open_for(cwd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("session_cleanup: economy::store::open_for failed ({e}); skipping rtk persist");
            return;
        }
    };
    for rec in records {
        if let Err(e) = economy::writer::record_savings(&conn, rec) {
            eprintln!("session_cleanup: record_savings failed: {e}");
            // Keep looping — a single bad row must not lose the rest.
        }
    }
}

/// Prune telemetry rows older than [`TELEMETRY_RETENTION_DAYS`] from the
/// dedicated telemetry store (`.harness/telemetry.db`). Best-effort: a
/// store-open failure or a prune error is dropped — telemetry retention must
/// never abort session cleanup.
fn prune_telemetry(cwd: &str) {
    let Ok(store) = mustard_core::telemetry::TelemetryStore::for_project(cwd) else {
        return;
    };
    let now_ms = now_millis().min(i64::MAX as u128) as i64;
    let _ = mustard_core::telemetry::writer::prune_older_than_days(
        store.conn(),
        TELEMETRY_RETENTION_DAYS,
        now_ms,
    );
}

impl Observer for SessionCleanup {
    /// On `SessionEnd`, clean stale state. Any other trigger is a no-op. Pure
    /// side effect — never panics, never affects a verdict.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::SessionEnd) {
            return;
        }
        let cwd = project_dir(input, ctx);
        let Ok(paths) = ClaudePaths::for_project(Path::new(&cwd)) else {
            return;
        };
        let claude = paths.claude_dir();

        // Finalize open amendment windows BEFORE other cleanup.
        if let Some(session_id) = input.session_id.as_deref().filter(|s| !s.is_empty()) {
            if let Ok(store) = SqliteEventStore::for_project(&cwd) {
                let _ = amend_finalize::run(session_id, &store);
            }
        }

        // Wave 2 (economia-didatica-e-economias-reais): drain the local
        // `rtk gain --json` ledger into `savings_records` once per session.
        // Mirrors the per-invocation persistence already done by
        // `mustard-rt run rtk-gain`, but for sessions that never explicitly
        // run that subcommand — without this hook, RTK rewrites never land in
        // the W1 savings table. Strict side-effect, fail-open.
        ingest_rtk_savings(&cwd, input.session_id.as_deref());

        // Wave 3 (economia-moat-unification): ingest the session transcript
        // into the W1 economy writer BEFORE we wipe state. Pulls one
        // `ApiCostFrame` per assistant turn out of the Claude Code JSONL so the
        // `run_usage` table sees the cheapest cost signal we have.
        ingest_session_transcript(&cwd, input.session_id.as_deref());

        // Telemetry retention: drop `run_usage`/`usage_totals` rows past the
        // window so telemetry.db does not grow without bound. Fail-open.
        prune_telemetry(&cwd);

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
            workspace_root: None,
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
        let paths = ClaudePaths::for_project(dir).unwrap();
        let states = paths.pipeline_states_dir();
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(paths.pipeline_state_file(name), state.to_string()).unwrap();
    }

    use serde_json::Value;

    #[test]
    fn non_session_end_trigger_is_noop() {
        let dir = tempdir().unwrap();
        write_state(dir.path(), "done", &json!({ "status": "completed" }));
        let other = Ctx {
            project_dir: dir.path().to_string_lossy().into_owned(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        SessionCleanup.observe(&session_end_input(), &other);
        // PreToolUse → cleanup did not run, the terminal state survives.
        assert!(ClaudePaths::for_project(dir.path()).unwrap().pipeline_state_file("done").exists());
    }

    #[test]
    fn terminal_states_are_removed() {
        let dir = tempdir().unwrap();
        write_state(dir.path(), "finished", &json!({ "status": "completed" }));
        write_state(dir.path(), "active-one", &json!({ "status": "implementing" }));
        SessionCleanup.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        assert!(!paths.pipeline_state_file("finished").exists());
        // Non-terminal state survives.
        assert!(paths.pipeline_state_file("active-one").exists());
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
        let paths = ClaudePaths::for_project(dir.path()).unwrap();
        let sp = paths.for_spec("old-spec").unwrap();
        std::fs::create_dir_all(sp.dir()).unwrap();
        std::fs::write(
            sp.spec_md_path(),
            "# old-spec\n### Status: completed\n",
        )
        .unwrap();
        SessionCleanup.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        assert!(!paths.pipeline_state_file("orphan").exists());
    }

    #[test]
    fn old_compact_state_files_are_pruned() {
        let dir = tempdir().unwrap();
        let compact = ClaudePaths::for_project(dir.path()).unwrap().claude_dir().join(".compact-state");
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
        let harness = ClaudePaths::for_project(dir.path()).unwrap().harness_dir();
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
