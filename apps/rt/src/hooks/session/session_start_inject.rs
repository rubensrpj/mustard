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
//!   30 days, and emits a `session.start` event. Events live in per-spec /
//!   per-session NDJSON logs under `.claude/` (the `mustard.db` SQLite store
//!   was retired — see `session_stop_observer`).
//! - `session-memory.js` — injects persistent memory (knowledge base,
//!   cross-session timeline, decisions, lessons) as `additionalContext`.
//! - `spec-hygiene.js` — auto-moves stale completed/cancelled specs from
//!   `spec/{name}/` (flat layout — lifecycle status lives in each spec's
//!   `meta.json` sidecar, no bucket moves).
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
//! handled in-binary here: [`spawn_otel_collector`] detaches the child through
//! [`crate::shared::proc::spawn_detached`], which on Windows routes via
//! `cmd /C start "" /B` so the long-lived collector does NOT inherit this
//! hook's stdout pipe — a plain `Command::spawn` would, leaving the pipe's
//! write end open in the daemon so the harness never sees EOF and hangs the
//! session. The collector authors its own
//! `<project>/.claude/.harness/.otel-collector.pid` after binding the port, so
//! the detached spawn (which cannot observe the real PID) still feeds the
//! idempotence check: a second `SessionStart` finds the PID file, sees the
//! process still up via [`is_process_alive`], and skips the spawn. Every
//! failure path is fail-open: a missing exe or a spawn error is logged via
//! `eprintln!` and the `SessionStart` payload continues unmodified.
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

use mustard_core::io::atomic_md::{MarkdownDoc, MarkdownStore};
use mustard_core::platform::error::Error;
use mustard_core::io::fs;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use mustard_core::time::now_iso8601;

/// Archived sessions older than this are pruned on `SessionStart` (30 days).
const RETENTION_MS: u128 = 30 * 24 * 60 * 60 * 1000;

/// Number of knowledge entries injected from `.claude/knowledge/*.md`.
const KB_MAX_ENTRIES: usize = 5;

/// Injection caps — total budget per memory section.
/// Top-3 entries from the active-spec context, plus top-2 global fallbacks.
/// Entries are surfaced newest-first by their frontmatter timestamp.
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

/// File where the OTEL collector records its PID, under the project's harness
/// directory. The collector authors it on startup (after binding the port); this
/// hook only reads it for the idempotence + rebuild checks, and `session_cleanup`
/// removes it on `SessionEnd`. Single source of truth lives in the OTEL module.
const OTEL_PID_FILE: &str = crate::commands::economy::otel::PID_FILENAME;

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

    // Detached spawn (`cmd /C start` on Windows): a plain child would inherit
    // this hook's stdout pipe and hang the whole session — see
    // `shared::proc::spawn_detached`. The collector writes its own PID file
    // after it binds the port, so there is no PID to capture or persist here.
    if let Err(e) = crate::shared::proc::spawn_detached(&exe, &["run", "otel-collector"]) {
        eprintln!("session_start: spawn `mustard-rt run otel-collector` failed ({e})");
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
    // Detached spawn, same rationale as the OTEL collector: a plain child
    // inherits the hook's stdout pipe on Windows and hangs the session.
    if let Err(e) = crate::shared::proc::spawn_detached(&exe, &["run", "transcript-watcher"]) {
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
// spec-hygiene — flat layout; no-op
// ===========================================================================

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

/// A doc's chronological sort key: the frontmatter timestamp (`captured_at`
/// for knowledge/decisions, `at` for agent memory), else the filename stem —
/// every writer in this family stamps ISO-8601 in both, so either key sorts
/// lexicographically = chronologically.
fn recency_key(doc: &MarkdownDoc) -> String {
    doc.frontmatter
        .as_ref()
        .and_then(|fm| fm.get_str("captured_at").or_else(|| fm.get_str("at")))
        .map(str::to_string)
        .unwrap_or_else(|| {
            doc.path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string()
        })
}

/// The newest `max` docs of `docs`, read IN FULL (frontmatter + body).
///
/// `scan_dir` is header-only (lazy body); the full `read_one` happens only for
/// the kept top-N, so the body read stays O(max). A doc whose re-read fails
/// keeps its header-only form — fail-open.
fn newest_full_docs(mut docs: Vec<MarkdownDoc>, max: usize) -> Vec<MarkdownDoc> {
    docs.sort_by(|a, b| recency_key(b).cmp(&recency_key(a)).then_with(|| b.path.cmp(&a.path)));
    docs.truncate(max);
    docs.into_iter()
        .map(|doc| MarkdownStore::read_one(&doc.path).unwrap_or(doc))
        .collect()
}

/// A doc's one-line label: frontmatter `name`/`description` when present, else
/// the first non-empty body line, else the filename stem. The knowledge and
/// decision writers put the payload in the BODY (frontmatter carries only
/// provenance), so without the body fallback every entry rendered as an
/// opaque timestamp-hash id.
fn doc_label(doc: &MarkdownDoc) -> String {
    if let Some(label) = doc
        .frontmatter
        .as_ref()
        .and_then(|fm| fm.get_str("name").or_else(|| fm.get_str("description")))
    {
        return label.to_string();
    }
    if let Some(line) = doc.body.lines().map(str::trim).find(|l| !l.is_empty()) {
        return line.to_string();
    }
    doc.path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string()
}

/// Load knowledge entries from `.claude/knowledge/*.md` via `MarkdownStore`.
///
/// Each `.md` file is one pattern entry, surfaced newest-first (see
/// [`newest_full_docs`]). The label is the frontmatter `name`/`description`
/// when present, else the first body line — where the knowledge writer puts
/// the pattern text. Accepts an empty or missing directory — returns an
/// empty vec.
fn load_knowledge_md(knowledge_dir: &Path) -> Vec<String> {
    newest_full_docs(MarkdownStore::scan_dir(knowledge_dir), KB_MAX_ENTRIES)
        .iter()
        .map(|doc| {
            format!(
                "- [pattern] (unverified — verify before recommending) {}",
                doc_label(doc)
            )
        })
        .collect()
}

/// Load memory entries from `.claude/memory/{decisions,lessons}/*.md`.
///
/// Scoped to the durable sub-stores: `memory/agent/` holds per-turn resume
/// markers (empty body, no label) that would otherwise crowd every real
/// decision out of the top-N. Surfaced newest-first; the label is the first
/// body line — where the decision writers put the content. Accepts empty or
/// missing directories — returns an empty vec.
fn load_memory_md(memory_dir: &Path, max: usize) -> Vec<String> {
    let mut docs = MarkdownStore::scan_dir(&memory_dir.join("decisions"));
    docs.extend(MarkdownStore::scan_dir(&memory_dir.join("lessons")));
    newest_full_docs(docs, max)
        .iter()
        .map(|doc| format!("- [memory] {}", doc_label(doc)))
        .collect()
}

/// Build the persistent-memory `additionalContext` payload, or `None` when no
/// source has any content.
///
/// Reads the memory sources from the filesystem via `MarkdownStore`:
/// - `.claude/knowledge/` → Project Knowledge
/// - `.claude/memory/{decisions,lessons}/` → Recent Decisions (the `agent/`
///   sub-store is per-turn resume state, never injected here)
///
/// Empty or absent directories are treated as zero entries — fail-open throughout.
/// Bounded by top-N newest entries per section ([`SPEC_SCOPED_MAX`] +
/// [`GLOBAL_FALLBACK_MAX`]) — recency is the filter, no char cap.
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
    // No char cap: the entry count is already bounded (top-N recent per section
    // via SPEC_SCOPED_MAX + GLOBAL_FALLBACK_MAX), each entry a one-line
    // description — recency/importance is the filter, not a char count.
    let context = parts.join("\n");
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
        // orient-census Level 1 (Terrain): project `grain.model.json` into a
        // once-per-session terrain map so the AI opens the session already
        // knowing the subprojects instead of grepping to orient. Composed with
        // the persistent-memory payload — either, both, or neither may inject.
        // Fail-open: a missing / unreadable model yields no terrain.
        let terrain = crate::commands::orient::render_terrain(
            &crate::commands::orient::compute_orientation(Path::new(&cwd)),
        );
        let parts: Vec<String> = [build_memory_context(&cwd), terrain]
            .into_iter()
            .flatten()
            .collect();
        Ok(if parts.is_empty() {
            Verdict::Allow
        } else {
            Verdict::Inject { context: parts.join("\n\n") }
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

    /// Write a knowledge file in the REAL writer's shape: provenance-only
    /// frontmatter (`kind`/`captured_at`), payload in the BODY.
    fn seed_knowledge_body(project: &std::path::Path, stem: &str, captured_at: &str, body: &str) {
        let knowledge_dir = project.join(".claude").join("knowledge");
        std::fs::create_dir_all(&knowledge_dir).unwrap();
        let content = format!("---\nkind: decision\ncaptured_at: {captured_at}\n---\n{body}\n");
        std::fs::write(knowledge_dir.join(format!("{stem}.md")), content).unwrap();
    }

    /// Write a decision file under `.claude/memory/decisions/` in the real
    /// writer's shape (payload in the body).
    fn seed_decision(project: &std::path::Path, stem: &str, captured_at: &str, body: &str) {
        let dir = project.join(".claude").join("memory").join("decisions");
        std::fs::create_dir_all(&dir).unwrap();
        let content = format!("---\nkind: decision\ncaptured_at: {captured_at}\n---\n{body}\n");
        std::fs::write(dir.join(format!("{stem}.md")), content).unwrap();
    }

    /// Write an agent turn marker under `.claude/memory/agent/` (empty body,
    /// provenance-only frontmatter) — the real `agent_memory` shape.
    fn seed_agent_marker(project: &std::path::Path, stem: &str, at: &str) {
        let dir = project.join(".claude").join("memory").join("agent");
        std::fs::create_dir_all(&dir).unwrap();
        let content =
            format!("---\nsession_id: s\nsummary: interrupted at wave ?\nat: {at}\n---\n");
        std::fs::write(dir.join(format!("{stem}.md")), content).unwrap();
    }

    #[test]
    fn knowledge_label_falls_back_to_body_line_not_stem() {
        // The real knowledge writer puts the content in the BODY and no
        // name/description in the frontmatter — the injected label must be
        // that body line, never the opaque timestamp-hash filename stem.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_knowledge_body(
            dir.path(),
            "20260602T115441987Z-dc82555e",
            "2026-06-02T11:54:41.987Z",
            "**D1** — actionable sections live at the executing level",
        );
        let verdict = SessionStartInject.evaluate(&session_input("s"), &ctx(project)).unwrap();
        let context = match verdict {
            Verdict::Inject { context } => context,
            other => panic!("expected Inject, got {other:?}"),
        };
        assert!(
            context.contains("**D1** — actionable sections live at the executing level"),
            "the body line is the label; got: {context}"
        );
        assert!(
            !context.contains("20260602T115441987Z-dc82555e"),
            "the opaque filename stem must not surface; got: {context}"
        );
    }

    #[test]
    fn memory_injects_newest_decisions_first_and_drops_oldest() {
        // Six decisions, cap is 5 (SPEC_SCOPED_MAX + GLOBAL_FALLBACK_MAX):
        // the OLDEST is the one dropped, and the newest renders before the
        // older ones — "Recent Decisions" means recent.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        for day in 1..=6 {
            seed_decision(
                dir.path(),
                &format!("2026060{day}T000000000Z-aaaaaaaa"),
                &format!("2026-06-0{day}T00:00:00.000Z"),
                &format!("decision-of-day-{day}"),
            );
        }
        let verdict = SessionStartInject.evaluate(&session_input("s"), &ctx(project)).unwrap();
        let context = match verdict {
            Verdict::Inject { context } => context,
            other => panic!("expected Inject, got {other:?}"),
        };
        assert!(!context.contains("decision-of-day-1"), "oldest dropped; got: {context}");
        for day in 2..=6 {
            assert!(context.contains(&format!("decision-of-day-{day}")), "kept day {day}");
        }
        let newest = context.find("decision-of-day-6").unwrap();
        let older = context.find("decision-of-day-2").unwrap();
        assert!(newest < older, "newest renders first; got: {context}");
    }

    #[test]
    fn memory_skips_agent_turn_markers() {
        // memory/agent/ holds per-turn resume markers; under the old
        // whole-dir scan they alphabetically crowded every real decision out
        // of the top-N. Only the decision may surface.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        for i in 0..5 {
            seed_agent_marker(
                dir.path(),
                &format!("2026060{}T00000{i}000Z-c5c446b9", 1),
                &format!("2026-06-01T00:00:0{i}.000Z"),
            );
        }
        seed_decision(
            dir.path(),
            "20260610T000000000Z-bbbbbbbb",
            "2026-06-10T00:00:00.000Z",
            "the-real-decision",
        );
        let verdict = SessionStartInject.evaluate(&session_input("s"), &ctx(project)).unwrap();
        let context = match verdict {
            Verdict::Inject { context } => context,
            other => panic!("expected Inject, got {other:?}"),
        };
        assert!(context.contains("the-real-decision"), "the decision surfaces: {context}");
        assert!(
            !context.contains("interrupted at wave"),
            "agent turn markers never surface: {context}"
        );
        assert!(
            !context.contains("c5c446b9"),
            "agent marker stems never surface: {context}"
        );
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
