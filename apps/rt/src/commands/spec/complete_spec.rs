//! `mustard-rt run complete-spec` — pipeline close + archival.
//!
//! Two stages:
//!
//! - **followup** (default): snapshot `affectedFiles` from harness events
//!   (NDJSON) and `git diff`, emit `pipeline.status → closed-followup` +
//!   `pipeline.complete` events into the per-spec NDJSON sink.
//! - **archive** (`--archive`): emit the terminal `pipeline.status: completed`
//!   event for the spec (idempotent).
//!
//! `--archive-stale` / `--archive-followups` sweep every `closed-followup`
//! spec (the former only those older than 24 h). Both query the per-spec
//! NDJSON logs via `pipeline_state_from_events` rather than reading the
//! filesystem-state JSON sidecars.
//!
//! All I/O is fail-soft. The stdout JSON line stays shape-compatible with the
//! JS version (the `/close` command parses it).
//!
//! W4C migration: every SQLite reader/writer was removed. Events are written
//! via [`crate::shared::events::writer_ndjson::write_event_with_ts`] and read via
//! [`mustard_core::view::projection::read_harness_events_from_ndjson_dir`].

use crate::shared::context::session_id;
use mustard_core::io::fs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use mustard_core::domain::model::event::{
    EVENT_PIPELINE_COMPLETE, EVENT_PIPELINE_STATUS, PipelineCompletePayload,
    PipelineStatusPayload,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Time-to-live for `closed-followup` states swept by `--archive-stale` (24 h).
const FOLLOWUP_TTL_MS: i64 = 24 * 60 * 60 * 1000;


/// Run the project VCS binary in `cwd`, returning trimmed stdout or `""` on any
/// error. The binary is read from `mustard.json#vcs` (default `git`); callers
/// resolve it once via [`mustard_core::ProjectConfig::vcs`] and thread it
/// here so the spec affected-files diff/log is not hardcoded to `git`.
fn vcs_run(vcs_bin: &str, cwd: &Path, args: &[&str]) -> String {
    Command::new(vcs_bin)
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Resolve the parent (merge-base) branch for `current_branch` from the
/// project's `mustard.json#git.flow` promotion map: the branch's own promotion
/// target, else the wildcard `*` default, else `main`.
fn parent_branch_for(config: &mustard_core::ProjectConfig, current_branch: &str) -> String {
    config
        .git
        .flow
        .get(current_branch)
        .or_else(|| config.git.flow.get("*"))
        .cloned()
        .unwrap_or_else(|| "main".to_string())
}

/// Resolve the canonical spec directory under `.claude/spec/{name}/`.
#[allow(dead_code)]
fn spec_dir(cwd: &Path, spec: &str) -> PathBuf {
    ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.dir().to_path_buf())
        .unwrap_or_else(|_| cwd.to_path_buf())
}

/// Resolve the per-spec NDJSON `.events/` directory.
fn spec_events_dir(cwd: &Path, spec: &str) -> Option<PathBuf> {
    ClaudePaths::for_project(cwd)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())
}

/// Read every harness event ever written for `spec` from its per-spec NDJSON
/// `.events/` directory.
fn read_events_for_spec(cwd: &Path, spec: &str) -> Vec<mustard_core::domain::model::event::HarnessEvent> {
    let Some(dir) = spec_events_dir(cwd, spec) else {
        return Vec::new();
    };
    read_harness_events_from_ndjson_dir(&dir)
}

/// Collect the files a spec touched: harness `target.file` payloads + the VCS
/// diff against the parent branch.
///
/// Shared by the post-EXECUTE `applied`-edge inference in
/// [`crate::commands::event::emit_phase`]: both "what did this spec touch"
/// callers derive the file set from the same two sources (per-spec NDJSON
/// `target.file` events + a VCS diff vs the parent branch), so the derivation
/// lives here once and is called module-qualified rather than duplicated.
///
/// The VCS binary is read from `mustard.json#vcs` (default `git`). A user who
/// pins `"vcs": ""` opts out of VCS-derived files entirely — only the NDJSON
/// `target.file` events then feed the set. The diff/log invocation shape stays
/// git-style (`rev-parse` + `diff --name-only`); a full multi-VCS abstraction
/// (jj/hg argument variants) is intentionally deferred — `vcs_run` fail-opens to
/// an empty result when the pinned binary does not understand those args, so a
/// non-git VCS degrades to events-only rather than erroring.
pub fn collect_affected_files(cwd: &Path, spec: &str) -> Vec<String> {
    let mut files: BTreeSet<String> = BTreeSet::new();

    // 1. Harness events tagged with this spec (NDJSON).
    for ev in read_events_for_spec(cwd, spec) {
        if let Some(f) = ev
            .payload
            .get("target")
            .and_then(|t| t.get("file"))
            .and_then(Value::as_str)
        {
            if !f.is_empty() {
                files.insert(f.to_string());
            }
        }
    }

    // 2. VCS diff against the parent branch — only when a VCS is configured
    //    (default git; `vcs: ""` is an explicit opt-out → skip this source).
    let config = mustard_core::ProjectConfig::load(cwd);
    if let Some(vcs_bin) = config.vcs() {
        let branch = vcs_run(&vcs_bin, cwd, &["rev-parse", "--abbrev-ref", "HEAD"]);
        if !branch.is_empty() {
            let parent = parent_branch_for(&config, &branch);
            if !parent.is_empty() && branch != parent {
                let range = format!("{parent}...HEAD");
                let diff = vcs_run(&vcs_bin, cwd, &["diff", "--name-only", &range]);
                for f in diff.lines() {
                    let t = f.trim();
                    if !t.is_empty() {
                        files.insert(t.to_string());
                    }
                }
            }
        }
    }

    files.into_iter().collect()
}

/// Emit a typed pipeline event via the canonical NDJSON sink.
fn emit_ndjson(cwd: &Path, spec: &str, event_name: &str, payload: &Value, ts: &str) {
    let sid = session_id();
    let kind = crate::shared::events::route::classify_kind(event_name);
    let _ = crate::shared::events::writer_ndjson::write_event_with_ts(
        cwd,
        Some(spec),
        None,
        &sid,
        event_name,
        kind,
        Some(0),
        Some(&sid),
        Some("complete-spec"),
        None,
        payload,
        Some(ts),
    );
}

/// Stage 1 — mark the spec `closed-followup` by emitting two pipeline events
/// into the per-spec NDJSON sink:
///   1. `pipeline.status` with `{ from: <current>, to: "closed-followup" }`
///   2. `pipeline.complete` with `{ closedAt, affectedFiles: [...] }`
fn mark_followup(cwd: &Path, spec: &str) -> Value {
    let affected = collect_affected_files(cwd, spec);
    let now = mustard_core::time::now_iso8601();

    // Read current projection status so we can record `from`.
    let events = read_events_for_spec(cwd, spec);
    let current_status = crate::commands::event::event_projections::pipeline_state_from_events(&events, spec, None)
        .and_then(|v| v.status);

    let status_payload = serde_json::to_value(PipelineStatusPayload {
        from: current_status,
        to: "closed-followup".to_string(),
    })
    .unwrap_or(Value::Null);
    let complete_payload = serde_json::to_value(PipelineCompletePayload {
        closed_at: Some(now.clone()),
        affected_files: affected.clone(),
    })
    .unwrap_or(Value::Null);

    // Emit phase=CLOSE before flipping the status. Idempotent.
    emit_phase_close(cwd, spec);
    emit_ndjson(cwd, spec, EVENT_PIPELINE_STATUS, &status_payload, &now);
    emit_ndjson(cwd, spec, EVENT_PIPELINE_COMPLETE, &complete_payload, &now);

    json!({
        "ok": true,
        "mode": "followup",
        "spec": spec,
        "affectedFiles": affected.len(),
    })
}


/// Stage 2 — finalize archival. Idempotent terminal emit; no filesystem move.
fn archive(cwd: &Path, spec: &str) -> (bool, bool) {
    emit_completed_status(cwd, spec);
    let states_path = ClaudePaths::for_project(cwd)
        .map(|p| p.pipeline_state_file(spec))
        .unwrap_or_else(|_| cwd.join(format!("{spec}.json")));
    let had_legacy_state = fs::exists(&states_path);
    if had_legacy_state {
        let _ = fs::remove_file(&states_path);
    }
    (true, had_legacy_state)
}

/// Emit `pipeline.status: completed` so projections see the terminal state.
/// Idempotent: skips when the projection already shows `completed` or
/// `cancelled`.
fn emit_completed_status(cwd: &Path, spec: &str) {
    let events = read_events_for_spec(cwd, spec);
    let current_status = crate::commands::event::event_projections::pipeline_state_from_events(&events, spec, None)
        .and_then(|v| v.status);
    if matches!(current_status.as_deref(), Some("completed" | "cancelled")) {
        return;
    }
    emit_phase_close(cwd, spec);
    let payload = serde_json::to_value(PipelineStatusPayload {
        from: current_status,
        to: "completed".to_string(),
    })
    .unwrap_or(Value::Null);
    emit_ndjson(cwd, spec, EVENT_PIPELINE_STATUS, &payload, &mustard_core::time::now_iso8601());
}

/// Idempotently emit `pipeline.phase: CLOSE` when the spec's latest phase is
/// not already CLOSE.
fn emit_phase_close(cwd: &Path, spec: &str) {
    let last = crate::commands::event::emit_phase::last_phase_for_spec(cwd, spec);
    if last.as_deref() == Some("CLOSE") {
        return;
    }
    let ts = mustard_core::time::now_iso8601();
    let sid = session_id();
    let payload = json!({ "from": last, "to": "CLOSE" });
    let kind = crate::shared::events::route::classify_kind("pipeline.phase");
    let _ = crate::shared::events::writer_ndjson::write_event_with_ts(
        cwd,
        Some(spec),
        None,
        &sid,
        "pipeline.phase",
        kind,
        Some(0),
        Some(&sid),
        Some("complete-spec"),
        None,
        &payload,
        Some(&ts),
    );
}

/// Distinct spec names known under `.claude/spec/`.
fn distinct_specs(cwd: &Path) -> Vec<String> {
    let Ok(paths) = ClaudePaths::for_project(cwd) else {
        return Vec::new();
    };
    let root = paths.spec_dir();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            if let Some(n) = entry.file_name().to_str() {
                out.push(n.to_string());
            }
        }
    }
    out.sort();
    out
}

/// Sweep every `closed-followup` spec, archiving it (TTL-gated when asked).
///
/// Queries per-spec NDJSON via `pipeline_state_from_events` for every distinct
/// spec name under `.claude/spec/`.
fn archive_followups(cwd: &Path, require_ttl: bool) -> (usize, usize) {
    let specs = distinct_specs(cwd);
    let (mut scanned, mut archived) = (0usize, 0usize);
    for spec_name in &specs {
        let events = read_events_for_spec(cwd, spec_name);
        let Some(view) = crate::commands::event::event_projections::pipeline_state_from_events(
            &events,
            spec_name,
            None,
        ) else {
            continue;
        };
        if view.status.as_deref() != Some("closed-followup") {
            continue;
        }
        scanned += 1;
        if require_ttl {
            let closed_ms = view.closed_at.as_deref().and_then(mustard_core::time::parse_iso_millis);
            match closed_ms {
                Some(c) => {
                    let now = i64::try_from(mustard_core::time::now_unix_millis() as u128).unwrap_or(i64::MAX);
                    if now - c < FOLLOWUP_TTL_MS {
                        continue;
                    }
                }
                None => continue,
            }
        }
        let (moved, had_state) = archive(cwd, spec_name);
        if moved || had_state {
            archived += 1;
        }
    }
    (scanned, archived)
}

/// Stage-1 close for `spec` (closed-followup → `pipeline.complete`) as a
/// reusable, non-printing Rust entry point.
///
/// Mirrors the default `run(...)` path: a fail-open QA pass, the two-event
/// followup mark via [`mark_followup`], then a fail-open registry rebuild.
/// Returns the followup JSON value (`{ ok, mode: "followup", spec,
/// affectedFiles }`) so callers — e.g.
/// [`crate::commands::pipeline::close_orchestrate`] auto-chaining after every
/// gate passes — can fold it into their own report without spawning a
/// subprocess. Deterministic and idempotent (the underlying emits are
/// idempotent on phase/status).
pub fn run_followup(cwd: &Path, spec: &str) -> Value {
    run_qa_fail_open(cwd, spec);
    let followup_value = mark_followup(cwd, spec);
    rebuild_one_fail_open(cwd, spec);
    followup_value
}

/// Dispatch `mustard-rt run complete-spec`, writing one JSON line to stdout.
pub fn run(spec: Option<&str>, archive_flag: bool, archive_stale: bool, archive_followups_flag: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());

    if archive_stale {
        let (scanned, archived) = archive_followups(&cwd, true);
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive-stale", "scanned": scanned, "archived": archived })
        );
        return;
    }
    if archive_followups_flag {
        let (scanned, archived) = archive_followups(&cwd, false);
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive-followups", "scanned": scanned, "archived": archived })
        );
        return;
    }

    let Some(spec) = spec else {
        eprintln!(
            "usage: complete-spec <spec-name> [--archive] | --archive-stale | --archive-followups"
        );
        std::process::exit(2);
    };

    if archive_flag {
        run_qa_fail_open(&cwd, spec);
        let (moved_spec, had_state) = archive(&cwd, spec);
        rebuild_one_fail_open(&cwd, spec);
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive", "spec": spec, "movedSpec": moved_spec, "hadState": had_state })
        );
        return;
    }

    let followup_value = run_followup(&cwd, spec);
    println!("{followup_value}");
}

fn rebuild_one_fail_open(cwd: &Path, spec: &str) {
    let project_dir = cwd.to_string_lossy();
    let _ = crate::commands::spec::rebuild_specs::rebuild_one(&project_dir, spec);
}

fn run_qa_fail_open(_cwd: &Path, spec: &str) {
    let outcome = crate::commands::review::qa_run::run_for_spec_with_options(
        spec,
        crate::commands::review::qa_run::QaRunOptions { self_invoked: true },
    );
    eprintln!(
        "[complete-spec] QA: spec={} overall={} passed={}/{}",
        outcome.spec, outcome.overall, outcome.passed, outcome.total,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_iso_millis_round_trips() {
        let ms = mustard_core::time::parse_iso_millis("2026-05-19T00:00:00.000Z").unwrap();
        assert_eq!(ms, 1_779_148_800_000);
    }

    #[test]
    fn parse_iso_millis_without_fraction() {
        assert!(mustard_core::time::parse_iso_millis("2026-05-19T12:30:45Z").is_some());
        assert!(mustard_core::time::parse_iso_millis("garbage").is_none());
    }

    /// The deterministic finalize step (the body of `run_followup`, minus the
    /// fail-open QA / registry side-effects that resolve against the live cwd):
    /// `mark_followup` flips the spec to `closed-followup` and emits
    /// `pipeline.complete` + `pipeline.phase: CLOSE` into the per-spec NDJSON.
    /// This is what `close_orchestrate` auto-chains in-process when all gates
    /// pass. Idempotent: a second call leaves the projection on the same status.
    #[test]
    fn mark_followup_emits_close_and_complete_to_ndjson() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();

        let out = mark_followup(cwd, "demo");
        assert_eq!(out.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(out.get("mode").and_then(Value::as_str), Some("followup"));

        // Projection reflects the closed-followup status.
        let events = read_events_for_spec(cwd, "demo");
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, "demo", None,
        )
        .expect("projection exists after mark_followup");
        assert_eq!(view.status.as_deref(), Some("closed-followup"));

        // The pipeline.complete event landed and is verifiable via the same
        // reusable helper close_orchestrate uses for its auto-verify step.
        assert!(crate::commands::event::verify_emit::verify_event_landed(
            cwd,
            "pipeline.complete",
            Some("demo"),
            Some("1h"),
        ));

        // Phase is CLOSE.
        assert_eq!(
            crate::commands::event::emit_phase::last_phase_for_spec(cwd, "demo").as_deref(),
            Some("CLOSE"),
        );

        // Idempotency: a second flip keeps the same terminal-ish status.
        let _ = mark_followup(cwd, "demo");
        let events2 = read_events_for_spec(cwd, "demo");
        let view2 = crate::commands::event::event_projections::pipeline_state_from_events(
            &events2, "demo", None,
        )
        .expect("projection still exists");
        assert_eq!(view2.status.as_deref(), Some("closed-followup"));
    }
}
