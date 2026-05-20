//! [`project_timeline`] — chronological event timeline for a single spec.
//!
//! Classifies each event into a [`TimelineKind`] and builds a short label +
//! single-line payload summary so the dashboard can render the timeline
//! without re-parsing the payload itself.

use crate::model::view::{Phase, TimelineKind, TimelineNode, TimeWindow};
use crate::model::event::HarnessEvent;

use super::parse_iso_millis;

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
        .map(classify_one)
        .collect();
    nodes.sort_by(|a, b| a.ts.cmp(&b.ts));
    nodes
}

/// Returns the lower bound (in epoch ms) for a window, or `None` for `All`.
fn window_cutoff_ms(window: TimeWindow) -> Option<i64> {
    // Approximate "now" by reading from the SQL fragment isn't possible in a
    // pure projection. We use a conservative approach: compute the cutoff
    // from system time when called by the SQLite reader. Pure projection
    // tests can pass `All` to skip windowing entirely. Real callers use the
    // SQL filter on the reader; this fallback handles in-memory tests that
    // post-filter via the projection.
    match window {
        TimeWindow::All => None,
        TimeWindow::Today => Some(today_cutoff_ms()),
        TimeWindow::SevenDays => Some(today_cutoff_ms() - 7 * 86_400_000),
        TimeWindow::ThirtyDays => Some(today_cutoff_ms() - 30 * 86_400_000),
    }
}

/// Today, 00:00:00 UTC, in epoch milliseconds. Reads from `SystemTime` —
/// not pure, but only used when the caller asked for a windowed projection
/// in memory. The SQLite reader path uses [`TimeWindow::sql_filter`] and
/// never calls this.
fn today_cutoff_ms() -> i64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    // Truncate to start-of-day.
    now_ms - (now_ms % 86_400_000)
}

/// Classify one event into a timeline node. Pure — no IO, no allocation
/// beyond the returned struct.
fn classify_one(ev: &HarnessEvent) -> TimelineNode {
    let kind = TimelineKind::classify(&ev.event);
    let phase = ev
        .payload
        .get("to")
        .and_then(serde_json::Value::as_str)
        .and_then(Phase::parse)
        .or_else(|| {
            // pipeline.scope-style events sometimes carry phase as a field
            // directly named `phase`.
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

    let label = build_label(kind, &ev.event, ev, phase, wave);
    let payload_summary = build_payload_summary(ev);

    TimelineNode {
        ts: ev.ts.clone(),
        kind,
        label,
        phase,
        wave,
        payload_summary,
        raw_event: ev.event.clone(),
    }
}

/// Build a short label for one event. Keeps copy minimal — the dashboard is
/// free to localize or rephrase.
fn build_label(
    kind: TimelineKind,
    raw_event: &str,
    ev: &HarnessEvent,
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
            let to = ev
                .payload
                .get("to")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            format!("Status → {to}")
        }
        TimelineKind::Task => {
            let name = ev
                .payload
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
            let overall = ev
                .payload
                .get("overall")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            format!("QA result: {overall}")
        }
        TimelineKind::Review => "Review result".into(),
        TimelineKind::Agent => {
            let agent_type = ev
                .payload
                .get("agent_type")
                .or_else(|| ev.payload.get("subagent_type"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("agent");
            if raw_event.ends_with("stop") {
                format!("{agent_type} stopped")
            } else {
                format!("{agent_type} started")
            }
        }
        TimelineKind::Tool => {
            let tool = ev
                .payload
                .get("tool")
                .or_else(|| ev.payload.get("tool_name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("tool");
            format!("{tool} used")
        }
        TimelineKind::Decision => {
            let title = ev
                .payload
                .get("title")
                .or_else(|| ev.payload.get("takeaway"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if title.is_empty() {
                raw_event.into()
            } else {
                let head: String = title.chars().take(80).collect();
                head
            }
        }
        TimelineKind::Other => raw_event.into(),
    }
}

/// Build a single-line summary of the payload — at most ~120 chars.
fn build_payload_summary(ev: &HarnessEvent) -> String {
    // Quick path: if the payload is a primitive, just stringify it.
    if ev.payload.is_null() {
        return String::new();
    }
    if let Some(s) = ev.payload.as_str() {
        return s.chars().take(120).collect();
    }
    // Compact JSON without whitespace.
    let serialized = serde_json::to_string(&ev.payload).unwrap_or_default();
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
    use crate::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use serde_json::json;

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
}
