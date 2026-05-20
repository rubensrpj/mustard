//! `mustard-rt run amend-finalize` — session-end amendment window finalization.
//!
//! ## Scope (Wave 3 — 2026-05-20-session-bound-amendments)
//!
//! Called at `SessionEnd` (and directly via CLI) to close every open amendment
//! window for the ending session. For each window it:
//!
//! 1. Decides the final `status` (`archived`, `closed-amend-pending`,
//!    `closed-amend-drift`, or `resolved`).
//! 2. Appends a `## Amendments` block (respecting the spec's language) to
//!    `spec.md`.
//! 3. Moves the spec dir from `active/` to `archived/` when `status ==
//!    "archived"`.
//! 4. Updates the DB row via `close_amend_window`.
//! 5. Emits a [`EVENT_PIPELINE_AMEND_CLOSE`] event.
//!
//! ## Fail-open
//!
//! Any per-window error is collected into the [`RunReport`] and reported as
//! JSON. The subcommand always exits `0`.

use crate::util::now_iso8601;
use mustard_core::error::Result;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::{AmendWindow, SqliteEventStore};
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, PipelineAmendClosePayload, PipelineScopePayload,
    SCHEMA_VERSION, EVENT_PIPELINE_AMEND_ACTIVITY, EVENT_PIPELINE_AMEND_CLOSE,
    EVENT_PIPELINE_AMEND_DRIFT, EVENT_PIPELINE_AMEND_INTENT, EVENT_PIPELINE_AMEND_OPEN,
    EVENT_PIPELINE_SCOPE,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Environment variable for project root override (test-only convention).
const PROJECT_ROOT_ENV: &str = "MUSTARD_PROJECT_ROOT";

/// One per-window result entry in the JSON summary.
#[derive(Debug, Clone)]
pub struct WindowResult {
    pub spec_id: String,
    pub status: String,
    pub error: Option<String>,
}

/// The summary returned by [`run`], printed as JSON to stdout.
pub struct RunReport {
    pub session_id: String,
    pub windows: Vec<WindowResult>,
}

impl RunReport {
    fn to_json(&self) -> Value {
        let windows: Vec<Value> = self
            .windows
            .iter()
            .map(|w| {
                let mut obj = json!({
                    "spec_id": w.spec_id,
                    "status": w.status,
                });
                if let Some(e) = &w.error {
                    obj["error"] = Value::String(e.clone());
                }
                obj
            })
            .collect();
        json!({
            "session_id": self.session_id,
            "windows": windows,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the project root: `MUSTARD_PROJECT_ROOT` env var (test override),
/// then `CLAUDE_PROJECT_DIR`, then `current_dir()`, then `"."`.
pub fn project_root() -> PathBuf {
    if let Ok(v) = std::env::var(PROJECT_ROOT_ENV) {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    if let Ok(v) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Derive `lang` from the latest `pipeline.scope` event for `spec_id`.
/// Defaults to `"en"` when absent or unreadable.
fn resolve_lang(store: &SqliteEventStore, spec_id: &str) -> String {
    let events = match store.query(Some(spec_id)) {
        Ok(v) => v,
        Err(_) => return "en".to_string(),
    };
    events
        .into_iter()
        .filter(|e| e.event == EVENT_PIPELINE_SCOPE)
        .last()
        .and_then(|e| serde_json::from_value::<PipelineScopePayload>(e.payload).ok())
        .and_then(|p| p.lang)
        .unwrap_or_else(|| "en".to_string())
}

/// Decide the final status string for a window.
///
/// Priority:
/// 1. `drift_emitted` → `"closed-amend-drift"`
/// 2. `build_verde_at.is_some()` AND (build_verde_at >= last_activity_at OR
///    last_activity_at.is_none()) → `"archived"`
/// 3. `last_activity_at.is_some()` → `"closed-amend-pending"`
/// 4. else → `"resolved"` (no activity at all)
fn decide_status(window: &AmendWindow) -> &'static str {
    if window.drift_emitted {
        return "closed-amend-drift";
    }
    if window.build_verde_at.is_some() {
        let build_after_or_equal = match (&window.build_verde_at, &window.last_activity_at) {
            (Some(bv), Some(la)) => bv.as_str() >= la.as_str(),
            (Some(_), None) => true,
            _ => false,
        };
        if build_after_or_equal {
            return "archived";
        }
    }
    if window.last_activity_at.is_some() {
        return "closed-amend-pending";
    }
    "resolved"
}

/// Locate the spec directory under `project_root/.claude/spec/`.
/// Returns `(path, in_active)`.
fn locate_spec_dir(project_root: &Path, spec_id: &str) -> Option<(PathBuf, bool)> {
    let active = project_root
        .join(".claude")
        .join("spec")
        .join("active")
        .join(spec_id);
    if active.exists() {
        return Some((active, true));
    }
    let archived = project_root
        .join(".claude")
        .join("spec")
        .join("archived")
        .join(spec_id);
    if archived.exists() {
        return Some((archived, false));
    }
    None
}

/// Build the `## Amendments` markdown block for the given window and events.
fn build_amendments_block(
    window: &AmendWindow,
    events: &[HarnessEvent],
    status: &str,
    lang: &str,
    now: &str,
) -> String {
    let session_short = window.session_id.chars().take(8).collect::<String>();
    let header = if lang == "pt" {
        format!(
            "## Amendments (session {}, {} → {})\n",
            session_short, window.closed_at, now
        )
    } else {
        format!(
            "## Amendments (session {}, {} → {})\n",
            session_short, window.closed_at, now
        )
    };

    let mut lines = vec![header];

    for ev in events {
        let at = ev.ts.as_str();
        match ev.event.as_str() {
            k if k == EVENT_PIPELINE_AMEND_INTENT => {
                let prompt_text = ev
                    .payload
                    .get("prompt_text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if lang == "pt" {
                    lines.push(format!("- {} prompt do usuário: \"{}\"", at, prompt_text));
                } else {
                    lines.push(format!("- {} user prompt: \"{}\"", at, prompt_text));
                }
            }
            k if k == EVENT_PIPELINE_AMEND_ACTIVITY => {
                let tool = ev
                    .payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let file_path = ev
                    .payload
                    .get("file_path")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                lines.push(format!("- {} {} `{}`", at, tool, file_path));
            }
            k if k == EVENT_PIPELINE_AMEND_DRIFT => {
                let n = ev
                    .payload
                    .get("unrelated_paths")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                if lang == "pt" {
                    lines.push(format!("- {} drift detectado: {} arquivos fora do escopo", at, n));
                } else {
                    lines.push(format!("- {} drift detected: {} files outside scope", at, n));
                }
            }
            // EVENT_PIPELINE_AMEND_OPEN is informational only — skip it.
            _ => {}
        }
    }

    // Check for build verde indicator (build_verde_at stamped on window).
    if let Some(bv) = &window.build_verde_at {
        if lang == "pt" {
            lines.push(format!("- {} build verde", bv));
        } else {
            lines.push(format!("- {} build green", bv));
        }
    }

    if lang == "pt" {
        lines.push(format!("- resolução: {}", status));
    } else {
        lines.push(format!("- resolution: {}", status));
    }

    lines.join("\n")
}

/// Append the amendments block to `spec.md` at `spec_dir`.
fn append_to_spec(spec_dir: &Path, block: &str) -> std::result::Result<(), String> {
    let spec_file = spec_dir.join("spec.md");
    let existing = std::fs::read_to_string(&spec_file)
        .map_err(|e| format!("read spec.md: {e}"))?;
    let updated = format!("{}\n{}\n", existing.trim_end_matches('\n'), block);
    std::fs::write(&spec_file, updated).map_err(|e| format!("write spec.md: {e}"))
}

/// Move `active/{spec_id}` to `archived/{spec_id}`. Creates `archived/` if needed.
fn move_to_archived(project_root: &Path, spec_id: &str) -> std::result::Result<(), String> {
    let src = project_root
        .join(".claude")
        .join("spec")
        .join("active")
        .join(spec_id);
    let dst_parent = project_root
        .join(".claude")
        .join("spec")
        .join("archived");
    let dst = dst_parent.join(spec_id);

    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&dst_parent)
        .map_err(|e| format!("create archived/: {e}"))?;
    std::fs::rename(&src, &dst).map_err(|e| format!("rename {src:?} → {dst:?}: {e}"))
}

/// Emit a [`EVENT_PIPELINE_AMEND_CLOSE`] harness event.
fn emit_amend_close(
    store: &SqliteEventStore,
    session_id: &str,
    spec_id: &str,
    status: &str,
    window: &AmendWindow,
) {
    let payload = PipelineAmendClosePayload {
        spec_id: spec_id.to_string(),
        session_id: session_id.to_string(),
        status: status.to_string(),
        closed_at: Some(window.closed_at.clone()),
        build_verde: Some(window.build_verde_at.is_some()),
        drift_emitted: Some(window.drift_emitted),
    };
    let payload_value = serde_json::to_value(&payload).unwrap_or(Value::Null);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Cli,
            id: Some("amend-finalize".to_string()),
            actor_type: None,
        },
        event: EVENT_PIPELINE_AMEND_CLOSE.to_string(),
        payload: payload_value,
        spec: Some(spec_id.to_string()),
    };
    let _ = store.append(&ev);
}

/// Finalize one amendment window. Returns the status string or an error message.
fn finalize_window(
    store: &SqliteEventStore,
    window: &AmendWindow,
    project_root: &Path,
) -> std::result::Result<String, String> {
    let status = decide_status(window);

    // Collect events for this window: all events for spec_id where session_id
    // matches and kind is one of the amend kinds.
    let all_events = store
        .query(Some(&window.spec_id))
        .map_err(|e| format!("query events: {e}"))?;

    let amend_kinds = [
        EVENT_PIPELINE_AMEND_OPEN,
        EVENT_PIPELINE_AMEND_ACTIVITY,
        EVENT_PIPELINE_AMEND_INTENT,
        EVENT_PIPELINE_AMEND_DRIFT,
    ];
    let mut window_events: Vec<HarnessEvent> = all_events
        .into_iter()
        .filter(|e| {
            e.session_id == window.session_id && amend_kinds.contains(&e.event.as_str())
        })
        .collect();
    // Sort ascending by timestamp (lexicographic ISO-8601).
    window_events.sort_by(|a, b| a.ts.cmp(&b.ts));

    // Resolve lang.
    let lang = resolve_lang(store, &window.spec_id);
    let now = now_iso8601();

    // Build the amendments block.
    let block = build_amendments_block(window, &window_events, status, &lang, &now);

    // Locate the spec dir.
    if let Some((spec_dir, in_active)) = locate_spec_dir(project_root, &window.spec_id) {
        // Append to spec.md (best-effort — report error but continue).
        if let Err(e) = append_to_spec(&spec_dir, &block) {
            eprintln!("[amend-finalize] WARN: could not append to spec.md: {e}");
        }
        // Move to archived if status == "archived" and still in active/.
        if status == "archived" && in_active {
            if let Err(e) = move_to_archived(project_root, &window.spec_id) {
                eprintln!("[amend-finalize] WARN: could not move spec to archived/: {e}");
            }
        }
    } else {
        eprintln!(
            "[amend-finalize] WARN: spec dir not found for '{}' — skipping spec.md append",
            window.spec_id
        );
    }

    // Update DB.
    store
        .close_amend_window(&window.spec_id, &window.session_id, status)
        .map_err(|e| format!("close_amend_window: {e}"))?;

    // Emit close event.
    emit_amend_close(store, &window.session_id, &window.spec_id, status, window);

    Ok(status.to_string())
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Finalize all open amendment windows for `session_id`.
///
/// This is the shared logic callable both from the `SessionEnd` hook (in-process)
/// and from the CLI face (`mustard-rt run amend-finalize --session-id`).
///
/// Fail-open: per-window errors are collected and reported; the function always
/// returns `Ok(RunReport)`.
pub fn run(session_id: &str, store: &SqliteEventStore) -> Result<RunReport> {
    let project_root = project_root();
    run_with_root(session_id, store, &project_root)
}

/// Like [`run`] but accepts an explicit project root. Used by tests.
pub fn run_with_root(
    session_id: &str,
    store: &SqliteEventStore,
    project_root: &Path,
) -> Result<RunReport> {
    let windows = store.amend_windows_by_session(session_id)?;
    let mut results = Vec::with_capacity(windows.len());
    for window in &windows {
        let window_result = match finalize_window(store, window, project_root) {
            Ok(status) => WindowResult {
                spec_id: window.spec_id.clone(),
                status,
                error: None,
            },
            Err(e) => WindowResult {
                spec_id: window.spec_id.clone(),
                status: "error".to_string(),
                error: Some(e),
            },
        };
        results.push(window_result);
    }
    Ok(RunReport {
        session_id: session_id.to_string(),
        windows: results,
    })
}

/// CLI face: `mustard-rt run amend-finalize --session-id <id>`.
///
/// Resolves the project store via `MUSTARD_DB_PATH` / `CLAUDE_PROJECT_DIR` /
/// `cwd`, runs finalization, and prints the JSON summary to stdout.
/// Always exits `0` (fail-open).
pub fn run_cli(session_id: &str) {
    let project_root = project_root();
    let store = match SqliteEventStore::for_project(&project_root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[amend-finalize] could not open store: {e} (skipping)");
            let report = RunReport {
                session_id: session_id.to_string(),
                windows: Vec::new(),
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&report.to_json())
                    .unwrap_or_else(|_| "{}".to_string())
            );
            return;
        }
    };
    match run_with_root(session_id, &store, &project_root) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report.to_json())
                    .unwrap_or_else(|_| "{}".to_string())
            );
        }
        Err(e) => {
            eprintln!("[amend-finalize] error: {e} (fail-open)");
            let report = RunReport {
                session_id: session_id.to_string(),
                windows: Vec::new(),
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&report.to_json())
                    .unwrap_or_else(|_| "{}".to_string())
            );
        }
    }
}
