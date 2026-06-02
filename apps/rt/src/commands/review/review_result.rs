//! `mustard-rt run review-result` — a port of `scripts/review-result.js`.
//!
//! Records the outcome of a pipeline REVIEW phase. REVIEW audits a pipeline
//! before CLOSE and yields `approved` / `rejected`; this writes a
//! `review.result` harness event and a `review` metric so `/stats` can show
//! whether pipelines were reviewed.
//!
//! Port note: the JS version shelled to `_lib/harness-event.js` and
//! `_lib/metrics-emit.js`; this port emits both directly via `mustard_core`.
//!
//! No `--format html`: `review-result` only records and echoes the verdict.

use crate::shared::context::{project_dir, session_id};
use mustard_core::io::fs;
use mustard_core::time::now_iso8601;
use mustard_core::platform::metrics::{emit_metric, MetricLine};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde_json::json;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Record a REVIEW outcome: emit the event + metric, return the payload JSON.
fn record_review(
    cwd: &Path,
    spec: &str,
    verdict: &str,
    critical_count: i64,
    subproject: Option<&str>,
) -> serde_json::Value {
    let payload = json!({
        "spec": spec,
        "verdict": verdict,
        "criticalCount": critical_count,
        "subproject": subproject,
    });

    // Harness event.
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("review-result".to_string()),
            actor_type: None,
        },
        event: "review.result".to_string(),
        payload: payload.clone(),
        spec: Some(spec.to_string()),
    };
    // `review.result` is non-pipeline → per-spec NDJSON via the W5 router.
    let _ = crate::shared::events::route::emit(cwd.to_string_lossy().as_ref(), &ev);

    // Metric (fail-silent).
    let line = MetricLine::new(now_iso8601(), "review").note(verdict).extras(json!({
        "spec": spec,
        "verdict": verdict,
        "criticalCount": critical_count,
        "category": "verification",
    }));
    let _ = emit_metric(cwd, &line);

    // D4: materialise the human-readable verdict beside the phase dir.
    write_review_verdict_md(cwd, spec, verdict, critical_count, subproject);

    payload
}

/// Write the materialised verdict at `.claude/spec/{spec}/review/verdict.md`
/// (D4). The review phase records its verdict by code so the result is durable
/// and visible in the dashboard, instead of depending on an agent filling in a
/// template. Atomic via [`fs::write_atomic`]. Fail-open: a missing project root
/// or write error is a silent no-op (the `review.result` event is the
/// load-bearing record; this file is the human-readable mirror).
fn write_review_verdict_md(
    cwd: &Path,
    spec: &str,
    verdict: &str,
    critical_count: i64,
    subproject: Option<&str>,
) {
    let Some(sp) = ClaudePaths::for_project(cwd)
        .ok()
        .and_then(|p| p.for_spec(spec).ok())
    else {
        return;
    };
    let review_dir = sp.dir().join("review");
    if fs::create_dir_all(&review_dir).is_err() {
        return;
    }

    let mut body = String::new();
    body.push_str("# Review Verdict\n\n");
    let _ = writeln!(body, "- Spec: `{spec}`");
    let _ = writeln!(body, "- Verdict: **{}**", verdict.to_uppercase());
    let _ = writeln!(body, "- Critical findings: {critical_count}");
    if let Some(sub) = subproject {
        let _ = writeln!(body, "- Subproject: `{sub}`");
    }
    body.push('\n');

    let _ = fs::write_atomic(review_dir.join("verdict.md"), body.as_bytes());
}

/// Dispatch `mustard-rt run review-result`.
pub fn run(spec: Option<&str>, verdict: Option<&str>, critical: i64, subproject: Option<&str>) {
    let (Some(spec), Some(verdict)) = (spec, verdict) else {
        eprintln!(
            "Usage: review-result --spec <name> --verdict approved|rejected [--critical <N>] [--subproject <name>]"
        );
        return;
    };
    if verdict != "approved" && verdict != "rejected" {
        eprintln!("[review-result] Invalid --verdict \"{verdict}\" — expected approved|rejected");
        return;
    }

    let cwd = std::env::current_dir()
        .ok()
        .or_else(|| Some(PathBuf::from(project_dir())))
        .unwrap_or_else(|| PathBuf::from("."));

    let payload = record_review(&cwd, spec, verdict, critical, subproject);
    let out = json!({ "event": "review.result", "payload": payload });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn record_emits_event_and_metric() {
        let dir = tempdir().unwrap();
        let payload = record_review(dir.path(), "demo", "approved", 0, Some("api"));
        assert_eq!(payload["verdict"], json!("approved"));
        assert_eq!(payload["subproject"], json!("api"));

        // W5: `review.result` is non-pipeline → per-spec NDJSON under
        // `<project>/.claude/spec/demo/.events/`.
        let events_dir = dir
            .path()
            .join(".claude")
            .join("spec")
            .join("demo")
            .join(".events");
        assert!(events_dir.exists(), ".events dir must exist");
        let mut found = false;
        for f in std::fs::read_dir(&events_dir).unwrap() {
            let body = std::fs::read_to_string(f.unwrap().path()).unwrap_or_default();
            if body.lines().any(|l| {
                serde_json::from_str::<serde_json::Value>(l)
                    .ok()
                    .and_then(|v| v["event"].as_str().map(str::to_string))
                    .as_deref()
                    == Some("review.result")
            }) {
                found = true;
            }
        }
        assert!(found, "review.result NDJSON line must be present");

        let metric = dir.path().join(".claude").join(".metrics").join("review.jsonl");
        assert!(metric.exists());
    }

    /// D4: `record_review` materialises `.claude/spec/{spec}/review/verdict.md`
    /// alongside the `review.result` event.
    #[test]
    fn review_verdict_md_is_materialized() {
        let dir = tempdir().unwrap();
        record_review(dir.path(), "demo", "approved", 2, Some("api"));

        let verdict_path = ClaudePaths::for_project(dir.path())
            .unwrap()
            .for_spec("demo")
            .unwrap()
            .dir()
            .join("review")
            .join("verdict.md");
        let md = std::fs::read_to_string(&verdict_path).unwrap();
        assert!(md.starts_with("# Review Verdict"));
        assert!(md.contains("Verdict: **APPROVED**"));
        assert!(md.contains("Critical findings: 2"));
        assert!(md.contains("Subproject: `api`"));
    }
}
