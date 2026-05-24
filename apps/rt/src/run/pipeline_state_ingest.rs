//! `mustard-rt run pipeline-state-ingest` — one-shot `.pipeline-states/*.json` → SQLite migration.
//!
//! Globs `.claude/.pipeline-states/*.json`, excluding `*.metrics.json`. For each
//! file it lenient-parses the legacy pipeline-state JSON and emits retroactive
//! typed pipeline events into the [`SqliteEventStore`], preserving the original
//! `updatedAt` / `createdAt` timestamps so the events sort correctly among live
//! events.
//!
//! With `--delete`, each file that was ingested without errors is removed.
//! A parse/emit failure for one file is pushed into `errors` and never aborts
//! the siblings (fail-open per file).
//!
//! Output:
//! ```json
//! { "ingested": 2, "deleted": 1, "errors": [] }
//! ```

use crate::run::env::project_dir as env_project_dir;
use crate::util::now_iso8601;
use mustard_core::fs;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
// Wave 3 of mustard-unification: the sidecar `meta.json` is the canonical
// source of `scope`/`lang`/`isWavePlan`/`totalWaves` going forward. The legacy
// `.pipeline-states/*.json` ingest now consults `meta.json` first for fields
// the legacy JSON omits, falling back to the legacy values otherwise.
use mustard_core::read_meta;
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, SCHEMA_VERSION,
    EVENT_PIPELINE_DISPATCH_FAILURE, EVENT_PIPELINE_PAUSE,
    EVENT_PIPELINE_SCOPE, EVENT_PIPELINE_STATUS,
    EVENT_PIPELINE_TASK_COMPLETE, EVENT_PIPELINE_TASK_DISPATCH,
    EVENT_PIPELINE_WAVE_COMPLETE,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

// ---------------------------------------------------------------------------
// Lenient pipeline-state deserialization
// ---------------------------------------------------------------------------

/// A lenient representation of the legacy `.pipeline-states/{spec}.json` shape.
/// Every field is `Option` with `#[serde(default)]` so missing or renamed fields
/// never cause a parse failure.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LenientPipelineState {
    spec_name: Option<String>,
    spec: Option<String>,
    status: Option<String>,
    scope: Option<String>,
    lang: Option<String>,
    model: Option<String>,
    is_wave_plan: Option<bool>,
    total_waves: Option<u32>,
    tasks: Vec<LenientTask>,
    completed_waves: Vec<u32>,
    last_dispatch_failure: Option<Value>,
    paused_at: Option<String>,
    pause_reason: Option<String>,
    updated_at: Option<String>,
    created_at: Option<String>,
}

/// A lenient representation of a task entry inside the pipeline-state JSON.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LenientTask {
    name: Option<String>,
    agent: Option<String>,
    wave: Option<u32>,
    role: Option<String>,
    files: Vec<String>,
    status: Option<String>,
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run pipeline-state-ingest`.
pub struct PipelineStateIngestOpts {
    /// When `true`, remove each successfully-ingested JSON file.
    pub delete: bool,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `mustard-rt run pipeline-state-ingest [--delete]`.
///
/// Scans `.claude/.pipeline-states/*.json` (excluding `*.metrics.json`),
/// lenient-parses each file, and emits retroactive pipeline events into the
/// harness SQLite event store. Fail-open: a bad file is pushed into `errors`
/// and never aborts siblings.
pub fn run(opts: PipelineStateIngestOpts) {
    let cwd = env_project_dir();
    let states_dir = Path::new(&cwd).join(".claude").join(".pipeline-states");

    // Open the store (creates + applies schema if absent).
    let store = match SqliteEventStore::for_project(&cwd) {
        Ok(s) => s,
        Err(e) => {
            let out = json!({
                "ingested": 0,
                "deleted": 0,
                "errors": [{ "file": "(store)", "error": e.to_string()}]
            });
            println!("{out}");
            return;
        }
    };

    // Collect candidate files.
    let candidates = match collect_candidates(&states_dir) {
        Ok(v) => v,
        Err(e) => {
            let out = json!({
                "ingested": 0,
                "deleted": 0,
                "errors": [{ "file": "(glob)", "error": e}]
            });
            println!("{out}");
            return;
        }
    };

    let mut errors: Vec<Value> = Vec::new();
    let mut ingested = 0usize;
    let mut deleted = 0usize;

    for path in &candidates {
        let file_label = path
            .file_name()
            .map_or_else(|| path.to_string_lossy().to_string(), |n| n.to_string_lossy().to_string());

        let raw = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                errors.push(json!({ "file": file_label, "error": e.to_string()}));
                continue;
            }
        };

        let state: LenientPipelineState = match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(e) => {
                errors.push(json!({ "file": file_label, "error": format!("parse: {e}") }));
                continue;
            }
        };

        // Derive spec name: prefer explicit fields, fall back to file stem.
        let spec = state
            .spec_name
            .clone()
            .or_else(|| state.spec.clone())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map_or_else(|| "unknown".to_string(), |s| s.to_string_lossy().to_string())
            });

        // Canonical timestamp for all emitted events.
        let at = state
            .updated_at
            .clone()
            .or_else(|| state.created_at.clone())
            .unwrap_or_else(now_iso8601);

        let mut file_errors: Vec<Value> = Vec::new();

        // --- pipeline.scope ---
        //
        // Read `.claude/spec/<spec>/meta.json` FIRST (Wave 3 mustard-unification):
        // the sidecar is canonical for `scope`/`lang`/`isWavePlan`/`totalWaves`.
        // The legacy JSON only fills the gap for fields the sidecar omits.
        let meta_path = Path::new(&cwd)
            .join(".claude")
            .join("spec")
            .join(&spec)
            .join("meta.json");
        let meta_sidecar = read_meta(&meta_path);

        let scope_value = meta_sidecar
            .as_ref()
            .and_then(|m| m.scope.clone())
            .or_else(|| state.scope.clone())
            .unwrap_or_else(|| "full".to_string());
        let lang_value = meta_sidecar
            .as_ref()
            .and_then(|m| m.lang.clone())
            .or_else(|| state.lang.clone());
        let is_wave_plan = meta_sidecar
            .as_ref()
            .and_then(|m| m.is_wave_plan)
            .or(state.is_wave_plan);
        let total_waves = meta_sidecar
            .as_ref()
            .and_then(|m| m.total_waves)
            .or(state.total_waves);

        if meta_sidecar.is_some()
            || state.scope.is_some()
            || state.lang.is_some()
            || state.model.is_some()
            || state.is_wave_plan.is_some()
            || state.total_waves.is_some()
        {
            let payload = json!({
                "scope": scope_value,
                "lang": lang_value,
                "model": state.model,
                "isWavePlan": is_wave_plan,
                "totalWaves": total_waves,
            });
            if let Err(e) = append(&store, EVENT_PIPELINE_SCOPE, &spec, &at, payload) {
                file_errors.push(json!({ "file": file_label, "event": EVENT_PIPELINE_SCOPE, "error": e}));
            }
        }

        // --- pipeline.status ---
        if let Some(ref status) = state.status {
            let payload = json!({ "from": Value::Null, "to": status });
            if let Err(e) = append(&store, EVENT_PIPELINE_STATUS, &spec, &at, payload) {
                file_errors.push(json!({ "file": file_label, "event": EVENT_PIPELINE_STATUS, "error": e}));
            }
        }

        // --- pipeline.task.dispatch + pipeline.task.complete per task ---
        for task in &state.tasks {
            let name = match &task.name {
                Some(n) if !n.is_empty() => n.clone(),
                _ => continue,
            };

            let dispatch_payload = json!({
                "name": name,
                "agent": task.agent,
                "wave": task.wave,
                "role": task.role,
                "files": if task.files.is_empty() { Value::Null } else { json!(task.files) },
            });
            if let Err(e) = append(&store, EVENT_PIPELINE_TASK_DISPATCH, &spec, &at, dispatch_payload) {
                file_errors.push(json!({ "file": file_label, "event": EVENT_PIPELINE_TASK_DISPATCH, "task": name, "error": e}));
            }

            if task.status.as_deref() == Some("completed") {
                let complete_payload = json!({
                    "name": name,
                    "agent": task.agent,
                    "wave": task.wave,
                });
                if let Err(e) = append(&store, EVENT_PIPELINE_TASK_COMPLETE, &spec, &at, complete_payload) {
                    file_errors.push(json!({ "file": file_label, "event": EVENT_PIPELINE_TASK_COMPLETE, "task": name, "error": e}));
                }
            }
        }

        // --- pipeline.wave.complete per completed wave ---
        for &wave in &state.completed_waves {
            let payload = json!({ "wave": wave });
            if let Err(e) = append(&store, EVENT_PIPELINE_WAVE_COMPLETE, &spec, &at, payload) {
                file_errors.push(json!({ "file": file_label, "event": EVENT_PIPELINE_WAVE_COMPLETE, "wave": wave, "error": e}));
            }
        }

        // --- pipeline.dispatch_failure ---
        if let Some(ref failure) = state.last_dispatch_failure {
            if let Err(e) = append(&store, EVENT_PIPELINE_DISPATCH_FAILURE, &spec, &at, failure.clone()) {
                file_errors.push(json!({ "file": file_label, "event": EVENT_PIPELINE_DISPATCH_FAILURE, "error": e}));
            }
        }

        // --- pipeline.pause ---
        if state.paused_at.is_some() || state.pause_reason.is_some() {
            let payload = json!({ "reason": state.pause_reason });
            // Use paused_at as the event timestamp when available.
            let pause_at = state.paused_at.as_deref().unwrap_or(&at).to_string();
            if let Err(e) = append(&store, EVENT_PIPELINE_PAUSE, &spec, &pause_at, payload) {
                file_errors.push(json!({ "file": file_label, "event": EVENT_PIPELINE_PAUSE, "error": e}));
            }
        }

        let had_error = !file_errors.is_empty();
        errors.extend(file_errors);

        if !had_error {
            ingested += 1;
            if opts.delete && fs::remove_file(path).is_ok() {
                deleted += 1;
            }
        }
    }

    let out = json!({
        "ingested": ingested,
        "deleted": deleted,
        "errors": errors,
    });
    println!("{out}");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Append one retroactive pipeline event to the store.
fn append(
    store: &SqliteEventStore,
    kind: &str,
    spec: &str,
    at: &str,
    payload: Value,
) -> Result<(), String> {
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: at.to_string(),
        session_id: "pipeline-state-ingest".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("pipeline-state-ingest".to_string()),
            actor_type: None,
        },
        event: kind.to_string(),
        payload,
        spec: Some(spec.to_string()),
    };
    store.append(&event).map_err(|e| e.to_string())
}

/// Collect `.claude/.pipeline-states/*.json`, excluding `*.metrics.json`.
fn collect_candidates(states_dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    if !states_dir.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(states_dir)
        .map_err(|e| format!("read_dir failed: {e}"))?;
    let mut result = Vec::new();
    for entry in entries {
        let name = &entry.file_name;
        if name.ends_with(".json") && !name.ends_with(".metrics.json") {
            result.push(entry.path);
        }
    }
    result.sort();
    Ok(result)
}
