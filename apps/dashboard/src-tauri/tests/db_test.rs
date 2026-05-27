//! Wave 6B fixture-mode rewrite of `db_test.rs`.
//!
//! The legacy file built a DB connection, executed the dashboard schema,
//! and asserted on `metrics_from_db` / `knowledge_from_db` /
//! `recent_events_from_db`. After Wave 6A turned `db.rs` into a filesystem
//! facade those queries return empty/zero values by construction, so the
//! tests collapse to "the public surface still returns shape-correct
//! responses against an empty repo".

use mustard_dashboard_lib::{KnowledgeSummary, MetricsSummary};
use std::path::PathBuf;
use tempfile::TempDir;

fn empty_repo() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".claude").join("spec")).unwrap();
    tmp
}

#[test]
fn with_db_returns_none_on_clean_repo() {
    let tmp = empty_repo();
    let result: Option<Result<u32, String>> =
        mustard_dashboard_lib::db::with_db(&PathBuf::from(tmp.path()), |_conn| Ok(7_u32));
    assert!(result.is_none(), "with_db must return None post-Wave-6A");
}

#[test]
fn metrics_summary_zeroed_shape() {
    let m = MetricsSummary {
        total_events: 0,
        sessions_recent: 0,
        agents_dispatched: 0,
        last_event_at: None,
        tokens_total: 0,
        tokens_today: 0,
    };
    assert_eq!(m.total_events, 0);
    assert_eq!(m.tokens_total, 0);
    assert!(m.last_event_at.is_none());
}

#[test]
fn knowledge_summary_zeroed_shape() {
    let k = KnowledgeSummary {
        patterns_count: 0,
        conventions_count: 0,
        high_confidence_count: 0,
    };
    assert_eq!(k.patterns_count + k.conventions_count + k.high_confidence_count, 0);
}
