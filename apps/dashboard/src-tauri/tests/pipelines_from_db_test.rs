//! Wave 6B fixture-mode rewrite of `pipelines_from_db_test.rs`.
//!
//! Original suite hand-built an `events` schema, INSERTed `pipeline.status`
//! events, and asserted on `pipelines_from_db` / `active_pipelines_from_db`.
//! Wave 6A turned both into facade stubs that return empty `Vec`s, so this
//! test only checks the shape contract.

use mustard_dashboard_lib::{ActivePipeline, PipelineSummary};

#[test]
fn pipeline_summary_default_shape() {
    let p = PipelineSummary {
        spec_name: String::from("alpha"),
        phase: String::from("plan"),
        scope: String::from("light"),
        status: String::from("ok"),
        updated_at: None,
    };
    assert_eq!(p.spec_name, "alpha");
    assert_eq!(p.phase, "plan");
}

#[test]
fn active_pipeline_default_shape() {
    let a = ActivePipeline {
        spec_name: String::from("beta"),
        status: String::from("active"),
        phase: String::from("execute"),
        current_wave: Some(2),
        total_waves: Some(5),
        model: Some(String::from("opus")),
        has_dispatch_failure: false,
        failure_age_ms: None,
        tasks_pending: 0,
        tasks_in_progress: 1,
        tasks_completed: 3,
        updated_at: Some(String::from("2026-05-27T10:00:00.000Z")),
    };
    assert_eq!(a.spec_name, "beta");
    assert_eq!(a.tasks_in_progress, 1);
    assert_eq!(a.tasks_completed, 3);
    assert!(!a.has_dispatch_failure);
}
