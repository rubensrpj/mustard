//! Wave 6B fixture-mode rewrite of `telemetry_aggregations_test.rs`.
//!
//! Legacy: built a SQLite events table, inserted phase / timeline / heatmap
//! / criteria / effort / agent rows and asserted that the aggregators
//! computed the right buckets. Wave 6B keeps the signatures but stubs every
//! body to an empty/default payload (the dedicated NDJSON aggregator lands
//! in a follow-up sub-spec). The closure handed to `db::with_db` is never
//! invoked (the facade short-circuits with `None`), so we exercise the
//! contract through that wrapper instead of constructing the uninhabited
//! `db::Connection` placeholder directly.

use mustard_dashboard_lib::db;
use std::path::PathBuf;
use tempfile::TempDir;

fn clean_repo() -> PathBuf {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".claude").join("spec")).unwrap();
    let path = tmp.path().to_path_buf();
    std::mem::forget(tmp); // leak the tempdir for the test lifetime
    path
}

#[test]
fn telemetry_agg_signatures_compile_against_facade() {
    let base = clean_repo();
    // Each closure references one telemetry-agg function — Rust still
    // type-checks every line even though the facade short-circuits before
    // the closure runs. This guards against signature drift across the
    // Wave 6A→6B boundary.
    let phases = db::with_db(&base, |conn| {
        Ok::<_, String>(
            mustard_dashboard_lib::telemetry_agg::telemetry_phases(conn, "all")
                .unwrap_or_default()
                .len(),
        )
    });
    let timeline = db::with_db(&base, |conn| {
        Ok::<_, String>(
            mustard_dashboard_lib::telemetry_agg::telemetry_timeline(conn, "all", 50)
                .unwrap_or_default()
                .len(),
        )
    });
    let heatmap = db::with_db(&base, |conn| {
        Ok::<_, String>(
            mustard_dashboard_lib::telemetry_agg::telemetry_heatmap(conn, "7d")
                .unwrap_or_default()
                .len(),
        )
    });
    let history = db::with_db(&base, |conn| {
        Ok::<_, String>(
            mustard_dashboard_lib::telemetry_agg::telemetry_history(conn, "all", 25)
                .unwrap_or_default()
                .len(),
        )
    });
    let criteria = db::with_db(&base, |conn| {
        Ok::<_, String>(
            mustard_dashboard_lib::telemetry_agg::telemetry_criteria(conn, "all")
                .unwrap_or_default()
                .len(),
        )
    });
    let effort = db::with_db(&base, |conn| {
        Ok::<_, String>(
            mustard_dashboard_lib::telemetry_agg::telemetry_effort(conn, "all")
                .unwrap_or_default()
                .top_files
                .len(),
        )
    });
    let agents = db::with_db(&base, |conn| {
        Ok::<_, String>(
            mustard_dashboard_lib::telemetry_agg::telemetry_agents(conn, "all")
                .unwrap_or_default()
                .len(),
        )
    });

    // Facade contract: every with_db call returns None on the clean repo.
    assert!(phases.is_none());
    assert!(timeline.is_none());
    assert!(heatmap.is_none());
    assert!(history.is_none());
    assert!(criteria.is_none());
    assert!(effort.is_none());
    assert!(agents.is_none());
}
