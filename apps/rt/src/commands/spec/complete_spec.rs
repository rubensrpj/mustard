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

/// Read a JSON file, returning `None` on any error.
fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
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

/// Collect the files a spec touched: harness `target.file` payloads, the git
/// diff against the parent branch.
fn collect_affected_files(cwd: &Path, spec: &str) -> Vec<String> {
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
    let now = crate::util::now_iso8601();

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

/// Parse an ISO-8601 timestamp into epoch millis (UTC). `None` on any failure.
pub(crate) fn parse_iso_millis(ts: &str) -> Option<i64> {
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
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + hh * 3600 + mm * 60 + ss) * 1000) + millis)
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
    emit_ndjson(cwd, spec, EVENT_PIPELINE_STATUS, &payload, &crate::util::now_iso8601());
}

/// Idempotently emit `pipeline.phase: CLOSE` when the spec's latest phase is
/// not already CLOSE.
fn emit_phase_close(cwd: &Path, spec: &str) {
    let last = crate::commands::event::emit_phase::last_phase_for_spec(cwd, spec);
    if last.as_deref() == Some("CLOSE") {
        return;
    }
    let ts = crate::util::now_iso8601();
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
    rebuild_one_fail_open(&cwd, spec);
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

    #[test]
    fn parse_iso_millis_round_trips() {
        let ms = parse_iso_millis("2026-05-19T00:00:00.000Z").unwrap();
        assert_eq!(ms, 1_779_148_800_000);
    }

    #[test]
    fn parse_iso_millis_without_fraction() {
        assert!(parse_iso_millis("2026-05-19T12:30:45Z").is_some());
        assert!(parse_iso_millis("garbage").is_none());
    }
}
