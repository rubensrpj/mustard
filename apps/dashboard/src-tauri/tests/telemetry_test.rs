use mustard_dashboard_lib::telemetry::dashboard_prompt_economy;
use rusqlite::{params, Connection};
use std::path::PathBuf;

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
CREATE TABLE claude_code_otel (
  ts_bucket INTEGER NOT NULL,
  signal TEXT NOT NULL,
  metric TEXT NOT NULL,
  session_id TEXT,
  model TEXT,
  token_type TEXT,
  sum REAL DEFAULT 0,
  count INTEGER DEFAULT 0,
  attrs TEXT,
  PRIMARY KEY (ts_bucket, metric, session_id, model, token_type)
);
"#;

struct TempRepo(PathBuf);
impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn make_repo(populate: bool) -> TempRepo {
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("mustard-test-{}-{}", std::process::id(), ns));
    let harness = base.join(".claude").join(".harness");
    std::fs::create_dir_all(&harness).unwrap();

    let db_path = harness.join("mustard.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();

    if populate {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let bucket = now_ms - (now_ms % 60_000);

        conn.execute(
            "INSERT INTO claude_code_otel \
             (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs) \
             VALUES (?1, 'metric', 'claude_code.cost.usage', 's-1', 'claude-opus-4-7', NULL, 12.5, 5, '{}')",
            params![bucket],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO claude_code_otel \
             (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs) \
             VALUES (?1, 'metric', 'claude_code.cost.usage', 's-2', 'claude-sonnet-4-6', NULL, 3.0, 2, '{}')",
            params![bucket],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO claude_code_otel \
             (ts_bucket, signal, metric, session_id, model, token_type, sum, count, attrs) \
             VALUES (?1, 'metric', 'claude_code.session.count', 's-1', NULL, NULL, 1.0, 1, '{}')",
            params![bucket],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO events (ts, session_id, wave, spec, event, actor_kind, actor_id, payload) \
             VALUES (?1, 's-1', 1, 't', 'mustard.subtraction.applied', 'hook', 'orch', \
                     '{\"type\":\"wave-slice\",\"bytes_omitted\":1000,\"prompt_bytes\":400,\"wave\":1,\"measured\":true}')",
            params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (ts, session_id, wave, spec, event, actor_kind, actor_id, payload) \
             VALUES (?1, 's-1', 1, 't', 'mustard.subtraction.applied', 'hook', 'orch', \
                     '{\"type\":\"wave-slice\",\"bytes_omitted\":800,\"prompt_bytes\":300,\"wave\":1,\"measured\":true}')",
            params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (ts, session_id, wave, spec, event, actor_kind, actor_id, payload) \
             VALUES (?1, 's-1', 2, 't', 'mustard.subtraction.applied', 'hook', 'orch', \
                     '{\"type\":\"wave-slice\",\"bytes_omitted\":500,\"prompt_bytes\":200,\"wave\":2,\"measured\":true}')",
            params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();
    }

    drop(conn);
    TempRepo(base)
}

#[test]
fn populated_db_returns_real_aggregates() {
    let repo = make_repo(true);
    let path = repo.0.to_string_lossy().to_string();
    let result = dashboard_prompt_economy(path).unwrap();

    assert!(
        (result.cost.usd_total - 15.5).abs() < 0.001,
        "usd_total expected 15.5, got {}",
        result.cost.usd_total
    );
    assert_eq!(result.cost.by_model.len(), 2);
    assert_eq!(result.cost.by_model[0].model, "claude-opus-4-7");
    assert!((result.cost.by_model[0].usd - 12.5).abs() < 0.001);

    assert_eq!(result.subtractions.event_count, 3);
    assert_eq!(result.subtractions.context_sent_bytes, 900);
    assert_eq!(result.subtractions.context_avoided_bytes, 2300);
    assert_eq!(result.subtractions.by_wave.len(), 2);
    assert_eq!(result.subtractions.by_wave[0].wave, 1);
    assert_eq!(result.subtractions.by_wave[0].sent_bytes, 700);
    assert_eq!(result.subtractions.by_wave[0].avoided_bytes, 1800);
    assert_eq!(result.subtractions.by_wave[0].count, 2);
    assert_eq!(result.subtractions.by_wave[1].wave, 2);
    assert_eq!(result.subtractions.by_wave[1].sent_bytes, 200);
    assert_eq!(result.subtractions.by_wave[1].avoided_bytes, 500);

    assert_eq!(result.claude_events.session_count, 1);

    assert!(result.freshness.last_metric_ts.is_some());
    assert!(result.freshness.last_subtraction_ts.is_some());
}

#[test]
fn empty_db_degrades_to_zeros() {
    let repo = make_repo(false);
    let path = repo.0.to_string_lossy().to_string();
    let result = dashboard_prompt_economy(path).unwrap();

    assert_eq!(result.cost.usd_total, 0.0);
    assert_eq!(result.cost.by_model.len(), 0);
    assert_eq!(result.subtractions.event_count, 0);
    assert_eq!(result.subtractions.by_wave.len(), 0);
    assert_eq!(result.claude_events.session_count, 0);
    assert!(result.freshness.last_metric_ts.is_none());
    assert!(!result.freshness.otel_healthy);
}

#[test]
fn missing_db_returns_descriptive_error() {
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("mustard-no-db-{}-{}", std::process::id(), ns));
    std::fs::create_dir_all(&base).unwrap();
    let result = dashboard_prompt_economy(base.to_string_lossy().to_string());
    let _ = std::fs::remove_dir_all(&base);

    let err = match result {
        Ok(_) => panic!("expected Err for missing mustard.db, got Ok"),
        Err(e) => e,
    };
    assert!(
        err.contains("mustard.db not found"),
        "expected descriptive error, got: {}",
        err
    );
}
