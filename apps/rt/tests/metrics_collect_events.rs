// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! `run metrics collect` reports the specs the event log actually holds.
//!
//! The pipeline half of the command used to read `.claude/.pipeline-states/`,
//! a directory the harness stopped writing. The read failed, an empty list came
//! back, and `tracked: 0` was published on a repository whose event log carried
//! 21 specs — a verification that reports zero when it means "I could not look".
//!
//! Two halves, matching the two failure modes:
//!
//! 1. a seeded event log is counted;
//! 2. an unreadable source publishes the explicit unknown marker, never zero.
//!
//! Lives in `tests/` rather than in-file because the acceptance criterion runs
//! `cargo test -p mustard-rt metrics_collect_reports_specs_from_events --
//! --exact`, and libtest matches `--exact` against the FULL test path — which
//! equals the bare function name only at the root of an integration-test binary.

use mustard_rt::commands::economy::metrics::build_collect;
use serde_json::{json, Value};
use std::path::Path;

/// Append one NDJSON event under `<root>/.claude/spec/<spec>/.events/seed.ndjson`
/// — the production shape `event_writer_ndjson.rs` writes and the canonical
/// walker reads.
fn seed_event(root: &Path, spec: &str, event_name: &str, ts: &str, payload: Value) {
    let events_dir = root
        .join(".claude")
        .join("spec")
        .join(spec)
        .join(".events");
    std::fs::create_dir_all(&events_dir).unwrap();
    let line = json!({
        "event": event_name,
        "kind": "pipeline",
        "ts": ts,
        "v": 1,
        "spec": spec,
        "session_id": "seed",
        "wave": 0,
        "actor": "test",
        "payload": payload,
    });
    let path = events_dir.join("seed.ndjson");
    let mut body = std::fs::read_to_string(&path).unwrap_or_default();
    body.push_str(&line.to_string());
    body.push('\n');
    std::fs::write(&path, body).unwrap();
}

#[test]
fn metrics_collect_reports_specs_from_events() {
    // --- 1. A seeded event log is counted ---------------------------------
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    seed_event(root, "alpha", "pipeline.phase", "2026-05-20T00:00:00.000Z", json!({ "to": "EXECUTE" }));
    seed_event(root, "alpha", "tool.use", "2026-05-20T00:00:01.000Z", json!({ "tool": "Edit" }));
    seed_event(root, "beta", "pipeline.phase", "2026-05-20T00:00:02.000Z", json!({ "to": "PLAN" }));

    let doc = build_collect(root, false);
    let pipelines = &doc["pipelines"];

    assert_eq!(pipelines["source"], json!("events"), "{doc:#}");
    assert_eq!(
        pipelines["tracked"],
        json!(2),
        "both seeded specs must be tracked: {doc:#}"
    );

    let names: Vec<&str> = pipelines["specs"]
        .as_array()
        .expect("specs must be an array")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert!(names.contains(&"alpha"), "alpha missing from {names:?}");
    assert!(names.contains(&"beta"), "beta missing from {names:?}");

    // The per-spec metrics come from the pipeline-state projection, so the
    // non-Read tool call on `alpha` is visible here too — proof the numbers are
    // folded from the log rather than copied off a state file.
    let alpha = pipelines["specs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == json!("alpha"))
        .expect("alpha row");
    assert_eq!(alpha["metrics"]["apiCalls"], json!(1), "{alpha:#}");

    // --- 2. An unreadable source yields the marker, not zero --------------
    // A tempdir with no `.claude/spec/` at all: there is nothing to read, which
    // is a different answer from "read it, found nothing".
    let blind = tempfile::tempdir().unwrap();
    let doc = build_collect(blind.path(), false);
    let pipelines = &doc["pipelines"];

    assert_eq!(pipelines["source"], json!("unreadable"), "{doc:#}");
    assert_eq!(
        pipelines["tracked"],
        json!("unknown"),
        "an unreadable source must publish the unknown marker: {doc:#}"
    );
    assert_ne!(
        pipelines["tracked"],
        json!(0),
        "zero would read as a measurement: {doc:#}"
    );
    for counter in ["active", "orphaned", "pass1", "pass1Pct"] {
        assert_eq!(
            pipelines[counter],
            json!("unknown"),
            "counter `{counter}` must carry the marker: {doc:#}"
        );
    }
}
