//! `mustard-rt run diagnose-otel` — a port of `scripts/diagnose-otel.js`.
//!
//! End-to-end health check for the Mustard ↔ Claude Code OTEL pipeline.
//! Sections: `env`, `collector`, `health`, `data`, `subtractions`.
//!
//! Fail-open: a missing OTEL config or a dead collector do NOT exit non-zero —
//! it is a diagnose tool, not a gate. Only `--expect-rows-after` can fail
//! (exit `1`); every other path exits `0`.
//!
//! ## `subtractions` and the SQLite `events` table
//!
//! The JS `checkSubtractions` queried a SQLite `events` table
//! (`event = 'mustard.subtraction.applied'`). That table is part of the shared
//! `EventStore` schema (`event-store.ts` `SCHEMA_SQL`), and the b3 harness now
//! writes the live event bus to `events.jsonl`, not that table. The port keeps
//! the SQLite query faithful — when the table is empty or absent the section
//! simply reports `0` / a reason, fail-open. See the agent return note.

use super::store::{claude_dir, SampleRow, Store};
use serde_json::{json, Value};
use std::net::TcpStream;
use std::time::Duration;

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
            .map(|o| {
                let out = String::from_utf8_lossy(&o.stdout);
                out.contains(&pid.to_string())
            })
            .unwrap_or(false)
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
    let raw = match std::fs::read_to_string(&pid_file) {
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

/// `[data]` — `claude_code_otel` row count, last bucket and a 5-row sample.
fn check_data(store: Option<&Store>) -> Value {
    let Some(store) = store else {
        return json!({ "ok": false, "reason": "event-store unavailable", "rows": 0, "sample": [] });
    };
    let count = match store.otel_row_count() {
        Ok(n) => n,
        Err(e) => return json!({ "ok": false, "reason": e.to_string(), "rows": 0, "sample": [] }),
    };
    let last = store.otel_last_bucket().unwrap_or(None);
    let sample = store.otel_sample().unwrap_or_default();
    json!({
        "ok": true,
        "rows": count,
        "lastBucketMs": last,
        "sample": sample.iter().map(sample_json).collect::<Vec<_>>(),
    })
}

/// Serialise one [`SampleRow`] to the JS diagnose sample shape.
fn sample_json(r: &SampleRow) -> Value {
    json!({
        "ts_bucket": r.ts_bucket,
        "metric": r.metric,
        "session_id": r.session_id,
        "model": r.model,
        "token_type": r.token_type,
        "sum": r.sum,
        "count": r.count,
    })
}

/// `[subtractions]` — `mustard.subtraction.applied` events in the last 24 h.
fn check_subtractions(store: Option<&Store>) -> Value {
    let Some(store) = store else {
        return json!({ "ok": false, "reason": "event-store unavailable", "count": 0 });
    };
    // 24 h ago, ISO-8601 — the `events.ts` column is a text timestamp.
    let since_ms = i64::try_from(crate::util::now_millis())
        .unwrap_or(i64::MAX)
        .saturating_sub(24 * 60 * 60 * 1000);
    let since_iso = iso_from_ms(since_ms);
    match store.subtractions_since(&since_iso) {
        Ok(n) => json!({ "ok": true, "count": n }),
        Err(e) => json!({ "ok": false, "reason": e.to_string(), "count": 0 }),
    }
}

/// Format an ms-epoch instant as an ISO-8601 UTC string (the `events.ts`
/// shape). Reuses the civil-date math already proven in `util::now_iso8601`.
pub(crate) fn iso_from_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

/// Render the human-readable report (everything but `--json`).
fn render_human(report: &Value) -> String {
    let mut out = String::from("=== Mustard OTEL Diagnose ===\n\n[env]\n");
    if let Some(status) = report["env"]["status"].as_object() {
        for (k, v) in status {
            let shown = v.as_str().map_or_else(|| "(unset)".to_string(), str::to_string);
            out.push_str(&format!("  {k} = {shown}\n"));
        }
    }
    let env_ok = if report["env"]["ok"].as_bool() == Some(true) { "OK" } else { "INCOMPLETE" };
    out.push_str(&format!("  status: {env_ok}\n\n[collector]\n"));
    out.push_str(&format!(
        "  pid: {}\n  alive: {}\n",
        report["collector"]["pid"].as_u64().map_or_else(|| "(none)".to_string(), |p| p.to_string()),
        report["collector"]["ok"].as_bool().unwrap_or(false),
    ));
    if report["collector"]["ok"].as_bool() != Some(true) {
        if let Some(r) = report["collector"]["reason"].as_str() {
            out.push_str(&format!("  reason: {r}\n"));
        }
    }
    out.push_str("\n[health]\n");
    out.push_str(&format!(
        "  status: {}\n  ok: {}\n",
        report["health"]["status"].as_u64().map_or_else(|| "(unreachable)".to_string(), |s| s.to_string()),
        report["health"]["ok"].as_bool().unwrap_or(false),
    ));
    out.push_str("\n[data]\n");
    if report["data"]["ok"].as_bool() == Some(true) {
        out.push_str(&format!("  rows: {}\n", report["data"]["rows"].as_i64().unwrap_or(0)));
        let last = report["data"]["lastBucketMs"]
            .as_i64()
            .map_or_else(|| "(none)".to_string(), iso_from_ms);
        out.push_str(&format!("  last bucket: {last}\n"));
        if let Some(sample) = report["data"]["sample"].as_array() {
            if !sample.is_empty() {
                out.push_str("  sample (latest 5):\n");
                for r in sample {
                    out.push_str(&format!(
                        "    - {} {} session={} model={} type={} sum={} count={}\n",
                        iso_from_ms(r["ts_bucket"].as_i64().unwrap_or(0)),
                        r["metric"].as_str().unwrap_or("-"),
                        r["session_id"].as_str().unwrap_or("-"),
                        r["model"].as_str().unwrap_or("-"),
                        r["token_type"].as_str().unwrap_or("-"),
                        r["sum"].as_f64().unwrap_or(0.0),
                        r["count"].as_i64().unwrap_or(0),
                    ));
                }
            }
        }
    } else if let Some(r) = report["data"]["reason"].as_str() {
        out.push_str(&format!("  reason: {r}\n"));
    }
    out.push_str("\n[subtractions]\n");
    if report["subtractions"]["ok"].as_bool() == Some(true) {
        out.push_str(&format!(
            "  applied (last 24h): {}\n",
            report["subtractions"]["count"].as_i64().unwrap_or(0),
        ));
    } else if let Some(r) = report["subtractions"]["reason"].as_str() {
        out.push_str(&format!("  reason: {r}\n"));
    }
    out
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

    // The store may be absent (no harness db yet) — every check tolerates that.
    let store = Store::open(&claude_dir()).ok();
    let rows_before = store.as_ref().and_then(|s| s.otel_row_count().ok());

    if let Some(ms) = opts.expect_rows_after_ms {
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(ms));
        }
    }

    let data = check_data(store.as_ref());
    let mut report = json!({
        "env": check_env(),
        "collector": check_collector(),
        "health": check_health(port),
        "data": data.clone(),
        "subtractions": check_subtractions(store.as_ref()),
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
        assert_eq!(iso_from_ms(0), "1970-01-01T00:00:00.000Z");
        // 2026-05-19T00:00:00.000Z — a fixed reference instant.
        let ms = 1_779_148_800_000;
        assert_eq!(iso_from_ms(ms), "2026-05-19T00:00:00.000Z");
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
    fn check_data_without_store_is_fail_open() {
        let v = check_data(None);
        assert_eq!(v["ok"].as_bool(), Some(false));
        assert_eq!(v["rows"].as_i64(), Some(0));
    }

    #[test]
    fn check_data_with_store_counts_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        // Bring up the schema via a throwaway file store, then reuse the path.
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(&tmp.path().join("mustard.db")).unwrap();
        store
            .upsert_metric(&super::super::store::MetricRow {
                ts_bucket: 60_000,
                metric: "m".to_string(),
                session_id: None,
                model: None,
                token_type: None,
                sum: 1.0,
                attrs: "{}".to_string(),
            })
            .unwrap();
        let v = check_data(Some(&store));
        assert_eq!(v["ok"].as_bool(), Some(true));
        assert_eq!(v["rows"].as_i64(), Some(1));
        drop(conn);
    }

    #[test]
    fn check_subtractions_without_events_table_fails_open() {
        // A store with only the otel schema has no `events` table; the query
        // must surface a reason, not panic, and report count 0.
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(&tmp.path().join("mustard.db")).unwrap();
        let v = check_subtractions(Some(&store));
        assert_eq!(v["ok"].as_bool(), Some(false));
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
