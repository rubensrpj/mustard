//! `pipeline-state` + `active-pipelines` projections. Extracted from
//! `event_projections` (F3 PERF-D split).

use mustard_core::ClaudePaths;
use mustard_core::io::fs;
use mustard_core::domain::model::event::{HarnessEvent, EVENT_PIPELINE_DISPATCH_FAILURE};
use mustard_core::domain::model::view::{Phase, Stage};
use mustard_core::view::projection::project_spec_view_with_header;
use serde_json::{json, Value};
use std::path::Path;

/// Bucket key for a failure recorded before any `pipeline.phase` event named
/// the phase it happened in — the failure is real, only its phase is unknown.
const UNKNOWN_PHASE: &str = "unknown";

/// `buildPipelineState` — current phase + dispatch failures + metrics.
///
/// `pub(crate)`: `run metrics collect` folds this same projection per spec so
/// the two commands report one set of numbers from one reader.
///
/// `dispatchFailures` / `dispatchFailuresByPhase` / `retries` are folded from
/// the real `pipeline.dispatch_failure` and `retry.attempt` events. They used to
/// be declared empty and never written, which made `retries` structurally `0`
/// and every consumer's first-pass rate structurally `100%` — a number nobody
/// measured. A phase-less failure lands in the [`UNKNOWN_PHASE`] bucket rather
/// than being dropped: losing the attribution must not lose the count.
pub(crate) fn build_pipeline_state(events: &[HarnessEvent], spec: Option<&str>) -> Value {
    let mut phase: Option<String> = None;
    let mut last_event_at: Option<String> = None;
    let mut started_at: Option<String> = None;
    let mut dispatch_failures: Vec<Value> = Vec::new();
    let mut decisions: Vec<Value> = Vec::new();
    let mut lessons: Vec<Value> = Vec::new();
    let mut api_calls = 0i64;
    let mut tool_breakdown: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut agent_count = 0i64;
    let mut failures_by_phase: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut retry_attempts = 0i64;

    for ev in events {
        if let Some(s) = spec {
            if ev.spec.as_deref() != Some(s) {
                continue;
            }
        }
        if !ev.ts.is_empty() {
            if started_at.is_none() {
                started_at = Some(ev.ts.clone());
            }
            last_event_at = Some(ev.ts.clone());
        }
        match ev.event.as_str() {
            "pipeline.phase" => {
                if let Some(to) = ev.payload.get("to").and_then(Value::as_str) {
                    phase = Some(to.to_string());
                } else if let Some(from) = ev.payload.get("from").and_then(Value::as_str) {
                    phase = Some(from.to_string());
                }
            }
            EVENT_PIPELINE_DISPATCH_FAILURE => {
                let bucket = phase.as_deref().unwrap_or(UNKNOWN_PHASE);
                let n = failures_by_phase.get(bucket).and_then(Value::as_i64).unwrap_or(0);
                failures_by_phase.insert(bucket.to_string(), json!(n + 1));
                dispatch_failures.push(json!({
                    "at": ev.payload.get("at").and_then(Value::as_str).unwrap_or(&ev.ts),
                    "phase": bucket,
                    "reason": ev.payload.get("reason").and_then(Value::as_str),
                    "agentType": ev.payload.get("agent_type").and_then(Value::as_str),
                }));
            }
            "retry.attempt" => retry_attempts += 1,
            "decision" => decisions.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "lesson" => lessons.push(serde_json::to_value(ev).unwrap_or(Value::Null)),
            "tool.use" => {
                let tool = ev.payload.get("tool").and_then(Value::as_str).unwrap_or("unknown");
                if tool != "Read" {
                    api_calls += 1;
                    let n = tool_breakdown.get(tool).and_then(Value::as_i64).unwrap_or(0);
                    tool_breakdown.insert(tool.to_string(), json!(n + 1));
                }
            }
            "agent.start" => agent_count += 1,
            _ => {}
        }
    }

    json!({
        "spec": spec,
        "phase": phase,
        "lastEventAt": last_event_at,
        "dispatchFailures": dispatch_failures,
        "decisions": decisions,
        "lessons": lessons,
        "metrics": {
            "apiCalls": api_calls,
            "toolBreakdown": tool_breakdown,
            // Every re-attempt the log knows about: a dispatch that had to be
            // re-issued plus an explicit `retry.attempt` (the signal
            // `metrics wave-status` already counts).
            "retries": failures_by_phase.values().filter_map(Value::as_i64).sum::<i64>()
                + retry_attempts,
            "agentCount": agent_count,
            "startedAt": started_at,
            "dispatchFailuresByPhase": failures_by_phase,
        },
    })
}

/// `buildActivePipelines` — specs that have at least one event (or a parseable
/// header on disk) and whose last `pipeline.stage` / `pipeline.phase` (event
/// stream) or `### Stage:` header (disk fallback) is not `Close`. Ordered by
/// `lastEventAt` descending. Specs with no activity in the last 30 days are
/// also excluded (defensive filter).
///
/// Two-pass algorithm:
/// 1. Fold the event stream exactly as before, populating `per_spec`.
/// 2. Glob `.claude/spec/*/spec.md` (+ `wave-plan.md` fallback). For every
///    spec present on disk but absent from the event-stream map, delegate to
///    `mustard_core::view::projection::project_spec_view_with_header` which parses
///    the `### Stage:` / `### Outcome:` header and emits a synthetic
///    `pipeline.status` event into the local SQLite store. The resulting
///    `SpecView` is merged into `per_spec` before the filter+sort step.
pub(super) fn build_active_pipelines(events: &[HarnessEvent], cwd: &Path) -> Value {
    use std::collections::BTreeMap;

    let now_ms = mustard_core::time::now_unix_millis() as u128 as i64;
    // 30-day window in milliseconds.
    let cutoff_ms = now_ms - 30 * 86_400_000;

    // Per-spec accumulator: (last_event_ts, last_stage).
    let mut per_spec: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();

    // --- Pass 1: fold the event stream ---

    for ev in events {
        let Some(spec) = ev.spec.as_deref() else { continue };
        if ev.ts.is_empty() { continue; }

        let entry = per_spec.entry(spec.to_string()).or_insert_with(|| (ev.ts.clone(), None));
        // Track the maximum timestamp (ISO-8601 lexicographic order is correct for UTC).
        if ev.ts > entry.0 {
            entry.0.clone_from(&ev.ts);
        }
        // Track the last stage from `pipeline.stage` (Title Case) OR
        // `pipeline.phase` (UPPERCASE → Title Case). Whichever appears latest
        // in the event stream wins (lexicographic ts comparison is safe for UTC).
        match ev.event.as_str() {
            "pipeline.stage" => {
                if let Some(stage) = ev.payload.get("to").and_then(Value::as_str) {
                    entry.1 = Some(stage.to_string());
                }
            }
            "pipeline.phase" => {
                let raw = ev.payload.get("to")
                    .or_else(|| ev.payload.get("from"))
                    .and_then(Value::as_str);
                if let Some(r) = raw {
                    if let Some(p) = Phase::parse(r) {
                        entry.1 = Some(format!("{p:?}"));
                    }
                }
            }
            _ => {}
        }
    }

    // --- Pass 2: disk fallback for specs absent from the event stream ---
    //
    // Glob `.claude/spec/*/spec.md` + `wave-plan.md`. For each spec whose
    // directory name is not already in `per_spec`, delegate to the canonical
    // core projection with the header fallback enabled. This covers the "git
    // pull brings a new spec from a teammate; no local event has been emitted
    // yet" case.
    let spec_root = ClaudePaths::for_project(cwd)
        .map(|p| p.spec_dir())
        .unwrap_or_else(|_| cwd.to_path_buf());
    if let Ok(rd) = fs::read_dir(&spec_root) {
        for entry in rd {
            let spec_dir = entry.path;
            if !spec_dir.is_dir() { continue; }

            let spec_name = match spec_dir.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Only process specs that have NO events — the event stream already
            // won for the others.
            if per_spec.contains_key(&spec_name) { continue; }

            // Resolve the spec.md path; accept wave-plan.md as a fallback.
            let spec_md = spec_dir.join("spec.md");
            let wave_plan_md = spec_dir.join("wave-plan.md");
            let spec_md_path = if spec_md.exists() {
                spec_md.clone()
            } else if wave_plan_md.exists() {
                wave_plan_md.clone()
            } else {
                continue; // no readable spec file — skip
            };

            // Delegate to the canonical core projection (parses header).
            // No SQLite sink — the filesystem header is the source of truth.
            let view = project_spec_view_with_header(
                &spec_name,
                events,
                Some(spec_md_path.as_path()),
            );

            // Extract the stage string. Use Debug formatting to match the
            // phase-based path above (`format!("{p:?}")`): "Execute", "Plan", etc.
            let stage_str = format!("{:?}", view.state.stage);

            // Skip specs whose header says Close (non-active) or has no stage
            // signal (NoEvents maps to Stage::Plan — treat as active, include).
            // Stage::Close means terminal; exclude.
            if view.state.stage == Stage::Close {
                continue;
            }

            // Derive a `lastEventAt` sentinel from the spec.md mtime when the
            // header fallback fires (last_event_at == None from the projection).
            // This ensures the row has a sortable timestamp and passes the
            // 30-day cutoff filter. Falls back to now_iso8601 when mtime is
            // unavailable so the row is always included.
            let last_event_at = view.last_event_at.clone().unwrap_or_else(|| {
                // Use spec.md mtime as a proxy for "when the spec was written".
                std::fs::metadata(&spec_md_path)
                    .and_then(|m| m.modified())
                    .map_or_else(|_| mustard_core::time::now_iso8601(), |mtime| {
                        use std::time::UNIX_EPOCH;
                        let secs = mtime
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs() as i64);
                        // Seconds-precision ISO sentinel (no `.mmm`); the
                        // calendar math lives in `mustard_core::time`.
                        let tod = secs.rem_euclid(86_400);
                        let (y, m, d) = mustard_core::time::civil_from_days(secs.div_euclid(86_400));
                        let (h, mi, s) = (tod / 3_600, (tod % 3_600) / 60, tod % 60);
                        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
                    })
            });

            per_spec.insert(spec_name, (last_event_at, Some(stage_str)));
        }
    }

    let mut pipelines: Vec<Value> = per_spec
        .into_iter()
        .filter(|(_, (last_ts, last_stage))| {
            // Exclude specs that never emitted a pipeline.stage event (legacy/abandoned).
            if last_stage.is_none() {
                return false;
            }
            // Exclude specs whose last stage is Close.
            if last_stage.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("close")) {
                return false;
            }
            // Exclude specs with no activity in the last 30 days.
            let ts_ms = mustard_core::time::parse_iso_millis(last_ts).unwrap_or(0);
            ts_ms >= cutoff_ms
        })
        .map(|(spec, (last_event_at, last_stage))| {
            json!({
                "spec": spec,
                "lastEventAt": last_event_at,
                "stage": last_stage,
            })
        })
        .collect();

    // Sort by lastEventAt descending (ISO-8601 lexicographic comparison is safe for UTC).
    pipelines.sort_by(|a, b| {
        let ta = a["lastEventAt"].as_str().unwrap_or("");
        let tb = b["lastEventAt"].as_str().unwrap_or("");
        tb.cmp(ta)
    });

    json!({ "pipelines": pipelines })
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
    fn pipeline_state_counts_tool_use_and_phase() {
        let events = vec![
            ev("pipeline.phase", Some("demo"), json!({ "to": "EXECUTE" })),
            ev("tool.use", Some("demo"), json!({ "tool": "Edit" })),
            ev("tool.use", Some("demo"), json!({ "tool": "Read" })),
        ];
        let v = build_pipeline_state(&events, Some("demo"));
        assert_eq!(v["phase"], json!("EXECUTE"));
        // Read is excluded from apiCalls.
        assert_eq!(v["metrics"]["apiCalls"], json!(1));
    }

    #[test]
    fn pipeline_state_counts_real_retries_by_phase() {
        let events = vec![
            ev("pipeline.phase", Some("demo"), json!({ "to": "EXECUTE" })),
            ev(
                "pipeline.dispatch_failure",
                Some("demo"),
                json!({ "reason": "agent refused", "agent_type": "implement" }),
            ),
            ev("retry.attempt", Some("demo"), json!({})),
        ];
        let v = build_pipeline_state(&events, Some("demo"));
        // One dispatch failure + one retry attempt = two re-attempts, and the
        // failure is attributed to the phase that was current when it fired.
        assert_eq!(v["metrics"]["retries"], json!(2));
        assert_eq!(v["metrics"]["dispatchFailuresByPhase"]["EXECUTE"], json!(1));
        assert_eq!(v["dispatchFailures"][0]["reason"], json!("agent refused"));
    }

    #[test]
    fn dispatch_failure_before_any_phase_is_still_counted() {
        // No `pipeline.phase` yet: the count must survive even when the phase
        // attribution cannot (dropping it would under-report a real failure).
        let events = vec![ev("pipeline.dispatch_failure", Some("demo"), json!({}))];
        let v = build_pipeline_state(&events, Some("demo"));
        assert_eq!(v["metrics"]["retries"], json!(1));
        assert_eq!(v["metrics"]["dispatchFailuresByPhase"][UNKNOWN_PHASE], json!(1));
    }

    #[test]
    fn a_clean_run_reports_zero_retries_not_a_missing_key() {
        // The honest zero: the log WAS read and holds no failure. `metrics
        // collect` distinguishes this from an absent key (which it publishes as
        // `unknown`), so the key must always be present.
        let events = vec![ev("pipeline.phase", Some("demo"), json!({ "to": "PLAN" }))];
        let v = build_pipeline_state(&events, Some("demo"));
        assert_eq!(v["metrics"]["retries"], json!(0));
    }
}
