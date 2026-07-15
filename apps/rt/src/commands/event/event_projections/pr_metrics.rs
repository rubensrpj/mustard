//! `pr-metrics` DORA projection. Extracted from `event_projections` (F3 PERF-D split).

use mustard_core::domain::model::event::HarnessEvent;
use serde_json::{json, Value};
use std::path::Path;

/// `buildPRMetrics` — DORA-style metrics from `pr.opened` / `pr.merged` /
/// `review.start` / `review.complete` events within the last `days`.
pub(super) fn build_pr_metrics(events: &[HarnessEvent], cwd: &Path, days: i64, now_ms: i64) -> Value {
    let _ = cwd;
    // `now_ms` is injected (not read from the wall clock here) so the window is
    // a pure function of the inputs — deterministically testable. The production
    // caller passes `now_unix_millis()`.
    let from_ms = now_ms - days * 86_400_000;
    let in_window = |ts: &str| -> bool {
        mustard_core::time::parse_iso_millis(ts)
            .is_some_and(|t| t >= from_ms && t <= now_ms)
    };
    let pair_key = |ev: &HarnessEvent| -> Option<String> {
        ev.payload
            .get("spec")
            .or_else(|| ev.payload.get("branch"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    let (mut opened, mut merged, mut review_start, mut review_complete): (
        Vec<&HarnessEvent>,
        Vec<&HarnessEvent>,
        Vec<&HarnessEvent>,
        Vec<&HarnessEvent>,
    ) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for ev in events {
        if ev.ts.is_empty() || !in_window(&ev.ts) {
            continue;
        }
        match ev.event.as_str() {
            "pr.opened" => opened.push(ev),
            "pr.merged" => merged.push(ev),
            "review.start" => review_start.push(ev),
            "review.complete" => review_complete.push(ev),
            _ => {}
        }
    }

    // Pair opened → merged (earliest opener first; one merge per opener).
    let pair_durations = |starts: &mut Vec<&HarnessEvent>, ends: &[&HarnessEvent]| -> Vec<i64> {
        starts.sort_by(|a, b| a.ts.cmp(&b.ts));
        let mut sorted_ends: Vec<&HarnessEvent> = ends.to_vec();
        sorted_ends.sort_by(|a, b| a.ts.cmp(&b.ts));
        let mut used = vec![false; sorted_ends.len()];
        let mut durations = Vec::new();
        for s in starts.iter() {
            let Some(key) = pair_key(s) else { continue };
            let Some(s_ms) = mustard_core::time::parse_iso_millis(&s.ts) else {
                continue;
            };
            for (i, e) in sorted_ends.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let Some(e_ms) = mustard_core::time::parse_iso_millis(&e.ts) else {
                    continue;
                };
                if e_ms < s_ms || pair_key(e) != Some(key.clone()) {
                    continue;
                }
                durations.push(e_ms - s_ms);
                used[i] = true;
                break;
            }
        }
        durations
    };
    let lead_times = pair_durations(&mut opened, &merged);
    let review_times = pair_durations(&mut review_start, &review_complete);
    let sizes: Vec<i64> = opened
        .iter()
        .filter_map(|e| e.payload.get("linesChanged").and_then(Value::as_i64))
        .filter(|n| *n > 0)
        .collect();

    let stat = |arr: &[i64]| -> Value {
        if arr.is_empty() {
            return json!({ "count": 0, "p50": Value::Null, "p90": Value::Null, "max": Value::Null });
        }
        let mut sorted = arr.to_vec();
        sorted.sort_unstable();
        let pct = |p: usize| -> i64 {
            let idx = ((p as f64 / 100.0) * sorted.len() as f64).floor() as usize;
            sorted[idx.min(sorted.len() - 1)]
        };
        json!({
            "count": sorted.len(),
            "p50": pct(50),
            "p90": pct(90),
            "max": *sorted.last().unwrap_or(&0),
        })
    };
    let bucket_by_day = |arr: &[&HarnessEvent]| -> Value {
        let mut map: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for e in arr {
            let day: String = e.ts.chars().take(10).collect();
            if !day.is_empty() {
                *map.entry(day).or_insert(0) += 1;
            }
        }
        Value::Array(
            map.into_iter()
                .map(|(date, count)| json!({ "date": date, "count": count }))
                .collect(),
        )
    };

    json!({
        "window": { "days": days },
        "totals": {
            "opened": opened.len(),
            "merged": merged.len(),
            "reviewsStarted": review_start.len(),
            "reviewsCompleted": review_complete.len(),
        },
        "leadTimeMs": stat(&lead_times),
        "reviewTimeMs": stat(&review_times),
        "prSize": stat(&sizes),
        "openedByDay": bucket_by_day(&opened),
        "mergedByDay": bucket_by_day(&merged),
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};

    fn ev(event: &str, spec: Option<&str>, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-19T00:00:00.000Z".to_string(),
            session_id: "s1".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: event.to_string(),
            payload,
            spec: spec.map(str::to_string),
        }
    }

    #[test]
    fn pr_metrics_pairs_lead_time() {
        let events = vec![
            ev("pr.opened", None, json!({ "spec": "auth", "linesChanged": 40 })),
            {
                let mut e = ev("pr.merged", None, json!({ "spec": "auth" }));
                e.ts = "2026-05-19T01:00:00.000Z".to_string();
                e
            },
        ];
        let dir = tempfile::tempdir().unwrap();
        // Deterministic window: anchor "now" just after the fixtures so the
        // opened (2026-05-19T00:00Z, the `ev` default) and merged
        // (2026-05-19T01:00Z) events both fall inside the 30-day window — no
        // dependence on the wall clock. This test once rotted silently: the
        // projection read the real clock, so once it passed 2026-05-19 + 30d the
        // hardcoded fixtures fell out of the window and `opened` dropped to 0.
        let now_ms = mustard_core::time::parse_iso_millis("2026-05-19T02:00:00.000Z").unwrap();
        let m = build_pr_metrics(&events, dir.path(), 30, now_ms);
        assert_eq!(m["totals"]["opened"], json!(1));
        assert_eq!(m["totals"]["merged"], json!(1));
        assert_eq!(m["leadTimeMs"]["count"], json!(1));
        assert_eq!(m["prSize"]["count"], json!(1));
    }
}
