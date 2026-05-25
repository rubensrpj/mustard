#![allow(clippy::unwrap_used)]
//! Integration tests for the economy attribution roll-ups.
//!
//! Wave 3 (telemetry-separation) moved every span-based aggregation onto the
//! self-attributed `run_usage` table in the dedicated `telemetry.db`. The
//! reader no longer reconstructs `(agent, spec, wave)` from a `spans`↔events
//! JOIN — the triple is stamped on each `run_usage` row at write time (Wave 2)
//! and backfilled for history (Wave 1). These tests therefore seed `run_usage`
//! rows directly and assert `per_agent_costs` / `per_spec_costs` /
//! `per_wave_costs` group them correctly.

use mustard_core::economy::{
    EconomyScope, per_agent_costs, per_spec_costs, per_wave_costs,
};
use mustard_core::economy::scope::{ProjectPath, SpecId, WaveId};
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::telemetry::TelemetryWriter;
use mustard_core::telemetry::model::RunUsage;
use mustard_core::telemetry::store::TelemetryStore;
use tempfile::tempdir;

/// Materialise the harness DB so a project root looks real, then return a
/// telemetry store rooted at the same project (`for_project` resolves
/// `{project}/.claude/.harness/telemetry.db`, which is where the economy reader
/// looks).
fn telemetry_for(dir: &std::path::Path) -> TelemetryStore {
    let _store = SqliteEventStore::new(dir.join(".claude/.harness/mustard.db")).unwrap();
    TelemetryStore::for_project(dir).unwrap()
}

/// Seed one self-attributed `run_usage` row.
#[allow(clippy::too_many_arguments)]
fn seed_run(
    store: &TelemetryStore,
    span_id: &str,
    spec: &str,
    wave: &str,
    agent: &str,
    cost: i64,
    tokens: i64,
) {
    store
        .record_run(&RunUsage {
            trace_id: None,
            span_id: span_id.into(),
            parent_span_id: None,
            name: None,
            started_at: Some(0),
            ended_at: None,
            duration_ms: None,
            attributes: None,
            spec: Some(spec.into()),
            phase: None,
            model: Some("claude-3-5-sonnet".into()),
            input_tokens: Some(tokens),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
            cost_usd_micros: Some(cost),
            is_error: false,
            project_path: None,
            ts_iso: Some("2026-05-21T10:00:00Z".into()),
            session_id: Some("sess-1".into()),
            wave_id: Some(wave.into()),
            tool_use_id: None,
            agent_id: Some(agent.into()),
        })
        .unwrap();
}

/// A throwaway mustard.db connection to satisfy the reader signature. The
/// span-based roll-ups read telemetry.db (resolved from the scope's project),
/// not this connection — but the savings/frames half still expects a real one.
fn open_conn(dir: &std::path::Path) -> rusqlite::Connection {
    let _store = SqliteEventStore::new(dir.join(".claude/.harness/mustard.db")).unwrap();
    rusqlite::Connection::open(dir.join(".claude/.harness/mustard.db")).unwrap()
}

#[test]
fn test_agent_rollup_single() {
    let dir = tempdir().unwrap();
    let store = telemetry_for(dir.path());
    let conn = open_conn(dir.path());
    seed_run(&store, "req-1", "spec-A", "wave-1", "core-impl", 10_000, 500);

    let rows =
        per_agent_costs(&conn, EconomyScope::Project(ProjectPath::new(dir.path()))).unwrap();
    assert_eq!(rows.len(), 1, "expected one attributed agent");
    assert_eq!(rows[0].agent_id.as_str(), "core-impl");
    assert_eq!(rows[0].cost_usd_micros, 10_000);
    assert_eq!(rows[0].tokens, 500);
    assert_eq!(rows[0].span_count, 1);
}

#[test]
fn test_agent_rollup_excludes_unattributed() {
    let dir = tempdir().unwrap();
    let store = telemetry_for(dir.path());
    let conn = open_conn(dir.path());
    // A run with no agent_id must be excluded from the per-agent roll-up.
    store
        .record_run(&RunUsage {
            trace_id: None,
            span_id: "no-agent".into(),
            parent_span_id: None,
            name: None,
            started_at: Some(0),
            ended_at: None,
            duration_ms: None,
            attributes: None,
            spec: Some("spec-F".into()),
            phase: None,
            model: Some("claude-3-5-sonnet".into()),
            input_tokens: Some(300),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
            cost_usd_micros: Some(7_500),
            is_error: false,
            project_path: None,
            ts_iso: Some("2026-05-21T10:05:00Z".into()),
            session_id: Some("sess-fallback".into()),
            wave_id: Some("wave-2".into()),
            tool_use_id: None,
            agent_id: None,
        })
        .unwrap();
    seed_run(&store, "with-agent", "spec-F", "wave-2", "core-explore", 7_500, 300);

    let rows =
        per_agent_costs(&conn, EconomyScope::Project(ProjectPath::new(dir.path()))).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].agent_id.as_str(), "core-explore");
    assert_eq!(rows[0].cost_usd_micros, 7_500);
    assert_eq!(rows[0].span_count, 1);
}

#[test]
fn test_empty_all_projects() {
    // AC-4: AllProjects scope with zero entries returns Vec empty, no error.
    let scope = EconomyScope::AllProjects(vec![]);
    let dir = tempdir().unwrap();
    let conn = open_conn(dir.path());
    let rows = per_agent_costs(&conn, scope).unwrap();
    assert!(rows.is_empty(), "empty AllProjects must return Vec::new()");

    let rows = per_spec_costs(&conn, EconomyScope::AllProjects(vec![])).unwrap();
    assert!(rows.is_empty());

    let rows = per_wave_costs(&conn, EconomyScope::AllProjects(vec![])).unwrap();
    assert!(rows.is_empty());
}

#[test]
fn test_per_spec_aggregation() {
    let dir = tempdir().unwrap();
    let store = telemetry_for(dir.path());
    let conn = open_conn(dir.path());

    seed_run(&store, "r1", "spec-X", "w1", "agent-x", 1_000, 100);
    seed_run(&store, "r2", "spec-X", "w1", "agent-x", 2_000, 200);
    seed_run(&store, "r3", "spec-Y", "w1", "agent-y", 5_000, 500);

    let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
    let by_spec = per_spec_costs(&conn, scope).unwrap();
    assert_eq!(by_spec.len(), 2, "expected one row per spec");
    // Sorted DESC by cost — spec-Y first (5_000), spec-X second (3_000).
    assert_eq!(by_spec[0].spec_id.as_str(), "spec-Y");
    assert_eq!(by_spec[0].cost_usd_micros, 5_000);
    assert_eq!(by_spec[0].span_count, 1);
    assert_eq!(by_spec[1].spec_id.as_str(), "spec-X");
    assert_eq!(by_spec[1].cost_usd_micros, 3_000);
    assert_eq!(by_spec[1].span_count, 2);
}

#[test]
fn test_per_wave_aggregation() {
    let dir = tempdir().unwrap();
    let store = telemetry_for(dir.path());
    let conn = open_conn(dir.path());

    seed_run(&store, "r1", "spec-W", "wave-alpha", "core-impl", 1_200, 120);
    seed_run(&store, "r2", "spec-W", "wave-beta", "core-impl", 3_400, 340);

    let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
    let by_wave = per_wave_costs(&conn, scope).unwrap();
    assert_eq!(by_wave.len(), 2, "expected one row per (spec, wave) pair");

    // Sorted DESC by cost: wave-beta (3_400) first, wave-alpha (1_200) second.
    assert_eq!(by_wave[0].spec_id.as_str(), "spec-W");
    assert_eq!(by_wave[0].wave_id.as_str(), "wave-beta");
    assert_eq!(by_wave[0].cost_usd_micros, 3_400);
    assert_eq!(by_wave[1].wave_id.as_str(), "wave-alpha");
    assert_eq!(by_wave[1].cost_usd_micros, 1_200);

    // Wave-scoped filter narrows to a single wave's roll-up.
    let wave_scope = EconomyScope::Wave {
        project: ProjectPath::new(dir.path()),
        spec: SpecId::new("spec-W"),
        wave: WaveId::new("wave-alpha"),
    };
    let scoped = per_wave_costs(&conn, wave_scope).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].wave_id.as_str(), "wave-alpha");
    assert_eq!(scoped[0].cost_usd_micros, 1_200);
}

/// Regression test for AC-6 — the literal name is grepped by the AC binary.
///
/// Models the scenario the superseded `metrics-writers-pipeline-key` spec
/// described: a run dispatched on a parent spec's session but carrying the
/// child wave it was launched against. With self-attribution the run row's own
/// `spec` / `wave_id` columns already hold the right values, so the roll-up
/// surfaces the child wave directly.
#[test]
fn test_parent_spec_child_wave_attribution() {
    let dir = tempdir().unwrap();
    let store = telemetry_for(dir.path());
    let conn = open_conn(dir.path());

    seed_run(&store, "req-pc", "parent-spec", "child-wave", "core-impl", 8_888, 444);

    let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
    let by_wave = per_wave_costs(&conn, scope.clone()).unwrap();
    assert_eq!(by_wave.len(), 1, "one (spec, wave) attribution expected");
    assert_eq!(by_wave[0].spec_id.as_str(), "parent-spec");
    assert_eq!(
        by_wave[0].wave_id.as_str(),
        "child-wave",
        "the run row's own wave_id must drive the wave attribution"
    );

    let by_spec = per_spec_costs(&conn, scope.clone()).unwrap();
    assert_eq!(by_spec.len(), 1);
    assert_eq!(by_spec[0].spec_id.as_str(), "parent-spec");
    assert_eq!(by_spec[0].cost_usd_micros, 8_888);

    let by_agent = per_agent_costs(&conn, scope).unwrap();
    assert_eq!(by_agent.len(), 1);
    assert_eq!(by_agent[0].agent_id.as_str(), "core-impl");
    assert_eq!(by_agent[0].cost_usd_micros, 8_888);
}
