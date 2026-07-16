//! `session_knowledge_observer` — the consolidated knowledge-extraction module.
//!
//! ## Scope (b3 Wave 5, knowledge family)
//!
//! This module consolidates two JavaScript hooks. Each is a distinct
//! *concern* kept as its own internal section — consolidation regroups, it
//! does not merge logic:
//!
//! - `session-knowledge.js` — `SessionEnd`: extracts friction telemetry from
//!   pipeline-states into `.claude/.metrics/friction.json` and emits one
//!   `retry.attempt` event per measured hook-level retry.
//! - `session-knowledge-inc.js` — `PostToolUse(Task)`: the incremental variant
//!   — throttled, writes friction telemetry for the most recent pipeline-state.
//!
//! The third historical concern (`memory-auto-extract` — spec bullets into a
//! knowledge store) was retired with the Mustard memory hybrid: decisions and
//! lessons are `decision`/`lesson` events now, emitted at CLOSE via
//! `run emit-event`.
//!
//! ## Contract shape
//!
//! All three are pure side effects — no verdict. `SessionKnowledgeObserver` is
//! an [`Observer`] only.
//!
//! ## Parity notes
//!
//! - `extractPatternsFromStates` (`_lib/knowledge-extract.js`) is **empty by
//!   design** — friction signals moved out of `knowledge.json`. The JS
//!   `session-knowledge` hooks therefore persist *no* knowledge patterns; they
//!   only write friction telemetry and emit `retry.attempt`. This port
//!   reproduces exactly that: it writes `friction.json` and emits
//!   `retry.attempt`.

use mustard_core::io::fs;
use mustard_core::view::projection::read_harness_events_from_ndjson_dir;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use mustard_core::time::now_iso8601;

/// Throttle window for `session-knowledge-inc` — 1 hour, max 3 runs.
const THROTTLE_WINDOW_MS: u128 = 3_600_000;
const THROTTLE_MAX: usize = 3;
/// Friction file keeps at most this many entries.
const FRICTION_MAX_ENTRIES: usize = 100;

/// The consolidated knowledge-extraction module.
pub struct SessionKnowledgeObserver;

// ===========================================================================
// Shared helpers
// ===========================================================================


/// The current session id — the `session_id` field, else `"unknown"`.
fn session_id(input: &HookInput) -> String {
    input
        .session_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string())
}

// ===========================================================================
// Friction extraction — port of _lib/knowledge-extract.js
// ===========================================================================

/// One pipeline-state object, with the filename fallback label attached.
struct StateObject {
    /// `specName`, else the state filename without `.json` — the JS `_file`.
    label: String,
    /// The whole parsed JSON.
    json: Value,
}

/// One friction entry — port of an `extractFrictionFromStates` record.
struct FrictionEntry {
    name: String,
    description: String,
    tags: Vec<String>,
    /// `retryCount` or `apiCalls`, whichever the heuristic produced.
    metric_field: (&'static str, i64),
    prescription: Option<String>,
}

/// Build the actionable prescription string. Port of `derivePrescription`.
fn derive_prescription(metrics: &Value) -> Option<String> {
    let n = |key: &str| -> i64 {
        metrics
            .get("toolBreakdown")
            .and_then(|b| b.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    let bash = n("Bash");
    let edit = n("Edit");
    let write = n("Write");
    let agent = n("Agent");
    let retries = metrics.get("retries").and_then(Value::as_i64).unwrap_or(0);
    let api_calls = metrics.get("apiCalls").and_then(Value::as_i64).unwrap_or(0);

    if bash + edit > 3 * agent && retries > 2 {
        return Some(
            "Next similar pipeline: delegate investigation via Task(general-purpose) \
             BEFORE editing files in sequence. Dominant Bash+Edit without Agent indicates \
             the parent did work that should have been delegated."
                .to_string(),
        );
    }
    if api_calls > 50 && retries > 3 {
        return Some(
            "Next similar pipeline: split into at least 2 smaller pipelines. \
             A single scope with >50 API calls and >3 retries indicates scope-creep."
                .to_string(),
        );
    }
    if edit > 15 && write < 3 {
        return Some(
            "Next similar pipeline: investigate with Read+Grep BEFORE editing. \
             High Edit with low Write count indicates trial-and-error iteration."
                .to_string(),
        );
    }
    None
}

/// Extract friction telemetry from pipeline-state objects. Port of
/// `extractFrictionFromStates`.
fn extract_friction(states: &[StateObject]) -> Vec<FrictionEntry> {
    let mut friction: Vec<FrictionEntry> = Vec::new();
    for state in states {
        let metrics = state.json.get("metrics").cloned().unwrap_or(Value::Null);
        let prescription = derive_prescription(&metrics);
        let retries = metrics.get("retries").and_then(Value::as_i64).unwrap_or(0);
        let api_calls = metrics.get("apiCalls").and_then(Value::as_i64).unwrap_or(0);
        let label = &state.label;

        if retries > 2 {
            let breakdown = metrics
                .get("toolBreakdown")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let mut tags = vec![
                "hook-retry".to_string(),
                "pipeline".to_string(),
                "friction".to_string(),
            ];
            if prescription.is_some() {
                tags.push("prescriptive".to_string());
            }
            friction.push(FrictionEntry {
                name: format!("high-hook-retry-{label}"),
                description: format!(
                    "Pipeline triggered {retries} hook-level retries \
                     (sandbox/stash-pop/re-prompts — not agent redispatches). \
                     Tool breakdown: {breakdown}"
                ),
                tags,
                metric_field: ("retryCount", retries),
                prescription: prescription.clone(),
            });
        }
        if api_calls > 50 {
            let mut tags = vec![
                "optimization".to_string(),
                "pipeline".to_string(),
                "friction".to_string(),
            ];
            if prescription.is_some() {
                tags.push("prescriptive".to_string());
            }
            friction.push(FrictionEntry {
                name: format!("heavy-pipeline-{label}"),
                description: format!(
                    "Pipeline used {api_calls} API calls. Consider splitting into smaller scope."
                ),
                tags,
                metric_field: ("apiCalls", api_calls),
                prescription,
            });
        }
    }
    friction
}

/// Persist friction telemetry to `.claude/.metrics/friction.json`, updating
/// entries in place by `name`. Port of `saveFriction`.
fn save_friction(entries: &[FrictionEntry], claude_dir: &Path) {
    if entries.is_empty() {
        return;
    }
    // Reverse-derive the project root from the passed `claude_dir` so we can
    // route through `ClaudePaths::metrics_dir`. Defensive: a malformed input
    // falls back to a no-op rather than mis-route writes.
    let metrics_dir = claude_dir
        .parent()
        .filter(|_| claude_dir.file_name().and_then(|s| s.to_str()) == Some(".claude"))
        .and_then(|root| ClaudePaths::for_project(root).ok())
        .map(|p| p.metrics_dir());
    let Some(metrics_dir) = metrics_dir else {
        return;
    };
    let _ = fs::create_dir_all(&metrics_dir);
    let friction_path = metrics_dir.join("friction.json");

    let mut store: Value = fs::read_to_string(&friction_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({ "version": 1, "entries": [] }));
    if !store.get("entries").is_some_and(Value::is_array) {
        store = json!({ "version": 1, "entries": [] });
    }
    let ts = now_iso8601();
    let Some(store_entries) = store.get_mut("entries").and_then(Value::as_array_mut) else {
        return;
    };

    for entry in entries {
        let mut record = serde_json::Map::new();
        record.insert("type".into(), json!("friction"));
        record.insert("name".into(), json!(entry.name));
        record.insert("description".into(), json!(entry.description));
        record.insert("source".into(), json!("session-knowledge"));
        record.insert("tags".into(), json!(entry.tags));
        record.insert(entry.metric_field.0.into(), json!(entry.metric_field.1));
        if let Some(p) = &entry.prescription {
            record.insert("prescription".into(), json!(p));
        }
        record.insert("updatedAt".into(), json!(ts));

        // New fields: verification metadata (AC-3).
        record.insert("verifiedAt".to_string(), Value::Null);
        record.insert("sourceFiles".to_string(), Value::Array(Vec::new()));

        let existing_idx = store_entries
            .iter()
            .position(|e| e.get("name").and_then(|n| n.as_str()) == Some(entry.name.as_str()));
        if let Some(idx) = existing_idx {
            let created = store_entries[idx]
                .get("createdAt")
                .and_then(|v| v.as_str())
                .unwrap_or(&ts)
                .to_string();
            record.insert("createdAt".into(), json!(created));
            store_entries[idx] = Value::Object(record);
        } else {
            record.insert("createdAt".into(), json!(ts));
            store_entries.push(Value::Object(record));
        }
    }
    // Keep the newest FRICTION_MAX_ENTRIES.
    store_entries.sort_by(|a, b| {
        let ta = a.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    store_entries.truncate(FRICTION_MAX_ENTRIES);

    let _ = fs::write_atomic(
        &friction_path,
        serde_json::to_string_pretty(&store).unwrap_or_default().as_bytes(),
    );
}

/// Read every `.pipeline-states/*.json` into [`StateObject`]s.
fn read_state_objects(paths: &ClaudePaths) -> Vec<StateObject> {
    let states_dir = paths.pipeline_states_dir();
    let Ok(entries) = fs::read_dir(&states_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries {
        if !std::path::Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&entry.path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let file_label = entry.file_name.trim_end_matches(".json").to_string();
        let label = json
            .get("specName")
            .and_then(|v| v.as_str())
            .map_or(file_label, str::to_string);
        out.push(StateObject { label, json });
    }
    out
}

// ===========================================================================
// session-knowledge — SessionEnd retry.attempt emission + friction
// ===========================================================================

/// `true` when the per-spec NDJSON log already carries a `retry.attempt` event.
///
/// W5: `retry.attempt` lives in the per-spec NDJSON sink, not in `pipeline_events`.
/// Existence-only probe (a single line is enough), so this returns early.
fn spec_has_retry_events(cwd: &str, spec: &str) -> bool {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return false;
    };
    let Ok(spec_paths) = paths.for_spec(spec) else {
        return false;
    };
    let events_dir = spec_paths.events_dir();
    for ev in read_harness_events_from_ndjson_dir(&events_dir) {
        if ev.event == "retry.attempt" {
            return true;
        }
    }
    false
}

/// Emit one `retry.attempt` event per measured hook-level retry. Idempotent:
/// a spec already carrying `retry.attempt` events is skipped. Port of
/// `emitRetryAttempts`.
fn emit_retry_attempts(state: &StateObject, input: &HookInput, cwd: &str) {
    let retries = state
        .json
        .get("metrics")
        .and_then(|m| m.get("retries"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if retries < 1 {
        return;
    }
    let spec = &state.label;
    if spec_has_retry_events(cwd, spec) {
        return;
    }
    for _ in 0..retries {
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: now_iso8601(),
            session_id: session_id(input),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("session-knowledge".to_string()),
                actor_type: None,
            },
            event: "retry.attempt".to_string(),
            payload: json!({ "reason": "hook-level", "tool": Value::Null }),
            spec: Some(spec.clone()),
        };
        // `retry.attempt` is non-pipeline → routed to the per-spec NDJSON
        // sink by the W5 split. `route::emit` is the single
        // classifier; see `apps/rt/src/run/event_route.rs`.
        let _ = crate::shared::events::route::emit(cwd, &event);
    }
}

/// `session-knowledge`: on `SessionEnd`, write friction telemetry and emit
/// `retry.attempt` events. Pure side effect — fail-open throughout.
fn run_session_knowledge(input: &HookInput, cwd: &str) {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let claude = paths.claude_dir();
    let states = read_state_objects(&paths);
    if states.is_empty() {
        return;
    }
    save_friction(&extract_friction(&states), &claude);
    for state in &states {
        emit_retry_attempts(state, input, cwd);
    }
    // NOTE: `extractPatternsFromStates` is empty by design (see module docs),
    // so the JS `toSave` knowledge-persist loop runs zero iterations — no
    // `memory.js knowledge` invocation. This port reproduces that exactly.
}

/// `session-knowledge-inc`: on `PostToolUse(Task)`, write friction telemetry
/// for the most recent pipeline-state, throttled. Pure side effect.
fn run_session_knowledge_inc(cwd: &str) {
    let Ok(paths) = ClaudePaths::for_project(Path::new(cwd)) else {
        return;
    };
    let seen_path = paths.knowledge_seen_path();
    let mut seen: Value = fs::read_to_string(&seen_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({ "_meta": { "recentExtractions": [] } }));

    // Throttle: prune the rolling window, bail when full.
    let now = mustard_core::time::now_unix_millis() as u128;
    let mut recent: Vec<String> = seen
        .get("_meta")
        .and_then(|m| m.get("recentExtractions"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter(|ts| now.saturating_sub(mustard_core::time::parse_iso_millis(ts).unwrap_or(0) as u128) < THROTTLE_WINDOW_MS)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if recent.len() >= THROTTLE_MAX {
        return;
    }

    // Most-recently modified pipeline-state.
    let states_dir = paths.pipeline_states_dir();
    let Ok(entries) = fs::read_dir(&states_dir) else {
        return;
    };
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in entries {
        if !std::path::Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
            continue;
        }
        let Ok(mtime) = fs::modified(&entry.path) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(t, _)| mtime > *t) {
            newest = Some((mtime, entry.path));
        }
    }
    let Some((_, latest_path)) = newest else {
        return;
    };
    let Ok(text) = fs::read_to_string(&latest_path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let file_label = latest_path
        .file_name()
        .map(|n| n.to_string_lossy().trim_end_matches(".json").to_string())
        .unwrap_or_default();
    let label = json
        .get("specName")
        .and_then(|v| v.as_str())
        .map_or(file_label, str::to_string);
    let state = StateObject { label, json };
    let claude = paths.claude_dir();

    save_friction(&extract_friction(std::slice::from_ref(&state)), &claude);

    // `extractPatternsFromStates` is empty → no eligible pattern to persist,
    // so the JS bails at `if (candidates.length === 0)`. The throttle window
    // is therefore *not* advanced (the JS only records an extraction after a
    // successful persist). This port mirrors that: write nothing to
    // `.knowledge-seen.json` when no pattern was persisted.
    let _ = &mut recent;
    let _ = &mut seen;
}

// ===========================================================================
// Contract impl
// ===========================================================================

impl Observer for SessionKnowledgeObserver {
    /// Dispatch by trigger: `SessionEnd` runs `session-knowledge`;
    /// `PostToolUse(Task)` runs `session-knowledge-inc`. Any other invocation
    /// is a no-op. Pure side effect — never panics.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let cwd = ctx.project_dir_or_cwd(input);
        match ctx.trigger {
            Some(Trigger::SessionEnd) => {
                run_session_knowledge(input, &cwd);
            }
            Some(Trigger::PostToolUse) => {
                if matches!(input.tool_name.as_deref(), Some("Task" | "Agent")) {
                    run_session_knowledge_inc(&cwd);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(trigger: Trigger, dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(trigger),
            workspace_root: None,
        }
    }

    /// Write a pipeline-state file.
    fn write_state(dir: &Path, name: &str, state: &Value) {
        let states = dir.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join(format!("{name}.json")), state.to_string()).unwrap();
    }

    // --- derive_prescription parity ----------------------------------------

    #[test]
    fn prescription_detects_l0_violation() {
        let metrics = json!({
            "retries": 3,
            "toolBreakdown": { "Bash": 10, "Edit": 5, "Agent": 1 },
        });
        let p = derive_prescription(&metrics).expect("L0 heuristic must fire");
        assert!(p.contains("delegate"));
    }

    #[test]
    fn prescription_none_for_clean_metrics() {
        let metrics = json!({ "retries": 0, "apiCalls": 3 });
        assert!(derive_prescription(&metrics).is_none());
    }

    // --- friction extraction parity ----------------------------------------

    #[test]
    fn high_retry_state_produces_friction_entry() {
        let state = StateObject {
            label: "demo".to_string(),
            json: json!({ "metrics": { "retries": 5, "toolBreakdown": {} } }),
        };
        let friction = extract_friction(std::slice::from_ref(&state));
        assert_eq!(friction.len(), 1);
        assert_eq!(friction[0].name, "high-hook-retry-demo");
        assert_eq!(friction[0].metric_field, ("retryCount", 5));
    }

    #[test]
    fn heavy_pipeline_state_produces_friction_entry() {
        let state = StateObject {
            label: "big".to_string(),
            json: json!({ "metrics": { "apiCalls": 99 } }),
        };
        let friction = extract_friction(std::slice::from_ref(&state));
        assert_eq!(friction.len(), 1);
        assert_eq!(friction[0].name, "heavy-pipeline-big");
    }

    #[test]
    fn low_activity_state_produces_no_friction() {
        let state = StateObject {
            label: "calm".to_string(),
            json: json!({ "metrics": { "retries": 1, "apiCalls": 10 } }),
        };
        assert!(extract_friction(std::slice::from_ref(&state)).is_empty());
    }

    #[test]
    fn session_knowledge_writes_friction_file() {
        let dir = tempdir().unwrap();
        write_state(
            dir.path(),
            "noisy",
            &json!({ "specName": "noisy", "metrics": { "retries": 4, "toolBreakdown": {} } }),
        );
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            ..HookInput::default()
        };
        SessionKnowledgeObserver.observe(&input, &ctx(Trigger::SessionEnd, dir.path().to_str().unwrap()));
        let friction = dir.path().join(".claude/.metrics/friction.json");
        assert!(friction.exists());
        let parsed: Value =
            serde_json::from_str(&std::fs::read_to_string(friction).unwrap()).unwrap();
        assert_eq!(parsed["entries"].as_array().unwrap().len(), 1);
    }

    /// Count `retry.attempt` rows across every per-spec NDJSON dir (W5).
    fn count_retry_events(project: &Path) -> usize {
        let Ok(paths) = ClaudePaths::for_project(project) else {
            return 0;
        };
        let specs_root = paths.spec_dir();
        let Ok(entries) = std::fs::read_dir(&specs_root) else {
            return 0;
        };
        let mut total = 0usize;
        for entry in entries.flatten() {
            let dir = entry.path().join(".events");
            for ev in read_harness_events_from_ndjson_dir(&dir) {
                if ev.event == "retry.attempt" {
                    total += 1;
                }
            }
        }
        total
    }

    #[test]
    fn session_knowledge_emits_retry_attempt_events() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        write_state(
            dir.path(),
            "retried",
            &json!({ "specName": "retried", "metrics": { "retries": 3 } }),
        );
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            session_id: Some("s-1".to_string()),
            ..HookInput::default()
        };
        SessionKnowledgeObserver.observe(&input, &ctx(Trigger::SessionEnd, project));
        assert_eq!(
            count_retry_events(dir.path()),
            3,
            "one retry.attempt per measured retry"
        );
    }

    #[test]
    fn retry_attempt_emission_is_idempotent() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        write_state(
            dir.path(),
            "once",
            &json!({ "specName": "once", "metrics": { "retries": 2 } }),
        );
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            ..HookInput::default()
        };
        // Run twice — the second run must not double-count.
        SessionKnowledgeObserver.observe(&input, &ctx(Trigger::SessionEnd, project));
        SessionKnowledgeObserver.observe(&input, &ctx(Trigger::SessionEnd, project));
        assert_eq!(
            count_retry_events(dir.path()),
            2,
            "idempotent — no re-emission"
        );
    }

    // --- routing -----------------------------------------------------------

    #[test]
    fn observe_ignores_unrelated_triggers() {
        let dir = tempdir().unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        // PreToolUse → no-op, must not panic.
        SessionKnowledgeObserver.observe(&input, &ctx(Trigger::PreToolUse, dir.path().to_str().unwrap()));
    }

    #[test]
    fn observe_inc_ignores_non_task_post_tool_use() {
        let dir = tempdir().unwrap();
        let input = HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("Bash".to_string()),
            ..HookInput::default()
        };
        // PostToolUse(Bash) → session-knowledge-inc must not run.
        SessionKnowledgeObserver.observe(&input, &ctx(Trigger::PostToolUse, dir.path().to_str().unwrap()));
    }

    #[test]
    fn knowledge_entry_carries_verification_metadata() {
        let dir = tempdir().unwrap();
        // Write a pipeline-state that produces a friction entry (retries > 2).
        write_state(
            dir.path(),
            "verify-meta",
            &json!({
                "specName": "verify-meta",
                "metrics": { "retries": 4, "toolBreakdown": {} }
            }),
        );
        let input = HookInput {
            hook_event_name: Some("SessionEnd".to_string()),
            ..HookInput::default()
        };
        SessionKnowledgeObserver.observe(&input, &ctx(Trigger::SessionEnd, dir.path().to_str().unwrap()));

        let friction_path = dir.path().join(".claude/.metrics/friction.json");
        assert!(friction_path.exists(), "friction.json must be written");
        let parsed: Value =
            serde_json::from_str(&std::fs::read_to_string(&friction_path).unwrap()).unwrap();
        let entries = parsed["entries"].as_array().expect("entries array");
        assert!(!entries.is_empty(), "at least one entry expected");
        let first = &entries[0];
        assert_eq!(first["verifiedAt"], Value::Null, "verifiedAt must default to null");
        assert_eq!(
            first["sourceFiles"],
            Value::Array(Vec::new()),
            "sourceFiles must default to empty array"
        );
    }
}
