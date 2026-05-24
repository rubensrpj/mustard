//! `session_start` — the consolidated `SessionStart` lifecycle module.
//!
//! ## Scope (b3 Wave 5, session family)
//!
//! This module consolidates three JavaScript hooks, all `SessionStart`. Each
//! is a distinct *concern* kept as its own internal section — consolidation
//! regroups, it does not merge logic:
//!
//! - `harness-init.js` — bootstraps the harness event bus: ensures
//!   `.claude/.harness/` exists, prunes legacy archived sessions older than
//!   30 days, and emits a `session.start` event. The events now live in a
//!   single WAL-mode `SQLite` store (`mustard.db`), so there is no NDJSON log
//!   to rotate per session.
//! - `session-memory.js` — injects persistent memory (knowledge base,
//!   cross-session timeline, decisions, lessons) as `additionalContext`.
//! - `spec-hygiene.js` — auto-moves stale completed/cancelled specs from
//!   `spec/{name}/` (flat layout — status lives in `SQLite`, no bucket moves).
//!
//! ## Contract shape
//!
//! `harness-init` and `spec-hygiene` are pure side effects (`Observer`).
//! `session-memory` produces an `additionalContext` payload — under the JS
//! hooks that was a `console.log` of a `hookSpecificOutput`; under the
//! consolidated binary it is surfaced as a [`Verdict::Inject`] so the single
//! `emit_outcome` owns the only stdout write. `SessionStart` is a `Check`.
//!
//! ## OTEL collector spawn (Wave 3 — economia-moat-unification)
//!
//! `harness-init.js` historically spawned an OTEL collector subprocess. With
//! the b4 port complete (`mustard-rt run otel-collector`) the spawn is now
//! handled in-binary here: [`spawn_otel_collector`] detaches a child via
//! `Command::new(env::current_exe()?).args(["run","otel-collector"]).spawn()?`
//! and writes the PID to `<project>/.claude/.harness/.otel-collector.pid`.
//! Idempotence is enforced by [`is_process_alive`] — a second `SessionStart` in
//! the same project finds the PID file, sees the process still up, and skips
//! the spawn. Every failure path is fail-open: a missing exe, a spawn error,
//! or an unwritable PID file is logged via `eprintln!` and the `SessionStart`
//! payload continues unmodified.
//!
//! The opt-in transcript watcher (`mustard-rt run transcript-watcher`) is
//! spawned the same way when `MUSTARD_TRANSCRIPT_WATCH=1` is set — see
//! [`spawn_transcript_watcher`].
//!
//! ## Profile gate
//!
//! `harness-init` / `session-memory` / `spec-hygiene` each called
//! `shouldRun()` from `_lib/hook-env.js`. The dispatcher has no profile
//! awareness (see spec Concern "Profile gate") — under `MUSTARD_HOOK_PROFILE=minimal`
//! these now run where the JS auto-skipped. They are all fail-open side
//! effects with no verdict impact, so the change is observably inert.

use mustard_core::error::Error;
use mustard_core::fs;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use rusqlite::params;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::util::now_iso8601;

/// Archived sessions older than this are pruned on `SessionStart` (30 days).
const RETENTION_MS: u128 = 30 * 24 * 60 * 60 * 1000;

/// The advisory-context size cap for the injected persistent memory.
const MEMORY_MAX_CHARS: usize = 2000;
/// Knowledge entries below this confidence are not injected.
const KB_MIN_CONFIDENCE: f64 = 0.5;
/// Number of knowledge entries injected, ranked by confidence × recency.
const KB_MAX_ENTRIES: usize = 5;

/// The consolidated `SessionStart` module.
pub struct SessionStart;

// ===========================================================================
// Shared helpers
// ===========================================================================

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

// ===========================================================================
// harness-init — SessionStart event-bus bootstrap
// ===========================================================================

/// The `.claude/.harness` directory for a project.
fn harness_dir(cwd: &str) -> PathBuf {
    Path::new(cwd).join(".claude").join(".harness")
}

/// The `.claude/.harness/sessions` directory for a project.
fn sessions_dir(cwd: &str) -> PathBuf {
    harness_dir(cwd).join("sessions")
}

/// The current session id for an invocation. Mirrors `getCurrentSessionId`:
/// the `session_id` field, else `"unknown"` (the consolidated dispatcher has
/// no env-var fallback — telemetry, not load-bearing).
fn current_session_id(input: &HookInput) -> String {
    input
        .session_id
        .clone()
        .or_else(|| {
            input
                .raw
                .get("sessionId")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// `harness-init`: ensure the harness dirs exist, prune legacy archived
/// sessions, and emit a `session.start` event. The harness event bus is a
/// single WAL-mode `SQLite` store, so there is no per-session NDJSON log to
/// rotate. Pure side effect — fail-open throughout.
fn run_harness_init(input: &HookInput, cwd: &str) {
    let harness = harness_dir(cwd);
    let sessions = sessions_dir(cwd);
    let _ = fs::create_dir_all(&harness);
    let _ = fs::create_dir_all(&sessions);

    let current_id = current_session_id(input);
    // Clean up legacy NDJSON session archives; WAL needs no file rotation.
    prune_old_sessions(&sessions);

    // Emit `session.start`.
    let source = input
        .raw
        .get("source")
        .or_else(|| input.raw.get("matcher"))
        .cloned()
        .unwrap_or(Value::Null);
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: current_id,
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("harness-init".to_string()),
            actor_type: None,
        },
        event: "session.start".to_string(),
        payload: json!({ "cwd": cwd, "source": source }),
        spec: None,
    };
    let _ = SqliteEventStore::for_project(cwd).and_then(|store| store.append(&event));
}

/// Delete archived `sessions/*.jsonl` files older than the retention window.
fn prune_old_sessions(sessions_dir: &Path) {
    let Ok(entries) = fs::read_dir(sessions_dir) else {
        return;
    };
    let now = now_millis();
    for entry in entries {
        if !std::path::Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl")) {
            continue;
        }
        let Ok(modified) = fs::modified(&entry.path) else {
            continue;
        };
        let mtime_ms = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        if now.saturating_sub(mtime_ms) > RETENTION_MS {
            let _ = fs::remove_file(&entry.path);
        }
    }
}

// ===========================================================================
// OTEL collector / transcript watcher spawn (Wave 3 — economia-moat-unification)
// ===========================================================================

/// File where the spawned OTEL collector records its PID, under the project's
/// harness directory. The same path `session_cleanup` removes on `SessionEnd`.
const OTEL_PID_FILE: &str = ".otel-collector.pid";

/// Environment opt-in for the transcript watcher daemon. Set to `"1"` to have
/// `SessionStart` also spawn `mustard-rt run transcript-watcher`. Default off
/// — the watcher is best for power users / dashboards driving live ingestion.
const TRANSCRIPT_WATCH_ENV: &str = "MUSTARD_TRANSCRIPT_WATCH";

/// Spawn the local OTEL collector detached, write its PID, and skip if a live
/// PID file is already present (idempotent across `SessionStart` invocations).
///
/// Fail-open at every step: a missing `current_exe`, an unwritable PID file,
/// or a spawn error degrades to an `eprintln!` warning and the `SessionStart`
/// payload continues unmodified. Telemetry is never load-bearing.
fn spawn_otel_collector(cwd: &str) {
    let pid_path = harness_dir(cwd).join(OTEL_PID_FILE);

    // Idempotence: if a previous SessionStart spawned the collector and the
    // process is still alive, do nothing — this project already owns the port.
    // A stale PID file (process gone) is overwritten by the fresh spawn below.
    if let Some(existing) = read_pid(&pid_path) {
        if is_process_alive(existing) {
            return;
        }
    }

    // Cross-project takeover: a previous project's collector may still be
    // holding the OTLP port (its SessionEnd may not have fired, or a kill may
    // have failed). Free the port before spawning, otherwise THIS project's
    // collector fails to bind and the foreign listener silently captures this
    // project's telemetry. Best-effort, fail-open.
    free_otel_port();

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("session_start: current_exe failed ({e}); skipping OTEL collector spawn");
            return;
        }
    };

    let child = Command::new(&exe)
        .args(["run", "otel-collector"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(c) => {
            let pid = c.id();
            // Best-effort PID write — collector keeps running even if we can't persist.
            if let Err(e) = fs::create_dir_all(harness_dir(cwd)) {
                eprintln!("session_start: create_dir_all for OTEL pid file failed ({e})");
                return;
            }
            if let Err(e) = fs::write_atomic(&pid_path, pid.to_string().as_bytes()) {
                eprintln!("session_start: write OTEL pid file failed ({e})");
            }
        }
        Err(e) => {
            eprintln!("session_start: spawn `mustard-rt run otel-collector` failed ({e})");
        }
    }
}

/// Spawn the transcript-watcher daemon when [`TRANSCRIPT_WATCH_ENV`] is `"1"`.
///
/// Fire-and-forget: no PID file (the watcher is opt-in tooling, not the
/// always-on collector). Fail-open on every error path.
fn spawn_transcript_watcher() {
    if std::env::var(TRANSCRIPT_WATCH_ENV).as_deref() != Ok("1") {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "session_start: current_exe failed ({e}); skipping transcript-watcher spawn"
            );
            return;
        }
    };
    let spawned = Command::new(&exe)
        .args(["run", "transcript-watcher"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(e) = spawned {
        eprintln!("session_start: spawn `mustard-rt run transcript-watcher` failed ({e})");
    }
}

/// Read a PID from `path`. Returns `None` for any IO/parse failure.
fn read_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Free the OTLP port so THIS project's collector can bind it. Finds whatever
/// process is listening on `127.0.0.1:<port>` and kills it. The port is
/// resolved from the same `resolve_port()` the collector uses (respects
/// `MUSTARD_OTEL_PORT`), so the takeover targets the exact port the new
/// collector will bind. Best-effort and fail-open at every step — a missing
/// `netstat`/`lsof`/`kill`, an empty result, or a kill error degrades to a
/// warning and the spawn proceeds (a duplicate that fails to bind exits
/// cleanly). The idempotence check above already short-circuits when this
/// project's own healthy collector owns the port, so this only ever reaps a
/// foreign or dead listener.
fn free_otel_port() {
    let port = crate::run::otel::collector::resolve_port();
    for pid in listening_pids(port) {
        kill_pid(pid);
    }
}

/// PIDs listening on `127.0.0.1:<port>`, parsed from a platform query. Empty
/// on any failure (no tool on PATH, nothing listening, unparseable output).
fn listening_pids(port: u16) -> Vec<u32> {
    #[cfg(windows)]
    {
        // `netstat -ano` rows look like:
        //   TCP    127.0.0.1:4318    0.0.0.0:0    LISTENING    12345
        // The trailing column is the owning PID. Filter to LISTENING rows for
        // our port and parse the last whitespace-separated token.
        let query = format!("netstat -ano | findstr :{port} | findstr LISTENING");
        let out = Command::new("cmd")
            .args(["/C", &query])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) => parse_netstat_pids(&String::from_utf8_lossy(&o.stdout), port),
            Err(e) => {
                eprintln!("session_start: netstat for port {port} failed ({e})");
                Vec::new()
            }
        }
    }
    #[cfg(not(windows))]
    {
        // `lsof -ti tcp:<port>` prints one PID per line (TCP, no header).
        let out = Command::new("sh")
            .args(["-c", &format!("lsof -ti tcp:{port}")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) => parse_lsof_pids(&String::from_utf8_lossy(&o.stdout)),
            Err(e) => {
                eprintln!("session_start: lsof for port {port} failed ({e})");
                Vec::new()
            }
        }
    }
}

/// Parse owning PIDs from `netstat -ano` output, keeping only LISTENING rows
/// whose local address ends in `:<port>`. The PID is the final whitespace token.
/// Pure string parse — unit-testable without spawning `netstat`.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn parse_netstat_pids(text: &str, port: u16) -> Vec<u32> {
    let suffix = format!(":{port}");
    let mut pids = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Expect: PROTO LOCAL REMOTE STATE PID (at least 5 columns).
        if cols.len() < 5 || !cols.iter().any(|c| c.eq_ignore_ascii_case("LISTENING")) {
            continue;
        }
        // Local address is column 1; match on the :<port> suffix.
        if !cols[1].ends_with(&suffix) {
            continue;
        }
        if let Ok(pid) = cols[cols.len() - 1].parse::<u32>() {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

/// Parse PIDs from `lsof -ti` output — one PID per line. Pure string parse —
/// unit-testable without spawning `lsof`.
#[cfg_attr(not(any(unix, test)), allow(dead_code))]
fn parse_lsof_pids(text: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    for line in text.lines() {
        if let Ok(pid) = line.trim().parse::<u32>() {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

/// Best-effort, signal-free process termination via a subprocess (the crate
/// forbids `unsafe`). `cmd /C taskkill /F /PID` on Windows; `sh -c kill` on
/// POSIX. Fail-open: any error degrades to a warning.
fn kill_pid(pid: u32) {
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
    if let Err(e) = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        eprintln!("session_start: kill pid {pid} failed ({e})");
    }
}

/// `true` if a process with `pid` is currently alive on the host.
///
/// Cross-platform without `unsafe`: on Unix, sends signal `0` via `kill -0`
/// (the POSIX existence probe). On Windows, queries `tasklist /FI` for the
/// PID — slower than `OpenProcess` but `windows-sys` is not a dep and the
/// crate forbids `unsafe`. A spawn failure (no `kill`/`tasklist` on PATH)
/// degrades to `false`, which simply forces a re-spawn — safe per the
/// idempotence contract: the second collector will fail to bind the port and
/// exit, leaving the first one running.
#[must_use]
fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        // `tasklist /NH /FI "PID eq <pid>"` prints either the matching row or
        // the literal "INFO: No tasks are running…" string when absent. Probe
        // stdout for the PID itself, which appears in the matching row only.
        let pid_str = pid.to_string();
        let out = Command::new("tasklist")
            .args(["/NH", "/FI", &format!("PID eq {pid_str}")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                // The PID appears as a whitespace-separated column only when a
                // row matched; the "No tasks" message never contains the
                // numeric PID.
                text.split_whitespace().any(|tok| tok == pid_str)
            }
            _ => false,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Unknown platform — pessimistically report not-alive so the caller
        // re-spawns; a duplicate collector will fail to bind and exit cleanly.
        let _ = pid;
        false
    }
}

// ===========================================================================
// spec-hygiene — flat layout; classification helpers kept for unit tests
// ===========================================================================

/// The classification of a spec (retained for tests; hygiene is a no-op in prod).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecClass {
    /// Completed/cancelled with all checklist items done → move to completed/.
    AutoMove,
    /// `implementing` but all done, or anything else → no action.
    Silent,
}

/// Classify a spec from its `spec.md` content. Port of `classify` (test-only).
#[cfg_attr(not(test), allow(dead_code))]
fn classify_spec(content: &str) -> SpecClass {
    // 1. Status from the `### Status:` header — first word.
    let Some(status_raw) = parse_status(content) else {
        return SpecClass::Silent;
    };
    // 2. A BLOCKED concern keeps the spec put.
    if let Some(concerns) = section_body(content, "Concerns") {
        if concerns.to_ascii_uppercase().contains("BLOCKED") {
            return SpecClass::Silent;
        }
    }
    // 3. Checklist completeness.
    let checklist = section_body(content, "Checklist").unwrap_or_else(|| content.to_string());
    let checked = count_occurrences_ci(&checklist, "[x]");
    let unchecked = checklist.matches("[ ]").count();
    let total = checked + unchecked;
    let all_done = total > 0 && unchecked == 0;

    if (status_raw == "completed" || status_raw == "cancelled") && all_done {
        return SpecClass::AutoMove;
    }
    SpecClass::Silent
}

/// Parse the first word of the `### Status:` header, lowercased (test-only).
#[cfg_attr(not(test), allow(dead_code))]
fn parse_status(content: &str) -> Option<String> {
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("###") {
            let rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix("Status:") {
                let word: String = after
                    .trim_start()
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect();
                if !word.is_empty() {
                    return Some(word.to_ascii_lowercase());
                }
            }
        }
    }
    None
}

/// Extract the body of an `## <name>` section up to the next `## ` heading (test-only).
#[cfg_attr(not(test), allow(dead_code))]
fn section_body(content: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if is_h2_named(line, name) {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
    let mut body = String::new();
    for line in &lines[start..] {
        if line.starts_with("## ") {
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    Some(body)
}

/// `true` if `line` is an `## <name>` heading (case-sensitive name match,
/// word-boundaried) (test-only).
#[cfg_attr(not(test), allow(dead_code))]
fn is_h2_named(line: &str, name: &str) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = rest.trim_start();
    if !rest.starts_with(name) {
        return false;
    }
    rest.as_bytes()
        .get(name.len())
        .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'))
}

/// Count case-insensitive occurrences of `needle` in `haystack` (test-only).
#[cfg_attr(not(test), allow(dead_code))]
fn count_occurrences_ci(haystack: &str, needle: &str) -> usize {
    haystack.to_ascii_lowercase().matches(needle).count()
}

/// `spec-hygiene`: flat layout — spec status lives in the `SQLite` event store;
/// no bucket directories to move specs between (wave-2 removed them).
/// Retained as a no-op so call sites remain stable while a future wave may
/// add SQLite-driven hygiene (e.g. pruning stale orphan pipeline-state files).
/// Pure side effect — fail-open throughout. Port of `runHygiene`.
fn run_spec_hygiene(_cwd: &str) {
    // No-op under flat layout. See wave-2 of
    // `2026-05-21-flatten-spec-layout-and-multi-collab`.
}

// ===========================================================================
// session-memory — persistent-memory injection (Wave 6b: reads from SQLite)
// ===========================================================================

/// Load up to `KB_MAX_ENTRIES` knowledge patterns from `knowledge_patterns`,
/// ordered by confidence DESC, `last_seen` DESC. Patterns below `KB_MIN_CONFIDENCE`
/// are excluded at the SQL layer.
///
/// Wave 6b: reads from the `knowledge_patterns` `SQLite` table instead of
/// `knowledge.json`. The `verifiedAt` column does not exist in the new table
/// (it was a legacy JSON field only); all SQL-backed entries are treated as
/// unverified to preserve the AC-4 prefix logic until a future wave adds the
/// column.
fn load_knowledge_sql(conn: &rusqlite::Connection) -> Vec<String> {
    let sql = "SELECT pattern, confidence FROM knowledge_patterns \
               WHERE confidence >= ?1 \
               ORDER BY confidence DESC, last_seen DESC \
               LIMIT ?2";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    // KB_MAX_ENTRIES is a small compile-time constant; cast to i64 cannot wrap.
    #[allow(clippy::cast_possible_wrap)]
    let max_entries_i64 = KB_MAX_ENTRIES as i64;
    let rows = stmt.query_map(
        params![KB_MIN_CONFIDENCE, max_entries_i64],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)),
    );
    let Ok(rows) = rows else {
        return Vec::new();
    };
    // All SQL-backed entries are prefixed as unverified (verifiedAt not in schema yet).
    rows.filter_map(std::result::Result::ok)
        .map(|(pattern, _confidence)| {
            format!("- [pattern] (unverified — verify before recommending) {pattern}")
        })
        .collect()
}

/// Load the `max` most-recent rows from `memory_decisions` or `memory_lessons`,
/// formatted as `- [source] content`. Ordered by `at DESC`.
fn load_memory_sql(conn: &rusqlite::Connection, table: &str, max: usize) -> Vec<String> {
    // Table name is controlled by this module (never from user input) so
    // format! interpolation is safe here.
    let sql = format!(
        "SELECT content, source FROM {table} ORDER BY at DESC LIMIT ?1"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    // max is a small runtime count; cast to i64 cannot wrap.
    #[allow(clippy::cast_possible_wrap)]
    let max_i64 = max as i64;
    let rows = stmt.query_map(params![max_i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(std::result::Result::ok)
        .map(|(content, source)| {
            let src = source.as_deref().unwrap_or("?");
            format!("- [{src}] {content}")
        })
        .collect()
}

/// Build the persistent-memory `additionalContext` payload, or `None` when no
/// source has any content.
///
/// Wave 6b: reads all three data sources from `SQLite` tables
/// (`knowledge_patterns`, `memory_decisions`, `memory_lessons`) instead of
/// JSON files. Injection cap of [`MEMORY_MAX_CHARS`] is preserved unchanged.
fn build_memory_context(cwd: &str) -> Option<String> {
    // Resolve the DB path the same way SqliteEventStore::for_project does,
    // so we hit the same file. We open a second connection to avoid borrow
    // complications with SqliteEventStore (which holds a private Connection).
    let db_path = match std::env::var("MUSTARD_DB_PATH") {
        Ok(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => Path::new(cwd)
            .join(".claude")
            .join(".harness")
            .join("mustard.db"),
    };

    // DB might not exist yet on a fresh project — fail-open to empty.
    if !db_path.exists() {
        return None;
    }

    // Open the DB ONCE and run all three reads on it, instead of opening a
    // fresh connection per query. Fail-open to empty on an open failure.
    let Ok(conn) = rusqlite::Connection::open(&db_path) else {
        return None;
    };

    let mut parts: Vec<String> = Vec::new();

    let kb = load_knowledge_sql(&conn);
    if !kb.is_empty() {
        parts.push("## Project Knowledge".to_string());
        parts.extend(kb);
    }
    let decisions = load_memory_sql(&conn, "memory_decisions", 5);
    if !decisions.is_empty() {
        parts.push("## Recent Decisions".to_string());
        parts.extend(decisions);
    }
    let lessons = load_memory_sql(&conn, "memory_lessons", 5);
    if !lessons.is_empty() {
        parts.push("## Lessons Learned".to_string());
        parts.extend(lessons);
    }
    if parts.is_empty() {
        return None;
    }
    let mut context = parts.join("\n");
    if context.chars().count() > MEMORY_MAX_CHARS {
        context = context.chars().take(MEMORY_MAX_CHARS).collect::<String>() + "\n...truncated";
    }
    Some(format!("[Persistent Memory]\n{context}"))
}

// ===========================================================================
// Contract impls
// ===========================================================================

impl Check for SessionStart {
    /// On `SessionStart`: bootstrap the event bus, run spec hygiene, and inject
    /// persistent memory. The first two are side effects; the memory payload
    /// is the verdict — `Inject` when there is memory to surface, else `Allow`.
    ///
    /// Any non-`SessionStart` trigger self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::SessionStart) {
            return Ok(Verdict::Allow);
        }
        let cwd = project_dir(input, ctx);
        run_harness_init(input, &cwd);
        // Wave 3 (economia-moat-unification): the OTEL collector is no longer
        // an "out-of-scope spawn" — fire it detached and let `session_cleanup`
        // remove the PID file on `SessionEnd`. The transcript watcher is opt-in.
        spawn_otel_collector(&cwd);
        spawn_transcript_watcher();
        run_spec_hygiene(&cwd);
        Ok(match build_memory_context(&cwd) {
            Some(context) => Verdict::Inject { context },
            None => Verdict::Allow,
        })
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
            trigger: Some(Trigger::SessionStart),
        }
    }

    fn session_input(session_id: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some(session_id.to_string()),
            ..HookInput::default()
        }
    }

    // --- routing -----------------------------------------------------------

    #[test]
    fn non_session_start_trigger_allows() {
        let input = session_input("s1");
        let other = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        assert_eq!(
            SessionStart.evaluate(&input, &other).expect("no error"),
            Verdict::Allow
        );
    }

    // --- harness-init parity -----------------------------------------------

    #[test]
    fn harness_init_creates_dirs_and_emits_session_start() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = session_input("s-new");
        SessionStart.evaluate(&input, &ctx(project)).unwrap();
        assert!(dir.path().join(".claude/.harness/sessions").is_dir());
        let events = SqliteEventStore::for_project(project)
            .and_then(|s| s.replay())
            .unwrap();
        assert!(events.iter().any(|e| e.event == "session.start"));
    }

    #[test]
    fn harness_init_writes_session_start_to_sqlite_store() {
        // The harness event bus is a single WAL-mode SQLite store; the
        // `session.start` event lands in `mustard.db`, with no NDJSON log.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        SessionStart
            .evaluate(&session_input("new-session"), &ctx(project))
            .unwrap();
        assert!(dir.path().join(".claude/.harness/mustard.db").exists());
        assert!(!dir.path().join(".claude/.harness/events.jsonl").exists());
    }

    // --- spec-hygiene parity -----------------------------------------------

    /// Write a spec with the given `spec.md` body (flat layout — no active/ bucket).
    fn write_active_spec(dir: &Path, name: &str, body: &str) {
        let spec_dir = dir.join(".claude/spec").join(name);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), body).unwrap();
    }

    #[test]
    fn hygiene_noop_completed_spec_stays_flat() {
        // Flat layout: no bucket moves — spec stays in spec/{name}/ regardless of status.
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "done-spec",
            "# Spec\n### Status: completed | Phase: CLOSE\n\n## Checklist\n- [x] One\n- [x] Two\n",
        );
        SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join(".claude/spec/done-spec").exists());
    }

    #[test]
    fn hygiene_noop_implementing_spec_stays_flat() {
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "wip-spec",
            "# Spec\n### Status: implementing\n\n## Checklist\n- [x] One\n- [ ] Two\n",
        );
        SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join(".claude/spec/wip-spec").exists());
    }

    #[test]
    fn hygiene_noop_blocked_spec_stays_flat() {
        let dir = tempdir().unwrap();
        write_active_spec(
            dir.path(),
            "blocked-spec",
            "# Spec\n### Status: completed\n\n## Concerns\n- BLOCKED on infra\n\n## Checklist\n- [x] One\n",
        );
        SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join(".claude/spec/blocked-spec").exists());
    }

    #[test]
    fn classify_completed_all_done_is_automove() {
        assert_eq!(
            classify_spec("### Status: completed\n## Checklist\n- [x] a\n"),
            SpecClass::AutoMove
        );
        assert_eq!(
            classify_spec("### Status: cancelled\n## Checklist\n- [x] a\n"),
            SpecClass::AutoMove
        );
    }

    #[test]
    fn classify_no_status_is_silent() {
        assert_eq!(classify_spec("# Spec\nno header here\n"), SpecClass::Silent);
    }

    // --- port-takeover PID parsing -----------------------------------------

    #[test]
    fn parse_netstat_pid_from_listening_row() {
        // Real `netstat -ano` shape: PROTO LOCAL REMOTE STATE PID.
        let text = "  TCP    127.0.0.1:4318    0.0.0.0:0    LISTENING    12345\r\n";
        assert_eq!(parse_netstat_pids(text, 4318), vec![12345]);
    }

    #[test]
    fn parse_netstat_ignores_other_ports_and_states() {
        let text = "\
  TCP    127.0.0.1:4318    0.0.0.0:0    LISTENING       12345\r\n\
  TCP    127.0.0.1:9999    0.0.0.0:0    LISTENING       67890\r\n\
  TCP    127.0.0.1:4318    127.0.0.1:55000  ESTABLISHED  24680\r\n";
        // Only the LISTENING row on :4318 contributes; ESTABLISHED + :9999 drop.
        assert_eq!(parse_netstat_pids(text, 4318), vec![12345]);
    }

    #[test]
    fn parse_netstat_empty_on_no_match() {
        assert!(parse_netstat_pids("", 4318).is_empty());
        assert!(parse_netstat_pids("garbage line with no pid", 4318).is_empty());
    }

    #[test]
    fn parse_lsof_pids_one_per_line_dedup() {
        let text = "12345\n67890\n12345\n";
        assert_eq!(parse_lsof_pids(text), vec![12345, 67890]);
    }

    #[test]
    fn parse_lsof_empty_on_blank() {
        assert!(parse_lsof_pids("").is_empty());
        assert!(parse_lsof_pids("\n  \n").is_empty());
    }

    // --- session-memory parity (Wave 6b: reads from SQLite) ---------------

    /// Seed the `knowledge_patterns` table directly so `build_memory_context`
    /// can load it without going through the `memory` run subcommand.
    fn seed_knowledge(db_path: &std::path::Path, pattern: &str, confidence: f64) {
        let conn = rusqlite::Connection::open(db_path).unwrap();
        let now = now_iso8601();
        conn.execute(
            "INSERT INTO knowledge_patterns (pattern, confidence, count, last_seen, source, created_at) \
             VALUES (?1, ?2, 1, ?3, NULL, ?3)",
            rusqlite::params![pattern, confidence, now],
        )
        .unwrap();
    }

    #[test]
    fn memory_injection_surfaces_knowledge_as_inject() {
        let dir = tempdir().unwrap();
        // Run SessionStart once to create the DB + schema.
        let project = dir.path().to_str().unwrap();
        SessionStart.evaluate(&session_input("s-init"), &ctx(project)).unwrap();
        // Now seed the knowledge table.
        let db_path = dir.path().join(".claude/.harness/mustard.db");
        seed_knowledge(&db_path, "Foo: use bar", 0.9);
        let verdict = SessionStart
            .evaluate(&session_input("s"), &ctx(project))
            .unwrap();
        match verdict {
            Verdict::Inject { context } => {
                assert!(context.contains("Persistent Memory"));
                assert!(context.contains("Project Knowledge"));
                assert!(context.contains("Foo"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }

    #[test]
    fn memory_injection_allows_when_no_sources() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let verdict = SessionStart
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        // No DB yet → build_memory_context returns None → Allow.
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn low_confidence_knowledge_is_filtered() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        // Initialise DB.
        SessionStart.evaluate(&session_input("s-init"), &ctx(project)).unwrap();
        let db_path = dir.path().join(".claude/.harness/mustard.db");
        // confidence 0.2 < KB_MIN_CONFIDENCE 0.5 → must be excluded by SQL WHERE.
        seed_knowledge(&db_path, "Weak: x", 0.2);
        // Below KB_MIN_CONFIDENCE → no Project Knowledge section → Allow.
        let verdict = SessionStart
            .evaluate(&session_input("s"), &ctx(project))
            .unwrap();
        // May return Inject (session.start event already emitted) but must NOT
        // surface "Weak" in context.
        match &verdict {
            Verdict::Inject { context } => {
                assert!(
                    !context.contains("Weak"),
                    "low-confidence entry must not appear; context: {context}"
                );
            }
            Verdict::Allow => {} // also acceptable — no knowledge injected
            other => panic!("unexpected verdict: {other:?}"),
        }
    }

    #[test]
    fn session_start_injection_marks_knowledge_as_unverified() {
        // Wave 6b: all knowledge_patterns entries are unverified (no verifiedAt
        // column in the schema). Both entries must carry the unverified prefix.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        SessionStart.evaluate(&session_input("s-init"), &ctx(project)).unwrap();
        let db_path = dir.path().join(".claude/.harness/mustard.db");
        seed_knowledge(&db_path, "alpha-entry: some description", 0.9);
        let verdict = SessionStart
            .evaluate(&session_input("s"), &ctx(project))
            .unwrap();
        let context = match verdict {
            Verdict::Inject { context } => context,
            other => panic!("expected Inject, got {other:?}"),
        };
        assert!(
            context.contains("(unverified — verify before recommending) alpha-entry"),
            "SQL-backed knowledge must carry the unverified prefix; got: {context}"
        );
    }
}
