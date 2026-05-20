//! [`project_quality`] — Acceptance Criteria roll-up over `qa.result` events.
//!
//! Multiple `qa.result` events may exist for a spec (one per QA run); the
//! projection keeps the **most recent** entry per AC id. This means a spec
//! that has been re-run reflects its latest state, not an aggregate across
//! all runs.

use crate::model::view::{AcStatus, AcceptanceCriterion, QualityRollup};
use crate::model::event::HarnessEvent;
use std::collections::BTreeMap;

/// Fold all `qa.result` events for `spec_name` into a [`QualityRollup`].
///
/// Returns [`QualityRollup::empty`] when no `qa.result` event has been
/// recorded — distinct from `None` (which would suggest "spec not found").
#[must_use]
pub fn project_quality(spec_name: &str, events: &[HarnessEvent]) -> QualityRollup {
    // Per-AC latest entry, keyed by id. BTreeMap so the final vector is
    // sorted by id ("AC-1", "AC-2", …) without an explicit sort pass.
    let mut latest: BTreeMap<String, AcceptanceCriterion> = BTreeMap::new();
    let mut last_run_at: Option<String> = None;

    for ev in events
        .iter()
        .filter(|e| e.spec.as_deref() == Some(spec_name))
        .filter(|e| e.event == "qa.result")
    {
        last_run_at = Some(ev.ts.clone());

        let Some(criteria) = ev.payload.get("criteria").and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        for entry in criteria {
            let Some(id) = entry.get("id").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let status = entry
                .get("status")
                .and_then(serde_json::Value::as_str)
                .and_then(AcStatus::parse)
                .unwrap_or(AcStatus::Pending);
            let label = entry
                .get("label")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(id)
                .to_string();
            let wave = entry
                .get("wave")
                .and_then(serde_json::Value::as_u64)
                .and_then(|w| u32::try_from(w).ok());
            let command = entry
                .get("command")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            let fail_reason = entry
                .get("stderr_excerpt")
                .or_else(|| entry.get("fail_reason"))
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(200).collect::<String>());

            // Newer event wins — `latest.insert` always overwrites.
            latest.insert(
                id.to_string(),
                AcceptanceCriterion {
                    id: id.to_string(),
                    label,
                    status,
                    wave,
                    command,
                    last_run_at: Some(ev.ts.clone()),
                    fail_reason,
                },
            );
        }
    }

    let criteria: Vec<AcceptanceCriterion> = latest.into_values().collect();
    let total = u32::try_from(criteria.len()).unwrap_or(u32::MAX);
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;
    let mut pending = 0u32;
    for c in &criteria {
        match c.status {
            AcStatus::Pass => passed = passed.saturating_add(1),
            AcStatus::Fail => failed = failed.saturating_add(1),
            AcStatus::Skip => skipped = skipped.saturating_add(1),
            AcStatus::Pending => pending = pending.saturating_add(1),
        }
    }
    QualityRollup {
        passed,
        total,
        failed,
        skipped,
        pending,
        last_run_at,
        criteria,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::event::{Actor, ActorKind, SCHEMA_VERSION};
    use serde_json::json;

    fn ev(spec: &str, ts: &str, payload: serde_json::Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.into(),
            session_id: "s1".into(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: "qa.result".into(),
            payload,
            spec: Some(spec.into()),
        }
    }

    #[test]
    fn empty_events_yield_empty_rollup() {
        let r = project_quality("auth", &[]);
        assert_eq!(r.total, 0);
        assert!(r.criteria.is_empty());
        assert!(r.last_run_at.is_none());
    }

    #[test]
    fn rollup_counts_each_status_bucket() {
        let events = vec![ev(
            "auth",
            "2026-05-20T10:00:00Z",
            json!({
                "criteria": [
                    { "id": "AC-1", "status": "pass" },
                    { "id": "AC-2", "status": "pass" },
                    { "id": "AC-3", "status": "fail" },
                    { "id": "AC-4", "status": "skip" },
                    { "id": "AC-5", "status": "pending" },
                ]
            }),
        )];
        let r = project_quality("auth", &events);
        assert_eq!(r.total, 5);
        assert_eq!(r.passed, 2);
        assert_eq!(r.failed, 1);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.pending, 1);
    }

    #[test]
    fn newer_event_overrides_per_ac_status() {
        let events = vec![
            ev(
                "auth",
                "2026-05-20T10:00:00Z",
                json!({
                    "criteria": [
                        { "id": "AC-1", "status": "fail" },
                    ]
                }),
            ),
            ev(
                "auth",
                "2026-05-20T11:00:00Z",
                json!({
                    "criteria": [
                        { "id": "AC-1", "status": "pass" },
                    ]
                }),
            ),
        ];
        let r = project_quality("auth", &events);
        assert_eq!(r.passed, 1);
        assert_eq!(r.failed, 0);
        assert_eq!(r.last_run_at.as_deref(), Some("2026-05-20T11:00:00Z"));
    }

    #[test]
    fn criteria_are_returned_sorted_by_id() {
        let events = vec![ev(
            "auth",
            "2026-05-20T10:00:00Z",
            json!({
                "criteria": [
                    { "id": "AC-3", "status": "pass" },
                    { "id": "AC-1", "status": "pass" },
                    { "id": "AC-2", "status": "fail" },
                ]
            }),
        )];
        let r = project_quality("auth", &events);
        let ids: Vec<_> = r.criteria.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["AC-1", "AC-2", "AC-3"]);
    }

    #[test]
    fn fail_reason_is_truncated_to_200_chars() {
        let long = "x".repeat(500);
        let events = vec![ev(
            "auth",
            "2026-05-20T10:00:00Z",
            json!({
                "criteria": [
                    { "id": "AC-1", "status": "fail", "stderr_excerpt": long },
                ]
            }),
        )];
        let r = project_quality("auth", &events);
        let fail_reason = r.criteria[0].fail_reason.as_ref().unwrap();
        assert_eq!(fail_reason.len(), 200);
    }
}
