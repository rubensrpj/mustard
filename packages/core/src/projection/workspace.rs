//! [`project_workspace`] — top-level Visão Geral roll-up.
//!
//! Takes the full event stream (across every spec) and produces a
//! [`WorkspaceSummary`]. Unlike the per-spec projections, this one needs to
//! see the whole event log: it computes `events_per_minute` across a recent
//! window, picks the top files of the day, and emits one `SpecTrack` per
//! active spec.

use crate::model::view::{
    FileCount, Phase, PhaseSegment, SegmentState, SpecStatus, SpecTrack, WorkspaceAlert,
    WorkspaceAlertKind, WorkspaceSummary,
};
use crate::projection::card::project_spec_view;
use crate::model::event::HarnessEvent;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::parse_iso_millis;

/// Maximum number of `top_files_today` entries in the result.
const TOP_FILES_CAP: usize = 10;

/// Fold the entire event stream into a [`WorkspaceSummary`].
///
/// `now_ms` is the current wall-clock time in epoch milliseconds. Tests pass
/// a fixed value for deterministic output; the SQLite reader path passes
/// `SystemTime::now()`.
#[must_use]
pub fn project_workspace(events: &[HarnessEvent], now_ms: i64) -> WorkspaceSummary {
    if events.is_empty() {
        return WorkspaceSummary::empty();
    }

    let one_minute_ago = now_ms - 60_000;
    let today_start = now_ms - (now_ms % 86_400_000);

    // events_per_minute: count of events in the last 60 seconds. Honest
    // measurement; ignores rows whose `ts` cannot be parsed.
    let mut events_last_minute = 0u32;
    let mut files_today: HashMap<String, u32> = HashMap::new();
    let mut tokens_saved_today: Option<i64> = None;
    let mut spec_events: HashMap<String, Vec<HarnessEvent>> = HashMap::new();
    let mut alerts: Vec<WorkspaceAlert> = Vec::new();

    for ev in events {
        let ts_ms = parse_iso_millis(&ev.ts).unwrap_or(0);

        // Recent window for the live rate.
        if ts_ms >= one_minute_ago {
            events_last_minute = events_last_minute.saturating_add(1);
        }

        // Track per-spec stream for the SpecTrack reduce step below.
        if let Some(spec) = &ev.spec {
            spec_events
                .entry(spec.clone())
                .or_default()
                .push(ev.clone());
        }

        // Today-only roll-ups.
        if ts_ms >= today_start {
            if ev.event == "tool.use" {
                if let Some(path) = ev
                    .payload
                    .get("file_path")
                    .or_else(|| ev.payload.get("tool_input").and_then(|t| t.get("file_path")))
                    .or_else(|| ev.payload.get("target").and_then(|t| t.get("file")))
                    .and_then(serde_json::Value::as_str)
                {
                    *files_today.entry(path.to_string()).or_insert(0) += 1;
                }
            }
            if matches!(
                ev.event.as_str(),
                "rtk.savings"
                    | "prompt.economy.saved"
                    | "hook.savings"
                    | "routing.savings"
            ) {
                if let Some(n) = ev
                    .payload
                    .get("saved")
                    .or_else(|| ev.payload.get("tokens_saved"))
                    .and_then(serde_json::Value::as_i64)
                {
                    tokens_saved_today = Some(tokens_saved_today.unwrap_or(0) + n);
                }
            }
        }

        // Alerts — collected across all time, deduplicated at the end.
        match ev.event.as_str() {
            "pipeline.pause" => {
                if let Some(spec) = &ev.spec {
                    let reason = ev
                        .payload
                        .get("reason")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("paused")
                        .to_string();
                    alerts.push(WorkspaceAlert {
                        spec: spec.clone(),
                        kind: WorkspaceAlertKind::Blocked,
                        message: reason,
                        ts: ev.ts.clone(),
                    });
                }
            }
            "qa.result" => {
                if let (Some(spec), Some("fail")) = (
                    &ev.spec,
                    ev.payload.get("overall").and_then(serde_json::Value::as_str),
                ) {
                    alerts.push(WorkspaceAlert {
                        spec: spec.clone(),
                        kind: WorkspaceAlertKind::QaFail,
                        message: "QA failed".into(),
                        ts: ev.ts.clone(),
                    });
                }
            }
            "pipeline.wave.failed" => {
                if let Some(spec) = &ev.spec {
                    let wave = ev
                        .payload
                        .get("wave")
                        .and_then(serde_json::Value::as_u64)
                        .map_or_else(String::new, |w| format!(" {w}"));
                    alerts.push(WorkspaceAlert {
                        spec: spec.clone(),
                        kind: WorkspaceAlertKind::WaveFailed,
                        message: format!("Wave{wave} failed"),
                        ts: ev.ts.clone(),
                    });
                }
            }
            "review.result" => {
                if let (Some(spec), Some("rejected")) = (
                    &ev.spec,
                    ev.payload.get("verdict").and_then(serde_json::Value::as_str),
                ) {
                    alerts.push(WorkspaceAlert {
                        spec: spec.clone(),
                        kind: WorkspaceAlertKind::ReviewRejected,
                        message: "Review rejected".into(),
                        ts: ev.ts.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    // SpecTrack list — one per spec, computed from the per-spec view.
    let mut tracks: Vec<SpecTrack> = spec_events
        .iter()
        .filter(|(name, _)| name.as_str() != "__orphan__")
        .map(|(name, evs)| build_track(name, evs))
        .collect();
    // Sort: active first, then by last_event_at desc.
    tracks.sort_by(|a, b| {
        a.status
            .is_active()
            .cmp(&b.status.is_active())
            .reverse()
            .then(b.last_event_at.cmp(&a.last_event_at))
    });
    let specs_active_count =
        u32::try_from(tracks.iter().filter(|t| t.status.is_active()).count()).unwrap_or(u32::MAX);

    // Deduplicate alerts — keep the latest one per (spec, kind).
    let mut alert_dedup: BTreeMap<(String, WorkspaceAlertKind), WorkspaceAlert> = BTreeMap::new();
    for a in alerts {
        let key = (a.spec.clone(), a.kind);
        let entry = alert_dedup.entry(key);
        match entry {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(a);
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                if a.ts > o.get().ts {
                    o.insert(a);
                }
            }
        }
    }
    let mut alerts: Vec<WorkspaceAlert> = alert_dedup.into_values().collect();
    alerts.sort_by(|a, b| b.ts.cmp(&a.ts));

    // top_files_today — sort by count desc, cap.
    let mut top: Vec<FileCount> = files_today
        .into_iter()
        .map(|(path, count)| FileCount { path, count })
        .collect();
    top.sort_by(|a, b| b.count.cmp(&a.count).then(a.path.cmp(&b.path)));
    top.truncate(TOP_FILES_CAP);

    WorkspaceSummary {
        events_per_minute: f64::from(events_last_minute),
        specs_active_count,
        tokens_saved_today,
        spec_tracks: tracks,
        alerts,
        top_files_today: top,
    }
}

/// Build a single [`SpecTrack`] for one spec from its slice of events.
fn build_track(spec: &str, events: &[HarnessEvent]) -> SpecTrack {
    let view = project_spec_view(spec, events);
    let segments = build_segments(view.phase, view.status);
    let agents_active = count_active_agents(events);
    let blocked_reason = if view.status == SpecStatus::Blocked {
        events
            .iter()
            .rev()
            .find(|e| e.event == "pipeline.pause")
            .and_then(|e| e.payload.get("reason").and_then(serde_json::Value::as_str))
            .map(str::to_string)
    } else {
        None
    };
    SpecTrack {
        spec: spec.to_string(),
        status: view.status,
        current_phase: view.phase,
        current_wave: view.current_wave,
        total_waves: view.total_waves,
        agents_active,
        last_event_at: view.last_event_at,
        blocked_reason,
        segments,
    }
}

/// Build the five phase segments for a SpecTrack.
fn build_segments(current: Option<Phase>, status: SpecStatus) -> Vec<PhaseSegment> {
    let target_idx = current.map(Phase::index);
    Phase::all()
        .into_iter()
        .map(|phase| {
            let state = match (status, target_idx) {
                (SpecStatus::Completed, _) => SegmentState::Completed,
                (_, Some(idx)) if phase.index() < idx => SegmentState::Completed,
                (_, Some(idx)) if phase.index() == idx => SegmentState::Active,
                _ => SegmentState::Future,
            };
            PhaseSegment { phase, state }
        })
        .collect()
}

/// Count distinct active agent ids — agents with `agent.start` but no
/// matching `agent.stop` (matched by `actor.id`).
fn count_active_agents(events: &[HarnessEvent]) -> u32 {
    let mut started: BTreeSet<String> = BTreeSet::new();
    let mut stopped: BTreeSet<String> = BTreeSet::new();
    for ev in events {
        let id = ev.actor.id.clone().unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        match ev.event.as_str() {
            "agent.start" => {
                started.insert(id);
            }
            "agent.stop" => {
                stopped.insert(id);
            }
            _ => {}
        }
    }
    u32::try_from(started.difference(&stopped).count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use serde_json::json;

    fn ev(spec: Option<&str>, ts: &str, kind: &str, payload: serde_json::Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.into(),
            session_id: "s1".into(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: kind.into(),
            payload,
            spec: spec.map(str::to_string),
        }
    }

    /// Sample `now`, derived from the canonical ISO via the in-crate parser
    /// so it always matches the fixture dates regardless of any
    /// hand-calculated arithmetic. Equivalent to `2026-05-20T12:00:00Z`.
    fn now_ms() -> i64 {
        parse_iso_millis("2026-05-20T12:00:00Z").expect("hard-coded ISO parses")
    }

    #[test]
    fn empty_events_yield_empty_summary() {
        let s = project_workspace(&[], now_ms());
        assert_eq!(s.specs_active_count, 0);
        assert!(s.spec_tracks.is_empty());
        assert!(s.alerts.is_empty());
    }

    #[test]
    fn events_per_minute_counts_only_last_60_seconds() {
        let events = vec![
            // Old: 5 minutes ago.
            ev(Some("a"), "2026-05-20T11:55:00Z", "tool.use", json!({})),
            // Recent: 30 seconds ago.
            ev(Some("a"), "2026-05-20T11:59:30Z", "tool.use", json!({})),
            ev(Some("a"), "2026-05-20T11:59:45Z", "tool.use", json!({})),
        ];
        let s = project_workspace(&events, now_ms());
        // 2 events in the last 60 seconds.
        assert!((s.events_per_minute - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn orphan_specs_are_excluded_from_tracks() {
        let events = vec![
            ev(Some("__orphan__"), "2026-05-20T10:00:00Z", "tool.use", json!({})),
            ev(Some("auth"), "2026-05-20T10:00:00Z", "tool.use", json!({})),
        ];
        let s = project_workspace(&events, now_ms());
        assert_eq!(s.spec_tracks.len(), 1);
        assert_eq!(s.spec_tracks[0].spec, "auth");
    }

    #[test]
    fn tokens_saved_today_is_none_when_no_savings_events() {
        let events = vec![ev(Some("a"), "2026-05-20T10:00:00Z", "tool.use", json!({}))];
        let s = project_workspace(&events, now_ms());
        assert!(s.tokens_saved_today.is_none());
    }

    #[test]
    fn tokens_saved_today_sums_known_event_kinds() {
        let events = vec![
            ev(
                Some("a"),
                "2026-05-20T10:00:00Z",
                "rtk.savings",
                json!({ "saved": 1000 }),
            ),
            ev(
                Some("a"),
                "2026-05-20T11:00:00Z",
                "hook.savings",
                json!({ "saved": 500 }),
            ),
        ];
        let s = project_workspace(&events, now_ms());
        assert_eq!(s.tokens_saved_today, Some(1500));
    }

    #[test]
    fn top_files_today_ranks_by_count() {
        let events = vec![
            ev(
                Some("a"),
                "2026-05-20T10:00:00Z",
                "tool.use",
                json!({ "file_path": "src/a.rs" }),
            ),
            ev(
                Some("a"),
                "2026-05-20T10:01:00Z",
                "tool.use",
                json!({ "file_path": "src/a.rs" }),
            ),
            ev(
                Some("a"),
                "2026-05-20T10:02:00Z",
                "tool.use",
                json!({ "file_path": "src/b.rs" }),
            ),
        ];
        let s = project_workspace(&events, now_ms());
        assert_eq!(s.top_files_today.len(), 2);
        assert_eq!(s.top_files_today[0].path, "src/a.rs");
        assert_eq!(s.top_files_today[0].count, 2);
    }

    #[test]
    fn qa_fail_event_produces_alert_and_deduplicates() {
        let events = vec![
            ev(
                Some("a"),
                "2026-05-20T10:00:00Z",
                "qa.result",
                json!({ "overall": "fail" }),
            ),
            ev(
                Some("a"),
                "2026-05-20T11:00:00Z",
                "qa.result",
                json!({ "overall": "fail" }),
            ),
        ];
        let s = project_workspace(&events, now_ms());
        // Two events, same (spec, kind) — should dedupe to one with the later ts.
        assert_eq!(s.alerts.len(), 1);
        assert_eq!(s.alerts[0].kind, WorkspaceAlertKind::QaFail);
        assert_eq!(s.alerts[0].ts, "2026-05-20T11:00:00Z");
    }

    #[test]
    fn spec_track_segments_mark_completed_then_active_then_future() {
        let events = vec![
            ev(
                Some("a"),
                "2026-05-20T10:00:00Z",
                "pipeline.scope",
                json!({ "scope": "full" }),
            ),
            ev(
                Some("a"),
                "2026-05-20T10:00:01Z",
                "pipeline.phase",
                json!({ "to": "execute" }),
            ),
        ];
        let s = project_workspace(&events, now_ms());
        let track = &s.spec_tracks[0];
        let segments_state: Vec<_> = track.segments.iter().map(|seg| (seg.phase, seg.state)).collect();
        assert_eq!(
            segments_state,
            vec![
                (Phase::Analyze, SegmentState::Completed),
                (Phase::Plan, SegmentState::Completed),
                (Phase::Execute, SegmentState::Active),
                (Phase::Qa, SegmentState::Future),
                (Phase::Close, SegmentState::Future),
            ]
        );
    }
}
