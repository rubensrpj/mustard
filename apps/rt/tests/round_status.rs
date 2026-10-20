#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "support/binary.rs"]
mod binary;

use serde_json::{Value, json};
#[test]
fn bare_round_reads_state_without_rewriting_requests_or_advancing_execution() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("mustard.json"), "{}").unwrap();
    let folder = root.join(".claude/spec/state-only");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("spec.ndjson");
    let original =
        format!("{}\n", json!({"v":1,"id":1,"type":"state","phase":"running","branch":"feature/state-only","base":"dev","author":"binary"}));
    std::fs::write(&path, &original).unwrap();
    for _ in 0..2 {
        let output = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "round", "--spec", "state-only"])
            .current_dir(root)
            .env("MUSTARD_RT_DELEGATED", "1")
            .output()
            .unwrap();
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{report}");
        assert_eq!(report["mode"], "status");
        assert_eq!(report["mutated"], false);
        assert!(report["command"].as_str().unwrap().contains("round --dispatch"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(!root.join(".claude/mustard").exists());
    }
}
