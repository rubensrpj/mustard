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

use crate::run::env::{project_dir, session_id};
use crate::util::now_iso8601;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::metrics::{emit_metric, MetricLine};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;
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
    let _ = SqliteEventStore::for_project(cwd).and_then(|store| store.append(&ev));

    // Metric (fail-silent).
    let line = MetricLine::new(now_iso8601(), "review").note(verdict).extras(json!({
        "spec": spec,
        "verdict": verdict,
        "criticalCount": critical_count,
        "category": "verification",
    }));
    let _ = emit_metric(cwd, &line);

    payload
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

        let store = SqliteEventStore::for_project(dir.path()).unwrap();
        let events = store.replay().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "review.result");

        let metric = dir.path().join(".claude").join(".metrics").join("review.jsonl");
        assert!(metric.exists());
    }
}
