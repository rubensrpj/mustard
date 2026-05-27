//! W5 — integration test: simulates a full pipeline wave-plan and verifies
//! that spec.md + meta.json stay aligned after every sync_status call.

use mustard_core::{Outcome, Stage};
use mustard_rt::run::spec_scaffold::sync_status;

/// Helper: write a minimal spec.md with canonical headers.
fn seed_spec_md(path: &std::path::Path, stage: &str, outcome: &str) {
    std::fs::write(
        path,
        format!(
            "# Test Spec\n\n### Stage: {stage}\n### Outcome: {outcome}\n### Flags: \n\n## Body\ncontent\n"
        ),
    )
    .unwrap();
}

/// Helper: read `stage` from meta.json at `dir/meta.json`.
fn read_meta_stage(dir: &std::path::Path) -> String {
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("meta.json")).unwrap(),
    )
    .unwrap();
    v["stage"].as_str().unwrap_or("").to_string()
}

fn read_meta_outcome(dir: &std::path::Path) -> String {
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("meta.json")).unwrap(),
    )
    .unwrap();
    v["outcome"].as_str().unwrap_or("").to_string()
}

fn read_spec_stage(spec_md: &std::path::Path) -> String {
    let content = std::fs::read_to_string(spec_md).unwrap();
    mustard_core::header_field(&content, "Stage").unwrap_or_default()
}

fn read_spec_outcome(spec_md: &std::path::Path) -> String {
    let content = std::fs::read_to_string(spec_md).unwrap();
    mustard_core::header_field(&content, "Outcome").unwrap_or_default()
}

#[test]
fn status_sync_full_pipeline_aligns_spec_and_meta() {
    let root = tempfile::tempdir().unwrap();

    // Build a fixture: parent spec + 5 waves.
    let spec_dir = root.path().join(".claude").join("spec").join("test-pipeline");
    std::fs::create_dir_all(&spec_dir).unwrap();

    // Seed parent spec.md in Plan/Active.
    seed_spec_md(&spec_dir.join("spec.md"), "Plan", "Active");
    // Seed parent meta.json.
    std::fs::write(
        spec_dir.join("meta.json"),
        r#"{"stage":"Plan","outcome":"Active","isWavePlan":true,"totalWaves":5}"#,
    )
    .unwrap();

    // Seed 5 wave subdirs.
    for n in 1..=5usize {
        let wave_dir = spec_dir.join(format!("wave-{n}-mixed"));
        std::fs::create_dir_all(&wave_dir).unwrap();
        seed_spec_md(&wave_dir.join("spec.md"), "Plan", "Active");
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Plan","outcome":"Active"}"#,
        )
        .unwrap();
    }

    // Close each wave.
    for n in 1..=5usize {
        let wave_dir = spec_dir.join(format!("wave-{n}-mixed"));
        sync_status(Stage::Close, Outcome::Completed, &wave_dir).unwrap();

        // Each wave is immediately aligned.
        assert_eq!(read_spec_stage(&wave_dir.join("spec.md")), "Close");
        assert_eq!(read_spec_outcome(&wave_dir.join("spec.md")), "Completed");
        assert_eq!(read_meta_stage(&wave_dir), "Close");
        assert_eq!(read_meta_outcome(&wave_dir), "Completed");
    }

    // Close the parent spec.
    sync_status(Stage::Close, Outcome::Completed, &spec_dir).unwrap();

    // Parent spec.md headers.
    assert_eq!(read_spec_stage(&spec_dir.join("spec.md")), "Close");
    assert_eq!(read_spec_outcome(&spec_dir.join("spec.md")), "Completed");

    // Parent meta.json fields.
    assert_eq!(read_meta_stage(&spec_dir), "Close");
    assert_eq!(read_meta_outcome(&spec_dir), "Completed");

    // Waves are still aligned (sync_status on parent must not touch waves).
    for n in 1..=5usize {
        let wave_dir = spec_dir.join(format!("wave-{n}-mixed"));
        assert_eq!(read_meta_stage(&wave_dir), "Close");
        assert_eq!(read_meta_outcome(&wave_dir), "Completed");
    }
}
