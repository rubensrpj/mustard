//! `mustard-rt run otel-collector` — a port of `scripts/otel-collector.js`.
//!
//! A local OTLP/JSON receiver for Claude Code native telemetry. Binds a
//! `tiny_http` server to `127.0.0.1` (loopback only — never the network) on
//! `MUSTARD_OTEL_PORT` (default 4318). Metrics and logs project into the
//! `claude_code_otel` table of `.claude/.harness/mustard.db`; traces land
//! span-level token usage as `run_usage` rows in `.harness/telemetry.db` via
//! the telemetry writer (each row stamped with attribution at write time).
//!
//! Routes:
//!   - `POST /v1/metrics` — OTLP MetricsService (`resourceMetrics[]`).
//!   - `POST /v1/logs`    — OTLP LogsService    (`resourceLogs[]`).
//!   - `POST /v1/traces`  — OTLP TracesService  (`resourceSpans[]`) → `run_usage`.
//!   - `GET  /healthz`    — liveness probe.
//!
//! Lifecycle: the harness spawns the collector as a long-lived child and
//! stops it with `SIGTERM` (Unix) or process termination (Windows). No
//! portable std API installs a `SIGTERM` handler without `unsafe` — forbidden
//! crate-wide — so the collector relies on the OS default action (terminate),
//! which closes the `rusqlite` connection on `Drop` and exits non-zero only
//! when killed. The accept loop additionally honours an in-process shutdown
//! flag, which is the seam the inline tests drive to drain cleanly.
//!
//! Fail-open contract: a parse error returns `400` but never crashes the
//! server — losing a few datapoints beats taking down the harness pipeline.
//! A canary log line (`.canary.log`) records each request and each error.

use super::project::project_metrics;
use super::store::{claude_dir, Store};
use mustard_core::economy::{sources::otel as otel_source, sources::IngestContext, SpanRecord};
use mustard_core::fs;
use mustard_core::telemetry::model::RunUsage;
use mustard_core::telemetry::{writer as telemetry_writer, TelemetryStore};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tiny_http::{Method, Response, Server};

use crate::run::env::{project_dir, session_id};

/// Default OTLP/HTTP port — the OpenTelemetry convention, and the value the
/// generated `settings.json` points `OTEL_EXPORTER_OTLP_ENDPOINT` at.
const DEFAULT_PORT: u16 = 4318;

/// Append one JSON record to `.claude/.harness/.canary.log`. Fail-silent: a
/// logging failure must never affect request handling.
fn canary(harness_dir: &Path, record: &Value) {
    let _ = fs::append_line(harness_dir.join(".canary.log"), &record.to_string());
}

/// Resolve the listen port from `MUSTARD_OTEL_PORT`, defaulting to 4318.
pub(crate) fn resolve_port() -> u16 {
    std::env::var("MUSTARD_OTEL_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Project a parsed OTLP body for `route` into the store. Returns the number
/// of rows upserted. A per-row store error is logged to canary but does not
/// abort the batch (fail-open, matching the JS per-datapoint `try/catch`).
fn project_into_store(store: &Store, harness_dir: &Path, route: &str, body: &Value, now_ms: i64) -> usize {
    let mut written = 0;
    if route == "/v1/metrics" {
        for row in project_metrics(body, now_ms) {
            match store.upsert_metric(&row) {
                Ok(()) => written += 1,
                Err(e) => canary(
                    harness_dir,
                    &serde_json::json!({
                        "ts": crate::util::now_iso8601(),
                        "level": "warn", "route": route,
                        "msg": "datapoint failed", "err": e.to_string(),
                    }),
                ),
            }
        }
    } else if route == "/v1/traces" {
        written = project_traces_into_economy(harness_dir, body);
    }
    // /v1/logs and any other route: nothing to persist. Claude Code log bodies
    // are not in `telemetry::CONSUMED_METRICS`, so they never reach the dashboard;
    // the collector accepts the payload (HTTP 200) but `written` stays 0.
    written
}

/// Translate OTLP/JSON `traces` into [`SpanRecord`]s via the W1 ingest adapter,
/// stamp each with its write-time attribution, then persist as `run_usage`.
///
/// Wave 2 (telemetry-separation): the trace route lands span-level token usage
/// into `telemetry.db`'s `run_usage` table. Each run is
/// born attributed — before writing, the collector looks up the attribution the
/// `agent.start` hook stamped for `(session_id, tool_use_id)` and copies
/// `spec` / `wave_id` / `agent_id` onto the run. No match → the run is written
/// unattributed (the same behaviour as the legacy no-match read-time JOIN).
///
/// Returns the number of `run_usage` rows persisted. Every failure path is
/// logged to canary and degraded — a malformed payload returns `0`, a store
/// failure returns `0`, a per-row insert failure is logged and the loop
/// continues.
fn project_traces_into_economy(harness_dir: &Path, body: &Value) -> usize {
    let cwd = project_dir();
    let session = session_id();
    let session_opt = if session == "unknown" || session.is_empty() {
        None
    } else {
        Some(session)
    };
    let ctx = IngestContext {
        project_path: cwd.clone(),
        session_id: session_opt,
    };

    // `sources::otel::ingest` takes the OTLP JSON as a string; re-stringify the
    // already-parsed `Value` to keep the adapter API surface narrow (and stay
    // tolerant if a future shape change wants a different representation).
    let payload = match serde_json::to_string(body) {
        Ok(s) => s,
        Err(e) => {
            canary(
                harness_dir,
                &serde_json::json!({
                    "ts": crate::util::now_iso8601(),
                    "level": "warn", "route": "/v1/traces",
                    "msg": "reserialize failed", "err": e.to_string(),
                }),
            );
            return 0;
        }
    };
    let records = match otel_source::ingest(&payload, &ctx) {
        Ok(v) => v,
        Err(e) => {
            canary(
                harness_dir,
                &serde_json::json!({
                    "ts": crate::util::now_iso8601(),
                    "level": "warn", "route": "/v1/traces",
                    "msg": "sources::otel::ingest failed", "err": e.to_string(),
                }),
            );
            return 0;
        }
    };
    if records.is_empty() {
        return 0;
    }
    let store = match TelemetryStore::for_project(&cwd) {
        Ok(s) => s,
        Err(e) => {
            canary(
                harness_dir,
                &serde_json::json!({
                    "ts": crate::util::now_iso8601(),
                    "level": "warn", "route": "/v1/traces",
                    "msg": "TelemetryStore::for_project failed", "err": e.to_string(),
                }),
            );
            return 0;
        }
    };
    let mut written = 0;
    for rec in records {
        let run = stamp_attribution(&store, span_to_run(rec));
        match telemetry_writer::record_run(store.conn(), &run) {
            Ok(()) => written += 1,
            Err(e) => canary(
                harness_dir,
                &serde_json::json!({
                    "ts": crate::util::now_iso8601(),
                    "level": "warn", "route": "/v1/traces",
                    "msg": "record_run failed", "err": e.to_string(),
                }),
            ),
        }
    }
    written
}

/// Map an ingested [`SpanRecord`] onto a [`RunUsage`], pulling the W4
/// `tool_use_id` attribution key out of the lenient `extra` map.
fn span_to_run(rec: SpanRecord) -> RunUsage {
    let tool_use_id = rec
        .extra
        .get("tool_use_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    RunUsage {
        trace_id: None,
        span_id: rec.span_id,
        parent_span_id: None,
        name: None,
        started_at: None,
        ended_at: None,
        duration_ms: None,
        attributes: None,
        spec: rec.spec,
        phase: rec.phase,
        model: rec.model,
        input_tokens: rec.input_tokens,
        output_tokens: rec.output_tokens,
        cache_read_input_tokens: rec.cache_read_input_tokens,
        cache_creation_input_tokens: rec.cache_creation_input_tokens,
        cost_usd_micros: rec.cost_usd_micros,
        is_error: rec.is_error,
        project_path: None,
        ts_iso: Some(rec.ts),
        session_id: rec.session_id,
        wave_id: None,
        tool_use_id,
        agent_id: None,
    }
}

/// Stamp `spec` / `wave_id` / `agent_id` onto `run` from the write-time
/// attribution map.
///
/// Two-tier lookup, both keyed on the run's `session_id` (a run without a
/// session can never be attributed and is returned unchanged):
///
/// 1. **Primary** — `(session_id, tool_use_id)`, when the run carries a
///    `tool_use_id`. This is the exact stamp the `agent.start` hook recorded.
/// 2. **Session-only fallback** — when the run has no `tool_use_id`, or the
///    primary lookup found no row. Picks the most-recent stamp for the session
///    at or before the run's timestamp, restoring the read-time CTE's
///    session-level fallback (a span without a `tool_use_id` used to match the
///    most-recent `agent.start` for the same session with `ts <= span.ts`).
///    Without this, any span that arrives without a `tool_use_id` would be left
///    permanently unattributed — the regression this repairs.
///
/// `before_ts` is derived from the run's `ts_iso` (ms-epoch); an unparseable /
/// absent timestamp drops the time bound so the fallback still recovers the
/// session's most-recent stamp. Fail-open: a lookup error leaves the run
/// unstamped.
fn stamp_attribution(store: &TelemetryStore, mut run: RunUsage) -> RunUsage {
    let Some(session) = run.session_id.clone() else {
        return run;
    };

    // Tier 1: exact (session, tool_use_id) stamp.
    if let Some(tool_use_id) = run.tool_use_id.as_deref() {
        if let Ok(Some(attr)) =
            telemetry_writer::lookup_attribution(store.conn(), &session, tool_use_id)
        {
            run.spec = attr.spec;
            run.wave_id = attr.wave_id;
            run.agent_id = attr.agent_id;
            return run;
        }
    }

    // Tier 2: session-only fallback (most-recent stamp at or before the span).
    let before_ts = run
        .ts_iso
        .as_deref()
        .and_then(mustard_core::projection::parse_iso_millis);
    if let Ok(Some(attr)) =
        telemetry_writer::lookup_attribution_by_session(store.conn(), &session, before_ts)
    {
        run.spec = attr.spec;
        run.wave_id = attr.wave_id;
        run.agent_id = attr.agent_id;
    }
    run
}

/// Dispatch `mustard-rt run otel-collector`. Runs until a shutdown signal or a
/// fatal bind/store failure; this function does not return on the happy path
/// (it `exit`s).
pub fn run() {
    let claude = claude_dir();
    let harness_dir = claude.join(".harness");
    let port = resolve_port();

    // Open the store FIRST — a store failure is fatal (the parent respawns).
    let store = match Store::open(&claude) {
        Ok(s) => s,
        Err(e) => {
            canary(
                &harness_dir,
                &serde_json::json!({
                    "ts": crate::util::now_iso8601(),
                    "level": "fatal", "msg": "store init failed", "err": e.to_string(),
                }),
            );
            std::process::exit(1);
        }
    };

    let addr = format!("127.0.0.1:{port}");
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            // EADDRINUSE (another collector bound) or any bind failure.
            canary(
                &harness_dir,
                &serde_json::json!({
                    "ts": crate::util::now_iso8601(),
                    "level": "fatal", "msg": "bind failed",
                    "port": port, "err": e.to_string(),
                }),
            );
            std::process::exit(1);
        }
    };

    // One-time cleanup: purge the dead `usage_totals` rows written before the
    // ingestion filter existed (every metric outside `CONSUMED_METRICS`).
    // Fail-open and idempotent — once the filter is in place this deletes
    // nothing on subsequent startups.
    let _ = store.purge_unconsumed_metrics();

    // The flag is the test seam (see module docs); in production the process
    // is terminated by the OS, so it stays `false` for the binary's lifetime.
    let shutdown = Arc::new(AtomicBool::new(false));
    canary(
        &harness_dir,
        &serde_json::json!({
            "ts": crate::util::now_iso8601(),
            "level": "info", "msg": "collector listening",
            "host": "127.0.0.1", "port": port, "pid": std::process::id(),
        }),
    );

    serve_loop(&server, &store, &harness_dir, &shutdown);
    std::process::exit(0);
}

/// The accept loop. Extracted so a test can drive it against a real bound
/// `tiny_http` server on an ephemeral port.
fn serve_loop(server: &Server, store: &Store, harness_dir: &Path, shutdown: &Arc<AtomicBool>) {
    while !shutdown.load(Ordering::SeqCst) {
        // `recv` blocks; the harness terminates the process on SIGTERM, so a
        // graceful drain only matters for the test seam, which flips the flag
        // and then issues one final request to unblock this `recv`.
        let Ok(request) = server.recv() else { break };
        handle_one(request, store, harness_dir);
    }
}

/// Handle a single request: route, parse, project, respond.
fn handle_one(mut request: tiny_http::Request, store: &Store, harness_dir: &Path) {
    let method = request.method().clone();
    let route = request.url().split('?').next().unwrap_or("").to_string();

    // GET /healthz — liveness probe.
    if method == Method::Get && route == "/healthz" {
        let _ = request.respond(Response::from_string("ok"));
        return;
    }
    // Only the three POST routes are projected; everything else is 404.
    // `/v1/traces` lands span-level token usage as `run_usage` rows in
    // telemetry.db via the telemetry writer (Wave 2 — telemetry-separation).
    if method != Method::Post
        || (route != "/v1/metrics" && route != "/v1/logs" && route != "/v1/traces")
    {
        let _ = request.respond(Response::from_string("not found").with_status_code(404));
        return;
    }

    let t0 = crate::util::now_millis();
    let mut buf = String::new();
    if request.as_reader().read_to_string(&mut buf).is_err() {
        canary(
            harness_dir,
            &serde_json::json!({
                "ts": crate::util::now_iso8601(),
                "level": "error", "route": route, "msg": "body read failed",
            }),
        );
        let _ = request.respond(Response::from_string("bad request").with_status_code(400));
        return;
    }

    let body: Value = match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(e) => {
            canary(
                harness_dir,
                &serde_json::json!({
                    "ts": crate::util::now_iso8601(),
                    "level": "error", "route": route,
                    "msg": "parse failed",
                    "err": e.to_string().chars().take(200).collect::<String>(),
                }),
            );
            let _ = request.respond(Response::from_string("bad request").with_status_code(400));
            return;
        }
    };

    let now_ms = i64::try_from(crate::util::now_millis()).unwrap_or(i64::MAX);
    let count = project_into_store(store, harness_dir, &route, &body, now_ms);
    let latency = crate::util::now_millis().saturating_sub(t0);
    canary(
        harness_dir,
        &serde_json::json!({
            "ts": crate::util::now_iso8601(),
            "route": route, "count": count, "latency_ms": latency,
        }),
    );

    // OTLP success envelope — an empty `partialSuccess` means "all accepted".
    let resp = Response::from_string(r#"{"partialSuccess":{}}"#)
        .with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap_or_else(|()| {
                    // An unparseable static header is impossible; degrade to a
                    // bare 200 rather than panic.
                    tiny_http::Header::from_bytes(&b"X"[..], &b"Y"[..])
                        .unwrap_or_else(|()|  unreachable!("static header"))
                }),
        );
    let _ = request.respond(resp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpStream;

    /// Spawn the accept loop on an ephemeral port in a background thread.
    /// Returns the bound port and the shutdown flag.
    fn spawn_server(tmp: &Path) -> (u16, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let harness = tmp.join(".harness");
        std::fs::create_dir_all(&harness).unwrap();
        let store = Store::open_at(&harness.join("mustard.db")).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || {
            serve_loop(&server, &store, &harness, &flag);
        });
        (port, shutdown, handle)
    }

    /// Minimal blocking HTTP request — avoids pulling in a client crate.
    fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        let status = resp
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let body = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    #[test]
    fn healthz_returns_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let (port, shutdown, handle) = spawn_server(tmp.path());
        let (status, body) = http(port, "GET", "/healthz", "");
        assert_eq!(status, 200);
        assert_eq!(body, "ok");
        shutdown.store(true, Ordering::SeqCst);
        let _ = http(port, "GET", "/healthz", ""); // unblock recv
        handle.join().unwrap();
    }

    #[test]
    fn unknown_route_is_404() {
        let tmp = tempfile::tempdir().unwrap();
        let (port, shutdown, handle) = spawn_server(tmp.path());
        let (status, _) = http(port, "GET", "/nope", "");
        assert_eq!(status, 404);
        shutdown.store(true, Ordering::SeqCst);
        let _ = http(port, "GET", "/healthz", "");
        handle.join().unwrap();
    }

    #[test]
    fn metrics_post_projects_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let (port, shutdown, handle) = spawn_server(tmp.path());
        let payload = r#"{"resourceMetrics":[{"scopeMetrics":[{"metrics":[
            {"name":"claude_code.token.usage","sum":{"dataPoints":[
              {"timeUnixNano":"90000000000","asInt":"50","attributes":[
                {"key":"session.id","value":{"stringValue":"s1"}}]}]}}]}]}]}"#;
        let (status, body) = http(port, "POST", "/v1/metrics", payload);
        assert_eq!(status, 200);
        assert!(body.contains("partialSuccess"));
        shutdown.store(true, Ordering::SeqCst);
        let _ = http(port, "GET", "/healthz", "");
        handle.join().unwrap();
        // The row landed in the store.
        let store = Store::open_at(&tmp.path().join(".harness").join("mustard.db")).unwrap();
        assert_eq!(store.otel_row_count().unwrap(), 1);
    }

    #[test]
    fn bad_json_returns_400_and_does_not_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let (port, shutdown, handle) = spawn_server(tmp.path());
        let (status, _) = http(port, "POST", "/v1/metrics", "{not json");
        assert_eq!(status, 400);
        // Server is still alive afterward.
        let (status2, _) = http(port, "GET", "/healthz", "");
        assert_eq!(status2, 200);
        shutdown.store(true, Ordering::SeqCst);
        let _ = http(port, "GET", "/healthz", "");
        handle.join().unwrap();
    }
}
