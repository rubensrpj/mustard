// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! Integration test for the `mcp` face — the JSON-RPC protocol path.
//!
//! Where `src/mcp/tests.rs` calls the tool methods in-process, this test
//! drives the **real `mustard-rt mcp` subprocess** over stdio: it seeds a
//! temporary `mustard.db`, spawns the binary with `MUSTARD_DB_PATH` pointing
//! at it, performs the MCP `initialize` handshake, and then invokes each of
//! the five tools through `tools/call` — the same wire protocol Claude Code
//! speaks to the server.
//!
//! The MCP stdio transport is newline-delimited JSON-RPC (one message per
//! line, no embedded newlines), so the test frames messages with `\n` and
//! reads responses line by line. `cargo test` selects this file with
//! `cargo test -p mustard-rt mcp` (file name `mcp.rs`).

use rusqlite::Connection;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use serde_json::{Value, json};
use tempfile::TempDir;

/// The MCP protocol version the test client advertises in `initialize`.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// A live `mustard-rt mcp` subprocess plus its piped stdio handles.
struct McpProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// Incremental JSON-RPC request id.
    next_id: i64,
}

impl McpProcess {
    /// Spawn `mustard-rt mcp` with `MUSTARD_DB_PATH` bound to `db_path`,
    /// `MUSTARD_TELEMETRY_DB_PATH` bound to `telemetry_path`, and the working
    /// directory pinned to `project` so the W5 per-spec NDJSON dir resolves to
    /// the seeded location (`<project>/.claude/spec/<spec>/events/`).
    fn spawn(
        project: &std::path::Path,
        db_path: &std::path::Path,
        telemetry_path: &std::path::Path,
    ) -> Self {
        // `CARGO_BIN_EXE_mustard-rt` is injected by Cargo for integration
        // tests — it is the freshly built binary, no PATH lookup needed.
        let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .arg("mcp")
            .current_dir(project)
            .env("MUSTARD_DB_PATH", db_path)
            .env("MUSTARD_TELEMETRY_DB_PATH", telemetry_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn mustard-rt mcp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    /// Write one newline-delimited JSON-RPC message to the server's stdin.
    fn send(&mut self, message: &Value) {
        let line = serde_json::to_string(message).expect("serialize message");
        self.stdin
            .write_all(line.as_bytes())
            .expect("write to child stdin");
        self.stdin.write_all(b"\n").expect("write newline");
        self.stdin.flush().expect("flush child stdin");
    }

    /// Read one newline-delimited JSON-RPC message from the server's stdout.
    fn recv(&mut self) -> Value {
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .expect("read from child stdout");
        assert!(read > 0, "server closed stdout before responding");
        serde_json::from_str(line.trim()).expect("response is JSON")
    }

    /// Send a JSON-RPC request and return the matching response.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        let response = self.recv();
        assert_eq!(response["id"], json!(id), "response id mismatch");
        response
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    /// Run the MCP `initialize` handshake and assert the server identity.
    fn initialize(&mut self) -> Value {
        let response = self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "mcp-integration-test", "version": "0.0.0" },
            }),
        );
        let result = &response["result"];
        assert!(
            result.is_object(),
            "initialize must return a result, got: {response}"
        );
        // The MCP lifecycle requires the `notifications/initialized` follow-up
        // before the server will service tool calls.
        self.notify("notifications/initialized", json!({}));
        result.clone()
    }

    /// Invoke a tool via `tools/call` and return the parsed JSON payload of
    /// its single text content block.
    fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let result = &response["result"];
        assert!(
            result.is_object(),
            "tools/call({name}) must return a result, got: {response}"
        );
        let text = result["content"][0]["text"]
            .as_str()
            .expect("tool result has a text content block");
        serde_json::from_str(text).expect("tool payload is JSON")
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        // Closing stdin signals the server to shut down; kill as a backstop.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Create a temp `mustard.db` + `telemetry.db`, apply the W5 schema via a
/// throwaway `SqliteEventStore::new`, then seed:
///
/// - `specs` (still in SQLite under W5)
/// - two NDJSON events under `<dir>/.claude/spec/demo-spec/events/` (the W5
///   replacement for the retired `events` table — `query_events` reads both
///   `pipeline_events` and NDJSON now).
///
/// `knowledge` + `metrics_projection` are W5 stubs (search_knowledge returns
/// `[]`, get_spec_metrics returns `{error}`) so they need no seeds.
fn seed_db() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    use mustard_core::store::sqlite_store::SqliteEventStore;

    let dir = TempDir::new().expect("temp dir");
    // Workspace anchor required by ClaudePaths (deep-refactor W1/W2).
    std::fs::write(dir.path().join("mustard.json"), b"{}").expect("workspace anchor");
    // The MCP server resolves `<project>/.claude/.harness/mustard.db` when
    // `MUSTARD_DB_PATH` is set. Put the DB exactly there so the per-spec
    // NDJSON dir resolution (under `<project>/.claude/spec/`) lines up.
    let claude_dir = dir.path().join(".claude").join(".harness");
    std::fs::create_dir_all(&claude_dir).expect("claude dir");
    let db_path = claude_dir.join("mustard.db");
    let telemetry_path = claude_dir.join("telemetry.db");

    // Apply the W5 schema via the canonical opener.
    let _ = SqliteEventStore::new(&db_path).expect("open mustard.db");

    // Seed the W5-shaped projections directly.
    let conn = Connection::open(&db_path).expect("reopen for seed");
    conn.execute_batch(
        "INSERT INTO specs (name, status, phase) VALUES ('demo-spec', 'active', 'EXECUTE');",
    )
    .expect("seed specs");
    drop(conn);

    // Seed the two demo-spec events into NDJSON — query_events folds NDJSON
    // alongside pipeline_events under W5.
    let events_dir = dir
        .path()
        .join(".claude")
        .join("spec")
        .join("demo-spec")
        .join("events");
    std::fs::create_dir_all(&events_dir).expect("events dir");
    let body = format!(
        "{}\n{}\n",
        serde_json::to_string(&json!({
            "ts": "2026-05-19T01:00:00.000Z",
            "ts_ms": 0,
            "event": "tool.use",
            "kind": "tool",
            "spec": "demo-spec",
            "session_id": "s-1",
            "actor": "h",
            "payload": {},
        }))
        .unwrap(),
        serde_json::to_string(&json!({
            "ts": "2026-05-19T02:00:00.000Z",
            "ts_ms": 0,
            "event": "decision",
            "kind": "knowledge",
            "spec": "demo-spec",
            "session_id": "s-1",
            "actor": "h",
            "payload": {},
        }))
        .unwrap()
    );
    std::fs::write(events_dir.join("seed.ndjson"), body).expect("write ndjson");

    // get_run_summary reads `run_usage` from the dedicated telemetry
    // database. Create it + the table directly (the server re-applies the
    // idempotent telemetry schema on open) and seed the run the test asserts.
    let tconn = Connection::open(&telemetry_path).expect("open telemetry db");
    tconn
        .execute_batch(
            "CREATE TABLE run_usage (
                trace_id TEXT, span_id TEXT PRIMARY KEY, parent_span_id TEXT,
                name TEXT, started_at INTEGER, ended_at INTEGER, duration_ms INTEGER,
                attributes TEXT, spec TEXT, phase TEXT, model TEXT,
                input_tokens INTEGER, output_tokens INTEGER,
                cache_read_input_tokens INTEGER, cache_creation_input_tokens INTEGER,
                cost_usd_micros INTEGER, is_error INTEGER, project_path TEXT,
                ts_iso TEXT, session_id TEXT, wave_id TEXT, tool_use_id TEXT,
                agent_id TEXT);
             INSERT INTO run_usage
                (span_id, spec, phase, model, input_tokens, output_tokens, duration_ms, is_error)
                VALUES ('sp-1', 'demo-spec', 'EXECUTE', 'opus', 120, 40, 800, 0);",
        )
        .expect("apply telemetry schema + seed");
    drop(tconn);

    (dir, db_path, telemetry_path)
}

/// One end-to-end run: `initialize` + `tools/list` + each of the five tools.
#[test]
fn mcp_server_handshakes_and_serves_all_five_tools() {
    let (dir, db_path, telemetry_path) = seed_db();
    let mut mcp = McpProcess::spawn(dir.path(), &db_path, &telemetry_path);

    // --- initialize handshake ---------------------------------------------
    let init = mcp.initialize();
    assert_eq!(
        init["serverInfo"]["name"],
        json!("mustard-memory"),
        "server identity"
    );

    // --- tools/list — all five tools must be advertised -------------------
    let listed = mcp.request("tools/list", json!({}));
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for expected in [
        "search_knowledge",
        "query_events",
        "find_similar_specs",
        "get_spec_metrics",
        "get_run_summary",
    ] {
        assert!(names.contains(&expected), "tool {expected} not advertised");
    }

    // --- tool 1: search_knowledge (W5 stub — always empty) ----------------
    let knowledge = mcp.call_tool("search_knowledge", json!({ "query": "event" }));
    let rows = knowledge.as_array().expect("knowledge array");
    assert!(
        rows.is_empty(),
        "search_knowledge is a W5 stub (legacy `knowledge` table retired)"
    );

    // --- tool 2: query_events ---------------------------------------------
    let events = mcp.call_tool("query_events", json!({ "spec": "demo-spec" }));
    assert_eq!(events.as_array().expect("events array").len(), 2);
    let only_decision =
        mcp.call_tool("query_events", json!({ "event": "decision" }));
    let decision_rows = only_decision.as_array().unwrap();
    assert_eq!(decision_rows.len(), 1);
    assert_eq!(decision_rows[0]["event"], json!("decision"));

    // --- tool 3: find_similar_specs ---------------------------------------
    let specs =
        mcp.call_tool("find_similar_specs", json!({ "description": "demo-spec execute" }));
    let matches = specs.as_array().expect("specs array");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["spec"]["name"], json!("demo-spec"));
    assert!(matches[0]["score"].as_u64().unwrap() >= 1);

    // --- tool 4: get_spec_metrics (W5 stub — always {error}) --------------
    let metrics = mcp.call_tool("get_spec_metrics", json!({ "spec": "demo-spec" }));
    assert_eq!(
        metrics["error"],
        json!("no metrics for spec"),
        "metrics_projection retired in W5"
    );
    let missing =
        mcp.call_tool("get_spec_metrics", json!({ "spec": "nope" }));
    assert_eq!(missing["error"], json!("no metrics for spec"));

    // --- tool 5: get_run_summary ------------------------------------------
    let summary = mcp.call_tool("get_run_summary", json!({ "spec": "demo-spec" }));
    assert_eq!(summary["count"], json!(1));
    assert_eq!(summary["totalInputTokens"], json!(120));
    assert_eq!(summary["totalOutputTokens"], json!(40));
    assert_eq!(summary["byModel"]["opus"]["count"], json!(1));
}
