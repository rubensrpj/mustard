//! `session_start_inject` — the consolidated `SessionStart` lifecycle module.
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
//! `emit_outcome` owns the only stdout write. `SessionStartInject` is a `Check`.
//!
//! ## OTEL collector spawn (Wave 3 — economia-moat-unification)
//!
//! `harness-init.js` historically spawned an OTEL collector subprocess. With
//! the b4 port complete (`mustard-rt run otel-collector`) the spawn is now
//! handled in-binary here: [`spawn_otel_collector`] detaches a child via
//! `Command::new(context::current_exe()?).args(["run","otel-collector"]).spawn()?`
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

use mustard_core::io::atomic_md::MarkdownStore;
use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

use mustard_core::time::now_iso8601;

/// Archived sessions older than this are pruned on `SessionStart` (30 days).
const RETENTION_MS: u128 = 30 * 24 * 60 * 60 * 1000;

/// The advisory-context size cap for the injected persistent memory.
const MEMORY_MAX_CHARS: usize = 2000;
/// Number of knowledge entries injected from `.claude/knowledge/*.md`.
const KB_MAX_ENTRIES: usize = 5;

/// W3B injection caps — total budget per memory section.
/// Top-3 entries from the active-spec context, plus top-2 global fallbacks.
/// W4B will add frontmatter-based ranking; for now filesystem order is used.
const SPEC_SCOPED_MAX: usize = 3;
const GLOBAL_FALLBACK_MAX: usize = 2;

/// The consolidated `SessionStart` module.
pub struct SessionStartInject;

// ===========================================================================
// harness-init — SessionStart event-bus bootstrap
// ===========================================================================

/// The `.claude/.harness` directory for a project.
fn harness_dir(cwd: &str) -> PathBuf {
    ClaudePaths::for_project(cwd)
        .map(|p| p.harness_dir())
        .unwrap_or_default()
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
    // `session.start` is non-pipeline → per-spec NDJSON (or session fallback
    // when there is no active spec yet) via the W5 router.
    let _ = crate::shared::events::route::emit(cwd, &event);
}

/// Delete archived `sessions/*.jsonl` files older than the retention window.
fn prune_old_sessions(sessions_dir: &Path) {
    let Ok(entries) = fs::read_dir(sessions_dir) else {
        return;
    };
    let now = mustard_core::time::now_unix_millis() as u128;
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

    // Idempotence + rebuild detection: if a previous SessionStart spawned the
    // collector and the process is still alive, normally we skip. BUT a stale
    // daemon from an older `mustard-rt.exe` build keeps an exclusive file lock
    // on the binary that traps any subsequent `cargo test`/`cargo build`. So
    // compare the running exe's mtime with the PID-file's mtime: if the exe is
    // newer than the PID file, a rebuild has happened since the spawn — kill
    // the stale daemon and respawn fresh. Otherwise the existing daemon is
    // current; honour the idempotence contract and skip.
    if let Some(existing) = read_pid(&pid_path) {
        if crate::shared::proc::is_process_alive(existing) {
            if exe_rebuilt_since_pid_file(&pid_path) {
                eprintln!(
                    "session_start: OTEL collector PID {existing} predates current exe; killing stale daemon and respawning"
                );
                crate::shared::proc::kill_pid(existing);
            } else {
                return;
            }
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

/// `true` when the running `mustard-rt` executable is more recent than the
/// PID file at `pid_path`. Used to detect a rebuild after the last spawn so
/// the daemon (which holds an exclusive lock on `target/debug/mustard-rt.exe`
/// on Windows) does not strand subsequent `cargo test`/`cargo build` runs.
/// Fail-open: any IO error degrades to `false`, preserving prior idempotent
/// behaviour for callers.
#[must_use]
fn exe_rebuilt_since_pid_file(pid_path: &Path) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Ok(exe_meta) = std::fs::metadata(&exe) else {
        return false;
    };
    let Ok(pid_meta) = std::fs::metadata(pid_path) else {
        return false;
    };
    let Ok(exe_mtime) = exe_meta.modified() else {
        return false;
    };
    let Ok(pid_mtime) = pid_meta.modified() else {
        return false;
    };
    exe_mtime > pid_mtime
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
    let port = crate::commands::economy::otel::collector::resolve_port();
    crate::shared::proc::free_port(port);
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
// session-memory — persistent-memory injection (W3B: reads from MarkdownStore)
// ===========================================================================

/// Load knowledge entries from `.claude/knowledge/*.md` via `MarkdownStore::scan_dir`.
///
/// W3B: replaces the `knowledge_patterns` SQLite table. Each `.md` file in the
/// knowledge dir is treated as one pattern entry. The file body is not read
/// (scan_dir is lazy); the frontmatter `name` and `description` fields (when
/// present) are used as the pattern text. Files without frontmatter use the
/// filename stem as a fallback label. Accepts an empty or missing directory —
/// returns an empty vec (top-N can be empty until W4B populates the content).
fn load_knowledge_md(knowledge_dir: &Path) -> Vec<String> {
    let docs = MarkdownStore::scan_dir(knowledge_dir);
    docs.into_iter()
        .take(KB_MAX_ENTRIES)
        .map(|doc| {
            let label = doc
                .frontmatter
                .as_ref()
                .and_then(|fm| fm.get_str("name").or_else(|| fm.get_str("description")))
                .map(str::to_string)
                .unwrap_or_else(|| {
                    doc.path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string()
                });
            format!("- [pattern] (unverified — verify before recommending) {label}")
        })
        .collect()
}

/// Load memory entries from a `.claude/{knowledge|memory}/*.md` directory.
///
/// W3B: replaces the `memory_decisions` / `memory_lessons` SQLite tables.
/// Each `.md` file is one memory entry. The frontmatter `name` field (or
/// filename stem) becomes the label; the frontmatter `description` is the
/// content snippet. Accepts an empty or missing directory — returns an empty
/// vec. Top-N ordering is filesystem-order (deterministic); W4B will add
/// frontmatter-based ranking when the files carry score metadata.
fn load_memory_md(memory_dir: &Path, max: usize) -> Vec<String> {
    let docs = MarkdownStore::scan_dir(memory_dir);
    docs.into_iter()
        .take(max)
        .map(|doc| {
            let name = doc
                .frontmatter
                .as_ref()
                .and_then(|fm| fm.get_str("name"))
                .map(str::to_string)
                .unwrap_or_else(|| {
                    doc.path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string()
                });
            let description = doc
                .frontmatter
                .as_ref()
                .and_then(|fm| fm.get_str("description"))
                .unwrap_or(&name)
                .to_string();
            format!("- [memory] {description}")
        })
        .collect()
}

/// Build the persistent-memory `additionalContext` payload, or `None` when no
/// source has any content.
///
/// W3B: reads all memory sources from the filesystem via `MarkdownStore::scan_dir`
/// instead of SQLite tables. Directories:
/// - `.claude/knowledge/` → Project Knowledge (replaces `knowledge_patterns`)
/// - `.claude/memory/` → Recent Decisions + Lessons Learned (replaces
///   `memory_decisions` / `memory_lessons`)
///
/// Empty or absent directories are treated as zero entries — fail-open throughout.
/// The injection cap [`MEMORY_MAX_CHARS`] is preserved unchanged. Top-N budget
/// is [`SPEC_SCOPED_MAX`] + [`GLOBAL_FALLBACK_MAX`] entries per section.
fn build_memory_context(cwd: &str) -> Option<String> {
    let Ok(paths) = ClaudePaths::for_project(cwd) else {
        return None;
    };
    let claude_dir = paths.claude_dir();
    let knowledge_dir = claude_dir.join("knowledge");
    let memory_dir = claude_dir.join("memory");

    let mut parts: Vec<String> = Vec::new();

    let kb = load_knowledge_md(&knowledge_dir);
    if !kb.is_empty() {
        parts.push("## Project Knowledge".to_string());
        parts.extend(kb);
    }

    let total_max = SPEC_SCOPED_MAX + GLOBAL_FALLBACK_MAX;
    let decisions = load_memory_md(&memory_dir, total_max);
    if !decisions.is_empty() {
        parts.push("## Recent Decisions".to_string());
        parts.extend(decisions);
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

impl Check for SessionStartInject {
    /// On `SessionStart`: bootstrap the event bus, run spec hygiene, and inject
    /// persistent memory. The first two are side effects; the memory payload
    /// is the verdict — `Inject` when there is memory to surface, else `Allow`.
    ///
    /// Any non-`SessionStart` trigger self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::SessionStart) {
            return Ok(Verdict::Allow);
        }
        // Recursion guard: the cold-path interpreter spawns `claude --print`
        // sub-sessions to label clusters. Those sub-sessions inherit the
        // parent `mustard-rt` hooks. Without this short-circuit, this very
        // function would re-spawn the OTEL collector, re-run spec-hygiene,
        // re-inject memory, and (more importantly) potentially trigger any
        // downstream side effect that calls back into the registry scan —
        // infinite recursion. The cold-path sets
        // `MUSTARD_COLD_PATH_INVOKED=1` on every subprocess it spawns; we
        // self-allow here so the sub-session is effectively hook-less while
        // OAuth/keychain auth still works (which `claude --bare` would have
        // broken). Any subprocess that sets `MUSTARD_COLD_PATH_INVOKED` is
        // treated as hook-less here.
        if std::env::var_os("MUSTARD_COLD_PATH_INVOKED").is_some() {
            return Ok(Verdict::Allow);
        }
        let cwd = ctx.project_dir_or_cwd(input);
        run_harness_init(input, &cwd);
        // Wave 3 (economia-moat-unification): the OTEL collector is no longer
        // an "out-of-scope spawn" — fire it detached and let `session_cleanup`
        // remove the PID file on `SessionEnd`. The transcript watcher is opt-in.
        spawn_otel_collector(&cwd);
        spawn_transcript_watcher();
        run_spec_hygiene(&cwd);
        // Wave 1 (mustard-unification): advisory probe for orphan agent
        // worktrees under `<repo>/.claude/worktrees/agent-*`. Read-only;
        // emits a single stderr warning when the orphan count exceeds the
        // module's threshold. Fail-open at every step.
        crate::commands::maint::worktree_gc::session_start_probe(Path::new(&cwd));
        // Deep-Refactor Wave 2 (T2.3 / claude-paths-single-source W2.T2.6):
        // advisory probe for drift in the project's `.claude/` directory.
        // Read-only; emits a single stderr warning when one or more children
        // classify as `ORPHAN` (no declared consumer in
        // `apps/{rt,cli,dashboard}`) — the underlying audit now derives its
        // documented-directory set from `mustard_core::ClaudePaths::documented_dirs`,
        // the single canonical catalog. Fail-open — never blocks.
        crate::commands::maint::claude_dir_prune::check_orphans(Path::new(&cwd));
        Ok(match build_memory_context(&cwd) {
            Some(context) => Verdict::Inject { context },
            None => Verdict::Allow,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `session.start` lands in the per-session NDJSON sink under W5.
    // W3B: memory is sourced from `.claude/knowledge/` and `.claude/memory/`
    // via `MarkdownStore::scan_dir`; no SQLite seeding required.
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::SessionStart),
            workspace_root: None,
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
            workspace_root: None,
        };
        assert_eq!(
            SessionStartInject.evaluate(&input, &other).expect("no error"),
            Verdict::Allow
        );
    }

    // --- harness-init parity -----------------------------------------------

    #[test]
    fn harness_init_creates_dirs_and_emits_session_start() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = session_input("s-new");
        SessionStartInject.evaluate(&input, &ctx(project)).unwrap();
        assert!(dir.path().join(".claude/.harness/sessions").is_dir());

        // W5: `session.start` is non-pipeline → lands in the per-session NDJSON
        // sink under `<project>/.claude/.session/<slug>/.events/`.
        let session_root = dir.path().join(".claude").join(".session");
        let mut found = false;
        if session_root.exists() {
            for entry in std::fs::read_dir(&session_root).unwrap() {
                let events_dir = entry.unwrap().path().join(".events");
                if !events_dir.exists() {
                    continue;
                }
                for f in std::fs::read_dir(&events_dir).unwrap() {
                    let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
                    if body.lines().any(|l| {
                        serde_json::from_str::<serde_json::Value>(l)
                            .ok()
                            .and_then(|v| v["event"].as_str().map(str::to_string))
                            .as_deref()
                            == Some("session.start")
                    }) {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "session.start NDJSON line must be present");
    }

    #[test]
    fn harness_init_creates_harness_dir_no_jsonl() {
        // W5: `session.start` is non-pipeline → it lands in the per-session
        // NDJSON sink, NOT in `mustard.db`. The harness directory still gets
        // created so later pipeline.* events can land there.
        // W3B: no event-store seeding required.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        SessionStartInject
            .evaluate(&session_input("new-session"), &ctx(project))
            .unwrap();
        assert!(dir.path().join(".claude/.harness").is_dir());
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
        SessionStartInject
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
        SessionStartInject
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
        SessionStartInject
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
    // The netstat/lsof parsers (and their tests) now live in the neutral
    // `crate::shared::proc` module, shared with `run otel-stop`.

    // --- session-memory parity (W3B: reads from MarkdownStore) ---------------

    /// Write a `.md` knowledge file to `.claude/knowledge/` so
    /// `build_memory_context` can load it via `MarkdownStore::scan_dir`.
    fn seed_knowledge_md(project: &std::path::Path, name: &str, description: &str) {
        let knowledge_dir = project.join(".claude").join("knowledge");
        std::fs::create_dir_all(&knowledge_dir).unwrap();
        let content = format!(
            "---\nname: {name}\ndescription: {description}\n---\n"
        );
        std::fs::write(knowledge_dir.join(format!("{name}.md")), content).unwrap();
    }

    #[test]
    fn memory_injection_surfaces_knowledge_as_inject() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        SessionStartInject.evaluate(&session_input("s-init"), &ctx(project)).unwrap();
        // Seed name uses capital `Foo` so the surfaced label (which loads
        // `name` preferentially via `load_knowledge_md`) contains the
        // assertion string. The description is informational only.
        seed_knowledge_md(dir.path(), "Foo-use-bar", "Foo: use bar");
        let verdict = SessionStartInject
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
        let verdict = SessionStartInject
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        // No knowledge/memory dirs → build_memory_context returns None → Allow.
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn session_start_injection_marks_knowledge_as_unverified() {
        // W3B: knowledge entries loaded from .claude/knowledge/*.md carry the
        // "unverified" prefix in the injected context.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        SessionStartInject.evaluate(&session_input("s-init"), &ctx(project)).unwrap();
        seed_knowledge_md(dir.path(), "alpha-entry", "alpha-entry: some description");
        let verdict = SessionStartInject
            .evaluate(&session_input("s"), &ctx(project))
            .unwrap();
        let context = match verdict {
            Verdict::Inject { context } => context,
            other => panic!("expected Inject, got {other:?}"),
        };
        assert!(
            context.contains("(unverified — verify before recommending) alpha-entry"),
            "MarkdownStore-backed knowledge must carry the unverified prefix; got: {context}"
        );
    }
}
