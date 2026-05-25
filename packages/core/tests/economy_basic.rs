#![allow(clippy::unwrap_used)]
//! Integration tests for the economy domain (Wave 1).
//!
//! Exercises the writer/reader/multi-project triplet end-to-end against a
//! real SQLite file (via `tempfile::tempdir`) — the unit tests inside the
//! crate cover individual functions, but these round-trip tests are what
//! catch a writer/reader/migration mismatch.

use mustard_core::economy::{
    ContextCostFrame, EconomyScope, MultiProjectReader, SavingsRecord, SavingsSource, SpanRecord,
    economy_summary, estimate_input_tokens, per_spec_costs, record_context_cost, record_run,
    record_savings,
};
use mustard_core::economy::scope::{AgentId, ProjectPath, SpecId, WaveId};
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::telemetry::TelemetryWriter;
use mustard_core::telemetry::model::{RunUsage, UsageMetric};
use mustard_core::telemetry::store::TelemetryStore;
use mustard_core::telemetry::writer::upsert_usage_metric;
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

/// Seed a self-attributed `run_usage` row into the telemetry.db the economy
/// reader opens for `dir` (`{dir}/.claude/.harness/telemetry.db`). Wave 3 moved
/// every span-based aggregation onto `run_usage`, so the round-trip tests seed
/// it directly. `wave` is optional (the per-wave / wave-scope tests set it).
#[allow(clippy::too_many_arguments)]
fn seed_run(
    dir: &std::path::Path,
    span_id: &str,
    spec: &str,
    wave: Option<&str>,
    agent: Option<&str>,
    cost: i64,
    tokens: i64,
) {
    let store = TelemetryStore::for_project(dir).unwrap();
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
            phase: Some("EXECUTE".into()),
            model: Some("claude-3-5-sonnet".into()),
            input_tokens: Some(tokens),
            output_tokens: Some(0),
            cache_read_input_tokens: Some(0),
            cache_creation_input_tokens: Some(0),
            cost_usd_micros: Some(cost),
            is_error: false,
            project_path: None,
            ts_iso: Some("2026-05-21T00:00:00Z".into()),
            session_id: Some("s-test".into()),
            wave_id: wave.map(Into::into),
            tool_use_id: None,
            agent_id: agent.map(Into::into),
        })
        .unwrap();
}

/// Seed a MEASURED `usage_totals` counter (Anthropic's billed OTEL metric) into
/// the same telemetry.db. `claude_code.cost.usage` is USD; `.token.usage` is a
/// token count. These back the unfiltered (project-wide) `economy_summary`
/// headline, which now prefers the measured source over `run_usage` estimates.
fn seed_measured(dir: &std::path::Path, metric: &str, sum: f64) {
    let store = TelemetryStore::for_project(dir).unwrap();
    upsert_usage_metric(
        store.conn(),
        &UsageMetric {
            metric: metric.into(),
            model: Some("claude-3-5-sonnet".into()),
            session_id: Some("s-test".into()),
            sum,
            updated_at: Some(0),
        },
    )
    .unwrap();
}

#[test]
fn test_writer_roundtrip_span() {
    // Wave 2: `record_run` writes telemetry.db's `run_usage`, not mustard.db's
    // `spans`. The connection is a TelemetryStore connection.
    let dir = tempdir().unwrap();
    let conn = mustard_core::telemetry::store::TelemetryStore::new(dir.path().join("telemetry.db"))
        .unwrap()
        .into_connection();
    let rec = span_for("spec-A", "req-1", 1234, 100);
    record_run(&conn, rec).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM run_usage WHERE span_id = 'req-1'",
            [],
            |r| r.get(0),
        )
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

    // 3 runs split across 2 specs (self-attributed `run_usage` in telemetry.db).
    seed_run(dir.path(), "r1", "spec-A", None, None, 1000, 100);
    seed_run(dir.path(), "r2", "spec-A", None, None, 2000, 200);
    // spec-B strictly larger so the per-spec roll-up order is unambiguous
    // (spec-A's two runs sum to exactly 3000, so a 3000 spec-B would tie).
    seed_run(dir.path(), "r3", "spec-B", None, None, 4000, 400);
    // Measured project-wide totals (no spec/wave dim) — the unfiltered
    // `economy_summary` headline reads these, not the run_usage estimate.
    // 0.007 USD -> 7000 micro-USD; 700 measured tokens.
    seed_measured(dir.path(), "claude_code.cost.usage", 0.007);
    seed_measured(dir.path(), "claude_code.token.usage", 700.0);
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

    // Project scope: cost/tokens from MEASURED usage_totals (7000 micros / 700
    // tokens), run count + savings from run_usage / savings_records.
    let project_scope = EconomyScope::Project(ProjectPath::new(dir.path()));
    let summary = economy_summary(&conn, project_scope.clone()).unwrap();
    assert_eq!(summary.total_cost_usd_micros, 7000);
    assert_eq!(summary.total_tokens, 700);
    assert_eq!(summary.total_tokens_saved, 1200);
    assert_eq!(summary.span_count, 3);

    // Per-spec roll-up: 2 rows, larger one first.
    let by_spec = per_spec_costs(&conn, project_scope).unwrap();
    assert_eq!(by_spec.len(), 2);
    assert_eq!(by_spec[0].spec_id.as_str(), "spec-B");
    assert_eq!(by_spec[0].cost_usd_micros, 4000);

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
    // Two project dirs, two DBs, fan-out merges results. The fan-out opens each
    // project's mustard.db (read-only); the run_usage totals live in each
    // project's telemetry.db, so the closure opens that sibling per project.
    let root = tempdir().unwrap();
    let path_a = root.path().join("project-a");
    let path_b = root.path().join("project-b");
    std::fs::create_dir_all(path_a.join(".claude/.harness")).unwrap();
    std::fs::create_dir_all(path_b.join(".claude/.harness")).unwrap();
    // Materialise each mustard.db so the fan-out can open it read-only.
    drop(open_conn(&path_a));
    drop(open_conn(&path_b));

    seed_run(&path_a, "r1", "spec-A", None, None, 1000, 100);
    seed_run(&path_b, "r2", "spec-B", None, None, 2000, 200);

    let reader = MultiProjectReader::new();
    let projects = vec![
        ProjectPath::new(path_a),
        ProjectPath::new(path_b),
    ];
    let per_project = reader.fan_out(&projects, |_c, project| {
        let tele = TelemetryStore::for_project(project.as_path()).unwrap();
        let total: i64 = tele
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(cost_usd_micros), 0) FROM run_usage",
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

/// Regression: `economy_summary(scope=Wave)` must aggregate ONLY runs from the
/// targeted wave, not every run in the spec. Two waves in the same spec; the
/// Wave-scoped summary must surface one wave's totals exclusively.
///
/// Wave 3: attribution is self-contained on each `run_usage` row (`spec` +
/// `wave_id` stamped at write time), so the wave filter is a plain `WHERE
/// wave_id = ?` — no `agent.start` JOIN to reconstruct it.
#[test]
fn test_economy_summary_wave_scope_filters_to_wave_only() {
    let dir = tempdir().unwrap();
    let conn = open_conn(dir.path());

    // One run per wave, same spec, self-attributed.
    seed_run(dir.path(), "r-w1", "spec-shared", Some("wave-1"), Some("agent-w1"), 1_000, 100);
    seed_run(dir.path(), "r-w2", "spec-shared", Some("wave-2"), Some("agent-w2"), 5_000, 500);

    // Spec-wide sanity: both runs present.
    let spec_scope = EconomyScope::Spec {
        project: ProjectPath::new(dir.path()),
        spec: SpecId::new("spec-shared"),
    };
    let spec_summary = economy_summary(&conn, spec_scope).unwrap();
    assert_eq!(spec_summary.span_count, 2, "spec scope must see both runs");
    assert_eq!(spec_summary.total_cost_usd_micros, 6_000);

    // Wave-1 scope: must see ONLY the wave-1 run (1_000 cost, 100 tokens).
    let wave_scope = EconomyScope::Wave {
        project: ProjectPath::new(dir.path()),
        spec: SpecId::new("spec-shared"),
        wave: WaveId::new("wave-1"),
    };
    let wave_summary = economy_summary(&conn, wave_scope).unwrap();
    assert_eq!(
        wave_summary.span_count, 1,
        "Wave scope must filter runs by wave_id"
    );
    assert_eq!(wave_summary.total_cost_usd_micros, 1_000);
    assert_eq!(wave_summary.total_tokens, 100);
}

/// `economy_summary` aggregates whatever `run_usage` carries — the Wave 3
/// telemetry table that every cost adapter (internal estimator + external
/// OTEL/JSONL) now funnels into via `record_run`. (Pre-Wave-3 this exercised a
/// `spans` ∪ `api_cost_frames` union in mustard.db; that union is retired now
/// that there is a single self-attributed table.)
#[test]
fn test_economy_summary_includes_api_cost_frames() {
    let dir = tempdir().unwrap();
    let conn = open_conn(dir.path());

    // 3 runs in one project's telemetry.db.
    seed_run(dir.path(), "span-1", "spec-A", None, None, 1_000, 100);
    seed_run(dir.path(), "acf-1", "spec-A", None, None, 2_500, 250);
    seed_run(dir.path(), "acf-2", "spec-A", None, None, 4_000, 375);
    // Measured project-wide totals back the unfiltered headline.
    // 0.0075 USD -> 7_500 micro-USD; 725 measured tokens.
    seed_measured(dir.path(), "claude_code.cost.usage", 0.0075);
    seed_measured(dir.path(), "claude_code.token.usage", 725.0);

    let project_scope = EconomyScope::Project(ProjectPath::new(dir.path()));
    let summary = economy_summary(&conn, project_scope).unwrap();

    // Measured: 0.0075 USD -> 7_500 micro-USD (not the run_usage estimate).
    assert_eq!(summary.total_cost_usd_micros, 7_500);
    // Measured tokens: 725.
    assert_eq!(summary.total_tokens, 725);
    // Run count still from run_usage.
    assert_eq!(summary.span_count, 3);
}

/// Seed one `run_usage` row at an explicit `started_at` (epoch-ms). The shared
/// `seed_run` helper hardcodes `started_at: Some(0)`, which is fine for the
/// other tests but useless for asserting recency-based ordering.
#[allow(clippy::too_many_arguments)]
fn seed_run_at(
    dir: &std::path::Path,
    span_id: &str,
    spec: &str,
    started_at: i64,
    cost: i64,
    tokens: i64,
) {
    let store = TelemetryStore::for_project(dir).unwrap();
    store
        .record_run(&RunUsage {
            trace_id: None,
            span_id: span_id.into(),
            parent_span_id: None,
            name: None,
            started_at: Some(started_at),
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
            ts_iso: Some("2026-05-21T00:00:00Z".into()),
            session_id: Some("s-test".into()),
            wave_id: None,
            tool_use_id: None,
            agent_id: Some("explore".into()),
        })
        .unwrap();
}

/// `per_spec_costs` must surface the freshest spec first (the dashboard's
/// "newest spec at the top" expectation), with cost desc only as a tiebreaker.
///
/// Three specs with distinct `started_at`. Cheap-but-fresh ranks above
/// expensive-but-stale — that's the inversion the cost-only sort produced
/// before, and the user feedback (spec C4) that motivated this change.
#[test]
fn per_spec_costs_orders_by_recency_desc() {
    let dir = tempdir().unwrap();
    let conn = open_conn(dir.path());

    // spec-old: highest cost but oldest timestamp.
    seed_run_at(dir.path(), "r-old", "spec-old", 1_000, 9_000, 900);
    // spec-mid: middle on both axes.
    seed_run_at(dir.path(), "r-mid", "spec-mid", 2_000, 5_000, 500);
    // spec-new: lowest cost but freshest timestamp — must rank first.
    seed_run_at(dir.path(), "r-new", "spec-new", 3_000, 1_000, 100);

    let scope = EconomyScope::Project(ProjectPath::new(dir.path()));
    let by_spec = per_spec_costs(&conn, scope).unwrap();
    assert_eq!(by_spec.len(), 3);
    assert_eq!(by_spec[0].spec_id.as_str(), "spec-new");
    assert_eq!(by_spec[0].last_started_at, Some(3_000));
    assert_eq!(by_spec[1].spec_id.as_str(), "spec-mid");
    assert_eq!(by_spec[1].last_started_at, Some(2_000));
    assert_eq!(by_spec[2].spec_id.as_str(), "spec-old");
    assert_eq!(by_spec[2].last_started_at, Some(1_000));
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
