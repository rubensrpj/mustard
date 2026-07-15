//! `session_start_inject` — the consolidated `SessionStart` lifecycle module.
//!
//! ## Scope (b3 Wave 5, session family)
//!
//! This module consolidates the `SessionStart` concerns. Each is a distinct
//! *concern* kept as its own internal section — consolidation regroups, it
//! does not merge logic:
//!
//! - `harness-init.js` — bootstraps the harness event bus: ensures
//!   `.claude/.harness/` exists, prunes legacy archived sessions older than
//!   30 days, and emits a `session.start` event. Events live in per-spec /
//!   per-session NDJSON logs under `.claude/` (the `mustard.db` SQLite store
//!   was retired — see `session_stop_observer`).
//! - terrain census — projects `grain.model.json` into a once-per-session
//!   terrain map injected as `additionalContext` (the only injection; the
//!   legacy persistent-memory block was retired — durable prose knowledge is
//!   Claude Code native auto-memory now).
//! - `spec-hygiene.js` — auto-moves stale completed/cancelled specs from
//!   `spec/{name}/` (flat layout — lifecycle status lives in each spec's
//!   `meta.json` sidecar, no bucket moves).
//!
//! ## Contract shape
//!
//! `harness-init` and `spec-hygiene` are pure side effects (`Observer`).
//! The terrain census produces an `additionalContext` payload, surfaced as a
//! [`Verdict::Inject`] so the single `emit_outcome` owns the only stdout
//! write. `SessionStartInject` is a `Check`.
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
//! ## Profile gate
//!
//! `harness-init` / `spec-hygiene` each called
//! `shouldRun()` from `_lib/hook-env.js`. The dispatcher has no profile
//! awareness (see spec Concern "Profile gate") — under `MUSTARD_HOOK_PROFILE=minimal`
//! these now run where the JS auto-skipped. They are all fail-open side
//! effects with no verdict impact, so the change is observably inert.

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
// OTEL collector spawn (Wave 3 — economia-moat-unification)
// ===========================================================================

/// File where the OTEL collector records its PID, under the project's harness
/// directory. The collector authors it on startup (after binding the port); this
/// hook only reads it for the idempotence + rebuild checks, and `session_cleanup`
/// removes it on `SessionEnd`. Single source of truth lives in the OTEL module.
const OTEL_PID_FILE: &str = crate::commands::economy::otel::PID_FILENAME;

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
    // compare the running exe mtime with the PID-file mtime: if the exe is
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

    // Cross-project takeover: a previous project collector may still be
    // holding the OTLP port (its SessionEnd may not have fired, or a kill may
    // have failed). Free the port before spawning, otherwise THIS project
    // collector fails to bind and the foreign listener silently captures this
    // project telemetry. Best-effort, fail-open.
    free_otel_port();

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("session_start: current_exe failed ({e}); skipping OTEL collector spawn");
            return;
        }
    };

    // Detached spawn (`cmd /C start` on Windows): a plain child would inherit
    // this hook stdout pipe and hang the whole session — see
    // `shared::proc::spawn_detached`. The collector writes its own PID file
    // after it binds the port, so there is no PID to capture or persist here.
    if let Err(e) = crate::shared::proc::spawn_detached(&exe, &["run", "otel-collector"]) {
        eprintln!("session_start: spawn `mustard-rt run otel-collector` failed ({e})");
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
// Contract impls
// ===========================================================================

impl Check for SessionStartInject {
    /// On `SessionStart`: bootstrap the event bus, run spec hygiene, and inject
    /// the terrain census. The first two are side effects; the terrain payload
    /// is the verdict — `Inject` when a grain model exists, else `Allow`.
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
        // re-inject the terrain, and (more importantly) potentially trigger any
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
        // remove the PID file on `SessionEnd`.
        spawn_otel_collector(&cwd);
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
        // knowing the subprojects instead of grepping to orient. The injected
        // block is terrain-only. Fail-open: a missing / unreadable model
        // yields no terrain and the verdict degrades to `Allow`.
        let terrain = crate::commands::orient::render_terrain(
            &crate::commands::orient::compute_orientation(Path::new(&cwd)),
        );
        Ok(match terrain {
            Some(context) => Verdict::Inject { context },
            None => Verdict::Allow,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `session.start` lands in the per-session NDJSON sink under W5.
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

    // --- terrain injection ---------------------------------------------------

    #[test]
    fn no_grain_model_returns_allow() {
        // No `grain.model.json` → no terrain → the verdict degrades to Allow
        // (the legacy persistent-memory block is gone; terrain is the only
        // injectable payload).
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let verdict = SessionStartInject
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert_eq!(verdict, Verdict::Allow);
    }

}
