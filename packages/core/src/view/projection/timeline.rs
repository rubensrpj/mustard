//! [`project_timeline`] — chronological event timeline for a single spec.
//!
//! ## W5 source switch
//!
//! Before W5 the timeline read every event from the SQLite `events` table.
//! W5 (`2026-05-24-mustard-unification`) moves the high-volume event stream
//! to per-spec NDJSON files under `.claude/spec/{name}/events/*.ndjson`
//! (written by `apps/rt/src/run/event_writer_ndjson.rs`). This module exposes
//! two folds with the same shape:
//!
//! - [`project_timeline`] keeps the in-memory fold contract (`&[HarnessEvent]
//!   → Vec<TimelineNode>`) the in-memory reader, tests, and any consumer
//!   that already holds events use.
//! - [`project_timeline_from_ndjson_dir`] reads the per-spec NDJSON directory
//!   directly — no SQLite involvement — and applies the same classification.
//!
//! Classification logic + label formatting are shared between the two paths.

use crate::domain::model::view::{Phase, TimelineKind, TimelineNode, TimeWindow};
use crate::domain::model::event::HarnessEvent;

use super::parse_iso_millis;

use serde::Deserialize;
use std::path::Path;

/// Fold events for `spec_name` into a chronologically-ordered list of
/// [`TimelineNode`]. `window` filters by `ts` — events outside the window
/// are dropped before classification.
///
/// The result is sorted oldest first so the dashboard can prepend "newest at
/// top" itself if needed — we never bake UI ordering into the data layer.
#[must_use]
pub fn project_timeline(
    spec_name: &str,
    events: &[HarnessEvent],
    window: TimeWindow,
) -> Vec<TimelineNode> {
    let cutoff_ms = window_cutoff_ms(window);

    let mut nodes: Vec<TimelineNode> = events
        .iter()
        .filter(|e| e.spec.as_deref() == Some(spec_name))
        .filter(|e| cutoff_ms.is_none_or(|c| parse_iso_millis(&e.ts).is_none_or(|ts| ts >= c)))
        .map(classify_harness_event)
        .collect();
    nodes.sort_by(|a, b| a.ts.cmp(&b.ts));
    nodes
}

/// Hydrate every `*.ndjson` file under `events_dir` (one level deep) into a
/// chronologically-ordered `Vec<HarnessEvent>`. The W5 dashboard reader uses
/// this alongside the SQLite `pipeline_events` slice to project tool/agent/qa
/// events that no longer live in the `mustard.db` event log.
///
/// Fail-open: a missing directory or unreadable file degrades to an empty
/// `Vec` rather than an error. Lines that fail JSON-decode are skipped.
///
/// The NDJSON shape is what `apps/rt/src/run/event_writer_ndjson.rs` writes;
/// missing fields fall back to harmless defaults (empty session id, hook
/// actor) since this slice is consumed by pure projections that only read
/// `event`, `payload`, `spec`, `ts`.
#[must_use]
pub fn read_harness_events_from_ndjson_dir(events_dir: &Path) -> Vec<HarnessEvent> {
    use crate::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};

    let Ok(entries) = crate::io::fs::read_dir(events_dir) else {
        return Vec::new();
    };
    let mut events: Vec<HarnessEvent> = Vec::new();
    for entry in entries {
        if entry.is_dir
            || !entry
                .file_name
                .rsplit('.')
                .next()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ndjson"))
        {
            continue;
        }
        let Ok(body) = crate::io::fs::read_to_string(&entry.path) else {
            continue;
        };
        for line in body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(record) = serde_json::from_str::<NdjsonRecord>(line) else {
                continue;
            };
            events.push(HarnessEvent {
                v: SCHEMA_VERSION,
                ts: record.ts,
                session_id: record.session_id.unwrap_or_default(),
                wave: record.wave.unwrap_or(0),
                actor: Actor {
                    kind: ActorKind::Hook,
                    id: None,
                    actor_type: None,
                },
                event: record.event,
                payload: record.payload,
                spec: record.spec,
            });
        }
    }
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    events
}

/// Read every `*.ndjson` file under `events_dir` (recursively) and project the
/// lines into a chronologically-sorted timeline. Returns an empty `Vec` if the
/// directory does not exist or holds no NDJSON files — the W5 contract is
/// fail-open: a missing directory means "no events yet", never an error.
///
/// `window` filters by `ts_ms` if present on each line; missing `ts_ms` falls
/// back to the ISO `ts` string parse. Lines that fail JSON-decode are skipped.
///
/// The NDJSON shape is the one written by
/// `apps/rt/src/run/event_writer_ndjson.rs`: `{ts, ts_ms, event, kind, spec,
/// wave, session_id, actor, parent_id, payload, tokens_in, tokens_out,
/// duration_ms}`.
#[must_use]
pub fn project_timeline_from_ndjson_dir(
    events_dir: &Path,
    window: TimeWindow,
) -> Vec<TimelineNode> {
    let cutoff_ms = window_cutoff_ms(window);
    let Ok(lines) = crate::io::fs::read_dir(events_dir) else {
        return Vec::new();
    };
    let mut nodes: Vec<TimelineNode> = Vec::new();
    for entry in lines {
        if entry.is_dir
            || !entry
                .file_name
                .rsplit('.')
                .next()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ndjson"))
        {
            continue;
        }
        let Ok(body) = crate::io::fs::read_to_string(&entry.path) else {
            continue;
        };
        for line in body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(record) = serde_json::from_str::<NdjsonRecord>(line) else {
                continue;
            };
            if let Some(cut) = cutoff_ms {
                let ts_ms = record
                    .ts_ms
                    .or_else(|| parse_iso_millis(&record.ts));
                if ts_ms.is_some_and(|ts| ts < cut) {
                    continue;
                }
            }
            nodes.push(classify_ndjson_record(&record));
        }
    }
    nodes.sort_by(|a, b| a.ts.cmp(&b.ts));
    nodes
}

/// Returns the lower bound (in epoch ms) for a window, or `None` for `All`.
fn window_cutoff_ms(window: TimeWindow) -> Option<i64> {
    match window {
        TimeWindow::All => None,
        TimeWindow::Today => Some(today_cutoff_ms()),
        TimeWindow::SevenDays => Some(today_cutoff_ms() - 7 * 86_400_000),
        TimeWindow::ThirtyDays => Some(today_cutoff_ms() - 30 * 86_400_000),
    }
}

/// Today, 00:00:00 UTC, in epoch milliseconds. Reads from `SystemTime`.
fn today_cutoff_ms() -> i64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    now_ms - (now_ms % 86_400_000)
}

/// Lenient deserialization of one NDJSON line. Every field is optional — the
/// writer ships a fixed shape but we never want a missing column to break the
/// reader (per [[core-lenient-serde-model]]).
#[derive(Debug, Deserialize)]
struct NdjsonRecord {
    #[serde(default)]
    ts: String,
    #[serde(default)]
    ts_ms: Option<i64>,
    #[serde(default)]
    event: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    spec: Option<String>,
    #[serde(default)]
    wave: Option<u32>,
    /// W8A-3 (no-sqlite Wave 8): populated so per-window event filters in
    /// `amend_finalize` can match a `HarnessEvent` back to its emitting
    /// session. The legacy reader defaulted this to the empty string, which
    /// silently broke session-scoped folds.
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    parent_id: Option<i64>,
    #[serde(default)]
    payload: serde_json::Value,
    #[serde(default)]
    tokens_in: Option<u64>,
    #[serde(default)]
    tokens_out: Option<u64>,
    #[serde(default)]
    duration_ms: Option<u64>,
}

/// Classify a stored harness event into a timeline node — the in-memory fold
/// path.
fn classify_harness_event(ev: &HarnessEvent) -> TimelineNode {
    let kind = TimelineKind::classify(&ev.event);
    let phase = ev
        .payload
        .get("to")
        .and_then(serde_json::Value::as_str)
        .and_then(Phase::parse)
        .or_else(|| {
            ev.payload
                .get("phase")
                .and_then(serde_json::Value::as_str)
                .and_then(Phase::parse)
        });
    let wave = ev
        .payload
        .get("wave")
        .and_then(serde_json::Value::as_u64)
        .and_then(|w| u32::try_from(w).ok());

    let label = build_label(kind, &ev.event, &ev.payload, phase, wave);
    let payload_summary = build_payload_summary(&ev.payload);

    let (input, output) = extract_io_pair(&ev.event, &ev.payload);
    TimelineNode {
        ts: ev.ts.clone(),
        kind,
        label,
        phase,
        wave,
        payload_summary,
        raw_event: ev.event.clone(),
        input,
        output,
        tokens_in: ev.payload.get("tokens_in").and_then(serde_json::Value::as_u64),
        tokens_out: ev.payload.get("tokens_out").and_then(serde_json::Value::as_u64),
        duration_ms: ev.payload.get("duration_ms").and_then(serde_json::Value::as_u64),
        parent_id: None,
    }
}

/// Classify a decoded NDJSON record into a timeline node — the per-spec
/// file-reading path. Render hints come pre-extracted off the wire.
fn classify_ndjson_record(rec: &NdjsonRecord) -> TimelineNode {
    // `kind` on the wire is a coarse render hint (`tool`, `phase`, …); the
    // strict mapping back to `TimelineKind` keeps coming from the event name
    // so a future writer that emits a new hint without updating both ends
    // still classifies correctly.
    let kind_enum = TimelineKind::classify(&rec.event);
    let _ = &rec.kind;
    let phase = rec
        .payload
        .get("to")
        .and_then(serde_json::Value::as_str)
        .and_then(Phase::parse)
        .or_else(|| {
            rec.payload
                .get("phase")
                .and_then(serde_json::Value::as_str)
                .and_then(Phase::parse)
        });
    let wave = rec.wave.or_else(|| {
        rec.payload
            .get("wave")
            .and_then(serde_json::Value::as_u64)
            .and_then(|w| u32::try_from(w).ok())
    });
    let label = build_label(kind_enum, &rec.event, &rec.payload, phase, wave);
    let payload_summary = build_payload_summary(&rec.payload);
    let (input, output) = extract_io_pair(&rec.event, &rec.payload);
    // Silence unused-field warning for the spec slug — the per-spec NDJSON
    // directory is already keyed by spec name, so the line's own `spec` field
    // is redundant for the projection but kept on the wire for greppability.
    let _ = &rec.spec;
    TimelineNode {
        ts: rec.ts.clone(),
        kind: kind_enum,
        label,
        phase,
        wave,
        payload_summary,
        raw_event: rec.event.clone(),
        input,
        output,
        tokens_in: rec.tokens_in,
        tokens_out: rec.tokens_out,
        duration_ms: rec.duration_ms,
        parent_id: rec.parent_id,
    }
}

/// Pre-extract `(input, output)` from a tool-use payload by tool kind. Best-
/// effort: any field that does not resolve falls back to `None`, so a renderer
/// that needs the raw payload can still drill in via `payload_summary`.
fn extract_io_pair(
    raw_event: &str,
    payload: &serde_json::Value,
) -> (Option<String>, Option<String>) {
    if raw_event != "tool.use" && raw_event != "tool.result" {
        return (None, None);
    }
    let tool = payload
        .get("tool")
        .or_else(|| payload.get("tool_name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let input = match tool {
        "Bash" => payload
            .get("input")
            .and_then(|i| i.get("command"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                payload
                    .get("tool_input")
                    .and_then(|i| i.get("command"))
                    .and_then(serde_json::Value::as_str)
            })
            .map(ToString::to_string),
        "Read" => payload
            .get("input")
            .and_then(|i| i.get("file_path"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                payload
                    .get("tool_input")
                    .and_then(|i| i.get("file_path"))
                    .and_then(serde_json::Value::as_str)
            })
            .map(ToString::to_string),
        "Edit" | "Write" => payload
            .get("input")
            .and_then(|i| i.get("file_path"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                payload
                    .get("tool_input")
                    .and_then(|i| i.get("file_path"))
                    .and_then(serde_json::Value::as_str)
            })
            .map(ToString::to_string),
        "Glob" | "Grep" => payload
            .get("input")
            .and_then(|i| i.get("pattern"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                payload
                    .get("tool_input")
                    .and_then(|i| i.get("pattern"))
                    .and_then(serde_json::Value::as_str)
            })
            .map(ToString::to_string),
        "Task" => payload
            .get("input")
            .and_then(|i| i.get("prompt"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                payload
                    .get("tool_input")
                    .and_then(|i| i.get("prompt"))
                    .and_then(serde_json::Value::as_str)
            })
            .map(|s| s.chars().take(2_000).collect::<String>()),
        _ => None,
    };
    let output = payload
        .get("output")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            payload
                .get("result")
                .and_then(serde_json::Value::as_str)
        })
        .map(|s| s.chars().take(8_000).collect::<String>());
    (input, output)
}

/// Build a short label for one event. Keeps copy minimal — the dashboard is
/// free to localize or rephrase.
fn build_label(
    kind: TimelineKind,
    raw_event: &str,
    payload: &serde_json::Value,
    phase: Option<Phase>,
    wave: Option<u32>,
) -> String {
    match kind {
        TimelineKind::Scope => "Pipeline scope opened".into(),
        TimelineKind::Phase => phase.map_or_else(
            || "Phase transition".into(),
            |p| format!("Phase → {}", phase_name(p)),
        ),
        TimelineKind::Status => {
            let to = payload
                .get("to")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            format!("Status → {to}")
        }
        TimelineKind::Task => {
            let name = payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("task");
            if raw_event.ends_with("complete") {
                format!("Task {name} completed")
            } else {
                format!("Task {name} dispatched")
            }
        }
        TimelineKind::Wave => match wave {
            Some(w) if raw_event.ends_with("failed") => format!("Wave {w} failed"),
            Some(w) => format!("Wave {w} completed"),
            None => "Wave update".into(),
        },
        TimelineKind::Qa => {
            let overall = payload
                .get("overall")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            format!("QA result: {overall}")
        }
        TimelineKind::Review => "Review result".into(),
        TimelineKind::Agent => {
            let agent_type = payload
                .get("agent_type")
                .or_else(|| payload.get("subagent_type"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("agent");
            if raw_event.ends_with("stop") {
                format!("{agent_type} stopped")
            } else {
                format!("{agent_type} started")
            }
        }
        TimelineKind::Tool => {
            let tool = payload
                .get("tool")
                .or_else(|| payload.get("tool_name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("tool");
            format!("{tool} used")
        }
        TimelineKind::Decision => {
            let title = payload
                .get("title")
                .or_else(|| payload.get("takeaway"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if title.is_empty() {
                raw_event.into()
            } else {
                title.chars().take(80).collect()
            }
        }
        TimelineKind::Other => raw_event.into(),
    }
}

/// Build a single-line summary of the payload — at most ~120 chars.
fn build_payload_summary(payload: &serde_json::Value) -> String {
    if payload.is_null() {
        return String::new();
    }
    if let Some(s) = payload.as_str() {
        return s.chars().take(120).collect();
    }
    let serialized = serde_json::to_string(payload).unwrap_or_default();
    serialized.chars().take(120).collect()
}

const fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Analyze => "ANALYZE",
        Phase::Plan => "PLAN",
        Phase::Execute => "EXECUTE",
        Phase::Qa => "QA",
        Phase::Close => "CLOSE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use serde_json::json;
    use tempfile::tempdir;

    fn ev(spec: &str, ts: &str, kind: &str, payload: serde_json::Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.into(),
            session_id: "s1".into(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: kind.into(),
            payload,
            spec: Some(spec.into()),
        }
    }

    #[test]
    fn empty_events_yield_empty_timeline() {
        assert!(project_timeline("auth", &[], TimeWindow::All).is_empty());
    }

    #[test]
    fn events_are_classified_and_sorted_chronologically() {
        let events = vec![
            ev(
                "auth",
                "2026-05-20T10:01:00Z",
                "pipeline.phase",
                json!({ "to": "execute" }),
            ),
            ev(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.scope",
                json!({ "scope": "full" }),
            ),
            ev(
                "auth",
                "2026-05-20T10:02:00Z",
                "qa.result",
                json!({ "overall": "pass" }),
            ),
        ];
        let timeline = project_timeline("auth", &events, TimeWindow::All);
        assert_eq!(timeline.len(), 3);
        assert_eq!(timeline[0].kind, TimelineKind::Scope);
        assert_eq!(timeline[1].kind, TimelineKind::Phase);
        assert_eq!(timeline[1].phase, Some(Phase::Execute));
        assert_eq!(timeline[2].kind, TimelineKind::Qa);
    }

    #[test]
    fn phase_event_label_contains_target_phase() {
        let events = vec![ev(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.phase",
            json!({ "to": "qa" }),
        )];
        let t = project_timeline("auth", &events, TimeWindow::All);
        assert!(t[0].label.contains("QA"));
    }

    #[test]
    fn other_spec_events_are_filtered_out() {
        let events = vec![
            ev("auth", "2026-05-20T10:00:00Z", "tool.use", json!({})),
            ev("billing", "2026-05-20T10:01:00Z", "tool.use", json!({})),
        ];
        let t = project_timeline("auth", &events, TimeWindow::All);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn all_window_keeps_old_events() {
        let events = vec![ev(
            "auth",
            "1990-01-01T00:00:00Z",
            "tool.use",
            json!({}),
        )];
        let t = project_timeline("auth", &events, TimeWindow::All);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn ndjson_reader_returns_empty_on_missing_dir() {
        let dir = tempdir().unwrap();
        let nodes = project_timeline_from_ndjson_dir(
            &dir.path().join("missing"),
            TimeWindow::All,
        );
        assert!(nodes.is_empty());
    }

    #[test]
    fn ndjson_reader_decodes_writer_shape() {
        let dir = tempdir().unwrap();
        let line1 = r#"{"ts":"2026-05-24T10:00:00.000Z","ts_ms":1779789600000,"event":"tool.use","kind":"tool","spec":"auth","payload":{"tool":"Bash","input":{"command":"ls"},"tokens_in":12,"tokens_out":34,"duration_ms":56},"tokens_in":12,"tokens_out":34,"duration_ms":56}"#;
        let line2 = r#"{"ts":"2026-05-24T10:00:01.000Z","ts_ms":1779789601000,"event":"pipeline.phase","kind":"phase","spec":"auth","payload":{"to":"execute"}}"#;
        let body = format!("{line1}\n{line2}\n");
        std::fs::write(dir.path().join("a.ndjson"), body).unwrap();

        let nodes = project_timeline_from_ndjson_dir(dir.path(), TimeWindow::All);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].kind, TimelineKind::Tool);
        assert_eq!(nodes[0].tokens_in, Some(12));
        assert_eq!(nodes[0].tokens_out, Some(34));
        assert_eq!(nodes[0].duration_ms, Some(56));
        assert_eq!(nodes[0].input.as_deref(), Some("ls"));
        assert_eq!(nodes[1].kind, TimelineKind::Phase);
        assert_eq!(nodes[1].phase, Some(Phase::Execute));
    }

    #[test]
    fn ndjson_reader_skips_malformed_lines() {
        let dir = tempdir().unwrap();
        let body = "not json\n{}\n";
        std::fs::write(dir.path().join("a.ndjson"), body).unwrap();
        let nodes = project_timeline_from_ndjson_dir(dir.path(), TimeWindow::All);
        // The bare `{}` decodes via lenient defaults — empty event name, classified as Other.
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, TimelineKind::Other);
    }
}
