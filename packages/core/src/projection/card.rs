//! [`project_spec_view`] — fold the event stream for one spec into a
//! [`SpecView`].
//!
//! Per the dashboard audit (2026-05-20), the dashboard's `spec_views.rs`
//! returned literal `"unknown"` whenever the SQL fallback fired. This fold
//! replaces that path with a deterministic projection over typed events:
//!
//! - `pipeline.scope` populates `scope`, `lang`, `model`, `is_wave_plan`,
//!   `total_waves`. It also transitions `status` away from `NoEvents`.
//! - `pipeline.status` transitions `status` (parsed via [`SpecStatus::parse`]).
//! - `pipeline.phase` updates `phase`.
//! - `pipeline.task.complete` accumulates `files_touched` (deduplicated).
//! - `pipeline.wave.complete` extends `completed_waves` and recomputes
//!   `current_wave`.
//! - `pipeline.complete` flips the status to `Completed`.
//! - `qa.result` overwrites `ac_passed`/`ac_total`/`ac_failed` from the
//!   latest event (newer wins; folds in chronological order).
//! - `tool.use` bumps `tools_used`.
//! - `agent.start` bumps `agents_dispatched`.
//!
//! Events with `spec != Some(spec_name)` are filtered out before the fold —
//! callers that pre-filtered (e.g. `store.query(Some(name))`) pay zero cost.

use crate::model::view::{Scope, SpecStatus, SpecView};
use crate::model::event::{
    HarnessEvent, PipelineScopePayload, PipelineTaskCompletePayload, PipelineWaveCompletePayload,
};
use std::collections::BTreeSet;

use super::{extract_to_phase, iso_diff_ms};

/// Fold `events` into a [`SpecView`] for `spec_name`.
///
/// Events that don't belong to `spec_name` are skipped. Order matters: the
/// projection assumes chronological order (oldest first), which matches
/// `SqliteEventStore::query` semantics.
#[must_use]
pub fn project_spec_view(spec_name: &str, events: &[HarnessEvent]) -> SpecView {
    let mut view = SpecView::empty(spec_name);
    let mut files: BTreeSet<String> = BTreeSet::new();

    for ev in events.iter().filter(|e| e.spec.as_deref() == Some(spec_name)) {
        // Time bookkeeping — every event refreshes `last_event_at` and may
        // seed `started_at`. Done before the per-event match so even Other
        // events anchor the timeline correctly.
        if view.started_at.is_none() {
            view.started_at = Some(ev.ts.clone());
        }
        view.last_event_at = Some(ev.ts.clone());

        match ev.event.as_str() {
            "pipeline.scope" => apply_scope(&mut view, ev),
            "pipeline.status" => apply_status(&mut view, ev),
            "pipeline.phase" => apply_phase(&mut view, ev),
            "pipeline.task.complete" => apply_task_complete(ev, &mut files),
            "pipeline.wave.complete" => apply_wave_complete(&mut view, ev),
            "pipeline.wave.failed" => apply_wave_failed(&mut view, ev),
            "pipeline.complete" => view.status = SpecStatus::Completed,
            "qa.result" => apply_qa_result(&mut view, ev),
            "tool.use" => view.tools_used = view.tools_used.saturating_add(1),
            "agent.start" => view.agents_dispatched = view.agents_dispatched.saturating_add(1),
            _ => {}
        }
    }

    view.files_touched = u32::try_from(files.len()).unwrap_or(u32::MAX);

    // Duration: only meaningful when both timestamps exist.
    if let (Some(start), Some(end)) = (view.started_at.as_deref(), view.last_event_at.as_deref()) {
        view.duration_ms = iso_diff_ms(start, end);
    }

    // current_wave: max completed + 1, capped at total_waves.
    if let Some(total) = view.total_waves {
        let max_completed = view.completed_waves.iter().copied().max().unwrap_or(0);
        view.current_wave = Some((max_completed + 1).min(total));
    }

    view
}

/// `pipeline.scope` — first observation of a spec's metadata. Promotes the
/// view from `NoEvents` to `Planning` and records scope/lang/model.
fn apply_scope(view: &mut SpecView, ev: &HarnessEvent) {
    if let Ok(payload) = serde_json::from_value::<PipelineScopePayload>(ev.payload.clone()) {
        view.scope = Scope::parse(&payload.scope);
        view.lang = payload.lang;
        view.model = payload.model;
        view.is_wave_plan = payload.is_wave_plan.unwrap_or(false);
        view.total_waves = payload.total_waves;
        // First scope event → leaves NoEvents behind. Status transitions
        // beyond Planning happen via `pipeline.status`.
        if view.status == SpecStatus::NoEvents {
            view.status = SpecStatus::Planning;
        }
    }
}

/// `pipeline.status` — typed transitions. Unknown strings leave status
/// unchanged rather than dropping back to `NoEvents`.
fn apply_status(view: &mut SpecView, ev: &HarnessEvent) {
    let Some(to) = ev.payload.get("to").and_then(serde_json::Value::as_str) else {
        return;
    };
    if let Some(parsed) = SpecStatus::parse(to) {
        view.status = parsed;
    }
}

/// `pipeline.phase` — current phase. Parsed via [`Phase::parse`].
fn apply_phase(view: &mut SpecView, ev: &HarnessEvent) {
    if let Some(phase) = extract_to_phase(ev) {
        view.phase = Some(phase);
    }
}

/// `pipeline.task.complete` — accumulates `files_touched` (deduplicated
/// across all tasks). Decoding failures skip the row, matching the rest of
/// the harness's fail-open style.
fn apply_task_complete(ev: &HarnessEvent, files: &mut BTreeSet<String>) {
    let Ok(payload) = serde_json::from_value::<PipelineTaskCompletePayload>(ev.payload.clone()) else {
        return;
    };
    if let Some(modified) = payload.files_modified {
        files.extend(modified);
    }
}

/// `pipeline.wave.complete` — track the wave number.
fn apply_wave_complete(view: &mut SpecView, ev: &HarnessEvent) {
    let Ok(payload) = serde_json::from_value::<PipelineWaveCompletePayload>(ev.payload.clone())
    else {
        return;
    };
    if !view.completed_waves.contains(&payload.wave) {
        view.completed_waves.push(payload.wave);
        view.completed_waves.sort_unstable();
    }
}

/// `pipeline.wave.failed` — track failed waves. The event has no typed
/// payload struct in `mustard-core` yet, so we read the `wave` field directly.
fn apply_wave_failed(view: &mut SpecView, ev: &HarnessEvent) {
    let Some(wave) = ev
        .payload
        .get("wave")
        .and_then(serde_json::Value::as_u64)
        .and_then(|w| u32::try_from(w).ok())
    else {
        return;
    };
    if !view.failed_waves.contains(&wave) {
        view.failed_waves.push(wave);
        view.failed_waves.sort_unstable();
    }
    view.status = SpecStatus::WaveFailed;
}

/// `qa.result` — overwrite the AC counts with the latest event's numbers.
/// Folds in chronological order so the last one wins.
fn apply_qa_result(view: &mut SpecView, ev: &HarnessEvent) {
    // Two payload shapes exist in the wild: the original `qa_run.rs`
    // emits a `criteria` array; some earlier emitters embedded `passed`/
    // `total` directly. Try the array form first.
    if let Some(criteria) = ev.payload.get("criteria").and_then(serde_json::Value::as_array) {
        let mut passed = 0u32;
        let mut failed = 0u32;
        for entry in criteria {
            let status = entry
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match status {
                "pass" => passed = passed.saturating_add(1),
                "fail" | "error" => failed = failed.saturating_add(1),
                _ => {}
            }
        }
        let total = u32::try_from(criteria.len()).unwrap_or(u32::MAX);
        view.ac_passed = passed;
        view.ac_failed = failed;
        view.ac_total = total;
        return;
    }

    // Legacy / shorthand payload form: numeric `passed`/`total`/`failed`.
    if let Some(passed) = ev
        .payload
        .get("passed")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
    {
        view.ac_passed = passed;
    }
    if let Some(total) = ev
        .payload
        .get("total")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
    {
        view.ac_total = total;
    }
    if let Some(failed) = ev
        .payload
        .get("failed")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
    {
        view.ac_failed = failed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::view::Phase;
    use crate::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use serde_json::json;

    /// Build a minimal event with given kind and payload, scoped to `spec`.
    fn event(spec: &str, ts: &str, kind: &str, payload: serde_json::Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.to_string(),
            session_id: "s1".into(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: kind.into(),
            payload,
            spec: Some(spec.into()),
        }
    }

    #[test]
    fn empty_events_yield_empty_view() {
        let view = project_spec_view("feature-a", &[]);
        assert_eq!(view.spec, "feature-a");
        assert_eq!(view.status, SpecStatus::NoEvents);
        assert_eq!(view.tools_used, 0);
        assert!(view.started_at.is_none());
    }

    #[test]
    fn events_for_other_specs_are_skipped() {
        let events = vec![
            event("feature-a", "2026-05-20T10:00:00Z", "tool.use", json!({})),
            event("feature-b", "2026-05-20T10:01:00Z", "tool.use", json!({})),
            event("feature-a", "2026-05-20T10:02:00Z", "tool.use", json!({})),
        ];
        let view = project_spec_view("feature-a", &events);
        assert_eq!(view.tools_used, 2);
    }

    #[test]
    fn scope_event_transitions_status_and_records_metadata() {
        let events = vec![event(
            "feature-a",
            "2026-05-20T10:00:00Z",
            "pipeline.scope",
            json!({
                "scope": "full",
                "lang": "pt",
                "model": "opus",
                "is_wave_plan": true,
                "total_waves": 4
            }),
        )];
        let view = project_spec_view("feature-a", &events);
        assert_eq!(view.status, SpecStatus::Planning);
        assert_eq!(view.scope, Some(Scope::Full));
        assert_eq!(view.lang.as_deref(), Some("pt"));
        assert_eq!(view.model.as_deref(), Some("opus"));
        assert!(view.is_wave_plan);
        assert_eq!(view.total_waves, Some(4));
    }

    #[test]
    fn status_events_transition_lifecycle_with_unknown_values_ignored() {
        let events = vec![
            event(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.status",
                json!({ "to": "implementing" }),
            ),
            event(
                "auth",
                "2026-05-20T10:01:00Z",
                "pipeline.status",
                json!({ "to": "garbage-state" }), // unknown → ignored
            ),
            event(
                "auth",
                "2026-05-20T10:02:00Z",
                "pipeline.status",
                json!({ "to": "completed" }),
            ),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.status, SpecStatus::Completed);
    }

    #[test]
    fn phase_event_updates_phase() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.phase",
            json!({ "to": "execute" }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.phase, Some(Phase::Execute));
    }

    #[test]
    fn task_complete_accumulates_distinct_files() {
        let events = vec![
            event(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.task.complete",
                json!({ "name": "wave-1", "files_modified": ["src/a.rs", "src/b.rs"] }),
            ),
            event(
                "auth",
                "2026-05-20T10:05:00Z",
                "pipeline.task.complete",
                json!({ "name": "wave-2", "files_modified": ["src/b.rs", "src/c.rs"] }),
            ),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.files_touched, 3); // a, b, c deduplicated
    }

    #[test]
    fn wave_complete_extends_list_and_drives_current_wave() {
        let events = vec![
            event(
                "auth",
                "2026-05-20T10:00:00Z",
                "pipeline.scope",
                json!({ "scope": "full", "total_waves": 4 }),
            ),
            event(
                "auth",
                "2026-05-20T10:05:00Z",
                "pipeline.wave.complete",
                json!({ "wave": 1 }),
            ),
            event(
                "auth",
                "2026-05-20T10:10:00Z",
                "pipeline.wave.complete",
                json!({ "wave": 2 }),
            ),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.completed_waves, vec![1, 2]);
        assert_eq!(view.current_wave, Some(3));
        assert_eq!(view.total_waves, Some(4));
    }

    #[test]
    fn qa_result_with_criteria_array_counts_pass_fail_total() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "qa.result",
            json!({
                "criteria": [
                    { "id": "AC-1", "status": "pass" },
                    { "id": "AC-2", "status": "pass" },
                    { "id": "AC-3", "status": "fail" },
                    { "id": "AC-4", "status": "skip" },
                ]
            }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.ac_total, 4);
        assert_eq!(view.ac_passed, 2);
        assert_eq!(view.ac_failed, 1);
    }

    #[test]
    fn qa_result_with_legacy_shorthand_counts_numeric_fields() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "qa.result",
            json!({ "passed": 5, "total": 7, "failed": 2 }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.ac_total, 7);
        assert_eq!(view.ac_passed, 5);
        assert_eq!(view.ac_failed, 2);
    }

    #[test]
    fn tool_use_and_agent_start_bump_counters() {
        let events = vec![
            event("auth", "2026-05-20T10:00:00Z", "tool.use", json!({})),
            event("auth", "2026-05-20T10:00:01Z", "tool.use", json!({})),
            event("auth", "2026-05-20T10:00:02Z", "agent.start", json!({})),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.tools_used, 2);
        assert_eq!(view.agents_dispatched, 1);
    }

    #[test]
    fn duration_is_diff_between_first_and_last_event() {
        let events = vec![
            event("auth", "2026-05-20T10:00:00Z", "tool.use", json!({})),
            event("auth", "2026-05-20T10:00:30Z", "tool.use", json!({})),
        ];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.started_at.as_deref(), Some("2026-05-20T10:00:00Z"));
        assert_eq!(view.last_event_at.as_deref(), Some("2026-05-20T10:00:30Z"));
        assert_eq!(view.duration_ms, Some(30_000));
    }

    #[test]
    fn pipeline_complete_transitions_to_completed_status() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.complete",
            json!({ "closedAt": "2026-05-20T10:00:00Z" }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.status, SpecStatus::Completed);
    }

    #[test]
    fn wave_failed_marks_status_and_records_wave() {
        let events = vec![event(
            "auth",
            "2026-05-20T10:00:00Z",
            "pipeline.wave.failed",
            json!({ "wave": 3, "reason": "build-broken" }),
        )];
        let view = project_spec_view("auth", &events);
        assert_eq!(view.status, SpecStatus::WaveFailed);
        assert_eq!(view.failed_waves, vec![3]);
    }
}
