//! Wave 6B fixture-mode rewrite of `telemetry_test.rs`.
//!
//! Legacy: opened the SQLite reader, asserted on `RtkBlock`, `RoutingBlock`,
//! `HookFireCount` payloads derived from the `events` table. Wave 6B
//! collapses these to fail-open stubs (full NDJSON readers ship in a
//! follow-up sub-spec); the test verifies they still return zero-shaped
//! defaults on a clean repo without panicking.

use mustard_dashboard::telemetry;
use std::path::PathBuf;
use tempfile::TempDir;

fn clean_repo() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".claude").join("spec")).expect("mkdir");
    tmp
}

/// `rtk_summary` does not READ the path it is given — it SPAWNS the `rtk` binary
/// inside it (`telemetry::run_rtk_gain`), and `rtk` falls back to its GLOBAL
/// store when the directory holds no project data of its own. So a clean repo
/// says nothing about availability: without `rtk` on PATH the block comes back
/// unavailable; with it — every Mustard development machine, since the harness
/// routes commands through `rtk` — the global numbers answer and `available` is
/// `true`.
///
/// The predecessor asserted `!r.available` and therefore measured the MACHINE
/// rather than the code: green only on a bare CI runner, red on every machine
/// that could actually run the dashboard. Measured on 2026-08-22: `rtk gain` in
/// an empty directory reported 13076 commands under "Global Scope".
///
/// What the function really guarantees is the invariant below, and BOTH branches
/// assert something — so the test is empty on neither machine. Unavailable means
/// the zero shape (`RtkBlock::default()`); available means at least one field was
/// actually measured, because a block claiming availability with nothing in it is
/// the failure this test exists to catch.
#[test]
fn rtk_summary_is_well_shaped_on_clean_repo() {
    let tmp = clean_repo();
    let r = telemetry::rtk_summary(&PathBuf::from(tmp.path()));

    if r.available {
        assert!(
            r.total_commands.is_some() || r.tokens_saved.is_some(),
            "an available rtk block must carry at least one measured field"
        );
    } else {
        assert!(r.daily.is_empty(), "an unavailable rtk block carries no daily rows");
        assert!(
            r.total_commands.is_none()
                && r.input_tokens.is_none()
                && r.output_tokens.is_none()
                && r.tokens_saved.is_none()
                && r.savings_pct.is_none()
                && r.total_exec_time_ms.is_none(),
            "an unavailable rtk block is the zero shape — every measured field is absent"
        );
    }
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
