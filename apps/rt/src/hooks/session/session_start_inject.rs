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
//! - declared injectables (orchestrator-redesign) — the `mustard.json#inject`
//!   entries with `on: sessionStart` are appended AFTER the terrain census,
//!   blank-line separated, in the same single `Inject` verdict. On a
//!   window-refreshing `SessionStart` — `source == "compact"` (auto-compaction)
//!   or `source == "clear"` (the user ran `/clear`) — the session's
//!   `injected-*` markers are cleared first (so the `once` entries of
//!   `userPromptSubmit` re-deliver on the next prompt) and the `sessionStart`
//!   entries re-inject immediately (markers ignored): the refreshed window
//!   lost them, so they must ride back in.
//! - version drift advisory — an installed project (`mustard.json` present)
//!   whose `version` stamp differs from the running harness gets a one-line
//!   nudge toward `/mustard:upsert`. Advisory, never blocking.
//! - stale plugin advisory — the running harness compared against the version
//!   the plugin registry records as INSTALLED; strictly older gets one line
//!   saying only a reload picks the new one up, since an upsert cannot. The
//!   drift advisory above cannot see this: it compares the stamp against the
//!   running harness, so a session on an old plugin reads as aligned.
//! - pending-prune advisory — delivered work units still carrying a live
//!   branch get one line naming what is owed. Advisory, never blocking.
//!
//! ## Contract shape
//!
//! `harness-init` and `spec-hygiene` are pure side effects (`Observer`).
//! The terrain census + injectables produce an `additionalContext` payload,
//! surfaced as a [`Verdict::Inject`] so the single `emit_outcome` owns the
//! only stdout write. `SessionStartInject` is a `Check`.
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
use mustard_core::SupportedLocale;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::shared::branch_state::{awaiting_prune, LocalOnlyPr};

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
        let cwd = ctx.project_dir_or_cwd(input);
        run_harness_init(input, &cwd);
        // Wave 3 (economia-moat-unification): the OTEL collector is no longer
        // an "out-of-scope spawn" — fire it detached and let `session_cleanup`
        // remove the PID file on `SessionEnd`.
        spawn_otel_collector(&cwd);
        run_spec_hygiene(&cwd);
        // Collect orphan worktrees — those under `<repo>/.claude/worktrees/`
        // whose name is not a work unit's `{base}_…`, plus the removal-proof
        // scratch trees an interrupted review left in the OS temp directory. It
        // REMOVES what is orphaned (owner gone) or stale, and never touches a
        // work unit's worktree or one holding uncommitted work. Fail-open at
        // every step.
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
        // knowing the subprojects instead of grepping to orient. Fail-open: a
        // missing / unreadable model yields no terrain.
        let terrain_lang =
            crate::shared::context::project_config_cached(Path::new(&cwd)).i18n().lang;
        let terrain = crate::commands::orient::render_terrain(
            &crate::commands::orient::compute_orientation(Path::new(&cwd)),
            terrain_lang,
        );
        // Declared injectables (`mustard.json#inject`, `on: sessionStart`).
        // A window-refreshing SessionStart first clears the session's
        // `injected-*` markers, then re-injects the sessionStart entries
        // immediately (markers ignored). Two sources refresh the window:
        // `compact` (auto-compaction) and `clear` (the user ran `/clear`) —
        // both drop every earlier injection, so the `once` userPromptSubmit
        // entries must re-deliver on the next prompt and the sessionStart
        // entries must ride back in. Fail-open throughout.
        let session = current_session_id(input);
        let source_refreshes_window = input
            .raw
            .get("source")
            .and_then(|v| v.as_str())
            .is_some_and(|s| {
                s.eq_ignore_ascii_case("compact") || s.eq_ignore_ascii_case("clear")
            });
        if source_refreshes_window {
            crate::hooks::session::injectables::clear_markers(&cwd, Some(session.as_str()));
        }
        let injected = crate::hooks::session::injectables::collect(
            &cwd,
            Some(session.as_str()),
            "sessionstart",
            source_refreshes_window,
            None,
        );
        // The `userPromptSubmit` family is NOT folded in here, and that is
        // deliberate. Doing so was tried and MEASURED at 11,973 characters on
        // this repository — past the 10,000 a hook RESPONSE carries, so the very
        // router it meant to rescue became a file path instead of text in
        // force. Worse, `collect` records the delivery markers, so each sibling
        // hook would then skip on the next prompt: the self-healing path would
        // be disarmed by the attempt to help it.
        //
        // Clearing the markers above IS the fix. The prompt family re-delivers
        // on the operator's next prompt, through its own sibling hooks, each
        // measured alone against its own ceiling. The window between the
        // compaction and that prompt carries no router — accepted, because the
        // alternative measured worse: no router at all, for the rest of the
        // session.
        // Version drift advisory: an installed project whose `mustard.json`
        // stamp differs from the running harness gets a one-paragraph nudge
        // toward `/mustard:upsert`. Advisory only — the user decides.
        let drift = version_drift_notice(Path::new(&cwd));
        // Stale-plugin advisory: the drift check above compares the stamp with
        // the RUNNING harness, and the running harness is what wrote the stamp
        // — so it is blind to a session still carrying a plugin an update has
        // already replaced on disk. This is the only line that can see it.
        let stale = stale_plugin_notice();
        // Plugin-behind-binary advisory: the two above compare the stamp with
        // the running harness, and the running plugin with the registry. A
        // package install moves neither pair — it replaces the SYSTEM binary
        // and leaves the plugin where it was, which is the shape the operator
        // hit on 2026-08-26.
        let behind = plugin_behind_binary_notice();
        // Pending-prune advisory: delivered work units whose branch is still
        // alive. The prune command already existed and worked; what was missing
        // was anyone SAYING it was owed, so six units piled up unnoticed.
        let prune = prune_pending_notice(Path::new(&cwd), terrain_lang);
        // ONE composed Inject (the dispatcher fold is last-writer-wins):
        // terrain first, injectables after, the advisories last — blank-line
        // separated.
        let parts: Vec<String> =
            [terrain, injected, drift, stale, behind, prune].into_iter().flatten().collect();
        Ok(if parts.is_empty() {
            Verdict::Allow
        } else {
            Verdict::Inject { context: parts.join("\n\n") }
        })
    }
}

/// One-paragraph advisory when the project's `mustard.json#version` stamp
/// differs from the running harness ([`mustard_core::harness_version`] — the
/// installed plugin's manifest, or the core line outside the plugin).
///
/// `None` when the project has no `mustard.json` (not installed — the
/// prompt-gate story covers that) or when the stamp matches. A missing
/// `version` key on an installed project counts as drift: it predates the
/// stamp and the first `/mustard:upsert` writes one.
fn version_drift_notice(root: &Path) -> Option<String> {
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    let stamped = mustard_core::ProjectConfig::load(root).version;
    let current = mustard_core::harness_version();
    if stamped.as_deref() == Some(current.as_str()) {
        return None;
    }
    let label = stamped.unwrap_or_else(|| "unstamped (pre-version era)".to_string());
    Some(format!(
        "[Mustard] Harness version drift — project stamp: {label}; running harness: \
         {current}. Tell the user this project's Mustard footprint is out of date and \
         suggest running /mustard:upsert to realign (a notice that persists after an \
         upsert means the plugin itself needs updating)."
    ))
}

/// One line when the plugin THIS session loaded is behind the one Claude Code's
/// registry records as installed — the session is running old prose and only a
/// reload changes that.
///
/// The gap this closes: `/mustard:upsert` installs a new plugin version, and
/// the running session keeps every command, skill and agent file of the old one
/// until the operator reloads. Nothing said so. [`version_drift_notice`]
/// structurally cannot: the stamp it reads was written by the running harness,
/// so the two agree by construction and a stale session reads as aligned.
fn stale_plugin_notice() -> Option<String> {
    stale_plugin_line(
        &mustard_core::harness_version(),
        mustard_core::installed_harness_version().as_deref(),
    )
}

/// The pure half of [`stale_plugin_notice`] — running version in, advisory out.
///
/// `None` unless the registry ANSWERED and the running version is strictly
/// older: an unreadable registry, a registry that does not list this plugin,
/// and a session already on the installed version all mean the same thing —
/// nothing to say. An advisory that cannot prove its claim stays quiet.
fn stale_plugin_line(running: &str, installed: Option<&str>) -> Option<String> {
    let installed = installed.filter(|latest| mustard_core::is_behind(running, latest))?;
    Some(format!(
        "[Mustard] Stale plugin — this session loaded {running}; {installed} is installed. \
         Tell the user the session is running the OLD commands, skills and agents, and that \
         only reloading Claude Code picks up {installed} — an upsert alone does not."
    ))
}

/// One line when the PLUGIN Claude Code would load is behind the `mustard-rt`
/// binary installed on this machine.
///
/// A third pair, and neither of the two above can see it.
/// [`version_drift_notice`] compares the project stamp with the running
/// harness; [`stale_plugin_notice`] compares the running plugin with the
/// registry. Both are blind to the case where the SYSTEM binary was updated and
/// the plugin was not — which is exactly what a package install does: `dpkg -i`
/// (or the Windows installer) replaces `/usr/lib/mustard/bin/mustard-rt` and
/// touches nothing under `~/.claude/plugins/`.
///
/// Found in the field, 2026-08-26: the operator installed 0.1.50 and the plugin
/// stayed on 0.1.49. Every version they could see said 0.1.50, and the harness
/// that actually ran their hooks was the old one. Nothing said so.
///
/// `None` whenever the claim cannot be proven — no registry, no answer, or the
/// two agree. An advisory that guesses is worse than silence.
fn plugin_behind_binary_notice() -> Option<String> {
    plugin_behind_binary_line(
        &mustard_core::harness_version(),
        mustard_core::installed_harness_version().as_deref(),
    )
}

/// The pure half of [`plugin_behind_binary_notice`]: the running binary's
/// version in, the advisory out.
///
/// The direction is the opposite of [`stale_plugin_line`], and that is the
/// whole point. There, the session is BEHIND what is installed and a reload
/// fixes it. Here, the plugin is behind the binary, and a reload changes
/// nothing — only `/mustard:upsert` refreshes the plugin itself.
fn plugin_behind_binary_line(binary: &str, plugin: Option<&str>) -> Option<String> {
    let plugin = plugin.filter(|p| mustard_core::is_behind(p, binary))?;
    Some(format!(
        "[Mustard] Plugin behind the installed binary — `mustard-rt` on this machine is \
         {binary}, but the Claude Code plugin is {plugin}. A package install replaces the \
         system binary and does NOT touch the plugin, so the hooks are still running {plugin}. \
         Tell the user to run `/mustard:upsert`, which refreshes the plugin, and then restart \
         Claude Code."
    ))
}

/// How many unit names the advisory spells out before it just counts the rest.
/// Six units piled up in the field report; a list that long stops being read.
const PRUNE_NOTICE_NAMES: usize = 4;

/// One line when delivered work units still carry a live branch — the missing
/// half of the exit ritual.
///
/// The command that prunes them already existed and worked; across six
/// consecutive units nobody ran it, because nothing ever said it was owed.
/// This is that saying, and nothing more: advisory, never blocking.
///
/// The count comes from the ONE classifier
/// ([`crate::shared::branch_state::awaiting_prune`]) with the lookup that asks
/// no provider — a session must not open a network connection per branch
/// before it starts, so only merges LOCAL ancestry proves are counted. Under-
/// reporting costs a nudge; over-reporting would point at branches nobody
/// verified. `shared` may not import the git primitive (it lives in the
/// `commands` face), so the read is injected here, exactly as `branch_state`
/// documents.
///
/// `None` for a project with no `mustard.json` (never installed — the harness
/// does not nag it) and whenever nothing is owed. Fail-open throughout: a git
/// that cannot answer yields no advisory.
fn prune_pending_notice(root: &Path, lang: SupportedLocale) -> Option<String> {
    if !mustard_core::ProjectConfig::exists(root) {
        return None;
    }
    let config = crate::shared::context::project_config_cached(root);
    // ROOTED: the sweep classifies REAL branches of THIS repository, and a unit
    // whose base only its own directory recorded (an emergency in a project
    // declaring several candidates) reads as base-less through the pure
    // derivation — `BranchEnumerator` then files it under an empty base, which
    // is a base group `refs_ahead_of_base` never measures.
    let flow = crate::shared::work_kind::BaseFlow::of_at(&config.git, root);
    let git_read = |args: &[&str]| crate::commands::git_settle::git_out(root, args);
    let pending = awaiting_prune(&git_read, &LocalOnlyPr, &flow);
    if pending.is_empty() {
        return None;
    }
    let named: Vec<&str> =
        pending.iter().take(PRUNE_NOTICE_NAMES).map(|state| state.branch.as_str()).collect();
    let rest = pending.len() - named.len();
    let branches = if rest > 0 {
        format!("{listed} (+{rest})", listed = named.join(", "))
    } else {
        named.join(", ")
    };
    Some(
        mustard_core::translate("prune.pending.notice", lang)
            .replace("{count}", &pending.len().to_string())
            .replace("{branches}", &branches),
    )
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
            inject_only: None,
        }
    }

    fn session_input(session_id: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some(session_id.to_string()),
            ..HookInput::default()
        }
    }


    /// AC-5 — a renewed window RE-ARMS the prompt family; it does not fold it
    /// into this response.
    ///
    /// An earlier revision did fold it in, and the response measured 11,973
    /// characters on this repository — over the 10,000 a hook response carries,
    /// so the router became a file path instead of text in force. `collect`
    /// also records the delivery markers, so each sibling hook would then skip
    /// on the next prompt: the self-healing path disarmed by the attempt to
    /// help it.
    ///
    /// Clearing the markers IS the fix, and this measures exactly that: after a
    /// compaction the markers are gone, so the next prompt re-delivers through
    /// the siblings, each against its own ceiling.
    #[test]
    fn compact_rearms_the_prompt_family_without_folding_it_in() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"inject":[{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/mustard")).unwrap();
        std::fs::write(dir.path().join(".claude/mustard/orchestrator.md"), "ROUTER-RULES").unwrap();

        // Burn the marker, as a first delivery would.
        let session = dir.path().join(".claude/.session/s2");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("injected-orchestrator.md"), "x").unwrap();

        let mut compacted = session_input("s2");
        compacted.raw = serde_json::json!({"source": "compact"});
        let verdict = SessionStartInject.evaluate(&compacted, &ctx(project)).expect("no error");

        // The router is NOT in this response — folding it here is what
        // overflowed it.
        let text = match verdict {
            Verdict::Inject { ref context } => context.clone(),
            _ => String::new(),
        };
        assert!(
            !text.contains("ROUTER-RULES"),
            "the prompt family must not ride the SessionStart response: {text}",
        );

        // The marker is gone, so the next prompt re-delivers through the
        // sibling hook that owns it.
        assert!(
            !session.join("injected-orchestrator.md").exists(),
            "a renewed window must clear the delivery markers",
        );
    }

    // --- routing -----------------------------------------------------------

    #[test]
    fn non_session_start_trigger_allows() {
        let input = session_input("s1");
        let other = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
            inject_only: None,
        };
        assert_eq!(
            SessionStartInject.evaluate(&input, &other).expect("no error"),
            Verdict::Allow
        );
    }

    // --- version drift advisory --------------------------------------------

    #[test]
    fn drift_notice_absent_without_mustard_json() {
        let dir = tempdir().unwrap();
        assert_eq!(version_drift_notice(dir.path()), None);
    }

    #[test]
    fn drift_notice_absent_when_stamp_matches() {
        let dir = tempdir().unwrap();
        let current = mustard_core::harness_version();
        std::fs::write(
            dir.path().join("mustard.json"),
            format!(r#"{{"version":"{current}"}}"#),
        )
        .unwrap();
        assert_eq!(version_drift_notice(dir.path()), None);
    }

    #[test]
    fn drift_notice_fires_on_mismatch_and_names_upsert() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"version":"0.0.0-test"}"#)
            .unwrap();
        let notice = version_drift_notice(dir.path()).expect("drift must fire");
        assert!(notice.contains("0.0.0-test"), "names the stamped version: {notice}");
        assert!(notice.contains("/mustard:upsert"), "points at the realign door: {notice}");
    }

    #[test]
    fn drift_notice_fires_on_missing_stamp() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"buildCommand":"make"}"#).unwrap();
        let notice = version_drift_notice(dir.path()).expect("unstamped must fire");
        assert!(notice.contains("unstamped"), "labels the pre-version era: {notice}");
    }

    // --- stale-plugin advisory ----------------------------------------------

    /// AC-5 — a session whose loaded plugin is behind the one the registry
    /// records as installed says so in ONE line, and says that reloading is
    /// what fixes it. The drift advisory cannot reach this case: the stamp it
    /// compares was written BY the running harness.
    #[test]
    fn stale_plugin_is_announced_at_session_start() {
        let notice = stale_plugin_line("0.1.42", Some("0.1.43")).expect("stale must fire");
        assert_eq!(notice.lines().count(), 1, "one line, not a paragraph: {notice}");
        assert!(notice.contains("0.1.42"), "names what the session loaded: {notice}");
        assert!(notice.contains("0.1.43"), "names what is installed: {notice}");
        assert!(
            notice.to_lowercase().contains("reload"),
            "says the reload is the missing step: {notice}"
        );
    }

    /// The three silences, which are the same silence: a session already on the
    /// installed version, one AHEAD of it (a local build), and a registry that
    /// could not answer at all.
    #[test]
    fn stale_plugin_notice_stays_quiet_without_proof() {
        assert_eq!(stale_plugin_line("0.1.43", Some("0.1.43")), None);
        assert_eq!(stale_plugin_line("0.2.0", Some("0.1.43")), None);
        assert_eq!(stale_plugin_line("0.1.42", None), None);
    }

    // --- pending-prune advisory ---------------------------------------------

    /// Run git in `dir`, failing the test loudly — a half-built fixture would
    /// make the assertions below prove nothing.
    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must be on PATH for this test");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn prune_advisory_absent_without_mustard_json() {
        let dir = tempdir().unwrap();
        assert_eq!(prune_pending_notice(dir.path(), SupportedLocale::default()), None);
    }

    /// The field cause, closed: a unit whose work landed but whose branch is
    /// still around gets SAID OUT LOUD at session start. The unmerged unit in
    /// the same repo is the control — the advisory names what is owed, never
    /// everything that exists.
    #[test]
    fn prune_advisory_names_units_whose_branch_outlived_the_merge() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .unwrap();
        git(root, &["add", "-A", "-f", "."]);
        git(root, &["commit", "-m", "seed"]);

        // One unit delivered: merged into its base, branch still alive.
        git(root, &["checkout", "-b", "dev_landed"]);
        git(root, &["commit", "--allow-empty", "-m", "work"]);
        git(root, &["checkout", "dev"]);
        git(root, &["merge", "--no-ff", "-m", "merge", "dev_landed"]);
        // One unit still in flight: nothing is owed for it.
        git(root, &["branch", "dev_live"]);
        git(root, &["checkout", "dev_live"]);
        git(root, &["commit", "--allow-empty", "-m", "in flight"]);
        git(root, &["checkout", "dev"]);

        let notice = prune_pending_notice(root, SupportedLocale::default())
            .expect("a delivered unit with a live branch must be surfaced");
        assert!(notice.contains("dev_landed"), "names the unit owed a prune: {notice}");
        assert!(
            !notice.contains("dev_live"),
            "an unmerged unit is not owed anything: {notice}"
        );
        assert!(notice.contains('1'), "carries the count: {notice}");
        assert!(
            notice.contains("git-settle"),
            "points at the command that was never called: {notice}"
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
        // No `grain.model.json` and no declared injectables → nothing to
        // inject → the verdict degrades to Allow.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        let verdict = SessionStartInject
            .evaluate(&session_input("s"), &ctx(dir.path().to_str().unwrap()))
            .unwrap();
        assert!(is_quiet(&verdict), "nothing to inject must stay quiet: {verdict:?}");
    }

    /// Is this verdict "nothing of substance", once the machine-state advisory
    /// is set aside?
    ///
    /// `stale_plugin_notice` reads the REAL plugin registry of the machine the
    /// test runs on — there is no seam to substitute it — so on a developer's
    /// box mid-upgrade every `Allow` here arrives as an `Inject` carrying that
    /// one line. Asserting on the bare verdict made three tests fail for a
    /// reason none of them is about (measured on this repository, with the
    /// session on 0.1.47 and 0.1.49 installed).
    ///
    /// So the question each of them actually asks is spelled out: nothing was
    /// injected EXCEPT possibly that advisory.
    fn is_quiet(verdict: &Verdict) -> bool {
        match verdict {
            Verdict::Allow => true,
            Verdict::Inject { context } => context.starts_with("[Mustard] Stale plugin"),
            _ => false,
        }
    }

    /// A package install moves the system binary and leaves the plugin behind,
    /// and until this advisory nothing said so.
    ///
    /// Measured in the field on 2026-08-26: `dpkg -i` put 0.1.50 on the machine
    /// and `~/.claude/plugins/` stayed on 0.1.49. Every version the operator
    /// could read said 0.1.50; the harness running their hooks was 0.1.49.
    ///
    /// The direction matters and is the opposite of the stale-plugin line: this
    /// one fires when the PLUGIN is behind, and a reload does not fix it —
    /// only `/mustard:upsert` refreshes the plugin.
    #[test]
    fn a_plugin_left_behind_by_a_package_install_is_named() {
        let notice = plugin_behind_binary_line("0.1.50", Some("0.1.49"))
            .expect("a plugin behind the binary must be named");
        assert!(notice.contains("0.1.50") && notice.contains("0.1.49"), "{notice}");
        assert!(notice.contains("upsert"), "it must name the one action that fixes it: {notice}");

        // Silent whenever the claim cannot be proven, or there is nothing to
        // claim. An advisory that guesses is worse than one that stays quiet.
        assert_eq!(plugin_behind_binary_line("0.1.50", Some("0.1.50")), None, "aligned");
        assert_eq!(plugin_behind_binary_line("0.1.49", Some("0.1.50")), None, "plugin AHEAD");
        assert_eq!(plugin_behind_binary_line("0.1.50", None), None, "no registry answer");
        // Numeric, not lexical: 0.1.9 is behind 0.1.10, which a string
        // comparison gets backwards.
        assert!(plugin_behind_binary_line("0.1.10", Some("0.1.9")).is_some(), "0.1.9 < 0.1.10");
    }

    // --- declared injectables (orchestrator-redesign) ------------------------

    /// Declare one `on: sessionStart, once: true` injectable + its file.
    fn seed_session_injectable(dir: &Path, body: &str) {
        // The fixture stamps the CURRENT harness version so the drift advisory
        // stays silent — these tests exercise the injectable path, not drift.
        std::fs::write(
            dir.join("mustard.json"),
            format!(
                r#"{{"version":"{}","inject":[{{"on":"sessionStart","file":".claude/mustard/response-style.md","once":true}}]}}"#,
                mustard_core::harness_version()
            ),
        )
        .unwrap();
        let mustard_dir = dir.join(".claude").join("mustard");
        std::fs::create_dir_all(&mustard_dir).unwrap();
        std::fs::write(mustard_dir.join("response-style.md"), body).unwrap();
    }

    fn session_input_with_source(session_id: &str, source: &str) -> HookInput {
        HookInput {
            hook_event_name: Some("SessionStart".to_string()),
            session_id: Some(session_id.to_string()),
            raw: json!({ "source": source }),
            ..HookInput::default()
        }
    }

    #[test]
    fn session_start_injects_declared_file_once() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_session_injectable(dir.path(), "STYLE-BODY\n");

        // Startup: the declared file rides the SessionStart inject.
        let v = SessionStartInject
            .evaluate(&session_input_with_source("s1", "startup"), &ctx(project))
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("STYLE-BODY"), "injectable missing: {context}");
            }
            other => panic!("expected Inject, got {other:?}"),
        }
        assert!(
            dir.path()
                .join(".claude/.session/s1/injected-response-style.md")
                .is_file(),
            "delivery marker recorded"
        );

        // A resume of the SAME session finds the marker → no re-delivery (no
        // terrain here, so the verdict degrades to Allow).
        let v = SessionStartInject
            .evaluate(&session_input_with_source("s1", "resume"), &ctx(project))
            .unwrap();
        assert!(is_quiet(&v), "once injectable must not re-deliver on resume: {v:?}");
    }

    #[test]
    fn compact_resets_markers_and_reinjects() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_session_injectable(dir.path(), "STYLE-BODY\n");
        // Plant a userPromptSubmit marker too — compact must clear BOTH so the
        // next prompt re-delivers its own once entries.
        let session = dir.path().join(".claude/.session/s1");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("injected-orchestrator.md"), "x").unwrap();

        // First startup burns the sessionStart marker.
        let _ = SessionStartInject
            .evaluate(&session_input_with_source("s1", "startup"), &ctx(project))
            .unwrap();
        assert!(session.join("injected-response-style.md").is_file());

        // Compact: prompt-side marker cleared AND the sessionStart entry
        // re-injects despite its (now cleared) marker.
        let v = SessionStartInject
            .evaluate(&session_input_with_source("s1", "compact"), &ctx(project))
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("STYLE-BODY"), "compact must re-inject: {context}");
            }
            other => panic!("expected re-inject on compact, got {other:?}"),
        }
        assert!(
            !session.join("injected-orchestrator.md").exists(),
            "compact clears the prompt-side once markers"
        );
        assert!(
            session.join("injected-response-style.md").is_file(),
            "the re-delivered sessionStart entry re-records its marker"
        );
    }

    #[test]
    fn clear_resets_markers_and_reinjects() {
        // A `/clear` refreshes the window exactly like a compaction: the
        // sessionStart entries must ride back in and the prompt-side `once`
        // markers must be cleared so the orchestrator re-delivers next prompt.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_session_injectable(dir.path(), "STYLE-BODY\n");
        let session = dir.path().join(".claude/.session/s1");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("injected-orchestrator.md"), "x").unwrap();

        // First startup burns the sessionStart marker.
        let _ = SessionStartInject
            .evaluate(&session_input_with_source("s1", "startup"), &ctx(project))
            .unwrap();
        assert!(session.join("injected-response-style.md").is_file());

        // Clear: prompt-side marker cleared AND the sessionStart entry
        // re-injects despite its (now cleared) marker.
        let v = SessionStartInject
            .evaluate(&session_input_with_source("s1", "clear"), &ctx(project))
            .unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("STYLE-BODY"), "clear must re-inject: {context}");
            }
            other => panic!("expected re-inject on clear, got {other:?}"),
        }
        assert!(
            !session.join("injected-orchestrator.md").exists(),
            "clear clears the prompt-side once markers"
        );
        assert!(
            session.join("injected-response-style.md").is_file(),
            "the re-delivered sessionStart entry re-records its marker"
        );
    }

    #[test]
    fn missing_declared_file_degrades_to_allow() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        // Stamped with the current harness version: the drift advisory stays
        // silent, isolating the missing-file behaviour under test.
        std::fs::write(
            dir.path().join("mustard.json"),
            format!(
                r#"{{"version":"{}","inject":[{{"on":"sessionStart","file":".claude/mustard/gone.md","once":true}}]}}"#,
                mustard_core::harness_version()
            ),
        )
        .unwrap();
        let v = SessionStartInject
            .evaluate(&session_input_with_source("s1", "startup"), &ctx(project))
            .unwrap();
        assert!(is_quiet(&v), "missing declared file must fail open: {v:?}");
    }
}
