//! `mustard-rt run complete-spec` — a port of `scripts/complete-spec.js`.
//!
//! Finalizes a pipeline spec in two stages:
//!
//! - **followup** (default): snapshot `affectedFiles` from harness events and
//!   `git diff`, emit `pipeline.status → closed-followup` + `pipeline.complete`
//!   events to the SQLite store. The legacy `.pipeline-states/{spec}.json` file
//!   is NOT written for new pipelines.
//! - **archive** (`--archive`): move `spec/active/<name>` → `spec/completed/`
//!   and delete the legacy state JSON if it still exists (back-compat for specs
//!   that ran before the event migration).
//!
//! `--archive-stale` / `--archive-followups` sweep every `closed-followup`
//! spec (the former only those older than 24 h). Both now query the event
//! store via `pipeline_state_for_spec` rather than reading the filesystem.
//!
//! All I/O is fail-soft. The stdout JSON line stays shape-compatible with the
//! JS version (the `/close` command parses it).
//!
//! Port note (archive_metrics_from_state): the legacy `state.metrics` sidecar
//! was written by pre-migration pipelines. Post-migration specs never have a
//! `.pipeline-states` JSON at all, so `archive_metrics_from_state` is only
//! reached for in-flight legacy specs. It is preserved here for that case only.

use crate::run::env::session_id;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, SCHEMA_VERSION,
    EVENT_PIPELINE_COMPLETE, EVENT_PIPELINE_STATUS,
    PipelineCompletePayload, PipelineStatusPayload,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Time-to-live for `closed-followup` states swept by `--archive-stale` (24 h).
const FOLLOWUP_TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// Read a JSON file, returning `None` on any error.
fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write a JSON value pretty-printed with a trailing newline. Fail-soft.
fn write_json(path: &Path, value: &Value) -> bool {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(value) {
        Ok(text) => std::fs::write(path, format!("{text}\n")).is_ok(),
        Err(_) => false,
    }
}

/// Run a git command in `cwd`, returning trimmed stdout or `""` on any error.
fn git(cwd: &Path, args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Resolve the parent branch for `current_branch` via `mustard.json` gitFlow.
fn parent_branch_for(cwd: &Path, current_branch: &str) -> String {
    let m = read_json(&cwd.join("mustard.json")).unwrap_or(Value::Null);
    if let Some(flow) = m.get("gitFlow") {
        if let Some(p) = flow
            .get("parentOf")
            .and_then(|po| po.get(current_branch))
            .and_then(Value::as_str)
        {
            return p.to_string();
        }
        if let Some(main) = flow.get("mainBranch").and_then(Value::as_str) {
            return main.to_string();
        }
    }
    "main".to_string()
}

/// Path helpers — mirror the JS `*SpecDir` / `pipelineStatePath` helpers.
fn active_spec_dir(cwd: &Path, spec: &str) -> PathBuf {
    cwd.join(".claude").join("spec").join("active").join(spec)
}
fn completed_spec_dir(cwd: &Path, spec: &str) -> PathBuf {
    cwd.join(".claude").join("spec").join("completed").join(spec)
}
fn pipeline_state_path(cwd: &Path, spec: &str) -> PathBuf {
    cwd.join(".claude")
        .join(".pipeline-states")
        .join(format!("{spec}.json"))
}

/// Collect the files a spec touched: harness `target.file` payloads, the git
/// diff against the parent branch, and any path-like `toolBreakdown` keys.
fn collect_affected_files(cwd: &Path, spec: &str, state: &Value) -> Vec<String> {
    let mut files: BTreeSet<String> = BTreeSet::new();

    // 1. Harness events tagged with this spec.
    let events = SqliteEventStore::for_project(cwd)
        .and_then(|store| store.replay())
        .unwrap_or_default();
    for ev in &events {
        if ev.spec.as_deref() != Some(spec) {
            continue;
        }
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

    // 2. Git diff against the parent branch.
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]);
    if !branch.is_empty() {
        let parent = parent_branch_for(cwd, &branch);
        if !parent.is_empty() && branch != parent {
            let range = format!("{parent}...HEAD");
            let diff = git(cwd, &["diff", "--name-only", &range]);
            for f in diff.lines() {
                let t = f.trim();
                if !t.is_empty() {
                    files.insert(t.to_string());
                }
            }
        }
    }

    // 3. Path-like keys in `state.metrics.toolBreakdown`.
    if let Some(tb) = state
        .get("metrics")
        .and_then(|m| m.get("toolBreakdown"))
        .and_then(Value::as_object)
    {
        for k in tb.keys() {
            if k.contains('/') || k.contains('\\') {
                files.insert(k.clone());
            }
        }
    }

    files.into_iter().collect()
}

/// Recursively copy `src` into `dst`.
fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

/// Move a directory, falling back to copy + remove across devices. Fail-soft.
fn move_dir(src: &Path, dst: &Path) -> bool {
    if let Some(parent) = dst.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(src, dst).is_ok() {
        return true;
    }
    if copy_recursive(src, dst).is_ok() {
        let _ = std::fs::remove_dir_all(src);
        return true;
    }
    false
}

/// Build a pipeline event with the standard envelope fields.
fn pipeline_event(kind: &str, spec: &str, payload: Value) -> HarnessEvent {
    HarnessEvent {
        v: SCHEMA_VERSION,
        ts: crate::util::now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("complete-spec".to_string()),
            actor_type: None,
        },
        event: kind.to_string(),
        payload,
        spec: Some(spec.to_string()),
    }
}

/// Stage 1 — mark the spec `closed-followup` by emitting two pipeline events:
///   1. `pipeline.status` with `{ from: <current>, to: "closed-followup" }`
///   2. `pipeline.complete` with `{ closedAt, affectedFiles: [...] }`
///
/// Fail-open: if the event store cannot be opened, falls back to writing the
/// legacy `.pipeline-states/{spec}.json` so that the CLOSE flow on a broken DB
/// still moves forward.
fn mark_followup(cwd: &Path, spec: &str) -> Value {
    // Collect affected files (harness events + git diff). We pass an empty
    // Value here because the state JSON is no longer the source for toolBreakdown.
    let affected = collect_affected_files(cwd, spec, &Value::Null);
    let now = crate::util::now_iso8601();

    // --- Try to emit events to the SQLite store ---
    let store_result = SqliteEventStore::for_project(cwd);
    match store_result {
        Ok(store) => {
            // Read current projection status so we can record `from`.
            let current_status = crate::run::event_projections::pipeline_state_for_spec(
                &store,
                spec,
                None,
            )
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

            let _ = store.append(&pipeline_event(EVENT_PIPELINE_STATUS, spec, status_payload));
            let _ = store.append(&pipeline_event(EVENT_PIPELINE_COMPLETE, spec, complete_payload));

            json!({
                "ok": true,
                "mode": "followup",
                "spec": spec,
                "affectedFiles": affected.len(),
            })
        }
        Err(e) => {
            // Fail-open: store unavailable — write legacy JSON so CLOSE flow survives.
            eprintln!(
                "[complete-spec] WARN: event store unavailable ({e}); \
                 falling back to legacy pipeline-state JSON"
            );
            let state_path = pipeline_state_path(cwd, spec);
            let state = json!({
                "specName": spec,
                "status": "closed-followup",
                "closedAt": now,
                "affectedFiles": affected,
            });
            let ok = write_json(&state_path, &state);
            json!({
                "ok": ok,
                "mode": "followup",
                "spec": spec,
                "affectedFiles": affected.len(),
                "fallback": "legacy-json",
            })
        }
    }
}

/// Archive the legacy `state.metrics` sidecar to `.claude/metrics/<spec>.json`.
fn archive_metrics_from_state(cwd: &Path, spec: &str, state: &Value) -> bool {
    let Some(m) = state.get("metrics") else {
        return false;
    };
    let metrics_dir = cwd.join(".claude").join("metrics");
    if std::fs::create_dir_all(&metrics_dir).is_err() {
        return false;
    }
    let duration = match (
        m.get("startedAt").and_then(Value::as_str),
        m.get("updatedAt").and_then(Value::as_str),
    ) {
        (Some(a), Some(b)) => match (parse_iso_millis(a), parse_iso_millis(b)) {
            (Some(sa), Some(ub)) => json!((ub - sa).max(0)),
            _ => Value::Null,
        },
        _ => Value::Null,
    };
    let retries = m.get("retries").and_then(Value::as_i64).unwrap_or(0);
    let out = json!({
        "name": spec,
        "completedAt": state.get("completedAt").and_then(Value::as_str)
            .map(str::to_string).unwrap_or_else(crate::util::now_iso8601),
        "durationMs": duration,
        "apiCalls": m.get("apiCalls").and_then(Value::as_i64).unwrap_or(0),
        "retries": retries,
        "pass1": retries == 0,
        "toolBreakdown": m.get("toolBreakdown").cloned().unwrap_or_else(|| json!({})),
        "agentCount": m.get("agentCount").cloned().unwrap_or(Value::Null),
        "dispatchFailuresByPhase": m.get("dispatchFailuresByPhase").cloned().unwrap_or(Value::Null),
        "source": "legacy-state",
    });
    write_json(&metrics_dir.join(format!("{spec}.json")), &out)
}

/// Parse an ISO-8601 timestamp into epoch millis (UTC). `None` on any failure.
///
/// Shared with the `epic-fold` port for event-duration computation.
pub(crate) fn parse_iso_millis(ts: &str) -> Option<i64> {
    // Expect `YYYY-MM-DDThh:mm:ss(.sss)?Z` — the shape `now_iso8601` emits.
    let bytes = ts.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let num = |a: usize, b: usize| ts.get(a..b)?.parse::<i64>().ok();
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hh = num(11, 13)?;
    let mm = num(14, 16)?;
    let ss = num(17, 19)?;
    let millis = if ts.len() >= 23 && bytes.get(19) == Some(&b'.') {
        num(20, 23).unwrap_or(0)
    } else {
        0
    };
    // Days from civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + hh * 3600 + mm * 60 + ss) * 1000) + millis)
}

/// Stage 2 — archive a spec: move the spec dir and drop the legacy state JSON
/// if it still exists (back-compat for specs written before the event migration).
///
/// Metrics archival via `archive_metrics_from_state` is only attempted when a
/// legacy `.pipeline-states/{spec}.json` with a `metrics` field is found —
/// post-migration specs have no such file, so they get no `.claude/metrics/`
/// sidecar (which is acceptable; nothing in CLOSE depends on it).
fn archive(cwd: &Path, spec: &str) -> (bool, bool) {
    let active = active_spec_dir(cwd, spec);
    let completed = completed_spec_dir(cwd, spec);
    let state_path = pipeline_state_path(cwd, spec);

    let moved_spec = if active.exists() && !completed.exists() {
        move_dir(&active, &completed)
    } else {
        // Already moved (completed exists, active gone) counts as success.
        !active.exists() && completed.exists()
    };

    // Legacy metrics archival — only for pre-migration specs that still had
    // a state JSON with a `metrics` field. Skip entirely for new specs.
    let state = read_json(&state_path);
    if let Some(ref s) = state {
        if s.get("metrics").is_some() {
            archive_metrics_from_state(cwd, spec, s);
        }
    }

    let had_legacy_state = state_path.exists();
    if had_legacy_state {
        let _ = std::fs::remove_file(&state_path);
    }
    (moved_spec, had_legacy_state)
}

/// Sweep every `closed-followup` spec, archiving it (TTL-gated when asked).
///
/// Queries the event store via `pipeline_state_for_spec` for all distinct
/// spec names; does not scan the `.pipeline-states/` filesystem directory.
/// Falls back to the legacy FS scan if the event store cannot be opened.
fn archive_followups(cwd: &Path, require_ttl: bool) -> (usize, usize) {
    // --- Try the event store first ---
    let store = match SqliteEventStore::for_project(cwd) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "[complete-spec] WARN: event store unavailable ({e}); \
                 falling back to legacy FS scan for archive-followups"
            );
            return archive_followups_legacy_fs(cwd, require_ttl);
        }
    };

    let specs = match store.distinct_specs() {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "[complete-spec] WARN: distinct_specs query failed ({e}); \
                 falling back to legacy FS scan"
            );
            return archive_followups_legacy_fs(cwd, require_ttl);
        }
    };

    let (mut scanned, mut archived) = (0usize, 0usize);
    for spec_name in &specs {
        let Some(view) = crate::run::event_projections::pipeline_state_for_spec(
            &store,
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
            let closed_ms = view.closed_at.as_deref().and_then(parse_iso_millis);
            match closed_ms {
                Some(c) => {
                    let now = i64::try_from(crate::util::now_millis()).unwrap_or(i64::MAX);
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

/// Legacy fallback: scan `.pipeline-states/` JSON files for `closed-followup`.
/// Used when the event store is unavailable.
fn archive_followups_legacy_fs(cwd: &Path, require_ttl: bool) -> (usize, usize) {
    let states_dir = cwd.join(".claude").join(".pipeline-states");
    let Ok(entries) = std::fs::read_dir(&states_dir) else {
        return (0, 0);
    };
    let (mut scanned, mut archived) = (0usize, 0usize);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") || name.ends_with(".metrics.json") {
            continue;
        }
        scanned += 1;
        let Some(state) = read_json(&entry.path()) else {
            continue;
        };
        if state.get("status").and_then(Value::as_str) != Some("closed-followup") {
            continue;
        }
        if require_ttl {
            let closed = state
                .get("closedAt")
                .and_then(Value::as_str)
                .and_then(parse_iso_millis);
            match closed {
                Some(c) => {
                    let now = i64::try_from(crate::util::now_millis()).unwrap_or(i64::MAX);
                    if now - c < FOLLOWUP_TTL_MS {
                        continue;
                    }
                }
                None => continue,
            }
        }
        let spec = state
            .get("specName")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| name.trim_end_matches(".json").to_string());
        let (moved, had_state) = archive(cwd, &spec);
        if moved || had_state {
            archived += 1;
        }
    }
    (scanned, archived)
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

    // Run QA before any state transition so `qa.result` is always emitted at
    // CLOSE time, regardless of whether the user ran `/mustard:qa` manually.
    // Fail-open: a QA failure never aborts the CLOSE flow.
    run_qa_fail_open(&cwd, spec);

    if archive_flag {
        let (moved_spec, had_state) = archive(&cwd, spec);
        rebuild_one_fail_open(&cwd, spec);
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive", "spec": spec, "movedSpec": moved_spec, "hadState": had_state })
        );
        return;
    }

    let followup_value = mark_followup(&cwd, spec);
    // Refresh the `specs` + `metrics_projection` rows so the dashboard sees
    // this spec's terminal status without waiting for the next manual
    // `mustard-rt run rebuild-specs`. Fail-open by design — telemetry never
    // blocks a CLI flow.
    rebuild_one_fail_open(&cwd, spec);
    println!("{followup_value}");
}

/// Invoke [`crate::run::rebuild_specs::rebuild_one`] but swallow every error.
///
/// `complete-spec` is part of the CLI hot path; a rebuild failure on the
/// projection side must not abort the user's archival / followup workflow.
fn rebuild_one_fail_open(cwd: &Path, spec: &str) {
    let project_dir = cwd.to_string_lossy();
    let _ = crate::run::rebuild_specs::rebuild_one(&project_dir, spec);
}

/// Invoke [`crate::run::qa_run::run_for_spec`] and log the outcome to stderr.
///
/// `cwd` is passed for documentation clarity; `run_for_spec` resolves the
/// project dir from the process working directory (same as `complete_spec::run`
/// resolved `cwd` from). Fail-open: a QA failure is logged and the function
/// returns normally — the CLOSE flow is never blocked.
fn run_qa_fail_open(_cwd: &Path, spec: &str) {
    // `self_invoked: true` makes `qa_run::run_for_spec` auto-exclude
    // `mustard-rt` and `mustard-dashboard` from any `cargo build/test
    // --workspace` AC. Closes the catch-22: this very process is foreground
    // holding the exe that cargo would have to relink. See
    // `qa_run::rewrite_self_invoked_cargo` for the rewrite rules.
    let outcome = crate::run::qa_run::run_for_spec_with_options(
        spec,
        crate::run::qa_run::QaRunOptions { self_invoked: true },
    );
    eprintln!(
        "[complete-spec] QA: spec={} overall={} passed={}/{}",
        outcome.spec, outcome.overall, outcome.passed, outcome.total,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::store::event_store::EventSink;
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use mustard_core::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use tempfile::tempdir;

    fn temp_store(cwd: &Path) -> SqliteEventStore {
        // Uses the MUSTARD_DB_PATH convention (for_project resolves standard path).
        SqliteEventStore::for_project(cwd).unwrap()
    }

    fn ev(event: &str, spec: &str, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T10:00:00.000Z".to_string(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Cli, id: None, actor_type: None },
            event: event.to_string(),
            payload,
            spec: Some(spec.to_string()),
        }
    }

    #[test]
    fn parse_iso_millis_round_trips() {
        // 2026-05-19T00:00:00.000Z — known epoch millis.
        let ms = parse_iso_millis("2026-05-19T00:00:00.000Z").unwrap();
        assert_eq!(ms, 1_779_148_800_000);
    }

    #[test]
    fn parse_iso_millis_without_fraction() {
        assert!(parse_iso_millis("2026-05-19T12:30:45Z").is_some());
        assert!(parse_iso_millis("garbage").is_none());
    }

    /// mark_followup emits pipeline.complete + pipeline.status events and does NOT
    /// write a .pipeline-states JSON file.
    #[test]
    fn mark_followup_emits_complete_event_no_json_write() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        // Pre-create the harness dir so for_project works.
        std::fs::create_dir_all(cwd.join(".claude").join(".harness")).unwrap();

        let result = mark_followup(cwd, "demo-spec");
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["mode"], json!("followup"));
        // No legacy state JSON should exist.
        assert!(!pipeline_state_path(cwd, "demo-spec").exists(), "no JSON sidecar expected");

        // Event store should have both events.
        let store = temp_store(cwd);
        let events = store.query(Some("demo-spec")).unwrap();
        let kinds: Vec<&str> = events.iter().map(|e| e.event.as_str()).collect();
        assert!(kinds.contains(&EVENT_PIPELINE_STATUS), "pipeline.status missing");
        assert!(kinds.contains(&EVENT_PIPELINE_COMPLETE), "pipeline.complete missing");

        // The pipeline.complete event should carry closedAt and affectedFiles.
        let complete_ev = events.iter().find(|e| e.event == EVENT_PIPELINE_COMPLETE).unwrap();
        let payload: PipelineCompletePayload =
            serde_json::from_value(complete_ev.payload.clone()).unwrap();
        assert!(payload.closed_at.is_some(), "closedAt should be set");
    }

    #[test]
    fn archive_moves_active_spec_to_completed() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let active = active_spec_dir(cwd, "s1");
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(active.join("spec.md"), "# spec").unwrap();
        let (moved, _) = archive(cwd, "s1");
        assert!(moved);
        assert!(completed_spec_dir(cwd, "s1").join("spec.md").exists());
        assert!(!active.exists());
    }

    /// archive_followups reads the event store (not the FS) to find closed-followup specs.
    #[test]
    fn archive_followups_uses_event_projection() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        std::fs::create_dir_all(cwd.join(".claude").join(".harness")).unwrap();
        let store = temp_store(cwd);

        // Spec "a" has status closed-followup.
        store.append(&ev(
            EVENT_PIPELINE_STATUS, "a",
            json!({ "to": "closed-followup" }),
        )).unwrap();
        store.append(&ev(
            EVENT_PIPELINE_COMPLETE, "a",
            json!({ "closedAt": "2026-05-20T10:00:00.000Z", "affectedFiles": [] }),
        )).unwrap();

        // Spec "b" has status active — should be skipped.
        store.append(&ev(
            EVENT_PIPELINE_STATUS, "b",
            json!({ "to": "active" }),
        )).unwrap();

        // Create active spec dirs so archive() has something to move.
        let active_a = active_spec_dir(cwd, "a");
        std::fs::create_dir_all(&active_a).unwrap();
        std::fs::write(active_a.join("spec.md"), "# a").unwrap();

        let (scanned, archived) = archive_followups(cwd, false);
        assert_eq!(scanned, 1, "only 1 spec should be in closed-followup state");
        assert_eq!(archived, 1, "spec a should be archived");
        assert!(completed_spec_dir(cwd, "a").exists(), "spec a moved to completed");
    }

    /// Legacy FS fallback: when the event store is empty, fall back to .pipeline-states/*.json.
    #[test]
    fn archive_followups_legacy_fs_fallback_sweeps_closed_followup() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let states = cwd.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        // Write legacy state files.
        write_json(&states.join("a.json"), &json!({ "specName": "a", "status": "closed-followup" }));
        write_json(&states.join("b.json"), &json!({ "specName": "b", "status": "active" }));
        // Call the legacy helper directly.
        let (scanned, archived) = archive_followups_legacy_fs(cwd, false);
        assert_eq!(scanned, 2);
        assert_eq!(archived, 1);
    }
}
