//! `mustard-rt run event-projections` — a port of `scripts/event-projections.js`.
//!
//! Read-only projections over the harness NDJSON event log
//! (`.claude/spec/*/.events/*.ndjson`). Each view derives a JSON document from
//! the replayed events; the CLI prints it to stdout. Exit `0` always
//! (fail-open).
//!
//! Views ported: `agent-visibility`, `pipeline-state`, `session-summary`,
//! `epic-summary`, `cross-session-timeline`, `spec-tree`, `pr-metrics`. The JS
//! `buildSlopeReport` projection is **deliberately not ported** — B3 deleted
//! the `duplication.warn` / `convention.warn` hooks that fed it, so nothing
//! emits those events anymore (b4 spec, dead-code removal). An unknown
//! `--view` returns `{ "error": ... }`.
//!
//! `--format json` (default) prints the projection. `--format html` wraps the
//! same JSON in a standalone HTML page and prints its path on stderr.

use crate::report::Report;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
use mustard_core::domain::model::event::{
    HarnessEvent, EVENT_PIPELINE_COMPLETE, EVENT_PIPELINE_DISPATCH_FAILURE,
    EVENT_PIPELINE_PAUSE, EVENT_PIPELINE_RESUME_MODE, EVENT_PIPELINE_SCOPE,
    EVENT_PIPELINE_STATUS, EVENT_PIPELINE_TASK_COMPLETE, EVENT_PIPELINE_TASK_DISPATCH,
    EVENT_PIPELINE_WAVE_COMPLETE, PipelineCompletePayload, PipelineDispatchFailurePayload,
};
use mustard_core::view::projection::read_workspace_events as core_read_workspace_events;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

mod agent;
mod pipeline;
mod session;
mod epic;
mod spec_tree;
mod pr_metrics;

/// Re-export of [`mustard_core::view::projection::read_workspace_events`] under the
/// crate path so existing rt callers (`resume_bootstrap`, `spec_children_tree`,
/// the projection `project` dispatcher below) continue to use the short name.
///
/// W8A-2 (no-sqlite Wave 8): the canonical walker moved to `mustard-core` so
/// both the rt crate and the dashboard Tauri layer can fold over the same
/// event slice without duplicating the converter logic.
pub(crate) fn read_workspace_events(cwd: &Path) -> Vec<HarnessEvent> {
    core_read_workspace_events(cwd)
}

/// Derive the latest phase for `spec` from a replayed event list. The single
/// source of truth post-`pipeline.phase` migration — phase no longer lives in
/// pipeline-state JSON. Returns the `to` field of the most recent
/// `pipeline.phase` event for the spec, or `None` if none exists.
fn phase_from_events(events: &[HarnessEvent], spec: &str) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|e| e.event == "pipeline.phase" && e.spec.as_deref() == Some(spec))
        .and_then(|e| {
            e.payload
                .get("to")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

/// Default `cross-session-timeline` session limit (`DEFAULT_CROSS_SESSION_LIMIT`).
const CROSS_SESSION_LIMIT: usize = 3;

/// Compute the projection for a `--view`.
fn project(cwd: &Path, view: &str, spec: Option<&str>, wave: Option<u32>) -> Value {
    match view {
        "agent-visibility" => agent::build_agent_visibility(&read_workspace_events(cwd), wave),
        "pipeline-state" => pipeline::build_pipeline_state(&read_workspace_events(cwd), spec),
        "session-summary" => session::build_session_summary(&read_workspace_events(cwd)),
        "epic-summary" => match spec {
            Some(s) => epic::build_epic_summary(&read_workspace_events(cwd), cwd, s),
            None => json!({ "error": "--spec is required for epic-summary view" }),
        },
        "cross-session-timeline" => {
            // `--wave` doubles as the optional `--limit` for this view.
            let limit = wave.map_or(CROSS_SESSION_LIMIT, |w| w as usize);
            session::build_cross_session_timeline(cwd, limit)
        }
        "spec-tree" => match spec {
            Some(s) => spec_tree::build_spec_tree(&read_workspace_events(cwd), cwd, s),
            None => json!({ "error": "--spec is required for spec-tree view" }),
        },
        "pr-metrics" => {
            // `--wave` doubles as the optional `--days` window for this view.
            let days = wave.map_or(30, i64::from);
            pr_metrics::build_pr_metrics(&read_workspace_events(cwd), cwd, days, mustard_core::time::now_unix_millis() as u128 as i64)
        }
        "active-pipelines" => pipeline::build_active_pipelines(&read_workspace_events(cwd), cwd),
        other => json!({ "error": format!("Unknown view: {other}") }),
    }
}

/// Write the standalone HTML report wrapping the projection JSON.
///
/// Event-projection reports are *not* per-spec QA reports — they are
/// workspace-wide diagnostic views. The W2 cache reorg keeps them under
/// `<root>/.claude/.metrics/event-projections/` (rather than the legacy
/// `.qa-reports/` directory, which is now reserved for the per-spec
/// `spec/{name}/qa-report.{json,html}` pair).
fn write_html_report(cwd: &Path, view: &str, json_text: &str) -> Option<PathBuf> {
    let paths = ClaudePaths::for_project(cwd).ok()?;
    let dir = paths.metrics_dir().join("event-projections");
    fs::create_dir_all(&dir).ok()?;
    let mut report = Report::new(format!("Event Projection — {view}"), "harness event log view");
    report.pre_section("Projection", json_text);
    let path = dir.join(format!("event-projection-{view}.html"));
    fs::write_atomic(&path, report.render().as_bytes()).ok()?;
    Some(path)
}

/// Dispatch `mustard-rt run event-projections`.
pub fn run(view: Option<&str>, spec: Option<&str>, wave: Option<u32>, format: &str) {
    let Some(view) = view else {
        eprintln!("Usage: event-projections --view <name> [--spec <name>] [--wave <n>] [--format json|html]");
        eprintln!("Views: agent-visibility, pipeline-state, session-summary, epic-summary, cross-session-timeline, spec-tree, pr-metrics, active-pipelines");
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = project(&cwd, view, spec, wave);
    let json_text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());

    if format == "html" {
        match write_html_report(&cwd, view, &json_text) {
            Some(path) => eprintln!("[event-projections] HTML report: {}", path.display()),
            None => eprintln!("[event-projections] WARN: could not write HTML report"),
        }
    }
    println!("{json_text}");
}

// ---------------------------------------------------------------------------
// Typed pipeline-state projection — Wave 2 of 2026-05-19-pipeline-state-from-sqlite
// ---------------------------------------------------------------------------

/// A single task tracked inside a pipeline run. Built from `pipeline.task.dispatch`
/// and `pipeline.task.complete` event pairs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PipelineTaskView {
    /// Human-readable task name matching the wave-plan heading.
    pub name: String,
    /// Agent sub-type used for this dispatch (e.g. `"general-purpose"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Wave number the task belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave: Option<u32>,
    /// Role label (e.g. `"implement"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Files in scope at dispatch time, plus any reported as modified at complete.
    #[serde(default)]
    pub files: Vec<String>,
    /// `"pending"` until a matching `pipeline.task.complete` is seen, then `"completed"`.
    pub status: String,
    /// ISO-8601 timestamp of the dispatch event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched_at: Option<String>,
    /// ISO-8601 timestamp of the complete event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// Wall-clock task duration from the complete payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Typed view of a spec's pipeline state, derived entirely from the event log.
/// Mirrors the legacy `.pipeline-states/{spec}.json` shape. camelCase serde
/// renames match the dashboard's existing JSON consumers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PipelineStateView {
    /// Spec identifier.
    pub spec: String,
    /// Last `pipeline.status` value (`payload.to`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Pipeline scope token, e.g. `"full"` or `"wave"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Spec language override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Model routed to for this pipeline run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `true` when the spec uses a wave plan (from event or FS fallback).
    #[serde(default, rename = "isWavePlan", skip_serializing_if = "Option::is_none")]
    pub is_wave_plan: Option<bool>,
    /// Total wave count declared in the spec.
    #[serde(default, rename = "totalWaves", skip_serializing_if = "Option::is_none")]
    pub total_waves: Option<u32>,
    /// Next wave to dispatch: `max(completed_waves) + 1`, or `1`.
    #[serde(rename = "currentWave")]
    pub current_wave: u32,
    /// Wave numbers that have emitted a `pipeline.wave.complete` event, sorted ASC.
    #[serde(rename = "completedWaves")]
    pub completed_waves: Vec<u32>,
    /// Task views built from dispatch+complete pairs.
    pub tasks: Vec<PipelineTaskView>,
    /// Most recent `pipeline.dispatch_failure` payload, cleared if older than 10 min.
    #[serde(default, rename = "lastDispatchFailure", skip_serializing_if = "Option::is_none")]
    pub last_dispatch_failure: Option<PipelineDispatchFailurePayload>,
    /// ISO-8601 timestamp of the last `pipeline.pause` event.
    #[serde(default, rename = "pausedAt", skip_serializing_if = "Option::is_none")]
    pub paused_at: Option<String>,
    /// Human-readable pause reason.
    #[serde(default, rename = "pauseReason", skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    /// Resume mode selected (e.g. `"continue"`, `"rewrite"`, `"abort"`).
    #[serde(default, rename = "resumeMode", skip_serializing_if = "Option::is_none")]
    pub resume_mode: Option<String>,
    /// ISO-8601 timestamp at which the pipeline was closed (from `pipeline.complete`).
    /// Falls back to the last `pipeline.status` event's `ts` when the complete event
    /// is absent but status is `"closed-followup"`.
    #[serde(default, rename = "closedAt", skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    /// Files touched during the pipeline run (from `pipeline.complete`).
    #[serde(default, rename = "affectedFiles")]
    pub affected_files: Vec<String>,
}

/// Ten minutes in milliseconds — the stale-failure cutoff matching the `/resume` Step 0 contract.
const DISPATCH_FAILURE_TTL_MS: i64 = 10 * 60 * 1_000;

/// Derive a [`PipelineStateView`] for `spec` by folding a pre-fetched event
/// slice. This is the canonical implementation used by all new call sites.
///
/// Fail-open on malformed payloads — a bad event is logged to stderr and
/// skipped. Returns `None` when no events exist for the spec.
///
/// `spec_dir` is an optional filesystem path to the spec directory
/// (`.claude/spec/{spec}` — flat layout). When provided and `wave-plan.md`
/// exists there, `is_wave_plan` is set to `true` even if no `pipeline.scope`
/// event recorded it yet.
#[must_use]
pub fn pipeline_state_from_events(
    events: &[HarnessEvent],
    spec: &str,
    spec_dir: Option<&std::path::Path>,
) -> Option<PipelineStateView> {
    // Filter to events for this spec only.
    let events: Vec<&HarnessEvent> = events
        .iter()
        .filter(|e| e.spec.as_deref() == Some(spec))
        .collect();
    if events.is_empty() {
        return None;
    }

    let mut view = PipelineStateView {
        spec: spec.to_string(),
        current_wave: 1,
        ..Default::default()
    };

    // Raw dispatch-failure payload + its timestamp (cleared after fold if stale).
    let mut raw_failure: Option<(PipelineDispatchFailurePayload, Option<String>)> = None;
    // Timestamp of the most recent pipeline.status event — used as closed_at
    // fallback when status=="closed-followup" but no pipeline.complete exists.
    let mut last_status_ts: Option<String> = None;

    for ev in events {
        match ev.event.as_str() {
            EVENT_PIPELINE_SCOPE => {
                // Lenient: missing fields default via #[serde(default)].
                match serde_json::from_value::<mustard_core::domain::model::event::PipelineScopePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        view.scope = Some(p.scope);
                        if p.lang.is_some() { view.lang = p.lang; }
                        if p.model.is_some() { view.model = p.model; }
                        if p.is_wave_plan.is_some() { view.is_wave_plan = p.is_wave_plan; }
                        if p.total_waves.is_some() { view.total_waves = p.total_waves; }
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_SCOPE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_STATUS => {
                match serde_json::from_value::<mustard_core::domain::model::event::PipelineStatusPayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        view.status = Some(p.to);
                        if !ev.ts.is_empty() {
                            last_status_ts = Some(ev.ts.clone());
                        }
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_STATUS} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_COMPLETE => {
                // Lenient: a bare `pipeline.complete` (no `--payload`) lands as
                // a `null` payload — treated as the all-default completion
                // rather than dropped with a serde "invalid type: null" error.
                match PipelineCompletePayload::from_value_lenient(ev.payload.clone()) {
                    Ok(p) => {
                        view.closed_at = p.closed_at;
                        view.affected_files = p.affected_files;
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_COMPLETE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_TASK_DISPATCH => {
                match serde_json::from_value::<mustard_core::domain::model::event::PipelineTaskDispatchPayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        let task = find_or_insert_task(&mut view.tasks, &p.name);
                        task.agent = p.agent;
                        task.wave = p.wave;
                        task.role = p.role;
                        if let Some(files) = p.files {
                            for f in files {
                                if !task.files.contains(&f) {
                                    task.files.push(f);
                                }
                            }
                        }
                        task.dispatched_at = Some(ev.ts.clone());
                        task.status = "pending".to_string();
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_TASK_DISPATCH} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_TASK_COMPLETE => {
                match serde_json::from_value::<mustard_core::domain::model::event::PipelineTaskCompletePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        let task = find_or_insert_task(&mut view.tasks, &p.name);
                        task.status = "completed".to_string();
                        task.completed_at = Some(ev.ts.clone());
                        task.duration_ms = p.duration_ms;
                        if let Some(modified) = p.files_modified {
                            for f in modified {
                                if !task.files.contains(&f) {
                                    task.files.push(f);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_TASK_COMPLETE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_WAVE_COMPLETE => {
                match serde_json::from_value::<mustard_core::domain::model::event::PipelineWaveCompletePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        if !view.completed_waves.contains(&p.wave) {
                            view.completed_waves.push(p.wave);
                        }
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_WAVE_COMPLETE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_DISPATCH_FAILURE => {
                match serde_json::from_value::<PipelineDispatchFailurePayload>(ev.payload.clone()) {
                    Ok(p) => {
                        let at = p.at.clone().or_else(|| {
                            if ev.ts.is_empty() { None } else { Some(ev.ts.clone()) }
                        });
                        raw_failure = Some((p, at));
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_DISPATCH_FAILURE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_PAUSE => {
                match serde_json::from_value::<mustard_core::domain::model::event::PipelinePausePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => {
                        // Use the event timestamp as the canonical pause time.
                        view.paused_at = if ev.ts.is_empty() { None } else { Some(ev.ts.clone()) };
                        view.pause_reason = p.reason;
                    }
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_PAUSE} payload for {spec}: {e}");
                    }
                }
            }
            EVENT_PIPELINE_RESUME_MODE => {
                match serde_json::from_value::<mustard_core::domain::model::event::PipelineResumeModePayload>(
                    ev.payload.clone(),
                ) {
                    Ok(p) => view.resume_mode = Some(p.mode),
                    Err(e) => {
                        eprintln!("[pipeline_state_from_events] bad {EVENT_PIPELINE_RESUME_MODE} payload for {spec}: {e}");
                    }
                }
            }
            _ => {} // Unknown event kinds are silently skipped (fail-open).
        }
    }

    // Post-fold: sort and deduplicate completed_waves.
    view.completed_waves.sort_unstable();
    view.completed_waves.dedup();

    // current_wave = max completed wave + 1, defaulting to 1.
    view.current_wave = view
        .completed_waves
        .iter()
        .max()
        .map_or(1, |w| w + 1);

    // FS fallback for is_wave_plan — takes priority only if not already set by event.
    if view.is_wave_plan.is_none() {
        if let Some(dir) = spec_dir {
            if dir.join("wave-plan.md").exists() {
                view.is_wave_plan = Some(true);
            }
        }
    }

    // closed_at fallback: if status is "closed-followup" but no pipeline.complete
    // event was recorded (e.g. legacy or partially-migrated spec), use the
    // timestamp of the last pipeline.status event instead.
    if view.closed_at.is_none()
        && view.status.as_deref() == Some("closed-followup")
    {
        view.closed_at = last_status_ts;
    }

    // Stale dispatch-failure check: clear if older than 10 minutes.
    view.last_dispatch_failure = match raw_failure {
        None => None,
        Some((payload, Some(at_str))) => {
            let now_ms = i64::try_from(mustard_core::time::now_unix_millis() as u128).unwrap_or(i64::MAX);
            let age_ms = mustard_core::time::parse_iso_millis(&at_str)
                .map_or(0, |at_ms| now_ms - at_ms);
            if age_ms > DISPATCH_FAILURE_TTL_MS {
                None // stale — cleared per /resume Step 0 contract
            } else {
                Some(payload)
            }
        }
        Some((payload, None)) => Some(payload), // no timestamp → keep (fail-open)
    };

    Some(view)
}

/// Find a task by name, or push a new default one and return a mutable ref.
fn find_or_insert_task<'a>(tasks: &'a mut Vec<PipelineTaskView>, name: &str) -> &'a mut PipelineTaskView {
    // Linear search — task counts are small (< 50 per spec).
    if let Some(pos) = tasks.iter().position(|t| t.name == name) {
        return &mut tasks[pos];
    }
    tasks.push(PipelineTaskView {
        name: name.to_string(),
        status: "pending".to_string(),
        ..Default::default()
    });
    tasks.last_mut().expect("just pushed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::event::{Actor, ActorKind, SCHEMA_VERSION};

    #[test]
    fn unknown_view_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let v = project(dir.path(), "slope-report", None, None);
        assert!(v.get("error").is_some());
    }

    fn pipeline_ev(event: &str, spec: &str, payload: Value) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T10:00:00.000Z".to_string(),
            session_id: "s-pipeline".to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: None, actor_type: None },
            event: event.to_string(),
            payload,
            spec: Some(spec.to_string()),
        }
    }

    /// Test 1 — no events for spec → None.
    #[test]
    fn ps_no_events_returns_none() {
        let events: Vec<HarnessEvent> = Vec::new();
        assert!(pipeline_state_from_events(&events, "ghost-spec", None).is_none());
    }

    /// Test 2 — scope + status events only → fields populated, tasks empty, current_wave=1.
    #[test]
    fn ps_scope_and_status_only() {
        let events = vec![
            pipeline_ev(
                EVENT_PIPELINE_SCOPE, "spec-a",
                json!({ "scope": "full", "lang": "en", "model": "claude-opus-4-5" }),
            ),
            pipeline_ev(
                EVENT_PIPELINE_STATUS, "spec-a",
                json!({ "to": "active" }),
            ),
        ];

        let view = pipeline_state_from_events(&events, "spec-a", None).unwrap();
        assert_eq!(view.scope.as_deref(), Some("full"));
        assert_eq!(view.lang.as_deref(), Some("en"));
        assert_eq!(view.model.as_deref(), Some("claude-opus-4-5"));
        assert_eq!(view.status.as_deref(), Some("active"));
        assert!(view.tasks.is_empty());
        assert_eq!(view.current_wave, 1);
        assert!(view.completed_waves.is_empty());
    }

    /// Test 3 — wave progression → completed_waves=[1,2], current_wave=3.
    #[test]
    fn ps_wave_progression() {
        let events = vec![
            pipeline_ev(
                EVENT_PIPELINE_WAVE_COMPLETE, "spec-b",
                json!({ "wave": 1 }),
            ),
            pipeline_ev(
                EVENT_PIPELINE_WAVE_COMPLETE, "spec-b",
                json!({ "wave": 2 }),
            ),
        ];

        let view = pipeline_state_from_events(&events, "spec-b", None).unwrap();
        assert_eq!(view.completed_waves, vec![1u32, 2u32]);
        assert_eq!(view.current_wave, 3);
    }

    /// Test 4 — task lifecycle: dispatch + complete → status=completed with timestamps.
    #[test]
    fn ps_task_lifecycle_dispatch_then_complete() {
        let mut dispatch_ev = pipeline_ev(
            EVENT_PIPELINE_TASK_DISPATCH, "spec-c",
            json!({ "name": "implement-auth", "agent": "general-purpose", "wave": 1 }),
        );
        dispatch_ev.ts = "2026-05-20T10:00:00.000Z".to_string();

        let mut complete_ev = pipeline_ev(
            EVENT_PIPELINE_TASK_COMPLETE, "spec-c",
            json!({ "name": "implement-auth", "duration_ms": 5000 }),
        );
        complete_ev.ts = "2026-05-20T10:05:00.000Z".to_string();

        let events = vec![dispatch_ev, complete_ev];

        let view = pipeline_state_from_events(&events, "spec-c", None).unwrap();
        assert_eq!(view.tasks.len(), 1);
        let task = &view.tasks[0];
        assert_eq!(task.name, "implement-auth");
        assert_eq!(task.status, "completed");
        assert_eq!(task.dispatched_at.as_deref(), Some("2026-05-20T10:00:00.000Z"));
        assert_eq!(task.completed_at.as_deref(), Some("2026-05-20T10:05:00.000Z"));
        assert_eq!(task.duration_ms, Some(5000));
    }

    /// Test 5 — conflicting status events → last wins.
    #[test]
    fn ps_last_status_wins() {
        let events = vec![
            pipeline_ev(
                EVENT_PIPELINE_STATUS, "spec-d",
                json!({ "to": "active" }),
            ),
            pipeline_ev(
                EVENT_PIPELINE_STATUS, "spec-d",
                json!({ "to": "completed" }),
            ),
        ];

        let view = pipeline_state_from_events(&events, "spec-d", None).unwrap();
        assert_eq!(view.status.as_deref(), Some("completed"));
    }

    /// Test 6 — stale dispatch_failure (>10 min old) → cleared in view.
    #[test]
    fn ps_stale_dispatch_failure_cleared() {
        // Use a timestamp far in the past (2020-01-01) to guarantee staleness.
        let events = vec![pipeline_ev(
            EVENT_PIPELINE_DISPATCH_FAILURE, "spec-e",
            json!({ "reason": "timeout", "at": "2020-01-01T00:00:00.000Z" }),
        )];

        let view = pipeline_state_from_events(&events, "spec-e", None).unwrap();
        assert!(view.last_dispatch_failure.is_none(), "stale failure should be cleared");
    }

    /// Test 7 — fresh dispatch_failure (<10 min old) → preserved in view.
    #[test]
    fn ps_fresh_dispatch_failure_kept() {
        // Use the current time as the failure timestamp — guarantees freshness.
        let recent_ts = mustard_core::time::now_iso8601();

        let events = vec![pipeline_ev(
            EVENT_PIPELINE_DISPATCH_FAILURE, "spec-f",
            json!({ "reason": "budget exceeded", "at": recent_ts }),
        )];

        let view = pipeline_state_from_events(&events, "spec-f", None).unwrap();
        assert!(view.last_dispatch_failure.is_some(), "fresh failure should be preserved");
        assert_eq!(
            view.last_dispatch_failure.as_ref().unwrap().reason.as_deref(),
            Some("budget exceeded"),
        );
    }

    /// Test — pipeline.complete sets closed_at and affected_files in the view.
    #[test]
    fn ps_pipeline_complete_sets_closed_at_and_files() {
        let events = vec![
            pipeline_ev(
                EVENT_PIPELINE_STATUS, "spec-complete",
                json!({ "to": "closed-followup" }),
            ),
            pipeline_ev(
                EVENT_PIPELINE_COMPLETE, "spec-complete",
                json!({
                    "closedAt": "2026-05-20T12:00:00.000Z",
                    "affectedFiles": ["src/foo.rs", "src/bar.rs"]
                }),
            ),
        ];

        let view = pipeline_state_from_events(&events, "spec-complete", None).unwrap();
        assert_eq!(view.status.as_deref(), Some("closed-followup"));
        assert_eq!(view.closed_at.as_deref(), Some("2026-05-20T12:00:00.000Z"));
        assert_eq!(view.affected_files, vec!["src/foo.rs", "src/bar.rs"]);
    }

    /// Test — closed_at fallback: status==closed-followup but no pipeline.complete event.
    #[test]
    fn ps_closed_at_falls_back_to_last_status_ts() {
        // Emit a pipeline.status event with a known timestamp.
        let mut status_ev = pipeline_ev(
            EVENT_PIPELINE_STATUS, "spec-fallback",
            json!({ "to": "closed-followup" }),
        );
        status_ev.ts = "2026-05-20T09:30:00.000Z".to_string();
        let mut events = vec![status_ev];
        // No pipeline.complete event.

        let view = pipeline_state_from_events(&events, "spec-fallback", None).unwrap();
        assert_eq!(view.status.as_deref(), Some("closed-followup"));
        // closed_at should fall back to the pipeline.status event's ts.
        assert_eq!(view.closed_at.as_deref(), Some("2026-05-20T09:30:00.000Z"));
        assert!(view.affected_files.is_empty(), "no affectedFiles when no complete event");

        // Sanity: if pipeline.complete IS present, it wins over the fallback.
        events.push(pipeline_ev(
            EVENT_PIPELINE_COMPLETE, "spec-fallback",
            json!({ "closedAt": "2026-05-20T10:00:00.000Z", "affectedFiles": [] }),
        ));
        let view2 = pipeline_state_from_events(&events, "spec-fallback", None).unwrap();
        assert_eq!(view2.closed_at.as_deref(), Some("2026-05-20T10:00:00.000Z"));
    }

    /// Test 8 — is_wave_plan via FS when no pipeline.scope event present.
    #[test]
    fn ps_is_wave_plan_via_fs_fallback() {
        // Add any event so the spec is present.
        let events = vec![pipeline_ev(
            EVENT_PIPELINE_STATUS, "spec-g",
            json!({ "to": "active" }),
        )];

        // Create the spec dir with wave-plan.md.
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path().join("spec-dir");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("wave-plan.md"), "# Wave Plan\n").unwrap();

        let view = pipeline_state_from_events(&events, "spec-g", Some(&spec_dir)).unwrap();
        assert_eq!(view.is_wave_plan, Some(true));
    }
}
