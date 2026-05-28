//! `mustard-rt run verify-emit` — a port of `scripts/verify-emit.js`.
//!
//! Confirms that a named event was emitted to the per-spec NDJSON `.events/`
//! directory within a recent time window. Used by the orchestrator after an
//! "emit-and-continue" step to catch a silently-failed emit instead of
//! trusting the emitter's fail-open semantics blindly.
//!
//! Scans the replayed log backward — the most-recent match wins an early
//! exit. Exit `0` on a match, `1` on no match within the window, `2` on bad
//! arguments (the JS contract).

use crate::shared::context;
use mustard_core::domain::model::event::HarnessEvent;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::ClaudePaths;
use serde_json::Value;

/// Parse a duration string (`30s`, `1m`, `500ms`, `2h`, or a bare ms integer)
/// into milliseconds. Defaults to `30_000` on an empty/invalid value, exactly
/// like the JS `parseDuration`.
fn parse_duration(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() {
        return 30_000;
    }
    let parse_prefix = |suffix: &str| -> Option<i64> {
        s.strip_suffix(suffix)
            .and_then(|n| n.parse::<i64>().ok())
    };
    if let Some(n) = parse_prefix("ms") {
        return n;
    }
    if let Some(n) = parse_prefix("s") {
        return n * 1000;
    }
    if let Some(n) = parse_prefix("m") {
        return n * 60_000;
    }
    if let Some(n) = parse_prefix("h") {
        return n * 3_600_000;
    }
    s.parse::<i64>().unwrap_or(30_000)
}

/// `verify-emit` argument bundle.
struct Args {
    event: String,
    since_ms: i64,
    payload_key: Option<String>,
    payload_value: Option<String>,
    spec: Option<String>,
    quiet: bool,
}

/// The outcome of a verification scan — maps directly to a process exit code.
#[derive(Debug, PartialEq, Eq)]
enum VerifyOutcome {
    /// A matching event was found `age_secs` seconds ago.
    Found { age_secs: i64 },
    /// No matching event within the window.
    Miss,
}

/// Scan the replayed events (oldest-first) for a match within the window.
///
/// `now_ms` is injected so the scan is deterministic under test. Returns
/// [`VerifyOutcome::Found`] for the first (newest) match, else `Miss`.
fn scan(events: &[HarnessEvent], args: &Args, now_ms: i64) -> VerifyOutcome {
    let cutoff = now_ms - args.since_ms;
    // Replay is oldest-first; scan in reverse so the newest match wins.
    for ev in events.iter().rev() {
        if ev.event != args.event {
            continue;
        }
        if let Some(spec) = &args.spec {
            if ev.spec.as_deref() != Some(spec.as_str()) {
                continue;
            }
        }
        if ev.ts.is_empty() {
            continue;
        }
        let Some(ts_ms) = mustard_core::time::parse_iso_millis(&ev.ts) else {
            continue;
        };
        if ts_ms < cutoff {
            // Scanning backward — anything earlier is also out of window.
            break;
        }
        if let Some(key) = &args.payload_key {
            let Some(payload_val) = ev.payload.get(key) else {
                continue;
            };
            if let Some(want) = &args.payload_value {
                let got = match payload_val {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if &got != want {
                    continue;
                }
            }
        }
        let age_secs = (now_ms - ts_ms) / 1000;
        return VerifyOutcome::Found { age_secs };
    }
    VerifyOutcome::Miss
}

/// Dispatch `mustard-rt run verify-emit`.
pub fn run(
    event: Option<&str>,
    since: Option<&str>,
    payload_key: Option<&str>,
    payload_value: Option<&str>,
    spec: Option<&str>,
    quiet: bool,
) {
    let Some(event) = event.filter(|e| !e.is_empty()) else {
        eprintln!("error: --event required");
        std::process::exit(2);
    };
    let args = Args {
        event: event.to_string(),
        since_ms: since.map_or(30_000, parse_duration),
        payload_key: payload_key.map(str::to_string),
        payload_value: payload_value.map(str::to_string),
        spec: spec.map(str::to_string),
        quiet,
    };

    // Read events exclusively from per-spec NDJSON `.events/` directories.
    let project = context::project_dir();
    let specs_root = match ClaudePaths::for_project(std::path::Path::new(&project)) {
        Ok(paths) => paths.spec_dir(),
        Err(_) => {
            if !args.quiet {
                eprintln!("[verify-emit] project path rejected by ClaudePaths guard");
            }
            std::process::exit(1);
        }
    };
    let mut events: Vec<HarnessEvent> = Vec::new();
    if let Some(spec) = args.spec.as_deref() {
        let dir = specs_root.join(spec).join(".events");
        events.extend(read_harness_events_from_ndjson_dir(&dir));
    } else if let Ok(entries) = std::fs::read_dir(&specs_root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let dir = entry.path().join(".events");
                events.extend(read_harness_events_from_ndjson_dir(&dir));
            }
        }
    }
    events.sort_by(|a, b| a.ts.cmp(&b.ts));

    let now_ms = mustard_core::time::now_unix_millis() as u128 as i64;
    match scan(&events, &args, now_ms) {
        VerifyOutcome::Found { age_secs } => {
            if !args.quiet {
                let spec_note = args
                    .spec
                    .as_ref()
                    .map(|s| format!(" (spec={s})"))
                    .unwrap_or_default();
                println!("[verify-emit] OK: {} {age_secs}s ago{spec_note}", args.event);
            }
            std::process::exit(0);
        }
        VerifyOutcome::Miss => {
            if !args.quiet {
                let win_sec = args.since_ms / 1000;
                let spec_note = args
                    .spec
                    .as_ref()
                    .map(|s| format!(" (spec={s})"))
                    .unwrap_or_default();
                eprintln!(
                    "[verify-emit] MISS: {} not found in last {win_sec}s{spec_note}",
                    args.event
                );
            }
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use serde_json::json;

    fn args(event: &str) -> Args {
        Args {
            event: event.to_string(),
            since_ms: 30_000,
            payload_key: None,
            payload_value: None,
            spec: None,
            quiet: true,
        }
    }

    /// Build a `HarnessEvent` for scan tests.
    fn ev(event: &str, ts: &str, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "s".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: event.to_string(),
            payload,
            spec: None,
        }
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30s"), 30_000);
        assert_eq!(parse_duration("1m"), 60_000);
        assert_eq!(parse_duration("500ms"), 500);
        assert_eq!(parse_duration("2h"), 7_200_000);
        assert_eq!(parse_duration("750"), 750);
        assert_eq!(parse_duration(""), 30_000);
        assert_eq!(parse_duration("garbage"), 30_000);
    }

    #[test]
    fn scan_finds_recent_event() {
        let events = vec![ev("close-gate.check", "2026-05-19T00:00:00.000Z", json!({}))];
        let now = mustard_core::time::parse_iso_millis("2026-05-19T00:00:05.000Z").unwrap();
        let r = scan(&events, &args("close-gate.check"), now);
        assert_eq!(r, VerifyOutcome::Found { age_secs: 5 });
    }

    #[test]
    fn scan_misses_old_event() {
        let events = vec![ev("close-gate.check", "2026-05-19T00:00:00.000Z", json!({}))];
        // 10 minutes later, default 30s window.
        let now = mustard_core::time::parse_iso_millis("2026-05-19T00:10:00.000Z").unwrap();
        assert_eq!(scan(&events, &args("close-gate.check"), now), VerifyOutcome::Miss);
    }

    #[test]
    fn scan_respects_payload_filter() {
        let events = vec![
            ev("qa", "2026-05-19T00:00:00.000Z", json!({ "result": "fail" })),
            ev("qa", "2026-05-19T00:00:01.000Z", json!({ "result": "pass" })),
        ];
        let now = mustard_core::time::parse_iso_millis("2026-05-19T00:00:02.000Z").unwrap();
        let mut a = args("qa");
        a.payload_key = Some("result".to_string());
        a.payload_value = Some("pass".to_string());
        assert_eq!(scan(&events, &a, now), VerifyOutcome::Found { age_secs: 1 });
        a.payload_value = Some("skip".to_string());
        assert_eq!(scan(&events, &a, now), VerifyOutcome::Miss);
    }
}
