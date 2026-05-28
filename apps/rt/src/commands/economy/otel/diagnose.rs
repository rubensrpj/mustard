//! `mustard-rt run diagnose-otel` — a port of `scripts/diagnose-otel.js`.
//!
//! End-to-end health check for the Mustard ↔ Claude Code OTEL pipeline.
//! Sections: `env`, `collector`, `health`, `data`, `subtractions`.
//!
//! Fail-open: a missing OTEL config or a dead collector do NOT exit non-zero —
//! it is a diagnose tool, not a gate. Only `--expect-rows-after` can fail
//! (exit `1`); every other path exits `0`.
//!
//! ## Persistence (post-W5A)
//!
//! The diagnose face reads NDJSON. Two event kinds drive the report:
//!
//! - `pipeline.telemetry.metric` — one record per accepted OTLP metric
//!   datapoint, written by [`super::collector`].
//! - `pipeline.telemetry.subtraction` — emitted by `mustard.subtraction.applied`
//!   producers. `check_subtractions` walks the per-spec NDJSON sink to count
//!   matches in the last 24 h.
//!
//! Cross-spec walking is done by [`mustard_core::EventReader::stream`] over
//! every `.events/*.ndjson` file under `<project>/.claude/spec/`.

use super::{claude_dir, SampleRow};
use mustard_core::io::fs;
use mustard_core::{Event, EventReader};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// NDJSON event kinds the diagnose face reads back.
const KIND_METRIC: &str = "pipeline.telemetry.metric";
const KIND_SUBTRACTION: &str = "pipeline.telemetry.subtraction";

/// Parsed `diagnose-otel` arguments.
struct Opts {
    json: bool,
    /// `--expect-rows-after` wait, in milliseconds, when given.
    expect_rows_after_ms: Option<u64>,
}

/// Parse `--expect-rows-after Xs|Xms` into milliseconds, mirroring the JS
/// regex `^(\d+)\s*(s|ms)?$` (a bare number is seconds).
fn parse_expect(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if let Some(ms) = raw.strip_suffix("ms") {
        return ms.trim().parse::<u64>().ok();
    }
    if let Some(s) = raw.strip_suffix('s') {
        return s.trim().parse::<u64>().ok().map(|n| n * 1000);
    }
    raw.parse::<u64>().ok().map(|n| n * 1000)
}

/// `[env]` — required telemetry environment variables.
fn check_env() -> Value {
    let required = [
        "CLAUDE_CODE_ENABLE_TELEMETRY",
        "OTEL_METRICS_EXPORTER",
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "MUSTARD_HARNESS_DUAL_EMIT",
    ];
    let mut status = serde_json::Map::new();
    let mut ok = true;
    for key in required {
        match std::env::var(key) {
            Ok(v) if !v.is_empty() => {
                status.insert(key.to_string(), Value::String(v));
            }
            _ => {
                status.insert(key.to_string(), Value::Null);
                ok = false;
            }
        }
    }
    json!({ "ok": ok, "status": status })
}

/// True when a process with `pid` is alive. Portable liveness probe — the JS
/// used `process.kill(pid, 0)`.
fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        // `tasklist` filtered by PID prints the image name when it is alive,
        // and "INFO: No tasks..." otherwise.
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .is_ok_and(|o| {
                let out = String::from_utf8_lossy(&o.stdout);
                out.contains(&pid.to_string())
            })
    }
    #[cfg(not(windows))]
    {
        // `kill -0` is a no-op signal that succeeds only for a live process
        // the caller may signal.
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// `[collector]` — PID file present and the process alive.
fn check_collector() -> Value {
    let pid_file = claude_dir().join(".harness").join(".otel-collector.pid");
    if !pid_file.exists() {
        return json!({ "ok": false, "reason": "no pid file", "pid": Value::Null });
    }
    let raw = match fs::read_to_string(&pid_file) {
        Ok(s) => s,
        Err(e) => {
            return json!({
                "ok": false, "reason": format!("pid read failed: {e}"), "pid": Value::Null
            });
        }
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return json!({ "ok": false, "reason": "invalid pid", "pid": Value::Null });
    };
    if pid == 0 {
        return json!({ "ok": false, "reason": "invalid pid", "pid": 0 });
    }
    if pid_alive(pid) {
        json!({ "ok": true, "pid": pid })
    } else {
        json!({ "ok": false, "reason": "process dead", "pid": pid })
    }
}

/// `[health]` — `GET /healthz` returns `200`.
///
/// Uses a raw TCP request to avoid an HTTP-client dependency. A connect or
/// read failure is reported, never thrown.
fn check_health(port: u16) -> Value {
    use std::io::{Read, Write};
    let stream = TcpStream::connect_timeout(
        &match format!("127.0.0.1:{port}").parse() {
            Ok(a) => a,
            Err(e) => return json!({ "ok": false, "status": Value::Null, "reason": e.to_string() }),
        },
        Duration::from_millis(1500),
    );
    let mut stream = match stream {
        Ok(s) => s,
        Err(e) => return json!({ "ok": false, "status": Value::Null, "reason": e.to_string() }),
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let req = "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    if let Err(e) = stream.write_all(req.as_bytes()) {
        return json!({ "ok": false, "status": Value::Null, "reason": e.to_string() });
    }
    let mut resp = String::new();
    if let Err(e) = stream.read_to_string(&mut resp) {
        return json!({ "ok": false, "status": Value::Null, "reason": e.to_string() });
    }
    let status = resp
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok());
    json!({ "ok": status == Some(200), "status": status })
}

/// Walk every `.events/*.ndjson` under `<project>/.claude/spec/` and the
/// cross-spec `.claude/.session/` channel, returning the concatenation of the
/// streams. Fail-open: an unreadable file is silently skipped.
fn read_all_events(claude_root: &Path) -> Vec<Event> {
    let mut out = Vec::new();
    let candidate_roots = [
        claude_root.join("spec"),
        claude_root.join(".session"),
    ];
    for root in candidate_roots {
        if !root.exists() {
            continue;
        }
        collect_ndjson_under(&root, &mut out);
    }
    out
}

/// Recursively collect `.ndjson` files under `dir` and append their parsed
/// events to `out`. Bounded depth: we only descend into directories that
/// can plausibly contain `.events/` subdirs (skip large irrelevant trees).
fn collect_ndjson_under(dir: &Path, out: &mut Vec<Event>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ndjson_under(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ndjson") {
            out.extend(EventReader::stream(&path));
        }
    }
}

/// Project the metric events into the diagnose `[data]` section: row count,
/// last bucket and a 5-row sample.
fn build_metric_view(events: &[Event]) -> (i64, Option<i64>, Vec<SampleRow>) {
    let metrics: Vec<&Event> = events.iter().filter(|e| e.kind == KIND_METRIC).collect();
    let count = metrics.len() as i64;
    let last = metrics
        .iter()
        .filter_map(|e| e.payload.get("ts_bucket").and_then(Value::as_i64))
        .max();
    let mut by_ts: Vec<&Event> = metrics.clone();
    // Sort newest-first by ts_bucket; fall back to insertion order when absent.
    by_ts.sort_by(|a, b| {
        let ta = a.payload.get("ts_bucket").and_then(Value::as_i64).unwrap_or(0);
        let tb = b.payload.get("ts_bucket").and_then(Value::as_i64).unwrap_or(0);
        tb.cmp(&ta)
    });
    let sample: Vec<SampleRow> = by_ts.into_iter().take(5).map(|e| {
        let p = &e.payload;
        SampleRow {
            metric: p.get("metric").and_then(Value::as_str).unwrap_or("").to_string(),
            session_id: p.get("session_id").and_then(Value::as_str).map(str::to_string),
            model: p.get("model").and_then(Value::as_str).map(str::to_string),
            sum: p.get("sum").and_then(Value::as_f64).unwrap_or(0.0),
            updated_at: p.get("ts_bucket").and_then(Value::as_i64),
        }
    }).collect();
    (count, last, sample)
}

/// `[data]` — `pipeline.telemetry.metric` row count, last bucket and a 5-row
/// sample. Replaces the legacy `usage_totals` SQLite query with an NDJSON
/// walk; the section is fail-open (returns `ok: true` with zero rows when no
/// events are present).
fn check_data(claude_root: &Path) -> Value {
    let events = read_all_events(claude_root);
    let (count, last, sample) = build_metric_view(&events);
    json!({
        "ok": true,
        "rows": count,
        "lastBucketMs": last,
        "sample": sample.iter().map(sample_json).collect::<Vec<_>>(),
    })
}

/// Serialise one [`SampleRow`] to the diagnose sample shape.
fn sample_json(r: &SampleRow) -> Value {
    json!({
        "metric": r.metric,
        "session_id": r.session_id,
        "model": r.model,
        "sum": r.sum,
        "updated_at": r.updated_at,
    })
}

/// `[subtractions]` — `pipeline.telemetry.subtraction` events in the last 24 h.
/// Fail-open: missing channel returns `ok: true, count: 0`.
fn check_subtractions(claude_root: &Path) -> Value {
    let now_ms = i64::try_from(mustard_core::time::now_unix_millis() as u128).unwrap_or(i64::MAX);
    let since_ms = now_ms.saturating_sub(24 * 60 * 60 * 1000);
    let events = read_all_events(claude_root);
    let count = events
        .iter()
        .filter(|e| e.kind == KIND_SUBTRACTION)
        .filter(|e| event_ts_ms(e).map_or(true, |ts| ts >= since_ms))
        .count() as i64;
    json!({ "ok": true, "count": count })
}

/// Pull an ms-epoch timestamp off an event, looking first at a top-level `ts`
/// (ISO-8601) and then at `payload.ts_bucket`. Returns `None` when neither is
/// usable.
fn event_ts_ms(e: &Event) -> Option<i64> {
    if let Some(iso) = e.raw.get("ts").and_then(Value::as_str) {
        if let Some(ms) = mustard_core::time::parse_iso_millis(iso) {
            return Some(ms);
        }
    }
    e.payload.get("ts_bucket").and_then(Value::as_i64)
}


/// Render the human-readable report (everything but `--json`).
fn render_human(report: &Value) -> String {
    let mut out = String::from("=== Mustard OTEL Diagnose ===\n\n[env]\n");
    if let Some(status) = report["env"]["status"].as_object() {
        for (k, v) in status {
            let shown = v.as_str().map_or_else(|| "(unset)".to_string(), str::to_string);
            let _ = writeln!(out, "  {k} = {shown}");
        }
    }
    let env_ok = if report["env"]["ok"].as_bool() == Some(true) { "OK" } else { "INCOMPLETE" };
    let _ = write!(out, "  status: {env_ok}\n\n[collector]\n");
    let _ = write!(out, "  pid: {}\n  alive: {}\n",
        report["collector"]["pid"].as_u64().map_or_else(|| "(none)".to_string(), |p| p.to_string()),
        report["collector"]["ok"].as_bool().unwrap_or(false));
    if report["collector"]["ok"].as_bool() != Some(true) {
        if let Some(r) = report["collector"]["reason"].as_str() {
            let _ = writeln!(out, "  reason: {r}");
        }
    }
    out.push_str("\n[health]\n");
    let _ = write!(out, "  status: {}\n  ok: {}\n",
        report["health"]["status"].as_u64().map_or_else(|| "(unreachable)".to_string(), |s| s.to_string()),
        report["health"]["ok"].as_bool().unwrap_or(false));
    out.push_str("\n[data]\n");
    if report["data"]["ok"].as_bool() == Some(true) {
        let _ = writeln!(out, "  rows: {}", report["data"]["rows"].as_i64().unwrap_or(0));
        let last = report["data"]["lastBucketMs"]
            .as_i64()
            .map_or_else(|| "(none)".to_string(), mustard_core::time::millis_to_iso);
        let _ = writeln!(out, "  last bucket: {last}");
        if let Some(sample) = report["data"]["sample"].as_array() {
            if !sample.is_empty() {
                out.push_str("  sample (latest 5):\n");
                for r in sample {
                    let _ = writeln!(out, "    - {} {} session={} model={} sum={}",
                        r["updated_at"].as_i64().map_or_else(|| "(none)".to_string(), mustard_core::time::millis_to_iso),
                        r["metric"].as_str().unwrap_or("-"),
                        r["session_id"].as_str().unwrap_or("-"),
                        r["model"].as_str().unwrap_or("-"),
                        r["sum"].as_f64().unwrap_or(0.0));
                }
            }
        }
    } else if let Some(r) = report["data"]["reason"].as_str() {
        let _ = writeln!(out, "  reason: {r}");
    }
    out.push_str("\n[subtractions]\n");
    if report["subtractions"]["ok"].as_bool() == Some(true) {
        let _ = writeln!(out, "  applied (last 24h): {}",
            report["subtractions"]["count"].as_i64().unwrap_or(0));
    } else if let Some(r) = report["subtractions"]["reason"].as_str() {
        let _ = writeln!(out, "  reason: {r}");
    }
    out
}

/// Resolve the `.claude` directory for the running process. Mirrors
/// `super::claude_dir` but stays addressable for testing via a path override
/// when a future caller wants to inject a tempdir.
fn claude_root() -> PathBuf {
    claude_dir()
}

/// Dispatch `mustard-rt run diagnose-otel`.
pub fn run(json_flag: bool, expect_rows_after: Option<&str>) {
    let opts = Opts {
        json: json_flag,
        expect_rows_after_ms: expect_rows_after.and_then(parse_expect),
    };
    let port = std::env::var("MUSTARD_OTEL_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(4318);

    let root = claude_root();
    let rows_before = {
        let events = read_all_events(&root);
        let (count, _, _) = build_metric_view(&events);
        Some(count)
    };

    if let Some(ms) = opts.expect_rows_after_ms {
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(ms));
        }
    }

    let data = check_data(&root);
    let mut report = json!({
        "env": check_env(),
        "collector": check_collector(),
        "health": check_health(port),
        "data": data.clone(),
        "subtractions": check_subtractions(&root),
    });

    // `--expect-rows-after` assertion — the only non-zero exit path.
    if let Some(ms) = opts.expect_rows_after_ms.filter(|m| *m > 0) {
        let rows_after = data["ok"].as_bool().unwrap_or(false).then(|| data["rows"].as_i64()).flatten();
        let passed = matches!((rows_before, rows_after), (Some(b), Some(a)) if a > b);
        report["expectRowsAfter"] = json!({
            "waitMs": ms, "before": rows_before, "after": rows_after, "passed": passed,
        });
        if opts.json {
            println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        } else {
            println!("{}", render_human(&report));
            println!("\n[expect-rows-after]");
            println!("  before: {}", rows_before.map_or_else(|| "null".into(), |n| n.to_string()));
            println!("  after:  {}", rows_after.map_or_else(|| "null".into(), |n| n.to_string()));
            println!("  passed: {passed}");
        }
        std::process::exit(i32::from(!passed));
    }

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    } else {
        println!("{}", render_human(&report));
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_expect_units() {
        assert_eq!(parse_expect("5s"), Some(5000));
        assert_eq!(parse_expect("250ms"), Some(250));
        assert_eq!(parse_expect("3"), Some(3000));
        assert_eq!(parse_expect("garbage"), None);
        assert_eq!(parse_expect(""), None);
    }

    #[test]
    fn iso_from_ms_matches_epoch() {
        assert_eq!(mustard_core::time::millis_to_iso(0), "1970-01-01T00:00:00.000Z");
        // 2026-05-19T00:00:00.000Z — a fixed reference instant.
        let ms = 1_779_148_800_000;
        assert_eq!(mustard_core::time::millis_to_iso(ms), "2026-05-19T00:00:00.000Z");
    }

    #[test]
    fn check_collector_missing_pid_file() {
        // No project dir override → cwd; the pid file almost certainly
        // does not exist here. The check must report, never panic.
        let v = check_collector();
        assert!(v["ok"].as_bool() == Some(false));
        assert!(v["reason"].is_string());
    }

    #[test]
    fn check_data_empty_root_is_ok_with_zero_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let v = check_data(tmp.path());
        assert_eq!(v["ok"].as_bool(), Some(true));
        assert_eq!(v["rows"].as_i64(), Some(0));
    }

    #[test]
    fn check_data_with_ndjson_counts_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path();
        let events_dir = claude.join("spec").join("demo").join(".events");
        std::fs::create_dir_all(&events_dir).unwrap();
        let line = json!({
            "kind": KIND_METRIC,
            "ts": "2026-05-27T10:00:00.000Z",
            "payload": {
                "ts_bucket": 1_779_148_800_000i64,
                "metric": "claude_code.token.usage",
                "session_id": "s1",
                "model": "claude-opus-4-7",
                "sum": 50.0
            }
        }).to_string();
        std::fs::write(events_dir.join("test.ndjson"), format!("{line}\n")).unwrap();

        let v = check_data(claude);
        assert_eq!(v["ok"].as_bool(), Some(true));
        assert_eq!(v["rows"].as_i64(), Some(1));
        let sample = v["sample"].as_array().unwrap();
        assert_eq!(sample.len(), 1);
        assert_eq!(sample[0]["metric"].as_str(), Some("claude_code.token.usage"));
    }

    #[test]
    fn check_subtractions_empty_root_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let v = check_subtractions(tmp.path());
        assert_eq!(v["ok"].as_bool(), Some(true));
        assert_eq!(v["count"].as_i64(), Some(0));
    }

    #[test]
    fn render_human_has_all_sections() {
        let report = json!({
            "env": { "ok": false, "status": { "CLAUDE_CODE_ENABLE_TELEMETRY": Value::Null } },
            "collector": { "ok": false, "reason": "no pid file", "pid": Value::Null },
            "health": { "ok": false, "status": Value::Null },
            "data": { "ok": true, "rows": 0, "lastBucketMs": Value::Null, "sample": [] },
            "subtractions": { "ok": true, "count": 0 },
        });
        let out = render_human(&report);
        for section in ["[env]", "[collector]", "[health]", "[data]", "[subtractions]"] {
            assert!(out.contains(section), "missing {section}");
        }
    }
}
