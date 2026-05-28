//! [`project_waves`] — wave-by-wave breakdown for `SpecDrillDown > Ondas`.
//!
//! Reconstructs the lifecycle of every wave inside a pipeline. Three event
//! kinds drive it:
//!
//! - `pipeline.task.dispatch` opens a wave row at `InProgress` (or creates it
//!   if absent) and records `started_at`, `role`, `agent_type`.
//! - `pipeline.task.complete` adds `files_modified` to the wave's file list.
//! - `pipeline.wave.complete` flips the row to `Completed` with `completed_at`
//!   and computes `duration_ms`.
//! - `pipeline.wave.failed` flips the row to `Failed`.
//!
//! Waves that only appear in a `pipeline.wave.complete` event (e.g. when the
//! dispatch event was filtered out by attribution gaps pre-v2) still get a
//! row, but with `Completed` status and no agent metadata.

use crate::domain::model::view::{WaveStatus, WaveView};
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
            "pipeline.task.dispatch" => apply_dispatch(&mut by_wave, ev),
            "pipeline.task.complete" => apply_complete(&mut by_wave, ev),
            "pipeline.wave.complete" => apply_wave_complete(&mut by_wave, ev),
            "pipeline.wave.failed" => apply_wave_failed(&mut by_wave, ev),
            _ => {}
        }
    }

    by_wave.into_values().collect()
}

fn ensure_wave(by_wave: &mut BTreeMap<u32, WaveView>, wave: u32) -> &mut WaveView {
    by_wave.entry(wave).or_insert_with(|| WaveView::queued(wave))
}

fn apply_dispatch(by_wave: &mut BTreeMap<u32, WaveView>, ev: &HarnessEvent) {
    let Ok(payload) =
        serde_json::from_value::<PipelineTaskDispatchPayload>(ev.payload.clone())
    else {
        return;
    };
    let Some(wave) = payload.wave else { return };
    let row = ensure_wave(by_wave, wave);
    if row.status == WaveStatus::Queued {
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
