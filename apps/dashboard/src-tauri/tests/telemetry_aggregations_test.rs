use rusqlite::Connection;
use mustard_dashboard_lib::telemetry_agg::{
    telemetry_agents, telemetry_heatmap, telemetry_phases,
};

/// Minimal schema — same columns as the real mustard.db events table.
const SCHEMA: &str = r#"
CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  session_id TEXT,
  wave INTEGER,
  spec TEXT,
  event TEXT NOT NULL,
  actor_kind TEXT,
  actor_id TEXT,
  payload TEXT
);
CREATE TABLE specs (
  name TEXT PRIMARY KEY,
  status TEXT,
  phase TEXT,
  started_at TEXT,
  completed_at TEXT,
  affected_files TEXT
);
"#;

/// Seed 9 events covering 2 specs, 3 phases, 2 agents.
fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();

    // pipeline.phase events — spec-a ANALYZE, spec-a EXECUTE, spec-b PLAN
    let phase_events = [
        ("2026-05-20T08:00:00Z", "s1", "spec-a", "pipeline.phase", r#"{"phase":"ANALYZE","to":"ANALYZE"}"#),
        ("2026-05-20T09:00:00Z", "s1", "spec-a", "pipeline.phase", r#"{"phase":"EXECUTE","to":"EXECUTE"}"#),
        ("2026-05-20T10:00:00Z", "s2", "spec-b", "pipeline.phase", r#"{"phase":"PLAN","to":"PLAN"}"#),
    ];
    for (ts, sid, spec, evt, payload) in &phase_events {
        conn.execute(
            "INSERT INTO events (ts, session_id, spec, event, payload) VALUES (?,?,?,?,?)",
            rusqlite::params![ts, sid, spec, evt, payload],
        )
        .unwrap();
    }

    // agent.start / agent.stop pairs — two agent types: "general-purpose" and "Explore"
    // Pair 1: session s1, actor agent-1 → general-purpose, ~60 s duration
    conn.execute(
        "INSERT INTO events (ts, session_id, actor_id, spec, event, payload) VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            "2026-05-20T09:00:00Z", "s1", "agent-1", "spec-a", "agent.start",
            r#"{"subagent_type":"general-purpose"}"#
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO events (ts, session_id, actor_id, spec, event, payload) VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            "2026-05-20T09:01:00Z", "s1", "agent-1", "spec-a", "agent.stop",
            r#"{"isError":0}"#
        ],
    )
    .unwrap();

    // Pair 2: session s2, actor agent-2 → Explore, ~30 s duration
    conn.execute(
        "INSERT INTO events (ts, session_id, actor_id, spec, event, payload) VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            "2026-05-20T10:00:00Z", "s2", "agent-2", "spec-b", "agent.start",
            r#"{"subagent_type":"Explore"}"#
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO events (ts, session_id, actor_id, spec, event, payload) VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            "2026-05-20T10:00:30Z", "s2", "agent-2", "spec-b", "agent.stop",
            r#"{"isError":1}"#
        ],
    )
    .unwrap();

    // extra tool.use and session.end to pad to 10 events
    conn.execute(
        "INSERT INTO events (ts, session_id, event, payload) VALUES (?,?,?,?)",
        rusqlite::params!["2026-05-20T11:00:00Z", "s1", "tool.use", r#"{"tool":"Read"}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO events (ts, session_id, event, payload) VALUES (?,?,?,?)",
        rusqlite::params!["2026-05-20T12:00:00Z", "s2", "session.end", "{}"],
    )
    .unwrap();

    // specs table rows
    conn.execute(
        "INSERT INTO specs (name, status, phase, started_at) VALUES (?,?,?,?)",
        rusqlite::params!["spec-a", "active", "EXECUTE", "2026-05-20"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO specs (name, status, phase, started_at) VALUES (?,?,?,?)",
        rusqlite::params!["spec-b", "active", "PLAN", "2026-05-20"],
    )
    .unwrap();

    conn
}

// ── telemetry_phases ─────────────────────────────────────────────────────────

#[test]
fn phases_returns_non_empty_for_all_range() {
    let conn = setup();
    let phases = telemetry_phases(&conn, "all").unwrap();
    // We inserted pipeline.phase events for ANALYZE, EXECUTE, PLAN
    assert!(!phases.is_empty(), "expected at least one phase summary");
}

#[test]
fn phases_shape_is_correct() {
    let conn = setup();
    let phases = telemetry_phases(&conn, "all").unwrap();
    for p in &phases {
        // sparkline always 7 slots
        assert_eq!(p.sparkline.len(), 7, "sparkline must be 7 slots");
        assert!(p.events_count >= 1, "events_count must be positive");
    }
}

#[test]
fn phases_counts_match_inserts() {
    let conn = setup();
    let phases = telemetry_phases(&conn, "all").unwrap();
    // Total across all phases must equal 3 pipeline.phase events inserted.
    let total: i64 = phases.iter().map(|p| p.events_count).sum();
    assert_eq!(total, 3, "total phase events should be 3");
}

// ── telemetry_heatmap ────────────────────────────────────────────────────────

#[test]
fn heatmap_returns_only_non_zero_cells() {
    let conn = setup();
    let cells = telemetry_heatmap(&conn, "all").unwrap();
    // We inserted events at 08:00, 09:00, 10:00, 11:00, 12:00 UTC on 2026-05-20.
    // All cells returned should have event_count >= 1.
    for c in &cells {
        assert!(c.event_count >= 1, "heatmap should not return zero-count cells");
        assert!((0..=6).contains(&c.day_of_week), "day_of_week must be 0–6");
        assert!((0..=23).contains(&c.hour), "hour must be 0–23");
    }
}

#[test]
fn heatmap_total_matches_all_events() {
    let conn = setup();
    let cells = telemetry_heatmap(&conn, "all").unwrap();
    let total: i64 = cells.iter().map(|c| c.event_count).sum();
    // 9 events total inserted in setup()
    assert_eq!(total, 9, "heatmap total should equal all 9 events");
}

// ── telemetry_agents ─────────────────────────────────────────────────────────

#[test]
fn agents_groups_by_subagent_type_not_actor_id() {
    let conn = setup();
    let agents = telemetry_agents(&conn, "all").unwrap();
    // We have 2 distinct subagent_types: "general-purpose" and "Explore"
    assert_eq!(agents.len(), 2, "expected 2 distinct subagent_type groups");

    let types: Vec<&str> = agents.iter().map(|a| a.subagent_type.as_str()).collect();
    assert!(
        types.contains(&"general-purpose"),
        "expected 'general-purpose' subagent_type"
    );
    assert!(
        types.contains(&"Explore"),
        "expected 'Explore' subagent_type"
    );
}

#[test]
fn agents_error_count_tracks_is_error_flag() {
    let conn = setup();
    let agents = telemetry_agents(&conn, "all").unwrap();

    let explore = agents.iter().find(|a| a.subagent_type == "Explore").unwrap();
    assert_eq!(explore.error_count, 1, "Explore agent.stop had isError=1");

    let gp = agents
        .iter()
        .find(|a| a.subagent_type == "general-purpose")
        .unwrap();
    assert_eq!(gp.error_count, 0, "general-purpose agent.stop had isError=0");
}

#[test]
fn agents_avg_duration_positive_when_pairs_matched() {
    let conn = setup();
    let agents = telemetry_agents(&conn, "all").unwrap();

    let gp = agents
        .iter()
        .find(|a| a.subagent_type == "general-purpose")
        .unwrap();
    // start=09:00:00, stop=09:01:00 → 60 000 ms
    assert_eq!(gp.avg_duration_ms, 60_000, "expected 60s duration for general-purpose");

    let explore = agents.iter().find(|a| a.subagent_type == "Explore").unwrap();
    // start=10:00:00, stop=10:00:30 → 30 000 ms
    assert_eq!(explore.avg_duration_ms, 30_000, "expected 30s duration for Explore");
}
