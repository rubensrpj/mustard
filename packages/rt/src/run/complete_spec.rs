//! `mustard-rt run complete-spec` — a port of `scripts/complete-spec.js`.
//!
//! Finalizes a pipeline spec in two stages:
//!
//! - **followup** (default): snapshot `affectedFiles` from harness events and
//!   `git diff`, then mark the pipeline-state `closed-followup` while leaving
//!   the spec under `spec/active/` so follow-up edits can still be linked.
//! - **archive** (`--archive`): move `spec/active/<name>` → `spec/completed/`,
//!   write archived metrics to `.claude/metrics/<name>.json`, and delete the
//!   pipeline-state file.
//!
//! `--archive-stale` / `--archive-followups` sweep every `closed-followup`
//! state (the former only those older than 24 h).
//!
//! All I/O is fail-soft. The stdout JSON line stays shape-compatible with the
//! JS version (the `/close` command parses it).
//!
//! Port note: the JS script derived metrics via `event-projections.js`
//! (`buildPipelineState`). That projection is not yet ported (a later b4 wave),
//! so this port archives metrics from the legacy `state.metrics` sidecar only
//! — pipelines that ran before the events migration. Post-migration pipelines
//! simply get no metrics sidecar, which is harmless (CLOSE never depends on it).

use mustard_core::io::event_store::JsonlEventStore;
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
    let events = JsonlEventStore::for_project(cwd).replay().unwrap_or_default();
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

/// Stage 1 — mark the spec `closed-followup` with its affected-file snapshot.
fn mark_followup(cwd: &Path, spec: &str) -> Value {
    let state_path = pipeline_state_path(cwd, spec);
    let mut state = read_json(&state_path).unwrap_or_else(|| json!({ "specName": spec }));
    let affected = collect_affected_files(cwd, spec, &state);
    let now = crate::util::now_iso8601();
    if let Some(obj) = state.as_object_mut() {
        obj.insert("status".to_string(), json!("closed-followup"));
        obj.insert("closedAt".to_string(), json!(now));
        obj.insert("affectedFiles".to_string(), json!(affected));
        obj.entry("specName").or_insert(json!(spec));
    }
    let ok = write_json(&state_path, &state);
    json!({
        "ok": ok,
        "mode": "followup",
        "spec": spec,
        "affectedFiles": affected.len(),
        "statePath": state_path.to_string_lossy(),
    })
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

/// Stage 2 — archive a spec: move the spec dir, archive metrics, drop state.
fn archive(cwd: &Path, spec: &str) -> (bool, bool) {
    let active = active_spec_dir(cwd, spec);
    let completed = completed_spec_dir(cwd, spec);
    let state_path = pipeline_state_path(cwd, spec);
    let state = read_json(&state_path);

    let moved_spec = if active.exists() && !completed.exists() {
        move_dir(&active, &completed)
    } else {
        // Already moved (completed exists, active gone) counts as success.
        !active.exists() && completed.exists()
    };

    archive_metrics_from_state(cwd, spec, state.as_ref().unwrap_or(&Value::Null));
    if state_path.exists() {
        let _ = std::fs::remove_file(&state_path);
    }
    (moved_spec, state.is_some())
}

/// Sweep every `closed-followup` state, archiving it (TTL-gated when asked).
fn archive_followups(cwd: &Path, require_ttl: bool) -> (usize, usize) {
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

    if archive_flag {
        let (moved_spec, had_state) = archive(&cwd, spec);
        println!(
            "{}",
            json!({ "ok": true, "mode": "archive", "spec": spec, "movedSpec": moved_spec, "hadState": had_state })
        );
        return;
    }

    println!("{}", mark_followup(&cwd, spec));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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

    #[test]
    fn mark_followup_writes_closed_followup_state() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let result = mark_followup(cwd, "demo-spec");
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["mode"], json!("followup"));
        let state = read_json(&pipeline_state_path(cwd, "demo-spec")).unwrap();
        assert_eq!(state["status"], json!("closed-followup"));
        assert_eq!(state["specName"], json!("demo-spec"));
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

    #[test]
    fn archive_followups_only_sweeps_closed_followup() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();
        let states = cwd.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        write_json(&states.join("a.json"), &json!({ "specName": "a", "status": "closed-followup" }));
        write_json(&states.join("b.json"), &json!({ "specName": "b", "status": "active" }));
        let (scanned, archived) = archive_followups(cwd, false);
        assert_eq!(scanned, 2);
        assert_eq!(archived, 1);
    }
}
