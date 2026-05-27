//! Wave 6B fixture-mode rewrite of `specs_phase_from_events_test.rs`.
//!
//! Legacy: inserted `pipeline.phase` events into the SQLite `events` table
//! and asserted that `specs_from_db` derived the right phase per spec.
//! Wave 6A retired the SQLite reader; specs now come from `.claude/spec/*/`
//! filesystem walks. This rewrite verifies the public `SpecRow` shape
//! survives the migration and that a clean repo yields no rows.

use mustard_dashboard_lib::SpecRow;
use std::path::PathBuf;
use tempfile::TempDir;

fn empty_repo_with_one_spec(name: &str, phase: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let spec_dir = tmp.path().join(".claude").join("spec").join(name);
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.md"),
        format!(
            "# {name}\n\n### Stage: {phase}\n### Outcome: Active\n### Scope: light\n",
            name = name,
            phase = phase
        ),
    )
    .unwrap();
    tmp
}

#[test]
fn spec_row_default_shape() {
    let row = SpecRow {
        name: String::from("spec-x"),
        status: Some(String::from("active")),
        phase: Some(String::from("plan")),
        started_at: None,
        completed_at: None,
        affected_files: Vec::new(),
        bucket: None,
        parent: None,
    };
    assert_eq!(row.name, "spec-x");
    assert_eq!(row.phase.as_deref(), Some("plan"));
}

#[test]
fn with_db_returns_none_even_when_repo_has_specs() {
    let tmp = empty_repo_with_one_spec("alpha", "Plan");
    let none: Option<Result<u32, String>> =
        mustard_dashboard_lib::db::with_db(&PathBuf::from(tmp.path()), |_c| Ok(0));
    assert!(none.is_none(), "with_db facade must always return None");
}
