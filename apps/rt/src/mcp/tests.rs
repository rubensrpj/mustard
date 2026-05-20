//! Unit tests for the `mcp` face — the pure tool logic and output shaping.
//!
//! These exercise the in-process tool handlers against a temporary
//! `mustard.db` (`tempfile`), with no transport involved: each test seeds the
//! store, calls a tool method, and asserts the JSON `CallToolResult` payload
//! matches the 1:1 shape ported from `mustard-memory.ts`. The end-to-end
//! JSON-RPC protocol path (`initialize` + the five tools over stdio) is
//! covered by the `tests/mcp.rs` integration test.
//!
//! `SqliteEventStore`'s only write API is `append` (events). To seed the
//! denormalized projections (`knowledge`, `specs`, `metrics_projection`,
//! `spans`) the tests open a second plain `rusqlite::Connection` to the same
//! database file — the store has already applied the schema on open, so the
//! tables exist.

use super::*;
use mustard_core::store::event_store::EventSink;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use std::path::Path;
use tempfile::TempDir;

/// Database path inside a temp project: `{dir}/.claude/.harness/mustard.db`.
fn db_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path()
        .join(".claude")
        .join(".harness")
        .join("mustard.db")
}

/// Open the harness store for a temp project (applies the schema).
fn store_in(dir: &TempDir) -> SqliteEventStore {
    SqliteEventStore::for_project(dir.path()).expect("open store")
}

/// Seed projection-table rows by opening a plain connection to the same file.
///
/// `SqliteEventStore` exposes no write API for the projections; the tests
/// reach the tables directly. The store must have been opened first so the
/// schema exists.
fn seed(path: &Path, sql: &str) {
    let conn = rusqlite::Connection::open(path).expect("open seed connection");
    conn.execute_batch(sql).expect("seed projection rows");
}

/// Build a server bound to a temp project directory.
fn server_in(dir: &TempDir) -> MustardMemory {
    MustardMemory::new(dir.path().to_path_buf())
}

/// Extract the single text payload from a `CallToolResult` and parse it.
fn payload(result: &CallToolResult) -> Value {
    let text = result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("a text content block");
    serde_json::from_str(&text).expect("payload is JSON")
}

/// A minimal harness event for seeding the `events` table.
fn event(name: &str, spec: Option<&str>, ts: &str) -> HarnessEvent {
    HarnessEvent {
        v: SCHEMA_VERSION,
        ts: ts.to_string(),
        session_id: "s-mcp-test".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("mcp-test".to_string()),
            actor_type: None,
        },
        event: name.to_string(),
        payload: json!({"k": "v"}),
        spec: spec.map(ToString::to_string),
    }
}

#[test]
fn search_knowledge_returns_ranked_rows_and_respects_type_filter() {
    let dir = TempDir::new().unwrap();
    let store = store_in(&dir);
    drop(store); // schema applied; close the store handle before seeding.
    seed(
        &db_path(&dir),
        "INSERT INTO knowledge (id, type, name, description, confidence) \
         VALUES ('k1', 'pattern', 'event sink', 'append events', 0.9); \
         INSERT INTO knowledge (id, type, name, description, confidence) \
         VALUES ('k2', 'convention', 'event naming', 'name events well', 0.5); \
         INSERT INTO knowledge_fts (id, name, description) \
         SELECT id, name, description FROM knowledge;",
    );

    let server = server_in(&dir);

    // No type filter: both rows match the term "event".
    let all = payload(
        &server
            .search_knowledge(Parameters(SearchKnowledgeArgs {
                query: "event".to_string(),
                r#type: None,
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert_eq!(all[0]["type"], json!("pattern"));

    // Type filter trims to the matching kind.
    let patterns = payload(
        &server
            .search_knowledge(Parameters(SearchKnowledgeArgs {
                query: "event".to_string(),
                r#type: Some("pattern".to_string()),
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(patterns.as_array().unwrap().len(), 1);
    assert_eq!(patterns[0]["id"], json!("k1"));

    // `limit` caps the result count.
    let capped = payload(
        &server
            .search_knowledge(Parameters(SearchKnowledgeArgs {
                query: "event".to_string(),
                r#type: None,
                limit: Some(1),
            }))
            .unwrap(),
    );
    assert_eq!(capped.as_array().unwrap().len(), 1);
}

#[test]
fn query_events_filters_by_spec_event_and_since() {
    let dir = TempDir::new().unwrap();
    let store = store_in(&dir);
    store
        .append(&event("tool.use", Some("spec-a"), "2026-05-19T01:00:00.000Z"))
        .unwrap();
    store
        .append(&event("tool.use", Some("spec-b"), "2026-05-19T02:00:00.000Z"))
        .unwrap();
    store
        .append(&event("decision", Some("spec-a"), "2026-05-19T03:00:00.000Z"))
        .unwrap();
    drop(store);

    let server = server_in(&dir);

    // Spec filter.
    let by_spec = payload(
        &server
            .query_events(Parameters(QueryEventsArgs {
                spec: Some("spec-a".to_string()),
                event: None,
                since: None,
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(by_spec.as_array().unwrap().len(), 2);

    // Event-name filter.
    let by_event = payload(
        &server
            .query_events(Parameters(QueryEventsArgs {
                spec: None,
                event: Some("tool.use".to_string()),
                since: None,
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(by_event.as_array().unwrap().len(), 2);

    // `since` lower bound (lexical ISO compare).
    let since = payload(
        &server
            .query_events(Parameters(QueryEventsArgs {
                spec: None,
                event: None,
                since: Some("2026-05-19T02:30:00.000Z".to_string()),
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(since.as_array().unwrap().len(), 1);
    assert_eq!(since[0]["event"], json!("decision"));
    // `sessionId` survives in the output shape.
    assert_eq!(since[0]["sessionId"], json!("s-mcp-test"));

    // `limit` caps the rows.
    let capped = payload(
        &server
            .query_events(Parameters(QueryEventsArgs {
                spec: None,
                event: None,
                since: None,
                limit: Some(1),
            }))
            .unwrap(),
    );
    assert_eq!(capped.as_array().unwrap().len(), 1);
}

#[test]
fn find_similar_specs_scores_by_token_overlap() {
    let dir = TempDir::new().unwrap();
    let store = store_in(&dir);
    drop(store);
    seed(
        &db_path(&dir),
        "INSERT INTO specs (name, status, phase) \
         VALUES ('2026-auth-login', 'active', 'EXECUTE'); \
         INSERT INTO specs (name, status, phase) \
         VALUES ('2026-billing', 'closed', 'CLOSE');",
    );

    let server = server_in(&dir);
    let matches = payload(
        &server
            .find_similar_specs(Parameters(FindSimilarSpecsArgs {
                description: "auth login execute".to_string(),
                limit: None,
            }))
            .unwrap(),
    );
    let arr = matches.as_array().unwrap();
    // Only the auth spec overlaps; billing scores zero and is filtered out.
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["spec"]["name"], json!("2026-auth-login"));
    assert!(arr[0]["score"].as_u64().unwrap() >= 1);

    // An empty description yields an empty result, never an error.
    let empty = payload(
        &server
            .find_similar_specs(Parameters(FindSimilarSpecsArgs {
                description: "   ".to_string(),
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(empty.as_array().unwrap().len(), 0);
}

#[test]
fn get_spec_metrics_returns_row_or_error_object() {
    let dir = TempDir::new().unwrap();
    let store = store_in(&dir);
    drop(store);
    seed(
        &db_path(&dir),
        "INSERT INTO specs (name) VALUES ('2026-spec'); \
         INSERT INTO metrics_projection (spec, api_calls, retries, pass1) \
         VALUES ('2026-spec', 12, 0, 1);",
    );

    let server = server_in(&dir);

    let hit = payload(
        &server
            .get_spec_metrics(Parameters(GetSpecMetricsArgs {
                spec: "2026-spec".to_string(),
            }))
            .unwrap(),
    );
    assert_eq!(hit["apiCalls"], json!(12));
    assert_eq!(hit["pass1"], json!(true));
    assert!(hit["toolBreakdown"].is_object());

    let miss = payload(
        &server
            .get_spec_metrics(Parameters(GetSpecMetricsArgs {
                spec: "no-such-spec".to_string(),
            }))
            .unwrap(),
    );
    assert_eq!(miss["error"], json!("no metrics for spec"));
    assert_eq!(miss["spec"], json!("no-such-spec"));
}

#[test]
fn get_span_summary_aggregates_totals_and_groups_by_model() {
    let dir = TempDir::new().unwrap();
    let store = store_in(&dir);
    drop(store);
    seed(
        &db_path(&dir),
        "INSERT INTO specs (name) VALUES ('2026-spec'); \
         INSERT INTO spans (span_id, spec, phase, model, input_tokens, output_tokens, duration_ms, is_error) \
         VALUES ('sp-1', '2026-spec', 'PLAN', 'opus', 100, 50, 1000, 0); \
         INSERT INTO spans (span_id, spec, phase, model, input_tokens, output_tokens, duration_ms, is_error) \
         VALUES ('sp-2', '2026-spec', 'EXECUTE', 'opus', 200, 80, 2000, 0); \
         INSERT INTO spans (span_id, spec, phase, model, input_tokens, output_tokens, duration_ms, is_error) \
         VALUES ('sp-3', '2026-spec', 'PLAN', 'haiku', 10, 5, 100, 0);",
    );

    let server = server_in(&dir);

    // Spec-scoped: all three spans aggregate.
    let summary = payload(
        &server
            .get_span_summary(Parameters(GetSpanSummaryArgs {
                spec: Some("2026-spec".to_string()),
                phase: None,
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(summary["count"], json!(3));
    assert_eq!(summary["totalInputTokens"], json!(310));
    assert_eq!(summary["totalOutputTokens"], json!(135));
    assert_eq!(summary["totalDurationMs"], json!(3100));
    assert_eq!(summary["byModel"]["opus"]["count"], json!(2));
    assert_eq!(summary["byModel"]["opus"]["in"], json!(300));
    assert_eq!(summary["byModel"]["haiku"]["count"], json!(1));

    // Phase filter trims to the PLAN spans.
    let plan = payload(
        &server
            .get_span_summary(Parameters(GetSpanSummaryArgs {
                spec: Some("2026-spec".to_string()),
                phase: Some("PLAN".to_string()),
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(plan["count"], json!(2));

    // No-spec aggregation fans out over the specs projection.
    let all = payload(
        &server
            .get_span_summary(Parameters(GetSpanSummaryArgs {
                spec: None,
                phase: None,
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(all["count"], json!(3));
}

#[test]
fn tools_on_empty_db_return_empty_results_not_errors() {
    let dir = TempDir::new().unwrap();
    let server = server_in(&dir);

    let k = payload(
        &server
            .search_knowledge(Parameters(SearchKnowledgeArgs {
                query: "anything".to_string(),
                r#type: None,
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(k.as_array().unwrap().len(), 0);

    let e = payload(
        &server
            .query_events(Parameters(QueryEventsArgs {
                spec: None,
                event: None,
                since: None,
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(e.as_array().unwrap().len(), 0);

    let s = payload(
        &server
            .get_span_summary(Parameters(GetSpanSummaryArgs {
                spec: None,
                phase: None,
                limit: None,
            }))
            .unwrap(),
    );
    assert_eq!(s["count"], json!(0));
    assert!(s["byModel"].as_object().unwrap().is_empty());
}

#[test]
fn get_info_advertises_mustard_memory_identity() {
    let dir = TempDir::new().unwrap();
    let server = server_in(&dir);
    let info = server.get_info();
    assert_eq!(info.server_info.name, "mustard-memory");
    assert_eq!(info.server_info.version, "2.0.0");
    assert!(info.capabilities.tools.is_some());
}
