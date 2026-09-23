//! End-to-end contract: the `scan facts` CLI loads a model and emits the
//! orchestrator FACTS (`projects` + `entities`) as JSON — the exact shape
//! `mustard-core`'s `ModelFacts` deserializes. Guards the scan↔mustard boundary.

use std::process::Command;

#[test]
fn facts_cli_emits_projects_and_entities() {
    // A minimal (partial) model.json — `scan facts` tolerates it because the
    // model structs default missing fields. Avoids depending on grammar
    // extraction so the test is fully deterministic.
    let temp = tempfile::Builder::new().prefix("scan-facts-it-").tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let model = dir.join("grain.model.json");
    std::fs::write(
        &model,
        r#"{"modules":[{"declarations":[{"name":"Invoice"},{"name":"User"},{"name":"User"}]}],
            "projects":[{"name":"demo","dir":"apps/demo","kind":"node","code_files":3}]}"#,
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_scan"))
        .args(["facts", model.to_str().unwrap()])
        .output()
        .expect("run scan facts");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON on stdout");
    let entities: Vec<String> =
        v["entities"].as_array().unwrap().iter().map(|e| e.as_str().unwrap().to_string()).collect();
    // Sorted + deduped: ["Invoice","User"].
    assert_eq!(entities, vec!["Invoice", "User"], "entities: {entities:?}");
    assert!(v["projects"].as_array().unwrap().iter().any(|p| p["name"] == "demo"));

}
