//! Wave 6B fixture-mode rewrite of `telemetry_test.rs`.
//!
//! Legacy: opened the SQLite reader, asserted on `RtkBlock`, `RoutingBlock`,
//! `HookFireCount` payloads derived from the `events` table. Wave 6B
//! collapses these to fail-open stubs (full NDJSON readers ship in a
//! follow-up sub-spec); the test verifies they still return zero-shaped
//! defaults on a clean repo without panicking.

use mustard_dashboard_lib::telemetry;
use std::path::PathBuf;
use tempfile::TempDir;

fn clean_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".claude").join("spec")).unwrap();
    tmp
}

#[test]
fn rtk_summary_is_unavailable_on_clean_repo() {
    let tmp = clean_repo();
    let r = telemetry::rtk_summary(&PathBuf::from(tmp.path()));
    assert!(!r.available);
    assert!(r.daily.is_empty());
}

#[test]
fn routing_breakdown_is_zeroed() {
    let tmp = clean_repo();
    let r = telemetry::routing_breakdown(&PathBuf::from(tmp.path()), None);
    assert_eq!(r.blocks + r.allows + r.session_blocks + r.session_allows, 0);
    assert!(r.by_intent.is_empty());
    assert!(r.by_note.is_empty());
}

#[test]
fn hook_fire_counts_empty() {
    let tmp = clean_repo();
    let v = telemetry::hook_fire_counts(&PathBuf::from(tmp.path()), None);
    assert!(v.is_empty());
}

#[test]
fn agent_activity_zeroed() {
    let tmp = clean_repo();
    let a = telemetry::agent_activity(&PathBuf::from(tmp.path()));
    assert_eq!(a.total_dispatches, 0);
    assert_eq!(a.total_errors, 0);
    assert!(a.agents.is_empty());
}
