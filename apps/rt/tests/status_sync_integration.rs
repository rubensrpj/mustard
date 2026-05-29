//! W5 — integration test: simulates a full pipeline wave-plan and verifies
//! that `meta.json` (the single source of truth) stays aligned after every
//! `sync_status` call, while `spec.md` is left as pure narrative (no lifecycle
//! header is ever injected or rewritten).

use mustard_core::{Flags, Outcome, SpecState, Stage};
use mustard_rt::commands::spec::spec_scaffold::sync_status;

/// Build a validated `SpecState` for the integration fixture.
fn st(stage: Stage, outcome: Outcome) -> SpecState {
    SpecState::new(stage, outcome, Flags::default()).expect("legal state")
}

/// Helper: write a minimal, header-less `spec.md` (pure narrative).
fn seed_spec_md(path: &std::path::Path) {
    std::fs::write(path, "# Test Spec\n\n## Body\ncontent\n").unwrap();
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

/// `spec.md` must carry no lifecycle header — `meta.json` owns lifecycle state.
fn spec_md_has_no_header(spec_md: &std::path::Path) -> bool {
    let content = std::fs::read_to_string(spec_md).unwrap();
    mustard_core::header_field(&content, "Stage").is_none()
        && mustard_core::header_field(&content, "Outcome").is_none()
}

#[test]
fn status_sync_full_pipeline_aligns_meta_and_leaves_spec_md_narrative() {
    let root = tempfile::tempdir().unwrap();

    // Build a fixture: parent spec + 5 waves.
    let spec_dir = root.path().join(".claude").join("spec").join("test-pipeline");
    std::fs::create_dir_all(&spec_dir).unwrap();

    // Seed parent spec.md as pure narrative + parent meta.json in Plan/Active.
    seed_spec_md(&spec_dir.join("spec.md"));
    std::fs::write(
        spec_dir.join("meta.json"),
        r#"{"stage":"Plan","outcome":"Active","isWavePlan":true,"totalWaves":5}"#,
    )
    .unwrap();

    // Seed 5 wave subdirs.
    for n in 1..=5usize {
        let wave_dir = spec_dir.join(format!("wave-{n}-mixed"));
        std::fs::create_dir_all(&wave_dir).unwrap();
        seed_spec_md(&wave_dir.join("spec.md"));
        std::fs::write(
            wave_dir.join("meta.json"),
            r#"{"stage":"Plan","outcome":"Active"}"#,
        )
        .unwrap();
    }

    // Close each wave.
    for n in 1..=5usize {
        let wave_dir = spec_dir.join(format!("wave-{n}-mixed"));
        sync_status(st(Stage::Close, Outcome::Completed), &wave_dir).unwrap();

        // Each wave's meta.json is immediately aligned; spec.md stays narrative.
        assert_eq!(read_meta_stage(&wave_dir), "Close");
        assert_eq!(read_meta_outcome(&wave_dir), "Completed");
        assert!(spec_md_has_no_header(&wave_dir.join("spec.md")));
    }

    // Close the parent spec.
    sync_status(st(Stage::Close, Outcome::Completed), &spec_dir).unwrap();

    // Parent meta.json fields aligned; spec.md untouched.
    assert_eq!(read_meta_stage(&spec_dir), "Close");
    assert_eq!(read_meta_outcome(&spec_dir), "Completed");
    assert!(spec_md_has_no_header(&spec_dir.join("spec.md")));

    // Waves are still aligned (sync_status on parent must not touch waves).
    for n in 1..=5usize {
        let wave_dir = spec_dir.join(format!("wave-{n}-mixed"));
        assert_eq!(read_meta_stage(&wave_dir), "Close");
        assert_eq!(read_meta_outcome(&wave_dir), "Completed");
    }
}
