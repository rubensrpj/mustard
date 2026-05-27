//! `mustard-rt run amend-finalize` — session-end amendment window finalization.
//!
//! ## Scope (W4C migration)
//!
//! Reads every `.amend-window.json` under `.claude/spec/*/` (the per-spec
//! filesystem state introduced in W3C), filters by `session_id`, and for each
//! window:
//!
//! 1. Decides the final `status` (`archived`, `closed-amend-pending`,
//!    `closed-amend-drift`, or `resolved`).
//! 2. Appends a `## Amendments` block (respecting the spec's language) to
//!    `spec.md`.
//! 3. Sets `closed: true` in the `.amend-window.json` via atomic write.
//! 4. Emits a `pipeline.amend_close` event into the per-spec NDJSON sink.
//!
//! ## Fail-open
//!
//! Any per-window error is collected into the [`RunReport`] and reported as
//! JSON. The subcommand always exits `0`.

use crate::util::now_iso8601;
use mustard_core::claude_paths::ClaudePaths;
use mustard_core::error::Result;
use mustard_core::fs;
use mustard_core::projection::read_harness_events_from_ndjson_dir;
use mustard_core::model::event::{
    HarnessEvent, PipelineAmendClosePayload, EVENT_PIPELINE_AMEND_ACTIVITY,
    EVENT_PIPELINE_AMEND_CLOSE, EVENT_PIPELINE_AMEND_DRIFT, EVENT_PIPELINE_AMEND_INTENT,
    EVENT_PIPELINE_AMEND_OPEN,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Environment variable for project root override (test-only convention).
const PROJECT_ROOT_ENV: &str = "MUSTARD_PROJECT_ROOT";

/// JSON schema persisted at `.claude/spec/{id}/.amend-window.json` — mirrors
/// the W3C `amend_capture::WindowState` (duplicated here so this run module
/// does not import from a sibling hook).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct WindowState {
    #[serde(default)]
    opened_at: String,
    #[serde(default)]
    expires_at: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    subprojects: Vec<String>,
    #[serde(default)]
    drift: Vec<String>,
    #[serde(default)]
    drift_emitted: bool,
    #[serde(default)]
    last_activity_at: Option<String>,
    #[serde(default)]
    build_verde_at: Option<String>,
    #[serde(default)]
    closed: bool,
    /// Optional — when the W3C writer does not set this, we fall back to the
    /// session id discovered from per-spec events.
    #[serde(default)]
    session_id: Option<String>,
}

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

/// Resolve the project root.
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

/// Walk `.claude/spec/*/.amend-window.json` and return every `(spec_id,
/// window)` pair whose `session_id` matches `session_id` — or, when the
/// window's session is absent (older files), every open window.
fn read_windows_for_session(project_root: &Path, session_id: &str) -> Vec<(String, WindowState)> {
    let Ok(cp) = ClaudePaths::for_project(project_root) else {
        return Vec::new();
    };
    let spec_root = cp.spec_dir();
    let Ok(entries) = std::fs::read_dir(&spec_root) else {
        return Vec::new();
    };
    let mut out: Vec<(String, WindowState)> = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let spec_id = match dir.file_name().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let win_path = dir.join(".amend-window.json");
        if !win_path.exists() {
            continue;
        }
        let Ok(text) = fs::read_to_string(&win_path) else {
            continue;
        };
        let Ok(win) = serde_json::from_str::<WindowState>(&text) else {
            continue;
        };
        if win.closed {
            continue;
        }
        // Match by session if window declares it; otherwise include (legacy).
        match &win.session_id {
            Some(s) if s != session_id => continue,
            _ => out.push((spec_id, win)),
        }
    }
    out
}

/// Derive `lang` from the latest `pipeline.scope` event for `spec_id`. Defaults
/// to `"en-US"` (BCP-47).
fn resolve_lang(cwd: &Path, spec_id: &str) -> String {
    let events = read_events_for_spec(cwd, spec_id);
    let raw = events
        .iter()
        .rfind(|e| e.event == "pipeline.scope")
        .and_then(|e| e.payload.get("lang"))
        .and_then(Value::as_str)
        .unwrap_or("en-US")
        .to_string();
    let lc = raw.trim().to_ascii_lowercase();
    if lc == "pt" || lc == "pt-br" {
        "pt-BR".to_string()
    } else {
        "en-US".to_string()
    }
}

fn read_events_for_spec(cwd: &Path, spec_id: &str) -> Vec<HarnessEvent> {
    let Ok(cp) = ClaudePaths::for_project(cwd) else {
        return Vec::new();
    };
    let Ok(sp) = cp.for_spec(spec_id) else {
        return Vec::new();
    };
    read_harness_events_from_ndjson_dir(&sp.events_dir())
}

/// Decide the final status string for a window.
fn decide_status(window: &WindowState) -> &'static str {
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

/// Locate the spec directory under `project_root/.claude/spec/{spec_id}/`.
fn locate_spec_dir(project_root: &Path, spec_id: &str) -> Option<PathBuf> {
    let flat = ClaudePaths::for_project(project_root)
        .and_then(|p| p.for_spec(spec_id))
        .ok()?
        .dir()
        .to_path_buf();
    if flat.exists() {
        Some(flat)
    } else {
        None
    }
}

/// Build the `## Amendments` markdown block for the given window and events.
fn build_amendments_block(
    window: &WindowState,
    events: &[HarnessEvent],
    status: &str,
    lang: &str,
    now: &str,
) -> String {
    let session_short = window
        .session_id
        .as_deref()
        .map(|s| s.chars().take(8).collect::<String>())
        .unwrap_or_default();
    let header = format!(
        "## Amendments (session {}, {} → {})\n",
        session_short, window.opened_at, now
    );
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
                if lang == "pt-BR" {
                    lines.push(format!("- {at} prompt do usuário: \"{prompt_text}\""));
                } else {
                    lines.push(format!("- {at} user prompt: \"{prompt_text}\""));
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
                lines.push(format!("- {at} {tool} `{file_path}`"));
            }
            k if k == EVENT_PIPELINE_AMEND_DRIFT => {
                let n = ev
                    .payload
                    .get("unrelated_paths")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                if lang == "pt-BR" {
                    lines.push(format!("- {at} drift detectado: {n} arquivos fora do escopo"));
                } else {
                    lines.push(format!("- {at} drift detected: {n} files outside scope"));
                }
            }
            _ => {}
        }
    }

    if let Some(bv) = &window.build_verde_at {
        if lang == "pt-BR" {
            lines.push(format!("- {bv} build verde"));
        } else {
            lines.push(format!("- {bv} build green"));
        }
    }
    if lang == "pt-BR" {
        lines.push(format!("- resolução: {status}"));
    } else {
        lines.push(format!("- resolution: {status}"));
    }
    lines.join("\n")
}

fn append_to_spec(spec_dir: &Path, block: &str) -> std::result::Result<(), String> {
    let spec_file = spec_dir.join("spec.md");
    let existing = fs::read_to_string(&spec_file)
        .map_err(|e| format!("read spec.md: {e}"))?;
    let updated = format!("{}\n{}\n", existing.trim_end_matches('\n'), block);
    fs::write_atomic(&spec_file, updated.as_bytes())
        .map_err(|e| format!("write spec.md: {e}"))
}

fn write_window_state(project_root: &Path, spec_id: &str, state: &WindowState) -> std::result::Result<(), String> {
    let cp = ClaudePaths::for_project(project_root).map_err(|e| format!("paths: {e}"))?;
    let sp = cp.for_spec(spec_id).map_err(|e| format!("spec paths: {e}"))?;
    let dest = sp.dir().join(".amend-window.json");
    let json = serde_json::to_vec_pretty(state).map_err(|e| format!("encode: {e}"))?;
    fs::write_atomic(&dest, &json).map_err(|e| format!("write: {e}"))
}

/// Emit a `pipeline.amend_close` event into the per-spec NDJSON sink.
fn emit_amend_close(
    cwd: &Path,
    session_id: &str,
    spec_id: &str,
    status: &str,
    window: &WindowState,
) {
    let payload = PipelineAmendClosePayload {
        spec_id: spec_id.to_string(),
        session_id: session_id.to_string(),
        status: status.to_string(),
        closed_at: Some(now_iso8601()),
        build_verde: Some(window.build_verde_at.is_some()),
        drift_emitted: Some(window.drift_emitted),
    };
    let payload_value = serde_json::to_value(&payload).unwrap_or(Value::Null);
    let kind = crate::run::event_route::classify_kind(EVENT_PIPELINE_AMEND_CLOSE);
    let ts = now_iso8601();
    let _ = crate::run::event_writer_ndjson::write_event_with_ts(
        cwd,
        Some(spec_id),
        None,
        session_id,
        EVENT_PIPELINE_AMEND_CLOSE,
        kind,
        Some(0),
        Some(session_id),
        Some("amend-finalize"),
        None,
        &payload_value,
        Some(&ts),
    );
}

/// Finalize one amendment window.
fn finalize_window(
    project_root: &Path,
    session_id: &str,
    spec_id: &str,
    window: &mut WindowState,
) -> std::result::Result<String, String> {
    let status = decide_status(window);

    // Collect amend events for this window from per-spec NDJSON.
    let all_events = read_events_for_spec(project_root, spec_id);
    let amend_kinds = [
        EVENT_PIPELINE_AMEND_OPEN,
        EVENT_PIPELINE_AMEND_ACTIVITY,
        EVENT_PIPELINE_AMEND_INTENT,
        EVENT_PIPELINE_AMEND_DRIFT,
    ];
    let mut window_events: Vec<HarnessEvent> = all_events
        .into_iter()
        .filter(|e| {
            e.session_id == session_id && amend_kinds.contains(&e.event.as_str())
        })
        .collect();
    window_events.sort_by(|a, b| a.ts.cmp(&b.ts));

    let lang = resolve_lang(project_root, spec_id);
    let now = now_iso8601();
    let block = build_amendments_block(window, &window_events, status, &lang, &now);

    if let Some(spec_dir) = locate_spec_dir(project_root, spec_id) {
        if let Err(e) = append_to_spec(&spec_dir, &block) {
            eprintln!("[amend-finalize] WARN: could not append to spec.md: {e}");
        }
    } else {
        eprintln!(
            "[amend-finalize] WARN: spec dir not found for '{spec_id}' — skipping spec.md append"
        );
    }

    // Flip closed flag and rewrite the json sidecar.
    window.closed = true;
    write_window_state(project_root, spec_id, window)?;

    // Emit close event into per-spec NDJSON sink.
    emit_amend_close(project_root, session_id, spec_id, status, window);

    Ok(status.to_string())
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Finalize all open amendment windows for `session_id`. Fail-open.
pub fn run(session_id: &str) -> Result<RunReport> {
    let project_root = project_root();
    run_with_root(session_id, &project_root)
}

/// Like [`run`] but accepts an explicit project root. Used by tests.
pub fn run_with_root(session_id: &str, project_root: &Path) -> Result<RunReport> {
    let windows = read_windows_for_session(project_root, session_id);
    let mut results = Vec::with_capacity(windows.len());
    for (spec_id, mut win) in windows {
        let window_result = match finalize_window(project_root, session_id, &spec_id, &mut win) {
            Ok(status) => WindowResult {
                spec_id,
                status,
                error: None,
            },
            Err(e) => WindowResult {
                spec_id,
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

/// CLI face: `mustard-rt run amend-finalize --session-id <id>`. Always
/// exits `0`.
pub fn run_cli(session_id: &str) {
    match run(session_id) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string_pretty(&report.to_json()).unwrap_or_else(|_| "{}".to_string())
        ),
        Err(e) => {
            eprintln!("[amend-finalize] error: {e} (fail-open)");
            let empty = RunReport {
                session_id: session_id.to_string(),
                windows: Vec::new(),
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&empty.to_json()).unwrap_or_else(|_| "{}".to_string())
            );
        }
    }
}
