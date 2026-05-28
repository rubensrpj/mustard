//! `mustard-rt run otel-collector` — a port of `scripts/otel-collector.js`.
//!
//! A local OTLP/JSON receiver for Claude Code native telemetry. Binds a
//! `tiny_http` server to `127.0.0.1` (loopback only — never the network) on
//! `MUSTARD_OTEL_PORT` (default 4318). Metrics and logs are appended to the
//! per-spec NDJSON event log as `pipeline.telemetry.metric` records; traces
//! land span-level token usage as `pipeline.telemetry.run` records keyed off
//! the request attribution stamped at write time.
//!
//! Routes:
//!   - `POST /v1/metrics` — OTLP MetricsService (`resourceMetrics[]`).
//!   - `POST /v1/logs`    — OTLP LogsService    (`resourceLogs[]`).
//!   - `POST /v1/traces`  — OTLP TracesService  (`resourceSpans[]`).
//!   - `GET  /healthz`    — liveness probe.
//!
//! Lifecycle: the harness spawns the collector as a long-lived child and
//! stops it with `SIGTERM` (Unix) or process termination (Windows). No
//! portable std API installs a `SIGTERM` handler without `unsafe` — forbidden
//! crate-wide — so the collector relies on the OS default action (terminate).
//! The accept loop additionally honours an in-process shutdown flag, which is
//! the seam the inline tests drive to drain cleanly.
//!
//! Fail-open contract: a parse error returns `400` but never crashes the
//! server — losing a few datapoints beats taking down the harness pipeline.
//! A canary log line (`.canary.log`) records each request and each error.
//!
//! ## Persistence (post-W5A)
//!
//! There is no SQLite store. Each accepted datapoint is serialised into the
//! per-spec NDJSON event log via
//! [`crate::shared::events::writer_ndjson::write_event_with_ts`]. The ingestion
//! filter ([`CONSUMED_METRICS`], a module-local constant) still drops every
//! metric the dashboard does not read, so the NDJSON sink only carries the
//! handful that matter.

use super::project::project_metrics;
use super::{claude_dir, MetricRow};
use mustard_core::domain::economy::{sources::otel as otel_source, sources::IngestContext};
use mustard_core::io::fs;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tiny_http::{Method, Response, Server};

use crate::shared::context::{project_dir, session_id};
use crate::shared::events::writer_ndjson;

/// Default OTLP/HTTP port — the OpenTelemetry convention, and the value the
/// generated `settings.json` points `OTEL_EXPORTER_OTLP_ENDPOINT` at.
const DEFAULT_PORT: u16 = 4318;

/// NDJSON event kinds the collector emits.
const KIND_METRIC: &str = "pipeline.telemetry.metric";
const KIND_RUN: &str = "pipeline.telemetry.run";

/// The only `usage_totals` metric names the dashboard ever reads.
///
/// Was a re-export from `mustard_core::telemetry::CONSUMED_METRICS`; moved
/// here as a module-local constant in W8A-1 (no-sqlite Wave 8) when the
/// SQLite telemetry crate-side module was deleted. The collector itself is
/// the only consumer of this filter — colocating the list keeps it within
/// the single responsibility that uses it.
const CONSUMED_METRICS: &[&str] = &[
    "claude_code.cost.usage",
    "claude_code.session.count",
    "claude_code.active_time.total",
    "claude_code.token.usage",
];

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

/// Resolve the session slug for the event-writer's `session_slug` argument.
/// `context::session_id` returns `"unknown"` for the unattached case; the
/// event-writer needs a non-empty slug to compose its output path.
fn session_slug() -> String {
    let sid = session_id();
    if sid.is_empty() || sid == "unknown" {
        "otel-unattached".to_string()
    } else {
        sid
    }
}

/// Serialize a `MetricRow` to the `pipeline.telemetry.metric` payload shape.
fn metric_payload(row: &MetricRow) -> Value {
    json!({
        "ts_bucket": row.ts_bucket,
        "metric": row.metric,
        "session_id": row.session_id,
        "model": row.model,
        "token_type": row.token_type,
        "sum": row.sum,
        "attrs": row.attrs,
    })
}

/// Project a parsed OTLP body for `route` into the NDJSON sink. Returns the
/// number of records written. A per-row write failure is logged to canary but
/// does not abort the batch (fail-open).
fn project_into_ndjson(harness_dir: &Path, route: &str, body: &Value, now_ms: i64) -> usize {
    if route == "/v1/metrics" {
        return write_metrics(harness_dir, body, now_ms);
    }
    if route == "/v1/traces" {
        return write_traces(harness_dir, body);
    }
    // /v1/logs and any other route: nothing to persist. Claude Code log bodies
    // are not in `CONSUMED_METRICS`, so they would never reach the dashboard;
    // the collector accepts the payload (HTTP 200) but writes 0.
    0
}

/// Write one NDJSON record per consumed metric datapoint.
fn write_metrics(harness_dir: &Path, body: &Value, now_ms: i64) -> usize {
    let project = PathBuf::from(project_dir());
    let slug = session_slug();
    let mut written = 0usize;
    for row in project_metrics(body, now_ms) {
        // Ingestion filter: only persist the metrics the dashboard reads.
        if !CONSUMED_METRICS.contains(&row.metric.as_str()) {
            continue;
        }
        let payload = metric_payload(&row);
        // ts_override is the row's bucket so cross-session aggregation can
        // re-bucket without re-clocking.
        let ts = ms_to_iso(row.ts_bucket);
        let outcome = writer_ndjson::write_event_with_ts(
            &project,
            None,           // spec — collector is cross-spec
            None,           // wave_role
            &slug,
            KIND_METRIC,
            KIND_METRIC,
            None,           // wave
            row.session_id.as_deref(),
            Some("otel-collector"),
            None,           // parent_id
            &payload,
            Some(&ts),
        );
        if outcome.is_some() {
            written += 1;
        } else {
            canary(harness_dir, &json!({
                "ts": crate::util::now_iso8601(),
                "level": "warn",
                "route": "/v1/metrics",
                "msg": "ndjson write failed",
                "metric": row.metric,
            }));
        }
    }
    written
}

/// Translate OTLP/JSON `traces` into [`mustard_core::domain::economy::SpanRecord`]s via
/// the W1 ingest adapter, then write one `pipeline.telemetry.run` record per
/// span to the NDJSON sink.
///
/// Attribution is carried within the SpanRecord (`spec`, `session_id`,
/// `tool_use_id` via `extra`); the dashboard reads NDJSON cross-spec and
/// reconciles attribution off those fields directly — there is no separate
/// `lookup_attribution` step now that the SQLite attribution map is gone.
fn write_traces(harness_dir: &Path, body: &Value) -> usize {
    let cwd = project_dir();
    let session = session_id();
    let session_opt = if session == "unknown" || session.is_empty() {
        None
    } else {
        Some(session)
    };
    let ctx = IngestContext {
        project_path: cwd.clone(),
        session_id: session_opt.clone(),
    };

    // `sources::otel::ingest` takes the OTLP JSON as a string; re-stringify the
    // already-parsed `Value` to keep the adapter API surface narrow.
    let payload = match serde_json::to_string(body) {
        Ok(s) => s,
        Err(e) => {
            canary(harness_dir, &json!({
                "ts": crate::util::now_iso8601(),
                "level": "warn", "route": "/v1/traces",
                "msg": "reserialize failed", "err": e.to_string(),
            }));
            return 0;
        }
    };
    let records = match otel_source::ingest(&payload, &ctx) {
        Ok(v) => v,
        Err(e) => {
            canary(harness_dir, &json!({
                "ts": crate::util::now_iso8601(),
                "level": "warn", "route": "/v1/traces",
                "msg": "sources::otel::ingest failed", "err": e.to_string(),
            }));
            return 0;
        }
    };
    if records.is_empty() {
        return 0;
    }

    let project = PathBuf::from(cwd);
    let slug = session_slug();
    let mut written = 0usize;
    for rec in records {
        // SpanRecord is serde-serializable; encode the entire record as the
        // payload so downstream readers (dashboard, MCP) get the full shape.
        let payload = match serde_json::to_value(&rec) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ts = rec.ts.clone();
        let outcome = writer_ndjson::write_event_with_ts(
            &project,
            rec.spec.as_deref(),
            None,
            &slug,
            KIND_RUN,
            KIND_RUN,
            None,
            rec.session_id.as_deref(),
            Some("otel-collector"),
            None,
            &payload,
            Some(&ts),
        );
        if outcome.is_some() {
            written += 1;
        } else {
            canary(harness_dir, &json!({
                "ts": crate::util::now_iso8601(),
                "level": "warn", "route": "/v1/traces",
                "msg": "ndjson write failed",
                "span_id": rec.span_id,
            }));
        }
    }
    written
}

/// Format an ms-epoch instant as an ISO-8601 UTC string. Mirrors the small
/// civil-date helper in `diagnose.rs` so the collector and the diagnose face
/// agree on the timestamp shape they emit/read.
fn ms_to_iso(ms: i64) -> String {
    super::diagnose::iso_from_ms(ms)
}

/// Dispatch `mustard-rt run otel-collector`. Runs until a shutdown signal or
/// a fatal bind failure; this function does not return on the happy path
/// (it `exit`s).
pub fn run() {
    let claude = claude_dir();
    let harness_dir = claude.join(".harness");
    // Ensure `.harness/` exists for the canary log; fail-silent if not.
    let _ = fs::create_dir_all(&harness_dir);
    let port = resolve_port();

    let addr = format!("127.0.0.1:{port}");
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            // EADDRINUSE (another collector bound) or any bind failure.
            canary(&harness_dir, &json!({
                "ts": crate::util::now_iso8601(),
                "level": "fatal", "msg": "bind failed",
                "port": port, "err": e.to_string(),
            }));
            std::process::exit(1);
        }
    };

    let shutdown = Arc::new(AtomicBool::new(false));
    canary(&harness_dir, &json!({
        "ts": crate::util::now_iso8601(),
        "level": "info", "msg": "collector listening",
        "host": "127.0.0.1", "port": port, "pid": std::process::id(),
    }));

    serve_loop(&server, &harness_dir, &shutdown);
    std::process::exit(0);
}

/// The accept loop. Extracted so a test can drive it against a real bound
/// `tiny_http` server on an ephemeral port.
fn serve_loop(server: &Server, harness_dir: &Path, shutdown: &Arc<AtomicBool>) {
    while !shutdown.load(Ordering::SeqCst) {
        // `recv` blocks; the harness terminates the process on SIGTERM, so a
        // graceful drain only matters for the test seam, which flips the flag
        // and then issues one final request to unblock this `recv`.
        let Ok(request) = server.recv() else { break };
        handle_one(request, harness_dir);
    }
}

/// Handle a single request: route, parse, project, respond.
fn handle_one(mut request: tiny_http::Request, harness_dir: &Path) {
    let method = request.method().clone();
    let route = request.url().split('?').next().unwrap_or("").to_string();

    // GET /healthz — liveness probe.
    if method == Method::Get && route == "/healthz" {
        let _ = request.respond(Response::from_string("ok"));
        return;
    }
    // Only the three POST routes are projected; everything else is 404.
    if method != Method::Post
        || (route != "/v1/metrics" && route != "/v1/logs" && route != "/v1/traces")
    {
        let _ = request.respond(Response::from_string("not found").with_status_code(404));
        return;
    }

    let t0 = crate::util::now_millis();
    let mut buf = String::new();
    if request.as_reader().read_to_string(&mut buf).is_err() {
        canary(harness_dir, &json!({
            "ts": crate::util::now_iso8601(),
            "level": "error", "route": route, "msg": "body read failed",
        }));
        let _ = request.respond(Response::from_string("bad request").with_status_code(400));
        return;
    }

    let body: Value = match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(e) => {
            canary(harness_dir, &json!({
                "ts": crate::util::now_iso8601(),
                "level": "error", "route": route,
                "msg": "parse failed",
                "err": e.to_string().chars().take(200).collect::<String>(),
            }));
            let _ = request.respond(Response::from_string("bad request").with_status_code(400));
            return;
        }
    };

    let now_ms = i64::try_from(crate::util::now_millis()).unwrap_or(i64::MAX);
    let count = project_into_ndjson(harness_dir, &route, &body, now_ms);
    let latency = crate::util::now_millis().saturating_sub(t0);
    canary(harness_dir, &json!({
        "ts": crate::util::now_iso8601(),
        "route": route, "count": count, "latency_ms": latency,
    }));

    // OTLP success envelope — an empty `partialSuccess` means "all accepted".
    let resp = Response::from_string(r#"{"partialSuccess":{}}"#)
        .with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap_or_else(|()| {
                    // An unparseable static header is impossible; degrade to a
                    // bare 200 rather than panic.
                    tiny_http::Header::from_bytes(&b"X"[..], &b"Y"[..])
                        .unwrap_or_else(|()| unreachable!("static header"))
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
        let harness = tmp.join(".claude").join(".harness");
        std::fs::create_dir_all(&harness).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        let harness_clone = harness.clone();
        let handle = std::thread::spawn(move || {
            serve_loop(&server, &harness_clone, &flag);
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
