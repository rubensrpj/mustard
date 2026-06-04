//! `mustard-rt run complete-spec` — single-stage pipeline close to `completed`.
//!
//! A spec that closes goes **straight to `completed`** — there is no
//! intermediate `closed-followup` grace window. Follow-up work is handled by a
//! separate linked sub-spec (or a future "reopen" action), not a TTL sweep.
//!
//! - **complete** (default): snapshot `affectedFiles` from harness events
//!   (NDJSON) and `git diff`, then emit — all coupled —
//!   `pipeline.status → completed` + `pipeline.complete { closedAt,
//!   affectedFiles }` into the per-spec NDJSON sink AND patch the root
//!   `meta.json` to `Close/Completed`. Idempotent: a second call is a no-op flip
//!   (skipped when the projection already shows `completed`/`cancelled`).
//! - **archive** (`--archive`): an idempotent alias of the single complete —
//!   re-emits `completed` + meta sync, safe to call twice. No filesystem move.
//!
//! All I/O is fail-soft. The stdout JSON line stays shape-compatible with the
//! `/close` command that parses it (the `{ ok, mode, spec, ... }` line).
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

/// Terminal complete — mark the spec `completed` by emitting the coupled
/// close transition into the per-spec NDJSON sink AND syncing the root
/// `meta.json`, all in one stage:
///   1. `pipeline.phase: CLOSE` (idempotent)
///   2. `pipeline.status` with `{ from: <current>, to: "completed" }`
///   3. `pipeline.complete` with `{ closedAt, affectedFiles: [...] }`
///   4. `meta.json` → `Close/Completed/CLOSE` (via `patch_meta_complete`)
///
/// Idempotent: skips the whole flip when the projection already shows
/// `completed` or `cancelled` (mirrors the guard the legacy archive stage used),
/// so a second call — or `--archive` after the auto-finalize — is a no-op. This
/// guarantees the event projection and the sidecar never diverge: both end on
/// `completed`.
fn mark_complete(cwd: &Path, spec: &str) -> Value {
    let affected = collect_affected_files(cwd, spec);

    // Read current projection status so we can record `from` and short-circuit
    // an already-terminal spec.
    let events = read_events_for_spec(cwd, spec);
    let current_status = crate::commands::event::event_projections::pipeline_state_from_events(&events, spec, None)
        .and_then(|v| v.status);
    if matches!(current_status.as_deref(), Some("completed" | "cancelled")) {
        return json!({
            "ok": true,
            "mode": "complete",
            "spec": spec,
            "affectedFiles": affected.len(),
        });
    }

    let now = mustard_core::time::now_iso8601();
    let status_payload = serde_json::to_value(PipelineStatusPayload {
        from: current_status,
        to: "completed".to_string(),
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

    // Sync the **root** `meta.json` to `stage=Close, outcome=Completed`. These
    // events go straight through `writer_ndjson` (not through `emit-pipeline
    // run`, which is where the sidecar-sync otherwise lives), so without this
    // call a completed spec would stay stuck at `Plan/Active` in its sidecar
    // while its event log says it is done. Coupling the meta patch here is what
    // keeps `status_word(&Meta)` and `status_word(&SpecState)` consistent.
    // Fail-open: a missing spec dir or write error is a silent no-op.
    crate::commands::event::emit_pipeline::patch_meta_complete(cwd, spec, &now);

    // Drop the session→spec binding now that the spec is terminal: events in
    // the gap after the close must not inherit this just-finished spec. Only on
    // the completed close, and only when a real session is resolvable (a
    // `"unknown"` id is skipped inside `unbind_session_spec`).
    let sid = session_id();
    crate::shared::context::unbind_session_spec(&cwd.to_string_lossy(), &sid);

    json!({
        "ok": true,
        "mode": "complete",
        "spec": spec,
        "affectedFiles": affected.len(),
    })
}


/// `--archive` — idempotent alias of the single complete. Re-runs
/// [`mark_complete`] (a no-op flip when the spec is already terminal) and drops
/// any legacy `.pipeline-states/{spec}.json` sidecar. No filesystem move.
fn archive(cwd: &Path, spec: &str) -> (bool, bool) {
    let _ = mark_complete(cwd, spec);
    let states_path = ClaudePaths::for_project(cwd)
        .map(|p| p.pipeline_state_file(spec))
        .unwrap_or_else(|_| cwd.join(format!("{spec}.json")));
    let had_legacy_state = fs::exists(&states_path);
    if had_legacy_state {
        let _ = fs::remove_file(&states_path);
    }
    (true, had_legacy_state)
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

/// Terminal close for `spec` (→ `completed` + `pipeline.complete` + meta sync)
/// as a reusable, non-printing Rust entry point.
///
/// Mirrors the default `run(...)` path: a fail-open QA pass, the coupled
/// terminal complete via [`mark_complete`], then a fail-open registry rebuild.
/// Returns the complete JSON value (`{ ok, mode: "complete", spec,
/// affectedFiles }`) so callers — e.g.
/// [`crate::commands::pipeline::close_orchestrate`] auto-chaining after every
/// gate passes — can fold it into their own report without spawning a
/// subprocess. Deterministic and idempotent (the underlying emits are
/// idempotent on phase/status and short-circuit an already-terminal spec).
pub fn run_complete(cwd: &Path, spec: &str) -> Value {
    run_qa_fail_open(cwd, spec);
    let complete_value = mark_complete(cwd, spec);
    rebuild_one_fail_open(cwd, spec);
    complete_value
}

/// Dispatch `mustard-rt run complete-spec`, writing one JSON line to stdout.
///
/// `--archive-stale` / `--archive-followups` are retained as harmless no-ops:
/// the single-stage close no longer produces `closed-followup` specs, so there
/// is nothing for the old TTL sweep to find. Each prints
/// `{ scanned: 0, archived: 0 }` so any caller still passing the flag keeps a
/// shape-compatible JSON line.
pub fn run(spec: Option<&str>, archive_flag: bool, archive_stale: bool, archive_followups_flag: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());

    if archive_stale {
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive-stale", "scanned": 0, "archived": 0 })
        );
        return;
    }
    if archive_followups_flag {
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive-followups", "scanned": 0, "archived": 0 })
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

    let complete_value = run_complete(&cwd, spec);
    println!("{complete_value}");
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

    /// The deterministic finalize step (the body of `run_complete`, minus the
    /// fail-open QA / registry side-effects that resolve against the live cwd):
    /// `mark_complete` flips the spec straight to `completed` and emits
    /// `pipeline.complete` + `pipeline.phase: CLOSE` into the per-spec NDJSON.
    /// This is what `close_orchestrate` auto-chains in-process when all gates
    /// pass. Idempotent: a second call leaves the projection on the same status.
    #[test]
    fn mark_complete_emits_close_and_complete_to_ndjson() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();

        let out = mark_complete(cwd, "demo");
        assert_eq!(out.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(out.get("mode").and_then(Value::as_str), Some("complete"));

        // Projection reflects the terminal `completed` status — no
        // `closed-followup` intermediate.
        let events = read_events_for_spec(cwd, "demo");
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, "demo", None,
        )
        .expect("projection exists after mark_complete");
        assert_eq!(view.status.as_deref(), Some("completed"));

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

        // Idempotency: a second flip keeps the same terminal status.
        let _ = mark_complete(cwd, "demo");
        let events2 = read_events_for_spec(cwd, "demo");
        let view2 = crate::commands::event::event_projections::pipeline_state_from_events(
            &events2, "demo", None,
        )
        .expect("projection still exists");
        assert_eq!(view2.status.as_deref(), Some("completed"));
    }

    /// No-divergence guarantee: after the auto-finalize, BOTH the event
    /// projection AND `meta.json` say `completed`. This is the bug the
    /// single-stage close fixes — the old `closed-followup` mark left the
    /// projection on `closed-followup` while the (separate) archive stage put
    /// the sidecar on `Completed`, so the two sources disagreed.
    #[test]
    fn mark_complete_keeps_projection_and_meta_consistent() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "demo-consistent";

        // Seed a spec dir whose meta.json is still mid-pipeline (Execute/Active).
        let spec_dir = cwd.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Execute","outcome":"Active","phase":"EXECUTE","scope":"full","lang":"pt-BR"}"#,
        )
        .unwrap();

        mark_complete(cwd, spec);

        // Event projection → completed.
        let events = read_events_for_spec(cwd, spec);
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, spec, None,
        )
        .expect("projection exists after mark_complete");
        assert_eq!(view.status.as_deref(), Some("completed"));

        // meta.json sidecar → Close/Completed/CLOSE (same verdict, no divergence).
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["stage"], json!("Close"), "{meta}");
        assert_eq!(meta["outcome"], json!("Completed"), "{meta}");
        assert_eq!(meta["phase"], json!("CLOSE"), "{meta}");
        // Other fields preserved.
        assert_eq!(meta["scope"], json!("full"), "{meta}");
        assert_eq!(meta["lang"], json!("pt-BR"), "{meta}");

        // The status word derived from the projection and from the meta agree.
        let meta_outcome = meta["outcome"].as_str().unwrap_or_default();
        assert_eq!(
            view.status.as_deref(),
            Some("completed"),
            "projection status must equal the meta outcome ({meta_outcome})",
        );
        assert_eq!(meta_outcome, "Completed");
    }

    /// FRONT 3 (D-lifecycle): the terminal complete emits the `completed` event
    /// AND syncs the root `meta.json` to `Close/Completed`. Before the fix the
    /// status flowed straight through `writer_ndjson`, leaving a finished spec
    /// stuck at its last `Plan/Active` sidecar value. The meta-sync now lives in
    /// `mark_complete` (the single close stage).
    #[test]
    fn complete_syncs_root_meta() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let spec = "demo-complete";

        // Seed a spec dir with a meta.json still in Plan/Active.
        let spec_dir = cwd.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Plan","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR"}"#,
        )
        .unwrap();

        super::mark_complete(cwd, spec);

        // The terminal status event landed.
        let events = read_events_for_spec(cwd, spec);
        let view = crate::commands::event::event_projections::pipeline_state_from_events(
            &events, spec, None,
        )
        .expect("projection exists after mark_complete");
        assert_eq!(view.status.as_deref(), Some("completed"));

        // The ROOT meta.json sidecar is now Close/Completed (the bug fix).
        let meta: Value = serde_json::from_str(
            &std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["stage"], json!("Close"), "{meta}");
        assert_eq!(meta["outcome"], json!("Completed"), "{meta}");
        assert_eq!(meta["phase"], json!("CLOSE"), "{meta}");
        // Other fields preserved.
        assert_eq!(meta["scope"], json!("full"), "{meta}");
        assert_eq!(meta["lang"], json!("pt-BR"), "{meta}");
    }
}
