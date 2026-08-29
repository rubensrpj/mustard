//! `session_cleanup_observer` — the `SessionEnd` state-cleanup module.
//!
//! ## Scope (b3 Wave 5, session family)
//!
//! Ports `session-cleanup.js` **alone** — a single concern with no sibling
//! hook to merge, kept as its own module so the registry wiring is one-to-one.
//! It triggers on `SessionEnd` and:
//!
//! 1. Kills this project's OTEL collector and removes its PID file.
//! 2. Removes the statusline git cache from the temp dir.
//! 3. Removes terminal pipeline-state files (`completed`, `cancelled`, …) and
//!    states whose spec is already done.
//! 4. Removes `.compact-state` files older than 24h.
//! 5. Prunes telemetry NDJSON files (`.claude/spec/*/.events/*.ndjson`,
//!    `.claude/.session/*/.events/*.ndjson`) older than the retention window.
//! 6. Drains the local `rtk gain --json` ledger into the savings events.
//! 7. Finalizes the per-session amendment window.
//!
//! ## Why the order is load-bearing
//!
//! `SessionEnd` is the shortest hook budget the harness hands out (15 s in
//! `plugin/hooks/hooks.json`); when it is exceeded the harness cancels the hook
//! mid-flight and whatever had not run yet simply does not run. Every step here
//! is cosmetic except the collector kill — see the OTEL note below — so the
//! plan is split in two halves and declared once: [`PROMPT_STEPS`], which
//! touches only paths known up front and spawns nothing but the collector kill
//! ITSELF, then [`DEFERRED_STEPS`], which walk a subtree of unknown size or
//! spawn a process that is not what the hook came here to do. [`cleanup_plan`]
//! chains them in that order and `observe` executes exactly that sequence, so
//! the collector teardown — its own `kill` / `taskkill` spawn included, which
//! on Windows is the costliest thing the first half does — is finished before
//! anything else that can block has started.
//!
//! Read the split as "cheapest useful work first", not as "no processes until
//! later": the one process the first half spawns is the payload the whole
//! ordering exists to protect.
//!
//! ## Contract shape
//!
//! Pure side effect — no verdict. `SessionCleanupObserver` is an [`Observer`] only.
//!
//! ## OTEL collector note
//!
//! `session_start_inject` spawns the OTEL collector (in
//! [`crate::hooks::session::session_start_inject`]); this module tears it down on `SessionEnd`.
//! Because there is one collector per machine on the OTLP port, [`clean_otel_pid`]
//! now **kills** the process whose PID is in `.otel-collector.pid` before
//! removing the file — leaving it alive would let the next project's telemetry
//! bind to this project's lingering listener (cross-project contamination).
//! The kill is signal-free (subprocess `taskkill`/`kill`, the crate forbids
//! `unsafe`) and fail-open: a dead PID or a missing kill binary degrades to a
//! warning and the PID file is still removed.

use mustard_core::domain::model::event::ActorKind;
use mustard_core::domain::economy::{
    self, sources::rtk as rtk_source, sources::IngestContext,
};
use mustard_core::io::fs;
use mustard_core::domain::spec;
use mustard_core::ClaudePaths;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;


/// Build + route a `HarnessEvent` from `(event_name, payload)` produced by an
/// `economy::writer::*_event` builder. Fail-open per the router's contract.///
/// `.compact-state` files older than this are pruned — 24 hours.
const ONE_DAY_MS: u128 = 24 * 60 * 60 * 1000;

/// Telemetry retention window — `run_usage`/`usage_totals` rows older than this
/// many days are pruned on `SessionEnd`. Fail-open: pruning never aborts cleanup.
const TELEMETRY_RETENTION_DAYS: i64 = 90;

/// Terminal pipeline-state statuses — these files are removed on cleanup.
const TERMINAL_STATUSES: &[&str] = &["implemented", "completed", "validated", "cancelled"];

/// The `SessionEnd` state-cleanup module.
pub struct SessionCleanupObserver;


/// Current time as milliseconds since the Unix epoch.///
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
/// buckets. Done means either the directory is gone or the spec's lifecycle
/// metadata reads `Completed`. **`meta.json` is the single source of truth**;
/// the legacy `### Status:` / `### Outcome:` header in `spec.md` /
/// `wave-plan.md` is the fallback for un-migrated specs.
fn is_spec_done(claude_dir: &Path, spec_name: &str) -> bool {
    // ClaudePaths-exempt: `claude_dir` is seam-produced upstream; re-deriving
    // via `for_project`/`for_spec` here would be circular and would add name
    // validation that changes the fail-open trigger.
    let spec_root = claude_dir.join("spec").join(spec_name);
    if !spec_root.exists() {
        // Spec deleted → treat as done.
        return true;
    }
    // meta.json wins: a terminal `Completed` outcome marks the spec done.
    if let Some(m) = mustard_core::domain::meta::read_meta_beside(&spec_root.join("spec.md")) {
        if let Some(outcome) = m.outcome.as_deref().and_then(mustard_core::Outcome::parse) {
            return outcome == mustard_core::Outcome::Completed;
        }
    }
    // Legacy fallback: read the lifecycle header from wave-plan.md / spec.md.
    let wave_plan = spec_root.join("wave-plan.md");
    if fs::exists(&wave_plan) {
        return fs::read_to_string(&wave_plan).is_ok_and(|t| header_marks_done(&t));
    }
    let spec_file = spec_root.join("spec.md");
    if !fs::exists(&spec_file) {
        // Spec dir empty / spec.md absent → treat as done.
        return true;
    }
    fs::read_to_string(&spec_file).is_ok_and(|t| header_marks_done(&t))
}

/// `true` when a spec's lifecycle header resolves to the terminal `Completed`
/// outcome. Legacy fallback only — see [`is_spec_done`]. Delegates to the
/// canonical [`mustard_core::domain::spec`] parser, so the new `### Stage:`/
/// `### Outcome:` header and every legacy `### Status:` shape
/// (`completed`/`done`/`closed`) are recognised. Fail-open: an unparseable
/// header is treated as not-done (the spec stays, its state file is not reaped).
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
    let now = mustard_core::time::now_unix_millis() as u128;
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

/// Pull every `rtk gain --json` rewrite into the W1 `savings_records` table
/// once per session.
///
/// Mirrors [`crate::commands::economy::rtk_gain`]'s own `persist_savings()` — same
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

    // W7B: emit one `pipeline.economy.savings.rtk-rewrite` NDJSON event per
    // record. The router fail-opens per call; a malformed record does not
    // block the rest.
    for rec in records {
        let (event_name, payload) = economy::writer::savings_event(&rec);
        crate::shared::events::economy::emit(cwd, ActorKind::Hook, "session-cleanup", &event_name, None, payload);
    }
}

/// Prune telemetry NDJSON files older than [`TELEMETRY_RETENTION_DAYS`].
///
/// W8A-1 (no-sqlite): the dedicated `telemetry.db` is gone — telemetry now
/// lives inline in the per-spec / per-session NDJSON event logs. Retention is
/// expressed at the file granularity: any `<root>/.claude/spec/*/.events/*.ndjson`
/// or `<root>/.claude/.session/*/.events/*.ndjson` whose `mtime` is older than
/// the cutoff is removed.
///
/// Internally delegates to [`prune_telemetry_with_cutoff`], which is the
/// testable form (the test passes an artificial cutoff so it does not need to
/// stamp mtimes far in the past).
///
/// Best-effort: any IO error degrades to a no-op — telemetry retention must
/// never abort session cleanup.
pub(crate) fn prune_telemetry(cwd: &str) {
    let now_ms = (mustard_core::time::now_unix_millis() as u128).min(i64::MAX as u128) as i64;
    let cutoff_ms = now_ms.saturating_sub(TELEMETRY_RETENTION_DAYS.saturating_mul(86_400_000));
    prune_telemetry_with_cutoff(cwd, cutoff_ms);
}

/// Inner form of [`prune_telemetry`]: delete every NDJSON file under the spec
/// and session `.events/` subtrees whose `mtime` precedes `cutoff_ms`
/// (Unix-epoch milliseconds).
///
/// `pub(crate)` for the integration test `session_cleanup_prune_ndjson`, which
/// drives the function with a synthetic future cutoff so two real files
/// (created milliseconds apart) split into "expired" and "kept".
pub(crate) fn prune_telemetry_with_cutoff(cwd: &str, cutoff_ms: i64) {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let spec_root = paths.spec_dir();
    let session_root = paths.claude_dir().join(".session");
    for root in [spec_root, session_root] {
        prune_ndjson_under(&root, cutoff_ms);
    }
}

/// Walk every immediate child of `root`, dive into `<child>/.events/`, and
/// remove every `*.ndjson` whose `mtime` is strictly older than `cutoff_ms`.
/// Fail-open at every layer.
fn prune_ndjson_under(root: &Path, cutoff_ms: i64) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let events_dir = entry.path().join(".events");
        let Ok(files) = std::fs::read_dir(&events_dir) else {
            continue;
        };
        for file in files.flatten() {
            let p = file.path();
            if p.extension().and_then(|x| x.to_str()) != Some("ndjson") {
                continue;
            }
            let Ok(meta) = file.metadata() else {
                continue;
            };
            let Ok(mtime) = meta.modified() else {
                continue;
            };
            let mtime_ms = mtime
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
                .unwrap_or(i64::MAX);
            if mtime_ms < cutoff_ms {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The cleanup plan
// ---------------------------------------------------------------------------

/// What every cleanup step is handed: the project root, its `.claude` dir and
/// the session id (absent for harness payloads that carry none).
struct CleanupTarget {
    cwd: String,
    claude: PathBuf,
    session_id: Option<String>,
}

/// The half of the plan that touches only paths known up front: bounded work,
/// no subtree walk, no directory of unknown size — and exactly ONE spawned
/// process, the collector kill in [`kill_pid`] (`sh -c kill` on POSIX,
/// `cmd /C taskkill` on Windows).
///
/// That spawn is not an exception to the rule, it IS the rule. On Windows it is
/// the most expensive thing this half does, and paying it first is the entire
/// reason the plan is split: a hook cancelled anywhere later has already done
/// the one step whose omission is not cosmetic. Anything that reads this
/// constant as "no processes here" is reading it backwards.
const PROMPT_STEPS: &[fn(&CleanupTarget)] = &[step_otel_pid, step_statusline_cache];

/// The half of the plan that walks a subtree of unknown size or spawns an
/// external process that is NOT what the hook came here to do (`rtk`, which
/// carries no deadline of its own). Every step here is cosmetic if it is
/// skipped, which is why it is the tail.
const DEFERRED_STEPS: &[fn(&CleanupTarget)] = &[
    step_pipeline_states,
    step_compact_state,
    step_prune_telemetry,
    step_ingest_rtk_savings,
    step_amend_finalize,
];

/// Kill this project's OTEL collector and drop its PID file — the only step
/// whose omission is not cosmetic, hence first in the plan.
fn step_otel_pid(target: &CleanupTarget) {
    clean_otel_pid(&target.claude);
}

/// Drop the statusline git cache from the temp dir.
fn step_statusline_cache(_target: &CleanupTarget) {
    clean_statusline_cache();
}

/// Reap terminal / orphaned pipeline-state files.
fn step_pipeline_states(target: &CleanupTarget) {
    clean_pipeline_states(&target.claude);
}

/// Reap `.compact-state` files past the 24h window.
fn step_compact_state(target: &CleanupTarget) {
    clean_compact_state(&target.claude);
}

/// Telemetry retention: drop NDJSON event files past the window so the spec and
/// session trees do not grow without bound. Fail-open.
fn step_prune_telemetry(target: &CleanupTarget) {
    prune_telemetry(&target.cwd);
}

/// Wave 2 (economia-didatica-e-economias-reais): drain the local
/// `rtk gain --json` ledger into `savings_records` once per session. Mirrors
/// the per-invocation persistence already done by `mustard-rt run rtk-gain`,
/// but for sessions that never explicitly run that subcommand — without this
/// hook, RTK rewrites never land in the W1 savings table. Strict side-effect,
/// fail-open, and it spawns `rtk`, which is why it trails the plan.
fn step_ingest_rtk_savings(target: &CleanupTarget) {
    ingest_rtk_savings(&target.cwd, target.session_id.as_deref());
}

/// wave-18-rt-followups (W4#1): finalize the per-session amendment window
/// before the session ends. The standalone CLI
/// (`mustard-rt run amend-finalize --session-id <id>`) is still available; this
/// re-wires the automatic SessionEnd hook that was dropped during the W3B
/// migration. Fail-open: no session id, or any internal error inside
/// `amend_finalize::run`, must never abort the rest of cleanup.
fn step_amend_finalize(target: &CleanupTarget) {
    let Some(sid) = target.session_id.as_deref() else {
        return;
    };
    if sid.is_empty() {
        return;
    }
    let _ = crate::commands::agent::amend_finalize::run(sid);
}

/// The sequence `observe` executes: [`PROMPT_STEPS`] and only then
/// [`DEFERRED_STEPS`].
///
/// It is a named function rather than an expression written inline in
/// `observe` because the order is the whole point of this module, and an order
/// stated in one place is an order a test can hold. `otel_pid_is_cleaned_before_
/// any_subprocess` asserts against THIS sequence, so swapping the two halves
/// cannot pass unnoticed — before the extraction the review found exactly that
/// hole: the test walked `PROMPT_STEPS` by hand, and flipping the chain in
/// `observe` left it green.
fn cleanup_plan() -> impl Iterator<Item = &'static fn(&CleanupTarget)> {
    PROMPT_STEPS.iter().chain(DEFERRED_STEPS)
}

impl Observer for SessionCleanupObserver {
    /// On `SessionEnd`, clean stale state. Any other trigger is a no-op. Pure
    /// side effect — never panics, never affects a verdict. Executes
    /// [`cleanup_plan`] — [`PROMPT_STEPS`] and only then [`DEFERRED_STEPS`] —
    /// so a hook cancelled part-way through has already done the step that
    /// mattered.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::SessionEnd) {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        let Ok(paths) = ClaudePaths::for_project(Path::new(&cwd)) else {
            return;
        };
        let target = CleanupTarget {
            claude: paths.claude_dir(),
            cwd,
            session_id: input.session_id.clone(),
        };
        for step in cleanup_plan() {
            step(&target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::SessionEnd),
            workspace_root: None,
            inject_only: None,
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
            inject_only: None,
        };
        SessionCleanupObserver.observe(&session_end_input(), &other);
        // PreToolUse → cleanup did not run, the terminal state survives.
        assert!(ClaudePaths::for_project(dir.path()).unwrap().pipeline_state_file("done").exists());
    }

    #[test]
    fn terminal_states_are_removed() {
        let dir = tempdir().unwrap();
        write_state(dir.path(), "finished", &json!({ "status": "completed" }));
        write_state(dir.path(), "active-one", &json!({ "status": "implementing" }));
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
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
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
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
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        assert!(!old.exists());
    }

    #[test]
    fn otel_pid_file_is_removed() {
        let dir = tempdir().unwrap();
        let harness = ClaudePaths::for_project(dir.path()).unwrap().harness_dir();
        std::fs::create_dir_all(&harness).unwrap();
        let pid = harness.join(".otel-collector.pid");
        std::fs::write(&pid, "12345").unwrap();
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        assert!(!pid.exists());
    }

    /// AC-3: on `SessionEnd` the collector teardown is the FIRST thing cleanup
    /// does — ahead of every subtree walk and every process that is not the
    /// kill itself.
    ///
    /// The name means "before any subprocess" the way the AC means it: before
    /// any subprocess that is not the collector kill. [`clean_otel_pid`] spawns
    /// `sh -c kill` / `cmd /C taskkill` of its own (see [`kill_pid`]) — that
    /// spawn is the payload the ordering exists to protect, not something it is
    /// racing.
    ///
    /// Two assertions, because either one alone is not a lock:
    ///
    /// 1. `observe` — the shipped entry point, not a hand-rolled loop — really
    ///    runs the whole plan: the PID file is gone AND the expired telemetry
    ///    file the deferred half owns was reaped.
    /// 2. The sequence `observe` consumes, [`cleanup_plan`], opens with exactly
    ///    [`PROMPT_STEPS`] in order, `step_otel_pid` first.
    ///
    /// Assertion 2 is what makes flipping the chain to
    /// `DEFERRED_STEPS.iter().chain(PROMPT_STEPS)` turn this test red — under
    /// that flip assertion 1 alone stays green, which is precisely the hole
    /// review found in the first cut of this test.
    #[test]
    fn otel_pid_is_cleaned_before_any_subprocess() {
        let dir = tempdir().unwrap();
        let paths = ClaudePaths::for_project(dir.path()).unwrap();

        let harness = paths.harness_dir();
        std::fs::create_dir_all(&harness).unwrap();
        let pid = harness.join(".otel-collector.pid");
        // A PID no live process can hold: the kill is a no-op, the removal is not.
        std::fs::write(&pid, format!("{}", u32::MAX)).unwrap();

        let events_dir = paths.spec_dir().join("some-spec").join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();
        let ndjson = events_dir.join("old.ndjson");
        std::fs::write(&ndjson, b"{\"event\":\"old\"}\n").unwrap();
        // Past the retention window, so the deferred half must reap it.
        let past_retention =
            Duration::from_secs((TELEMETRY_RETENTION_DAYS as u64 + 1) * 24 * 60 * 60);
        filetime_set(&ndjson, SystemTime::now() - past_retention).unwrap();

        // 1 — through the real hook entry point.
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
        assert!(
            !pid.exists(),
            "observe() must tear the collector down and drop its PID file"
        );
        assert!(
            !ndjson.exists(),
            "observe() must also run the deferred half — an expired telemetry \
             file survived, so the plan did not finish"
        );

        // 2 — and in that order. `cleanup_plan()` IS the sequence the loop in
        // `observe` walks, so this holds the real execution order, not a copy
        // of it.
        let plan: Vec<&'static fn(&CleanupTarget)> = cleanup_plan().collect();
        assert_eq!(
            plan.len(),
            PROMPT_STEPS.len() + DEFERRED_STEPS.len(),
            "cleanup_plan() must run every declared step exactly once"
        );
        assert!(
            std::ptr::fn_addr_eq(*plan[0], step_otel_pid as fn(&CleanupTarget)),
            "the collector teardown must be the FIRST step cleanup executes"
        );
        for (i, declared) in PROMPT_STEPS.iter().enumerate() {
            assert!(
                std::ptr::fn_addr_eq(*plan[i], *declared),
                "step {i} of the executed plan is not PROMPT_STEPS[{i}] — the \
                 prompt half no longer leads, so a cancelled hook can lose the \
                 collector kill"
            );
        }
    }

    #[test]
    fn observe_is_infallible_on_empty_project() {
        let dir = tempdir().unwrap();
        // No .claude dir at all — observe must not panic.
        SessionCleanupObserver.observe(&session_end_input(), &ctx(dir.path().to_str().unwrap()));
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

    // ---------------------------------------------------------------------
    // W8A-1 (no-sqlite) — AC-PRUNE tests for the NDJSON retention pruner.
    // ---------------------------------------------------------------------

    fn mtime_ms(path: &Path) -> i64 {
        std::fs::metadata(path)
            .expect("metadata")
            .modified()
            .expect("modified")
            .duration_since(UNIX_EPOCH)
            .expect("epoch")
            .as_millis() as i64
    }

    #[test]
    fn prune_telemetry_removes_old_ndjson_and_keeps_new() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let events_dir = project
            .join(".claude")
            .join("spec")
            .join("test-spec")
            .join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();

        let old_path = events_dir.join("old.ndjson");
        let new_path = events_dir.join("new.ndjson");

        // Write both, then backdate "old" by 60 seconds using the test helper
        // `filetime_set` (already in this module).
        std::fs::write(&old_path, b"{\"event\":\"old\"}\n").unwrap();
        std::fs::write(&new_path, b"{\"event\":\"new\"}\n").unwrap();
        filetime_set(
            &old_path,
            SystemTime::now() - std::time::Duration::from_secs(60),
        )
        .expect("backdate old mtime");

        let old_mtime = mtime_ms(&old_path);
        let new_mtime = mtime_ms(&new_path);
        assert!(
            new_mtime > old_mtime,
            "backdating must produce older mtime: old={old_mtime} new={new_mtime}"
        );

        // Cutoff between the two mtimes — old must be pruned, new must survive.
        let cutoff = (old_mtime + new_mtime) / 2;
        prune_telemetry_with_cutoff(project.to_str().unwrap(), cutoff);

        assert!(!old_path.exists(), "old.ndjson should have been pruned");
        assert!(new_path.exists(), "new.ndjson should have survived");
    }

    #[test]
    fn prune_telemetry_fail_open_on_missing_spec_root() {
        let dir = tempdir().unwrap();
        // No `.claude/spec/` tree at all — must not panic.
        prune_telemetry_with_cutoff(dir.path().to_str().unwrap(), i64::MAX);
    }

    #[test]
    fn prune_telemetry_walks_session_subtree_too() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let session_events = project
            .join(".claude")
            .join(".session")
            .join("otel-x")
            .join(".events");
        std::fs::create_dir_all(&session_events).unwrap();
        let old = session_events.join("old.ndjson");
        std::fs::write(&old, b"{\"event\":\"old\"}\n").unwrap();

        let old_mtime = mtime_ms(&old);
        // Cutoff in the future so the file qualifies as "old".
        let cutoff = old_mtime + 1_000_000;
        prune_telemetry_with_cutoff(project.to_str().unwrap(), cutoff);

        assert!(
            !old.exists(),
            "session-tree NDJSON should have been pruned alongside the spec tree"
        );
    }
}
