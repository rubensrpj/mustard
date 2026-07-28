//! [`project_waves`] — wave-by-wave breakdown for `SpecDrillDown > Ondas`.
//!
//! Reconstructs the lifecycle of every wave inside a pipeline. Three event
//! kinds drive it:
//!
//! - `pipeline.wave.start` opens a wave row at `InProgress` (or creates it if
//!   absent) and records `started_at` — the explicit "wave began executing"
//!   signal (emitted by the `wave_start_observer` on the first child's
//!   `SubagentStart`). It makes `in_progress = started-without-complete` precise
//!   instead of inferring the start from a `pipeline.task.dispatch`.
//! - `pipeline.task.dispatch` opens a wave row at `InProgress` (or creates it
//!   if absent) and records `started_at`, `role`, `agent_type`.
//! - `pipeline.task.complete` adds `files_modified` to the wave's file list.
//! - `pipeline.wave.complete` flips the row to `Completed` with `completed_at`
//!   and computes `duration_ms`.
//! - `pipeline.wave.failed` flips the row to `Failed`.
//! - `pipeline.status` / `pipeline.outcome` transitioning the SPEC to a
//!   deliberate terminal outcome (`cancelled` / `abandoned`) flips every wave
//!   still open to `Dropped` — the work was let go on purpose, and leaving it
//!   at `Queued` is what made a decision read as a pending task.
//!
//! Waves that only appear in a `pipeline.wave.complete` event (e.g. when the
//! dispatch event was filtered out by attribution gaps pre-v2) still get a
//! row, but with `Completed` status and no agent metadata.

use crate::domain::model::view::{Outcome, WaveStatus, WaveView};
use crate::domain::model::event::{
    HarnessEvent, PipelineTaskCompletePayload, PipelineTaskDispatchPayload,
};
use std::collections::BTreeMap;

use crate::platform::time::iso_diff_ms;

/// Fold events for `spec_name` into a sorted list of [`WaveView`].
///
/// The returned vector is sorted by `wave` ascending. Order of events matters
/// for `started_at` / `completed_at` correctness (chronological assumed).
#[must_use]
pub fn project_waves(spec_name: &str, events: &[HarnessEvent]) -> Vec<WaveView> {
    let mut by_wave: BTreeMap<u32, WaveView> = BTreeMap::new();

    for ev in events.iter().filter(|e| e.spec.as_deref() == Some(spec_name)) {
        match ev.event.as_str() {
            "pipeline.wave.start" => apply_wave_start(&mut by_wave, ev),
            "pipeline.task.dispatch" => apply_dispatch(&mut by_wave, ev),
            "pipeline.task.complete" => apply_complete(&mut by_wave, ev),
            "pipeline.wave.complete" => apply_wave_complete(&mut by_wave, ev),
            "pipeline.wave.failed" => apply_wave_failed(&mut by_wave, ev),
            "pipeline.status" | "pipeline.outcome" => apply_spec_outcome(&mut by_wave, ev),
            _ => {}
        }
    }

    by_wave.into_values().collect()
}

fn ensure_wave(by_wave: &mut BTreeMap<u32, WaveView>, wave: u32) -> &mut WaveView {
    by_wave.entry(wave).or_insert_with(|| WaveView::queued(wave))
}

/// `pipeline.wave.start` — the explicit "wave began" signal. Opens the row at
/// `InProgress` and records `started_at`. Forward-only: a row already
/// `Completed` / `Failed` is not regressed (a late, out-of-order start event
/// must never resurrect a finished wave). The payload is the bare `{wave: <n>}`
/// shape (same correlation as `pipeline.wave.complete`).
fn apply_wave_start(by_wave: &mut BTreeMap<u32, WaveView>, ev: &HarnessEvent) {
    let Some(wave) = ev
        .payload
        .get("wave")
        .and_then(serde_json::Value::as_u64)
        .and_then(|w| u32::try_from(w).ok())
    else {
        return;
    };
    let row = ensure_wave(by_wave, wave);
    if reopens_into_progress(row.status) {
        row.status = WaveStatus::InProgress;
    }
    if row.started_at.is_none() {
        row.started_at = Some(ev.ts.clone());
    }
}

/// Whether a start/dispatch signal may move the row to `InProgress`. A queued
/// wave obviously may. A DROPPED one may too: the drop was a decision, and a
/// later dispatch is the harness observing that the decision was reversed —
/// keeping it `Dropped` would state something the event stream denies.
/// `Completed` / `Failed` never regress (a late, out-of-order start must not
/// resurrect a finished wave).
const fn reopens_into_progress(status: WaveStatus) -> bool {
    matches!(status, WaveStatus::Queued | WaveStatus::Dropped)
}

fn apply_dispatch(by_wave: &mut BTreeMap<u32, WaveView>, ev: &HarnessEvent) {
    let Ok(payload) =
        serde_json::from_value::<PipelineTaskDispatchPayload>(ev.payload.clone())
    else {
        return;
    };
    let Some(wave) = payload.wave else { return };
    let row = ensure_wave(by_wave, wave);
    if reopens_into_progress(row.status) {
        row.status = WaveStatus::InProgress;
    }
    if row.started_at.is_none() {
        row.started_at = Some(ev.ts.clone());
    }
    if row.role.is_none() {
        row.role = payload.role;
    }
    if row.agent_type.is_none() {
        row.agent_type = payload.agent;
    }
}

fn apply_complete(by_wave: &mut BTreeMap<u32, WaveView>, ev: &HarnessEvent) {
    let Ok(payload) =
        serde_json::from_value::<PipelineTaskCompletePayload>(ev.payload.clone())
    else {
        return;
    };
    let Some(wave) = payload.wave else { return };
    let row = ensure_wave(by_wave, wave);
    if row.agent_type.is_none() {
        row.agent_type = payload.agent;
    }
    if let Some(files) = payload.files_modified {
        for f in files {
            if !row.files_changed.contains(&f) {
                row.files_changed.push(f);
            }
        }
    }
}

fn apply_wave_complete(by_wave: &mut BTreeMap<u32, WaveView>, ev: &HarnessEvent) {
    let Some(wave) = ev
        .payload
        .get("wave")
        .and_then(serde_json::Value::as_u64)
        .and_then(|w| u32::try_from(w).ok())
    else {
        return;
    };
    let row = ensure_wave(by_wave, wave);
    row.status = WaveStatus::Completed;
    row.completed_at = Some(ev.ts.clone());
    if let Some(start) = row.started_at.as_deref() {
        row.duration_ms = iso_diff_ms(start, &ev.ts);
    }
    // Sort the deduplicated file list once on close so downstream renders
    // are stable across queries.
    row.files_changed.sort();
}

/// `pipeline.status` / `pipeline.outcome` — a SPEC-level transition. Only the
/// two deliberate terminal outcomes matter here: `cancelled` and `abandoned`
/// both mean somebody decided the remaining work will not happen. Every wave
/// row that has not settled becomes `Dropped` (with the transition's timestamp
/// as `completed_at`, so the row reads as closed rather than open-forever).
///
/// A `wave` in the payload narrows the drop to that single wave — the shape
/// used when only one wave was let go and the spec itself carries on. Waves
/// that already reached a terminal state are left exactly as they were: a
/// finished wave was not dropped, and a failed one failed.
fn apply_spec_outcome(by_wave: &mut BTreeMap<u32, WaveView>, ev: &HarnessEvent) {
    let is_deliberate_stop = ev
        .payload
        .get("to")
        .and_then(serde_json::Value::as_str)
        .and_then(Outcome::parse)
        .is_some_and(|o| matches!(o, Outcome::Cancelled | Outcome::Abandoned));
    if !is_deliberate_stop {
        return;
    }
    let only = ev
        .payload
        .get("wave")
        .and_then(serde_json::Value::as_u64)
        .and_then(|w| u32::try_from(w).ok());
    for (n, row) in by_wave.iter_mut() {
        if only.is_some_and(|w| w != *n) || row.status.is_terminal() {
            continue;
        }
        row.status = WaveStatus::Dropped;
        row.completed_at = Some(ev.ts.clone());
        if let Some(start) = row.started_at.as_deref() {
            row.duration_ms = iso_diff_ms(start, &ev.ts);
        }
    }
}

fn apply_wave_failed(by_wave: &mut BTreeMap<u32, WaveView>, ev: &HarnessEvent) {
    let Some(wave) = ev
        .payload
        .get("wave")
        .and_then(serde_json::Value::as_u64)
        .and_then(|w| u32::try_from(w).ok())
    else {
        return;
    };
    let row = ensure_wave(by_wave, wave);
    row.status = WaveStatus::Failed;
    row.completed_at = Some(ev.ts.clone());
    if let Some(start) = row.started_at.as_deref() {
        row.duration_ms = iso_diff_ms(start, &ev.ts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};
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
    fn empty_events_yield_no_waves() {
        assert!(project_waves("auth", &[]).is_empty());
    }

    #[test]
    fn dispatch_then_complete_yields_completed_wave() {
        let events = vec![
            ev(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 1, "name": "core", "agent": "general-purpose", "role": "impl" }),
            ),
            ev(
                "auth",
                "2026-05-20T10:05:00Z",
                "pipeline.task.complete",
                json!({ "wave": 1, "name": "core", "files_modified": ["src/a.rs"] }),
            ),
            ev(
                "auth",
                "2026-05-20T10:05:30Z",
                "pipeline.wave.complete",
                json!({ "wave": 1 }),
            ),
        ];
        let waves = project_waves("auth", &events);
        assert_eq!(waves.len(), 1);
        let w = &waves[0];
        assert_eq!(w.wave, 1);
        assert_eq!(w.status, WaveStatus::Completed);
        assert_eq!(w.agent_type.as_deref(), Some("general-purpose"));
        assert_eq!(w.role.as_deref(), Some("impl"));
        assert_eq!(w.files_changed, vec!["src/a.rs"]);
        assert_eq!(w.started_at.as_deref(), Some("2026-05-20T10:00:00Z"));
        assert_eq!(w.completed_at.as_deref(), Some("2026-05-20T10:05:30Z"));
        assert_eq!(w.duration_ms, Some(330_000));
    }

    #[test]
    fn dispatch_without_complete_stays_in_progress() {
        let events = vec![ev(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.task.dispatch",
            json!({ "wave": 1, "name": "core" }),
        )];
        let waves = project_waves("auth", &events);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].status, WaveStatus::InProgress);
    }

    #[test]
    fn wave_start_opens_in_progress_row() {
        // An explicit `pipeline.wave.start` with no later dispatch still yields
        // an InProgress row carrying the start timestamp — the "started without
        // complete" signal the dashboard derives `in_progress` from.
        let events = vec![ev(
            "auth",
            "2026-05-20T09:59:00Z",
            "pipeline.wave.start",
            json!({ "wave": 1 }),
        )];
        let waves = project_waves("auth", &events);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].status, WaveStatus::InProgress);
        assert_eq!(waves[0].started_at.as_deref(), Some("2026-05-20T09:59:00Z"));
        assert!(waves[0].completed_at.is_none());
    }

    #[test]
    fn wave_start_then_complete_yields_completed_with_duration() {
        // start → complete: the row closes as Completed and `duration_ms` is
        // computed off the start event's timestamp.
        let events = vec![
            ev("auth", "2026-05-20T10:00:00Z", "pipeline.wave.start", json!({ "wave": 1 })),
            ev("auth", "2026-05-20T10:05:00Z", "pipeline.wave.complete", json!({ "wave": 1 })),
        ];
        let waves = project_waves("auth", &events);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].status, WaveStatus::Completed);
        assert_eq!(waves[0].started_at.as_deref(), Some("2026-05-20T10:00:00Z"));
        assert_eq!(waves[0].duration_ms, Some(300_000));
    }

    #[test]
    fn wave_start_does_not_regress_completed_row() {
        // A late, out-of-order start must not resurrect a finished wave.
        let events = vec![
            ev("auth", "2026-05-20T10:00:00Z", "pipeline.wave.start", json!({ "wave": 1 })),
            ev("auth", "2026-05-20T10:05:00Z", "pipeline.wave.complete", json!({ "wave": 1 })),
            ev("auth", "2026-05-20T10:06:00Z", "pipeline.wave.start", json!({ "wave": 1 })),
        ];
        let waves = project_waves("auth", &events);
        assert_eq!(waves[0].status, WaveStatus::Completed);
        // started_at keeps the first start's timestamp, not the stray one.
        assert_eq!(waves[0].started_at.as_deref(), Some("2026-05-20T10:00:00Z"));
    }

    #[test]
    fn wave_failed_overrides_status() {
        let events = vec![
            ev(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 2, "name": "core" }),
            ),
            ev(
                "auth",
                "2026-05-20T10:10:00Z",
                "pipeline.wave.failed",
                json!({ "wave": 2, "reason": "review-rejected" }),
            ),
        ];
        let waves = project_waves("auth", &events);
        assert_eq!(waves[0].status, WaveStatus::Failed);
        assert!(waves[0].completed_at.is_some());
    }

    #[test]
    fn abandoning_the_spec_drops_the_waves_still_open() {
        // Wave 1 finished, wave 2 was in flight, wave 3 never started. The
        // operator abandons the pipeline: the two unfinished waves are work
        // let go ON PURPOSE, and must stop reading as "still to do".
        let events = vec![
            ev("auth", "2026-05-20T10:00:00Z", "pipeline.wave.start", json!({ "wave": 1 })),
            ev("auth", "2026-05-20T10:05:00Z", "pipeline.wave.complete", json!({ "wave": 1 })),
            ev("auth", "2026-05-20T10:06:00Z", "pipeline.wave.start", json!({ "wave": 2 })),
            ev(
                "auth",
                "2026-05-20T10:07:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 3, "name": "docs" }),
            ),
            ev("auth", "2026-05-20T11:00:00Z", "pipeline.status", json!({ "to": "abandoned" })),
        ];
        let waves = project_waves("auth", &events);
        assert_eq!(waves[0].status, WaveStatus::Completed, "a finished wave was not dropped");
        assert_eq!(waves[1].status, WaveStatus::Dropped);
        assert_eq!(waves[2].status, WaveStatus::Dropped);
        assert_eq!(
            waves[1].completed_at.as_deref(),
            Some("2026-05-20T11:00:00Z"),
            "the dropped wave closes at the decision, not open-forever"
        );
        assert_eq!(waves[1].duration_ms, Some(3_240_000));
    }

    #[test]
    fn cancelling_one_wave_leaves_the_others_alone() {
        // A `wave` in the payload narrows the drop to that wave; and a wave
        // that FAILED keeps its failure — dropped and failed are not the same
        // answer.
        let events = vec![
            ev(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 1, "name": "core" }),
            ),
            ev(
                "auth",
                "2026-05-20T10:01:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 2, "name": "ui" }),
            ),
            ev("auth", "2026-05-20T10:02:00Z", "pipeline.wave.failed", json!({ "wave": 1 })),
            ev(
                "auth",
                "2026-05-20T10:03:00Z",
                "pipeline.outcome",
                json!({ "to": "cancelled", "wave": 2 }),
            ),
        ];
        let waves = project_waves("auth", &events);
        assert_eq!(waves[0].status, WaveStatus::Failed, "the failure is not overwritten");
        assert_eq!(waves[1].status, WaveStatus::Dropped);
    }

    #[test]
    fn a_non_terminal_status_never_drops_a_wave() {
        // Only a deliberate terminal outcome drops. A `blocked` / `completed`
        // spec transition must not silently mark waves as let-go.
        let events = vec![
            ev(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 1, "name": "core" }),
            ),
            ev("auth", "2026-05-20T10:01:00Z", "pipeline.status", json!({ "to": "blocked" })),
            ev("auth", "2026-05-20T10:02:00Z", "pipeline.status", json!({ "to": "completed" })),
        ];
        assert_eq!(project_waves("auth", &events)[0].status, WaveStatus::InProgress);
    }

    #[test]
    fn a_dispatch_after_the_drop_reopens_the_wave() {
        // The drop was a decision; a later dispatch is the harness watching
        // that decision be reversed. Reporting `Dropped` then would state
        // something this very event stream denies.
        let events = vec![
            ev("auth", "2026-05-20T10:00:00Z", "pipeline.wave.start", json!({ "wave": 1 })),
            ev("auth", "2026-05-20T10:01:00Z", "pipeline.status", json!({ "to": "abandoned" })),
            ev(
                "auth",
                "2026-05-20T12:00:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 1, "name": "core" }),
            ),
        ];
        assert_eq!(project_waves("auth", &events)[0].status, WaveStatus::InProgress);
    }

    #[test]
    fn waves_are_sorted_by_number() {
        let events = vec![
            ev(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 3, "name": "ui" }),
            ),
            ev(
                "auth",
                "2026-05-20T10:01:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 1, "name": "core" }),
            ),
            ev(
                "auth",
                "2026-05-20T10:02:00Z",
                "pipeline.task.dispatch",
                json!({ "wave": 2, "name": "api" }),
            ),
        ];
        let waves = project_waves("auth", &events);
        let numbers: Vec<u32> = waves.iter().map(|w| w.wave).collect();
        assert_eq!(numbers, vec![1, 2, 3]);
    }

    #[test]
    fn complete_event_without_dispatch_still_creates_row() {
        // Pre-v2 attribution gap: dispatch event might have spec = NULL while
        // wave.complete has the spec. We still want a row for the wave so the
        // dashboard can show "wave 2 finished, no dispatch details available".
        let events = vec![ev(
            "auth",
            "2026-05-20T10:05:00Z",
            "pipeline.wave.complete",
            json!({ "wave": 2 }),
        )];
        let waves = project_waves("auth", &events);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].wave, 2);
        assert_eq!(waves[0].status, WaveStatus::Completed);
        assert!(waves[0].started_at.is_none()); // no dispatch event recorded one
    }
}
