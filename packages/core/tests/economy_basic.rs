//! Integration tests for the economy domain (Wave 1).
//!
//! Exercises the writer/reader/multi-project triplet end-to-end against a
//! real SQLite file (via `tempfile::tempdir`) — the unit tests inside the
//! crate cover individual functions, but these round-trip tests are what
//! catch a writer/reader/migration mismatch.

use mustard_core::economy::{
    ContextCostFrame, EconomyScope, MultiProjectReader, SavingsRecord, SavingsSource, SpanRecord,
    economy_summary, estimate_input_tokens, per_spec_costs, record_context_cost, record_savings,
    record_span,
};
use mustard_core::economy::scope::{AgentId, ProjectPath, SpecId, WaveId};
use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::Connection;
use serde_json::Map;
use tempfile::tempdir;

fn open_conn(dir: &std::path::Path) -> Connection {
    let _store = SqliteEventStore::new(dir.join(".claude/.harness/mustard.db")).unwrap();
    Connection::open(dir.join(".claude/.harness/mustard.db")).unwrap()
}

fn span_for(spec: &str, span_id: &str, cost: i64, tokens: i64) -> SpanRecord {
    SpanRecord {
        ts: "2026-05-21T00:00:00Z".into(),
        session_id: Some("s-test".into()),
        span_id: span_id.into(),
        model: Some("claude-3-5-sonnet".into()),
        spec: Some(spec.into()),
        phase: Some("EXECUTE".into()),
        input_tokens: Some(tokens),
        output_tokens: Some(0),
        cache_read_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        cost_usd_micros: Some(cost),
        is_error: false,
        extra: Map::new(),
    }
}

#[test]
fn test_writer_roundtrip_span() {
    let dir = tempdir().unwrap();
    let conn = open_conn(dir.path());
    let rec = span_for("spec-A", "req-1", 1234, 100);
    record_span(&conn, rec).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM spans WHERE span_id = 'req-1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_writer_roundtrip_savings() {
    let dir = tempdir().unwrap();
    let conn = open_conn(dir.path());
    let rec = SavingsRecord {
        ts: "2026-05-21T00:00:00Z".into(),
        source: SavingsSource::RtkRewrite,
        tokens_saved: 4321,
        model_target: Some("claude-3-5-sonnet".into()),
        project_path: ProjectPath::new(dir.path()),
        spec_id: Some(SpecId::new("spec-A")),
        wave_id: Some(WaveId::new("w1")),
        agent_id: Some(AgentId::new("explore")),
        extra: Map::new(),
    };
    record_savings(&conn, rec).unwrap();

    let tokens: i64 = conn
        .query_row(
            "SELECT tokens_saved FROM savings_records WHERE source = 'rtk_rewrite'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tokens, 4321);
}

#[test]
fn test_writer_roundtrip_context_cost() {
    let dir = tempdir().unwrap();
    let conn = open_conn(dir.path());
    let rec = ContextCostFrame {
        ts: "2026-05-21T00:00:00Z".into(),
        agent_id: AgentId::new("core-impl"),
        wave_id: Some(WaveId::new("w1")),
        spec_id: Some(SpecId::new("spec-A")),
        project_path: ProjectPath::new(dir.path()),
        prompt_size_bytes: Some(20_000),
        prefix_stable_bytes: Some(15_000),
        slice_bytes: Some(3_000),
        recipe_bytes: Some(500),
        wave_slice_bytes: Some(1_500),
        return_size_bytes: Some(800),
        retry_overhead_bytes: Some(0),
        extra: Map::new(),
    };
    record_context_cost(&conn, rec).unwrap();

    let prompt_size: i64 = conn
        .query_row(
            "SELECT prompt_size_bytes FROM context_cost_frames WHERE agent_id = 'core-impl'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(prompt_size, 20_000);
}

#[test]
fn test_reader_scope_project_aggregates() {
    let dir = tempdir().unwrap();
    let conn = open_conn(dir.path());

    // 3 spans split across 2 specs.
    record_span(&conn, span_for("spec-A", "r1", 1000, 100)).unwrap();
    record_span(&conn, span_for("spec-A", "r2", 2000, 200)).unwrap();
    record_span(&conn, span_for("spec-B", "r3", 3000, 300)).unwrap();
    // 2 savings, one per spec.
    for (spec, tokens) in [("spec-A", 500i64), ("spec-B", 700)] {
        record_savings(
            &conn,
            SavingsRecord {
                ts: "2026-05-21T00:00:00Z".into(),
                source: SavingsSource::BashGuardBlock,
                tokens_saved: tokens,
                model_target: None,
                project_path: ProjectPath::new(dir.path()),
                spec_id: Some(SpecId::new(spec)),
                wave_id: None,
                agent_id: None,
                extra: Map::new(),
            },
        )
        .unwrap();
    }

    // Project scope: sums everything.
    let project_scope = EconomyScope::Project(ProjectPath::new(dir.path()));
    let summary = economy_summary(&conn, project_scope.clone()).unwrap();
    assert_eq!(summary.total_cost_usd_micros, 6000);
    assert_eq!(summary.total_tokens, 600);
    assert_eq!(summary.total_tokens_saved, 1200);
    assert_eq!(summary.span_count, 3);

    // Per-spec roll-up: 2 rows, larger one first.
    let by_spec = per_spec_costs(&conn, project_scope).unwrap();
    assert_eq!(by_spec.len(), 2);
    assert_eq!(by_spec[0].spec_id.as_str(), "spec-B");
    assert_eq!(by_spec[0].cost_usd_micros, 3000);

    // Spec scope: filters to spec-A only.
    let spec_scope = EconomyScope::Spec {
        project: ProjectPath::new(dir.path()),
        spec: SpecId::new("spec-A"),
    };
    let spec_summary = economy_summary(&conn, spec_scope).unwrap();
    assert_eq!(spec_summary.total_cost_usd_micros, 3000);
    assert_eq!(spec_summary.total_tokens, 300);
    assert_eq!(spec_summary.total_tokens_saved, 500);
    assert_eq!(spec_summary.span_count, 2);
}

#[test]
fn test_multi_project_reader_fanout() {
    // Two project dirs, two DBs, fan-out merges results.
    let root = tempdir().unwrap();
    let path_a = root.path().join("project-a");
    let path_b = root.path().join("project-b");
    std::fs::create_dir_all(path_a.join(".claude/.harness")).unwrap();
    std::fs::create_dir_all(path_b.join(".claude/.harness")).unwrap();
    let conn_a = open_conn(&path_a);
    let conn_b = open_conn(&path_b);

    record_span(&conn_a, span_for("spec-A", "r1", 1000, 100)).unwrap();
    record_span(&conn_b, span_for("spec-B", "r2", 2000, 200)).unwrap();

    // Drop the per-project handles so the readers can open read-only without
    // contention on Windows (where the writer's WAL holds a shared lock).
    drop(conn_a);
    drop(conn_b);

    let reader = MultiProjectReader::new();
    let projects = vec![
        ProjectPath::new(path_a),
        ProjectPath::new(path_b),
    ];
    let per_project = reader.fan_out(&projects, |c| {
        let total: i64 = c
            .query_row(
                "SELECT COALESCE(SUM(cost_usd_micros), 0) FROM spans",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok(total)
    });
    assert_eq!(per_project.len(), 2);
    let totals: i64 = per_project.values().sum();
    assert_eq!(totals, 3000);
}

#[test]
fn test_estimator_within_tolerance() {
    // The string below is 11 cl100k tokens. Anthropic lands within ±1 token
    // on short English snippets, so accept [9, 13] as the tolerance band.
    let text = "The quick brown fox jumps over the lazy dog.";
    let count = estimate_input_tokens(text, "claude-3-5-sonnet");
    assert!(
        (9..=13).contains(&count),
        "expected 9..=13 tokens, got {count}"
    );
}
