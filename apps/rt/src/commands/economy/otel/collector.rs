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
use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tiny_http::{Method, Response, Server};

use crate::shared::context::{project_dir, session_id};
use crate::shared::events::writer_ndjson;
use crate::util::home_dir;

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

/// The last-resort sink slug for a metric whose origin session cannot be
/// resolved to a project at all.
const UNATTACHED_SLUG: &str = "otel-unattached";

/// Where a metric datapoint should be written: the project root that owns the
/// `.session/<slug>/.events/` directory, plus the session slug under it.
///
/// Three cases, in priority order (see [`metric_target`]):
/// 1. **In-project** — the origin session ran in *this* collector's project
///    (its `.session/<id>/` dir already exists on disk). Target the collector
///    project under the session's own slug.
/// 2. **Cross-project** — the origin session ran in *another* project, resolved
///    via the global Claude Code transcript store. Target that foreign project
///    root under the session's own slug, eliminating the cross-project leak.
/// 3. **Unresolvable** — no session dir here and no transcript anywhere. Stay
///    in the collector project under `otel-unattached`.
struct MetricTarget {
    /// The project root whose `.claude/.session/` tree receives the write.
    project: PathBuf,
    /// The `.session/<slug>/` segment — the origin session id, or
    /// `otel-unattached` when unresolvable.
    slug: String,
}

/// Resolve the `(project_root, slug)` a metric datapoint should be written
/// under, given the collector's own `project` root and the global Claude Code
/// transcript store at `projects_root` (`~/.claude/projects/`).
///
/// The OTLP metric payload carries the *originating* `session.id` (projected
/// into `MetricRow::session_id`). The collector is a single shared process, so
/// that id is unrelated to the collector's own session — we must route by it:
///
/// 1. When the origin session ran in *this* project, `<project>/.claude/.session/<id>/`
///    already exists (its hooks materialised it) → attach there.
/// 2. Otherwise the session may belong to *another* project. Every Claude Code
///    session writes a transcript at `~/.claude/projects/<encoded-cwd>/<id>.jsonl`
///    whose lines carry a lossless absolute `cwd` — [`foreign_project_for_session`]
///    finds that file and reads the cwd, giving the foreign project root. The
///    metric then lands under *that* project's `.session/<id>/`, not the
///    collector's. This is the cross-project leak fix.
/// 3. A missing/`"unknown"` id, or a session with no resolvable transcript,
///    falls back to `otel-unattached` under the collector's own project.
fn metric_target(project: &Path, projects_root: Option<&Path>, row_session_id: Option<&str>) -> MetricTarget {
    let Some(sid) = row_session_id.filter(|s| !s.is_empty() && *s != "unknown") else {
        return MetricTarget { project: project.to_path_buf(), slug: UNATTACHED_SLUG.to_string() };
    };
    // (1) In-project: the session's own dir already exists here.
    if session_dir_exists(project, sid) {
        return MetricTarget { project: project.to_path_buf(), slug: sid.to_string() };
    }
    // (2) Cross-project: resolve the originating project via the transcript store.
    if let Some(root) = projects_root.and_then(|pr| foreign_project_for_session(pr, sid)) {
        return MetricTarget { project: root, slug: sid.to_string() };
    }
    // (3) Unresolvable: last-resort sink under the collector's own project.
    MetricTarget { project: project.to_path_buf(), slug: UNATTACHED_SLUG.to_string() }
}

/// The global Claude Code transcript store, `~/.claude/projects/`, or `None`
/// when the home directory cannot be resolved. Reuses [`home_dir`] +
/// [`ClaudePaths`] (the same composition `transcript_watcher` and
/// `session_cleanup` use) so the path encoding cannot drift.
fn global_projects_root() -> Option<PathBuf> {
    let home = home_dir()?;
    let paths = ClaudePaths::for_project(&home).ok()?;
    Some(paths.claude_dir().join("projects"))
}

/// Resolve the originating project root for a foreign `session_id` by scanning
/// the global transcript store `projects_root` (`~/.claude/projects/`).
///
/// Each Claude Code session writes `<projects_root>/<encoded-cwd>/<id>.jsonl`.
/// The directory name is a *lossy* encoding of the cwd (`/`, `\`, `:` all
/// collapse to `-`, see [`crate::util::encode_cwd`]) so it cannot be decoded
/// back to a path — but every transcript line carries a lossless absolute
/// `cwd` field, which is the authoritative project root. We find the `<id>.jsonl`
/// file under any encoded-cwd subdir and read the first `cwd` from it.
///
/// Fail-open: a missing store, an unreadable transcript, or a transcript with
/// no `cwd` returns `None` (the caller degrades to `otel-unattached`). The
/// resolved root is validated by [`ClaudePaths::for_project`] so a malformed
/// `cwd` (e.g. one terminating in `.claude`) is rejected rather than routed.
fn foreign_project_for_session(projects_root: &Path, session_id: &str) -> Option<PathBuf> {
    let target = format!("{session_id}.jsonl");
    let entries = fs::read_dir(projects_root).ok()?;
    for dir in entries.into_iter().filter(|e| e.is_dir) {
        let candidate = dir.path.join(&target);
        if !fs::exists(&candidate) {
            continue;
        }
        if let Some(cwd) = transcript_cwd(&candidate) {
            let root = PathBuf::from(cwd);
            // Validate via the same I1 guard every writer uses; a rejected
            // root degrades to the unattached fallback rather than producing
            // a `.claude/.claude/` path.
            if ClaudePaths::for_project(&root).is_ok() {
                return Some(root);
            }
        }
    }
    None
}

/// Read the first non-empty `cwd` string from a Claude Code transcript JSONL.
/// Returns `None` when the file cannot be read or no line carries a `cwd`.
fn transcript_cwd(transcript: &Path) -> Option<String> {
    let contents = fs::read_to_string(transcript).ok()?;
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
            if !cwd.is_empty() {
                return Some(cwd.to_string());
            }
        }
    }
    None
}

/// `true` when `<project>/.claude/.session/<session_id>/` exists — i.e. the
/// origin session ran in this project and its hooks already materialised the
/// session directory. Mirrors the `.session/<slug>/` composition the event
/// writer uses (see [`writer_ndjson::event_dir`]); the `.session/` directory is
/// not exposed via `ClaudePaths` (it is the session sidebar's own concern), so
/// the path is composed manually here. Fail-open: an I1-rejected project root
/// degrades to the unchecked compose so the probe never panics.
fn session_dir_exists(project: &Path, session_id: &str) -> bool {
    let claude = ClaudePaths::for_project(project)
        .map(|p| p.claude_dir())
        .unwrap_or_else(|_| ClaudePaths::compose_unchecked(project).claude_dir());
    claude.join(".session").join(session_id).is_dir()
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

/// Write one NDJSON record per consumed metric datapoint. Reads the collector's
/// project root and the global transcript store from the process environment,
/// then delegates to [`write_metrics_into`].
fn write_metrics(harness_dir: &Path, body: &Value, now_ms: i64) -> usize {
    write_metrics_into(
        &PathBuf::from(project_dir()),
        global_projects_root().as_deref(),
        harness_dir,
        body,
        now_ms,
    )
}

/// Project a metrics body into the NDJSON sink. Resolves each datapoint's
/// *originating* project via [`metric_target`] and writes under that project's
/// `.session/<slug>/` tree — in-project, cross-project (via `projects_root`),
/// or `otel-unattached` (under the collector's own `project`).
///
/// Split out from [`write_metrics`] so both the env-derived collector cwd and
/// the global transcript store are injectable — the AC routing tests drive this
/// directly against `tempdir`s with no `~/.claude` dependency.
fn write_metrics_into(
    project: &Path,
    projects_root: Option<&Path>,
    harness_dir: &Path,
    body: &Value,
    now_ms: i64,
) -> usize {
    let mut written = 0usize;
    for row in project_metrics(body, now_ms) {
        // Ingestion filter: only persist the metrics the dashboard reads.
        if !CONSUMED_METRICS.contains(&row.metric.as_str()) {
            continue;
        }
        let payload = metric_payload(&row);
        // Attribute the datapoint to its *originating* session, not the
        // collector's own env: the collector is one shared process serving
        // multiple projects, so its `session_id()` is unrelated to the metric.
        // Route to the originating project's `.session/<origin-id>/` — same
        // project (dir on disk) or a foreign one (resolved via the transcript
        // store); only a genuinely unresolvable id stays in `otel-unattached`.
        let target = metric_target(project, projects_root, row.session_id.as_deref());
        // ts_override is the row's bucket so cross-session aggregation can
        // re-bucket without re-clocking.
        let ts = mustard_core::time::millis_to_iso(row.ts_bucket);
        let outcome = writer_ndjson::write_event_with_ts(
            &target.project,
            None,           // spec — collector is cross-spec
            None,           // wave_role
            &target.slug,
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
                "ts": mustard_core::time::now_iso8601(),
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
                "ts": mustard_core::time::now_iso8601(),
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
                "ts": mustard_core::time::now_iso8601(),
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
                "ts": mustard_core::time::now_iso8601(),
                "level": "warn", "route": "/v1/traces",
                "msg": "ndjson write failed",
                "span_id": rec.span_id,
            }));
        }
    }
    written
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
                "ts": mustard_core::time::now_iso8601(),
                "level": "fatal", "msg": "bind failed",
                "port": port, "err": e.to_string(),
            }));
            std::process::exit(1);
        }
    };

    // Self-register the PID *after* a successful bind, so the file always names
    // the process that actually owns the port. The `SessionStart` hook spawns us
    // fully detached (`cmd /C start` — see `shared::proc::spawn_detached`) and so
    // cannot observe our real PID; authoring it here keeps the hook's idempotence
    // check working (it skips a respawn when this PID is alive). Best-effort.
    let pid_path = harness_dir.join(super::PID_FILENAME);
    let _ = fs::write_atomic(&pid_path, std::process::id().to_string().as_bytes());

    let shutdown = Arc::new(AtomicBool::new(false));
    canary(&harness_dir, &json!({
        "ts": mustard_core::time::now_iso8601(),
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

    let t0 = mustard_core::time::now_unix_millis() as u128;
    let mut buf = String::new();
    if request.as_reader().read_to_string(&mut buf).is_err() {
        canary(harness_dir, &json!({
            "ts": mustard_core::time::now_iso8601(),
            "level": "error", "route": route, "msg": "body read failed",
        }));
        let _ = request.respond(Response::from_string("bad request").with_status_code(400));
        return;
    }

    let body: Value = match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(e) => {
            canary(harness_dir, &json!({
                "ts": mustard_core::time::now_iso8601(),
                "level": "error", "route": route,
                "msg": "parse failed",
                "err": e.to_string().chars().take(200).collect::<String>(),
            }));
            let _ = request.respond(Response::from_string("bad request").with_status_code(400));
            return;
        }
    };

    let now_ms = i64::try_from(mustard_core::time::now_unix_millis() as u128).unwrap_or(i64::MAX);
    let count = project_into_ndjson(harness_dir, &route, &body, now_ms);
    let latency = (mustard_core::time::now_unix_millis() as u128).saturating_sub(t0);
    canary(harness_dir, &json!({
        "ts": mustard_core::time::now_iso8601(),
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

    // -----------------------------------------------------------------------
    // AC3 — metric attribution to the originating session.
    // -----------------------------------------------------------------------

    /// `metric_target` routes to the metric's own `session_id` under the
    /// collector's project when that session's `.session/<id>/` dir exists, and
    /// falls back to `otel-unattached` (same project) for an absent dir with no
    /// transcript, a `None`, or an `"unknown"` id. (`projects_root = None`
    /// disables cross-project resolution for this in-project unit.)
    #[test]
    fn metric_target_routes_to_existing_origin_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path();
        let sid = "fda6d733-a4a6-49bc-b00d-affec1182ca9";
        // The session's own hooks create this dir; the collector only probes it.
        std::fs::create_dir_all(project.join(".claude").join(".session").join(sid)).unwrap();

        let in_proj = metric_target(project, None, Some(sid));
        assert_eq!(in_proj.slug, sid);
        assert_eq!(in_proj.project, project);
        // A session that never ran here (no dir, no transcript) → last-resort.
        let miss = metric_target(project, None, Some("never-ran-here"));
        assert_eq!(miss.slug, UNATTACHED_SLUG);
        assert_eq!(miss.project, project);
        // Missing / unknown id → last-resort sink.
        assert_eq!(metric_target(project, None, None).slug, UNATTACHED_SLUG);
        assert_eq!(metric_target(project, None, Some("unknown")).slug, UNATTACHED_SLUG);
        assert_eq!(metric_target(project, None, Some("")).slug, UNATTACHED_SLUG);
    }

    /// End-to-end: given a `claude_code.token.usage` metric carrying
    /// `session_id` X and an existing session dir for X under project P, the
    /// writer targets `P/.claude/.session/X/.events/`, NOT `otel-unattached`.
    #[test]
    fn otel_metric_routed_to_origin_session() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path();
        let sid = "fda6d733-a4a6-49bc-b00d-affec1182ca9";
        // Materialise the origin session dir as that session's hooks would.
        let session_root = project.join(".claude").join(".session");
        std::fs::create_dir_all(session_root.join(sid)).unwrap();
        let harness = project.join(".claude").join(".harness");
        std::fs::create_dir_all(&harness).unwrap();

        // A token.usage metric body carrying the originating session id.
        let body = json!({
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "claude_code.token.usage",
                        "sum": { "dataPoints": [{
                            "timeUnixNano": "1779580800000000000",
                            "asInt": "100",
                            "attributes": [
                                { "key": "session.id", "value": { "stringValue": sid } },
                                { "key": "model", "value": { "stringValue": "opus" } },
                                { "key": "type", "value": { "stringValue": "input" } }
                            ]
                        }]}
                    }]
                }]
            }]
        });

        // Exercise the real production path with an injected project root,
        // bypassing the env-derived collector cwd. `projects_root = None` keeps
        // this an in-project case (no cross-project transcript scan).
        let written = write_metrics_into(project, None, &harness, &body, 0);
        assert!(written >= 1, "at least one metric datapoint written");

        // The metric must land under the origin session's .events/, never the
        // unattached sink.
        let origin_events = session_root.join(sid).join(".events");
        assert!(
            origin_events.is_dir() && std::fs::read_dir(&origin_events).unwrap().count() > 0,
            "metric must attach to the origin session's .events/ dir"
        );
        let unattached = session_root.join(UNATTACHED_SLUG);
        assert!(
            !unattached.exists(),
            "resolvable metric must NOT spill into otel-unattached"
        );
    }

    /// Build a metrics body carrying one `claude_code.token.usage` datapoint
    /// attributed to `sid`.
    fn token_usage_body(sid: &str) -> Value {
        json!({
            "resourceMetrics": [{ "scopeMetrics": [{ "metrics": [{
                "name": "claude_code.token.usage",
                "sum": { "dataPoints": [{
                    "asInt": "50",
                    "attributes": [
                        { "key": "session.id", "value": { "stringValue": sid } },
                        { "key": "type", "value": { "stringValue": "output" } }
                    ]
                }]}
            }]}]}]
        })
    }

    /// Write a minimal Claude Code transcript at
    /// `<projects_root>/<encoded-cwd>/<sid>.jsonl` carrying the lossless `cwd`,
    /// exactly as Claude Code does. Mirrors the real layout the resolver scans.
    fn seed_transcript(projects_root: &Path, origin_project: &Path, sid: &str) {
        let encoded = crate::util::encode_cwd(&origin_project.to_string_lossy());
        let dir = projects_root.join(encoded);
        std::fs::create_dir_all(&dir).unwrap();
        let line = json!({
            "type": "attachment",
            "sessionId": sid,
            "cwd": origin_project.to_string_lossy(),
        });
        std::fs::write(dir.join(format!("{sid}.jsonl")), format!("{line}\n")).unwrap();
    }

    /// AC-1 — a metric whose `session_id` resolves (via the global transcript
    /// store) to a *different* project than the collector's lands under THAT
    /// project's `.claude/.session/<id>/.events/`, not the collector's. This is
    /// the cross-project leak fix: the foreign session's tokens follow the
    /// session home, not the surviving collector.
    #[test]
    fn otel_metric_routed_cross_project() {
        let collector_tmp = tempfile::tempdir().unwrap();
        let origin_tmp = tempfile::tempdir().unwrap();
        let projects_tmp = tempfile::tempdir().unwrap();
        let collector = collector_tmp.path();
        let origin = origin_tmp.path();
        let projects_root = projects_tmp.path();

        let harness = collector.join(".claude").join(".harness");
        std::fs::create_dir_all(&harness).unwrap();

        // The foreign session ran in `origin` (NOT the collector's project), so
        // no `.session/<sid>/` dir exists under the collector. Its transcript
        // lives in the global store and carries the lossless cwd.
        let sid = "fda6d733-a4a6-49bc-b00d-affec1182ca9";
        seed_transcript(projects_root, origin, sid);

        let body = token_usage_body(sid);
        let written = write_metrics_into(collector, Some(projects_root), &harness, &body, 0);
        assert!(written >= 1, "at least one metric datapoint written");

        // The metric must land under the ORIGIN project's session dir.
        let origin_events = origin.join(".claude").join(".session").join(sid).join(".events");
        assert!(
            origin_events.is_dir() && std::fs::read_dir(&origin_events).unwrap().count() > 0,
            "cross-project metric must attach to the origin project's .events/"
        );
        // …and NOT leak into the collector's project at all.
        let collector_session = collector.join(".claude").join(".session");
        assert!(
            !collector_session.exists(),
            "cross-project metric must NOT leak into the collector's project"
        );
    }

    /// AC-2 — a metric whose `session_id` resolves nowhere (no session dir under
    /// the collector AND no transcript in the global store) stays in
    /// `otel-unattached` under the collector's own project. The in-project case
    /// is unaffected (covered by `otel_metric_routed_to_origin_session`).
    #[test]
    fn otel_metric_unresolvable_stays_unattached() {
        let collector_tmp = tempfile::tempdir().unwrap();
        let projects_tmp = tempfile::tempdir().unwrap();
        let collector = collector_tmp.path();
        // An empty global store — the session has no transcript anywhere.
        let projects_root = projects_tmp.path();
        std::fs::create_dir_all(projects_root).unwrap();

        let harness = collector.join(".claude").join(".harness");
        std::fs::create_dir_all(&harness).unwrap();

        let foreign = "00000000-1111-2222-3333-444444444444";
        let body = token_usage_body(foreign);
        let written = write_metrics_into(collector, Some(projects_root), &harness, &body, 0);
        assert!(written >= 1);

        let session_root = collector.join(".claude").join(".session");
        assert!(
            !session_root.join(foreign).exists(),
            "must not fabricate a foreign session dir"
        );
        assert!(
            session_root.join(UNATTACHED_SLUG).join(".events").is_dir(),
            "unresolvable metric lands in otel-unattached under the collector"
        );
    }

    /// The transcript resolver reads the lossless `cwd` from a seeded transcript
    /// and ignores a session id with no transcript at all.
    #[test]
    fn foreign_project_for_session_reads_cwd_from_transcript() {
        let origin_tmp = tempfile::tempdir().unwrap();
        let projects_tmp = tempfile::tempdir().unwrap();
        let origin = origin_tmp.path();
        let projects_root = projects_tmp.path();
        let sid = "abcdef01-2345-6789-abcd-ef0123456789";
        seed_transcript(projects_root, origin, sid);

        let resolved = foreign_project_for_session(projects_root, sid);
        assert_eq!(resolved.as_deref(), Some(origin));
        // A session with no transcript resolves to None.
        assert!(foreign_project_for_session(projects_root, "no-such-session").is_none());
    }
}
