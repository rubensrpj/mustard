// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! Integration test: `spec-children-tree` projects a parent spec's waves,
//! acceptance criteria and sub-specs into a single JSON document.
//!
//! Approach: seed a tempdir event store with two waves (dispatch + one
//! completion), one `qa.result` carrying 3 ACs (pass / fail / skip→pending),
//! plus a sub-spec discoverable via its on-disk `### Parent:` header. Then
//! invoke `mustard-rt run spec-children-tree --spec <parent>` as a subprocess
//! and assert the JSON shape (Wave 2 of `spec-lifecycle-unification`).

use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, SCHEMA_VERSION, EVENT_PIPELINE_TASK_DISPATCH,
    EVENT_PIPELINE_WAVE_COMPLETE,
};
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use serde_json::{json, Value};
use std::path::Path;
use tempfile::TempDir;

/// Temp project dir with the standard harness layout.
fn project_dir() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join(".claude").join(".harness")).expect("harness dir");
    dir
}

/// Seed a HarnessEvent through the store's append path. With W5 this only
/// works for `pipeline.*` events — `qa.result` and other non-pipeline kinds
/// must be seeded via the binary subprocess so they land in NDJSON. Use the
/// `subprocess_seed` helper below for those.
fn seed(store: &SqliteEventStore, spec: &str, ts: &str, kind: &str, payload: Value) {
    store
        .append(&HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.into(),
            session_id: "s-tree".into(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("test".into()),
                actor_type: None,
            },
            event: kind.into(),
            payload,
            spec: Some(spec.into()),
        })
        .expect("append");
}

/// Seed a non-pipeline event by writing a one-line NDJSON file directly under
/// `<project>/.claude/spec/<spec>/.events/`. Matches the W5 writer shape so the
/// timeline reader picks it up.
fn seed_ndjson(project: &Path, spec: &str, ts: &str, kind: &str, payload: Value) {
    let dir = project.join(".claude").join("spec").join(spec).join(".events");
    std::fs::create_dir_all(&dir).expect("events dir");
    let path = dir.join("seed.ndjson");
    let line = serde_json::to_string(&json!({
        "ts": ts,
        "ts_ms": 0,
        "event": kind,
        "kind": "qa",
        "spec": spec,
        "session_id": "s-tree",
        "actor": "test",
        "payload": payload,
    }))
    .unwrap();
    let body = format!("{line}\n");
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .expect("open ndjson file");
    f.write_all(body.as_bytes()).expect("write ndjson line");
}

/// Write a sub-spec under `.claude/spec/<slug>/spec.md` with a `### Parent:`
/// header pointing at `parent` (header-only link — no `spec.link` event).
fn write_subspec(project: &Path, slug: &str, parent: &str, status: &str) {
    let dir = project.join(".claude").join("spec").join(slug);
    std::fs::create_dir_all(&dir).expect("subspec dir");
    let body = format!("# {slug}\n\n### Parent: [[{parent}]]\n### Status: {status}\n\n## Resumo\nx\n");
    std::fs::write(dir.join("spec.md"), body).expect("write subspec");
}

fn run_tree(project: &Path, spec: &str) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    std::process::Command::new(bin)
        .args(["run", "spec-children-tree", "--spec", spec])
        .current_dir(project)
        .env("CLAUDE_PROJECT_DIR", project.to_string_lossy().as_ref())
        .output()
        .expect("run mustard-rt")
}

#[test]
fn spec_children_tree_emits_waves_acs_and_subspecs() {
    let tmp = project_dir();
    let project = tmp.path();
    let parent = "2026-05-21-demo-parent";

    let store = SqliteEventStore::for_project(project).expect("open store");

    // --- Two waves: wave 1 completed, wave 2 dispatched (in-progress). ------
    seed(
        &store,
        parent,
        "2026-05-21T10:00:00.000Z",
        EVENT_PIPELINE_TASK_DISPATCH,
        json!({ "wave": 1, "name": "wave-1-rt", "role": "rt" }),
    );
    seed(
        &store,
        parent,
        "2026-05-21T10:05:00.000Z",
        EVENT_PIPELINE_WAVE_COMPLETE,
        json!({ "wave": 1 }),
    );
    seed(
        &store,
        parent,
        "2026-05-21T10:10:00.000Z",
        EVENT_PIPELINE_TASK_DISPATCH,
        json!({ "wave": 2, "name": "wave-2-ui", "role": "ui" }),
    );

    // --- One qa.result with 3 ACs: pass / fail / pending. -------------------
    // W5: qa.result is non-pipeline → per-spec NDJSON. Seed it directly.
    seed_ndjson(
        project,
        parent,
        "2026-05-21T10:20:00.000Z",
        "qa.result",
        json!({
            "spec": parent,
            "overall": "fail",
            "criteria": [
                { "id": "AC-1", "status": "pass", "exit": 0, "stderr_excerpt": "" },
                { "id": "AC-2", "status": "fail", "exit": 101, "stderr_excerpt": "exit 101" },
                { "id": "AC-3", "status": "pending", "exit": null, "stderr_excerpt": "" }
            ]
        }),
    );

    // --- One sub-spec discoverable via filesystem header only. --------------
    write_subspec(project, "2026-05-21-demo-tactical", parent, "approved");

    let out = run_tree(project, parent);
    assert!(
        out.status.success(),
        "spec-children-tree must exit 0. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let tree: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON ({e}):\n{stdout}"));

    // spec field round-trips.
    assert_eq!(tree["spec"], json!(parent));

    // --- Waves -------------------------------------------------------------
    let waves = tree["waves"].as_array().expect("waves array");
    assert_eq!(waves.len(), 2, "two waves: {waves:?}");
    assert_eq!(waves[0]["idx"], json!(1));
    assert_eq!(waves[0]["role"], json!("rt"));
    assert_eq!(waves[0]["status"], json!("completed"));
    assert_eq!(waves[1]["idx"], json!(2));
    assert_eq!(waves[1]["status"], json!("in-progress"));
    assert_eq!(waves[1]["completed_at"], Value::Null);

    // --- ACs ---------------------------------------------------------------
    let acs = tree["acs"].as_array().expect("acs array");
    assert_eq!(acs.len(), 3, "three ACs: {acs:?}");
    let by_id: std::collections::HashMap<&str, &Value> = acs
        .iter()
        .map(|a| (a["id"].as_str().unwrap_or(""), a))
        .collect();
    assert_eq!(by_id["AC-1"]["status"], json!("pass"));
    assert_eq!(by_id["AC-2"]["status"], json!("fail"));
    assert_eq!(by_id["AC-3"]["status"], json!("pending"));
    // The failing AC carries evidence (the captured stderr excerpt).
    assert!(
        by_id["AC-2"]["evidence"].as_str().is_some_and(|s| s.contains("101")),
        "fail AC should carry evidence: {:?}",
        by_id["AC-2"]
    );

    // --- Sub-specs ---------------------------------------------------------
    let subspecs = tree["subspecs"].as_array().expect("subspecs array");
    assert_eq!(subspecs.len(), 1, "one sub-spec: {subspecs:?}");
    assert_eq!(subspecs[0]["spec"], json!("2026-05-21-demo-tactical"));
    // `state.stage` is the canonical kebab Stage ("approved" → Plan).
    assert_eq!(subspecs[0]["state"]["stage"], json!("plan"));
}
