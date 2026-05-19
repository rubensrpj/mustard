//! `mustard-rt run otel-collector` — a port of `scripts/otel-collector.js`.
//!
//! A local OTLP/JSON receiver for Claude Code native telemetry. Binds a
//! `tiny_http` server to `127.0.0.1` (loopback only — never the network) on
//! `MUSTARD_OTEL_PORT` (default 4318) and projects incoming payloads into the
//! `claude_code_otel` table of `.claude/.harness/mustard.db`.
//!
//! Routes:
//!   - `POST /v1/metrics` — OTLP MetricsService (`resourceMetrics[]`).
//!   - `POST /v1/logs`    — OTLP LogsService    (`resourceLogs[]`).
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

use super::project::{project_logs, project_metrics};
use super::store::{claude_dir, Store};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tiny_http::{Method, Response, Server};

/// Default OTLP/HTTP port — the OpenTelemetry convention, and the value the
/// generated `settings.json` points `OTEL_EXPORTER_OTLP_ENDPOINT` at.
const DEFAULT_PORT: u16 = 4318;

/// Append one JSON record to `.claude/.harness/.canary.log`. Fail-silent: a
/// logging failure must never affect request handling.
fn canary(harness_dir: &Path, record: &Value) {
    let _ = std::fs::create_dir_all(harness_dir);
    let line = format!("{record}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(harness_dir.join(".canary.log"))
    {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
    }
}

/// Resolve the listen port from `MUSTARD_OTEL_PORT`, defaulting to 4318.
fn resolve_port() -> u16 {
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
    } else {
        for row in project_logs(body, now_ms) {
            match store.upsert_log(&row) {
                Ok(()) => written += 1,
                Err(e) => canary(
                    harness_dir,
                    &serde_json::json!({
                        "ts": crate::util::now_iso8601(),
                        "level": "warn", "route": route,
                        "msg": "logRecord failed", "err": e.to_string(),
                    }),
                ),
            }
        }
    }
    written
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
    // Only the two POST routes are projected; everything else is 404.
    if method != Method::Post || (route != "/v1/metrics" && route != "/v1/logs") {
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
                        .unwrap_or_else(|_| unreachable!("static header"))
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
