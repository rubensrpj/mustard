//! `amend_capture` — session-bound amendment window enforcement module.
//!
//! ## Scope (Wave 2 — 2026-05-20-session-bound-amendments, W3C migration)
//!
//! Fires on `PostToolUse(Bash|Write|Edit)` and `UserPromptSubmit` to track
//! in-session edits that happen after a pipeline is closed. When the number of
//! edits that fall outside the original pipeline scope (`drift`) exceeds the
//! configured threshold, an advisory is injected into the agent's context.
//!
//! ## Persistence (W3C)
//!
//! The amendment window state is no longer stored in SQLite. It is persisted to
//! `.claude/spec/{spec_id}/.amend-window.json` via atomic write (tmpfile +
//! rename) using [`mustard_core::io::fs::write_atomic`]. Reading is idempotent: a
//! missing file returns a default closed window.
//!
//! Schema: `{ "opened_at": iso, "expires_at": iso, "files": [..],
//! "subprojects": [..], "drift": [..], "drift_emitted": bool,
//! "last_activity_at": iso|null, "build_verde_at": iso|null }`.
//!
//! ## Dual-face design
//!
//! [`AmendCapture`] implements both [`Observer`] (side-effects, no decision)
//! and [`Check`] (look-ahead drift injection, `Verdict::Allow` with optional
//! `Inject`). The `Check` side never writes state — it only forecasts; the
//! `Observer` side owns all mutations.
//!
//! ## Fail-open guarantee
//!
//! All errors inside `observe` are swallowed (`let _ = …`). `evaluate` returns
//! `Ok(Verdict::Allow)` on any failure to load the window.

use crate::shared::context::current_spec;
use mustard_core::time::now_iso8601;
use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::domain::model::event::{
    Actor, ActorKind, HarnessEvent, PipelineAmendActivityPayload, PipelineAmendClosePayload,
    PipelineAmendDriftPayload, PipelineAmendIntentPayload, PipelineAmendOpenPayload,
    SCHEMA_VERSION, EVENT_PIPELINE_AMEND_ACTIVITY, EVENT_PIPELINE_AMEND_CLOSE,
    EVENT_PIPELINE_AMEND_DRIFT, EVENT_PIPELINE_AMEND_INTENT, EVENT_PIPELINE_AMEND_OPEN,
};
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Default drift threshold — may be overridden by mustard.json
// ---------------------------------------------------------------------------

/// Default number of out-of-scope edits before a drift warning fires.
const DEFAULT_DRIFT_THRESHOLD: u32 = 3;

/// Amendment window expiry: 72 h after the pipeline was closed.
const WINDOW_EXPIRY_SECS: u64 = 72 * 60 * 60;

/// Read `amend.drift_threshold` from `{project_dir}/.claude/mustard.json`,
/// falling back to [`DEFAULT_DRIFT_THRESHOLD`] on any error.
fn drift_threshold(project_dir: &str) -> u32 {
    let path = match ClaudePaths::for_project(project_dir) {
        Ok(cp) => cp.mustard_json_path(),
        Err(_) => return DEFAULT_DRIFT_THRESHOLD,
    };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    v.get("amend")
        .and_then(|a| a.get("drift_threshold"))
        .and_then(serde_json::Value::as_u64)
        .map_or(DEFAULT_DRIFT_THRESHOLD, |n| {
            u32::try_from(n).unwrap_or(DEFAULT_DRIFT_THRESHOLD)
        })
}

// ---------------------------------------------------------------------------
// JSON window state
// ---------------------------------------------------------------------------

/// The amendment window state persisted to `.claude/spec/{id}/.amend-window.json`.
///
/// Replaces the `AmendWindow` SQLite row (W3C migration).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WindowState {
    /// ISO-8601 timestamp when the window was opened (pipeline closed).
    #[serde(default)]
    pub opened_at: String,
    /// ISO-8601 timestamp when the window expires.
    #[serde(default)]
    pub expires_at: String,
    /// The allowed file set from the original pipeline run.
    #[serde(default)]
    pub files: Vec<String>,
    /// Subproject prefixes active during the original pipeline run.
    #[serde(default)]
    pub subprojects: Vec<String>,
    /// Paths edited outside `files`/`subprojects` (drift candidates).
    #[serde(default)]
    pub drift: Vec<String>,
    /// Whether at least one drift event was emitted for this window.
    #[serde(default)]
    pub drift_emitted: bool,
    /// ISO-8601 timestamp of the most recent activity, or `null`.
    #[serde(default)]
    pub last_activity_at: Option<String>,
    /// ISO-8601 timestamp at which the build turned green, or `null`.
    #[serde(default)]
    pub build_verde_at: Option<String>,
    /// Whether the window has been closed (resolved / pending).
    #[serde(default)]
    pub closed: bool,
}

/// Resolve the path of `.amend-window.json` for `spec_id` in `project_dir`.
/// Returns `None` when `spec_id` is empty or `ClaudePaths` cannot be built.
fn window_path(project_dir: &str, spec_id: &str) -> Option<std::path::PathBuf> {
    if spec_id.is_empty() {
        return None;
    }
    let paths = ClaudePaths::for_project(project_dir).ok()?;
    let sp = paths.for_spec(spec_id).ok()?;
    Some(sp.dir().join(".amend-window.json"))
}

/// Read the window state for `spec_id`. Returns a default (closed) window when
/// the file is absent or malformed — idempotent fail-open.
fn read_window(project_dir: &str, spec_id: &str) -> WindowState {
    let Some(path) = window_path(project_dir, spec_id) else {
        return WindowState::default();
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return WindowState::default(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Read the current open window for `session_id` by checking the spec
/// associated with `project_dir`. Returns `None` when no spec is active or the
/// window is closed/absent.
///
/// We infer the spec from [`current_spec`] (env var → pipeline-state file).
fn active_window(project_dir: &str) -> Option<(String, WindowState)> {
    let spec_id = current_spec(project_dir)?;
    let win = read_window(project_dir, &spec_id);
    if win.closed || win.opened_at.is_empty() {
        return None;
    }
    Some((spec_id, win))
}

/// Write `state` atomically to `.amend-window.json` for `spec_id`.
/// Ensures the parent spec directory exists first. Fail-open: errors are
/// silently dropped by the caller.
fn write_window(project_dir: &str, spec_id: &str, state: &WindowState) -> Result<(), ()> {
    let path = window_path(project_dir, spec_id).ok_or(())?;
    // Ensure the spec directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ())?;
    }
    let json = serde_json::to_vec_pretty(state).map_err(|_| ())?;
    mustard_core::io::fs::write_atomic(&path, &json).map_err(|_| ())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------


/// Extract `file_path` from a Write/Edit `tool_input`.
fn tool_file_path(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Extract `command` from a Bash `tool_input`.
fn tool_command(input: &HookInput) -> Option<&str> {
    input.tool_input.get("command").and_then(serde_json::Value::as_str)
}

/// `exit_code` from `tool_response`, defaulting to `1` when absent.
fn exit_code(input: &HookInput) -> i64 {
    input
        .raw
        .get("tool_response")
        .and_then(|r| r.get("exit_code"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(1)
}

/// `true` when `command` matches `mustard-rt run emit-pipeline … --kind
/// pipeline.complete … --spec <id>` and `exit_code` is 0.
fn is_pipeline_complete_bash(cmd: &str) -> bool {
    cmd.contains("mustard-rt")
        && cmd.contains("emit-pipeline")
        && cmd.contains("pipeline.complete")
}

/// Extract `--spec <id>` from a command string. Returns `None` when absent.
fn extract_spec_arg(cmd: &str) -> Option<String> {
    let mut parts = cmd.split_whitespace().peekable();
    while let Some(part) = parts.next() {
        if part == "--spec" {
            if let Some(value) = parts.next() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// `true` when `command` matches a build/test invocation and `exit_code` is 0.
fn is_build_success(cmd: &str) -> bool {
    let c = cmd.trim();
    c.contains("cargo build")
        || c.contains("cargo test")
        || c.contains("cargo check")
        || c.contains("pnpm build")
        || c.contains("pnpm test")
        || c.contains("npm test")
        || (c.contains("npm run") && c.contains("build"))
        || c.contains("tsc")
}

/// Derive unique subproject prefixes from a set of file paths.
///
/// Extracts `apps/{X}/` and `packages/{X}/` prefixes. A path like
/// `apps/rt/src/hooks/mod.rs` produces `apps/rt/`. Forward slashes only
/// for portability.
fn derive_subprojects(paths: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for p in paths {
        let normalized = p.replace('\\', "/");
        for prefix in ["apps/", "packages/"] {
            if let Some(rest) = normalized.strip_prefix(prefix) {
                if let Some(end) = rest.find('/') {
                    let sub = format!("{}{}/", prefix, &rest[..end]);
                    seen.insert(sub);
                }
            }
        }
    }
    seen.into_iter().collect()
}

/// `true` if `file_path` is in scope for the amendment window.
///
/// In-scope means: the path is in `files` OR it starts with any of the
/// `subprojects` prefixes (using forward-slash normalization).
fn is_in_scope(file_path: &str, window: &WindowState) -> bool {
    let normalized = file_path.replace('\\', "/");
    if window
        .files
        .iter()
        .any(|p| p.replace('\\', "/") == normalized)
    {
        return true;
    }
    window.subprojects.iter().any(|sub| {
        let sub_norm = sub.replace('\\', "/");
        normalized.starts_with(&sub_norm)
    })
}

/// Emit a harness event best-effort via the NDJSON route; failures are silently dropped.
fn emit(project_dir: &str, session_id: &str, event_name: &str, payload: serde_json::Value) {
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id.to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("amend_capture".to_string()),
            actor_type: None,
        },
        event: event_name.to_string(),
        payload,
        spec: current_spec(project_dir),
    };
    let _ = crate::shared::events::route::emit(project_dir, &ev);
}

// ---------------------------------------------------------------------------
// Observer — side effects, no decision
// ---------------------------------------------------------------------------

/// The amendment capture module — both an [`Observer`] and a [`Check`].
pub struct AmendCapture;

impl Observer for AmendCapture {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let Some(trigger) = ctx.trigger else {
            return;
        };
        let pdir = ctx.project_dir_or_cwd(input);
        let session_id = match input.session_id.as_deref() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return,
        };

        match trigger {
            Trigger::PostToolUse => {
                let tool = input.tool_name.as_deref().unwrap_or("");
                match tool {
                    "Bash" => {
                        if let Some(cmd) = tool_command(input) {
                            // Arm 1: pipeline.complete detection → open window.
                            if is_pipeline_complete_bash(cmd) && exit_code(input) == 0 {
                                if let Some(spec_id) = extract_spec_arg(cmd) {
                                    observe_pipeline_complete(
                                        &pdir,
                                        &session_id,
                                        &spec_id,
                                    );
                                }
                            }
                            // Arm 2: build/test success → stamp build_verde_at.
                            if is_build_success(cmd) && exit_code(input) == 0 {
                                let now = now_iso8601();
                                if let Some((spec_id, mut win)) = active_window(&pdir) {
                                    if win.build_verde_at.is_none() {
                                        win.build_verde_at = Some(now);
                                        let _ = write_window(&pdir, &spec_id, &win);
                                    }
                                }
                            }
                        }
                    }
                    "Write" | "Edit" => {
                        if let Some(file_path) = tool_file_path(input) {
                            observe_write_edit(&pdir, &session_id, tool, &file_path);
                        }
                    }
                    _ => {}
                }
            }
            Trigger::UserPromptSubmit => {
                observe_user_prompt(input, ctx, &pdir, &session_id);
            }
            _ => {}
        }
    }
}

/// Handle PostToolUse(Bash) where a `pipeline.complete` emit was detected.
fn observe_pipeline_complete(project_dir: &str, session_id: &str, spec_id: &str) {
    // Gather the file set that the pipeline touched. We derive it from the
    // `pipeline.complete` event payload stored in the NDJSON event log. Fail-
    // open: an empty file set still opens the window (all edits become drift).
    let file_set = gather_pipeline_file_set(project_dir, spec_id);
    let subprojects = derive_subprojects(&file_set);
    let now = now_iso8601();
    // Compute expires_at = now + WINDOW_EXPIRY_SECS.
    let expires_at = {
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
            .saturating_add(WINDOW_EXPIRY_SECS);
        // Use the same civil-from-days logic as now_iso8601.
        mustard_core::time::millis_to_iso(secs as i64 * 1000)
    };
    let state = WindowState {
        opened_at: now.clone(),
        expires_at,
        files: file_set.clone(),
        subprojects: subprojects.clone(),
        ..Default::default()
    };
    let _ = write_window(project_dir, spec_id, &state);
    let payload = PipelineAmendOpenPayload {
        spec_id: spec_id.to_string(),
        session_id: session_id.to_string(),
        closed_at: now,
        pipeline_file_set: file_set,
        subprojects,
    };
    let event_payload = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
    emit(project_dir, session_id, EVENT_PIPELINE_AMEND_OPEN, event_payload);
}

/// Derive the file set from the last `pipeline.complete` event for `spec_id`.
/// Reads the per-spec NDJSON `.events/` directory. Fail-open: empty on error.
fn gather_pipeline_file_set(project_dir: &str, spec_id: &str) -> Vec<String> {
    use mustard_core::EventReader;

    let Ok(cp) = ClaudePaths::for_project(project_dir) else {
        return Vec::new();
    };
    let Ok(sp) = cp.for_spec(spec_id) else {
        return Vec::new();
    };
    let events_dir = sp.events_dir();
    let Ok(entries) = std::fs::read_dir(&events_dir) else {
        return Vec::new();
    };

    let mut file_set: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|x| x.to_str()) != Some("ndjson") {
            continue;
        }
        for ev in EventReader::stream(&p) {
            if ev.kind == "pipeline.complete" {
                if let Some(files) = ev.payload.get("affected_files").and_then(|v| v.as_array()) {
                    file_set = files
                        .iter()
                        .filter_map(|f| f.as_str().map(str::to_string))
                        .collect();
                }
            }
        }
    }
    file_set
}


/// Handle PostToolUse(Write|Edit) — record activity or accumulate drift.
fn observe_write_edit(project_dir: &str, session_id: &str, tool: &str, file_path: &str) {
    let Some((spec_id, mut window)) = active_window(project_dir) else {
        return;
    };

    if is_in_scope(file_path, &window) {
        let now = now_iso8601();
        window.last_activity_at = Some(now.clone());
        let _ = write_window(project_dir, &spec_id, &window);
        let act_payload = serde_json::to_value(PipelineAmendActivityPayload {
            spec_id: spec_id.clone(),
            session_id: session_id.to_string(),
            tool: tool.to_string(),
            file_path: file_path.to_string(),
            at: Some(now),
        })
        .unwrap_or(serde_json::Value::Null);
        emit(project_dir, session_id, EVENT_PIPELINE_AMEND_ACTIVITY, act_payload);
    } else {
        // Drift candidate.
        if window.drift_emitted {
            return;
        }
        let normalized = file_path.replace('\\', "/");
        if !window.drift.iter().any(|p| p.replace('\\', "/") == normalized) {
            window.drift.push(file_path.to_string());
        }
        let threshold = drift_threshold(project_dir);
        let new_len = window.drift.len();
        let _ = write_window(project_dir, &spec_id, &window);
        if u32::try_from(new_len).unwrap_or(u32::MAX) >= threshold {
            // Re-read to get the current drift list, then mark emitted.
            let mut win2 = read_window(project_dir, &spec_id);
            win2.drift_emitted = true;
            let _ = write_window(project_dir, &spec_id, &win2);
            let drift_payload = serde_json::to_value(PipelineAmendDriftPayload {
                spec_id: spec_id.clone(),
                session_id: session_id.to_string(),
                unrelated_paths: win2.drift.clone(),
                threshold: Some(threshold),
            })
            .unwrap_or(serde_json::Value::Null);
            emit(project_dir, session_id, EVENT_PIPELINE_AMEND_DRIFT, drift_payload);
        }
    }
}

/// Handle `UserPromptSubmit` — emit intent event when a window is open.
fn observe_user_prompt(input: &HookInput, _ctx: &Ctx, project_dir: &str, session_id: &str) {
    let Some((spec_id, _window)) = active_window(project_dir) else {
        return;
    };
    let prompt_text = input
        .raw
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let now = now_iso8601();
    let intent_payload = serde_json::to_value(PipelineAmendIntentPayload {
        spec_id: spec_id.clone(),
        session_id: session_id.to_string(),
        prompt_text,
        at: Some(now),
    })
    .unwrap_or(serde_json::Value::Null);
    emit(project_dir, session_id, EVENT_PIPELINE_AMEND_INTENT, intent_payload);
}

// ---------------------------------------------------------------------------
// Check — look-ahead drift injection, no state writes
// ---------------------------------------------------------------------------

impl Check for AmendCapture {
    /// `PreToolUse(Write|Edit)`: when the file would push drift past the
    /// threshold, inject a warning into the agent's context. No side effects.
    ///
    /// All other triggers return `Allow` immediately.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if !matches!(input.tool_name.as_deref(), Some("Write" | "Edit")) {
            return Ok(Verdict::Allow);
        }
        let Some(file_path) = tool_file_path(input) else {
            return Ok(Verdict::Allow);
        };
        let pdir = ctx.project_dir_or_cwd(input);
        let Some((spec_id, window)) = active_window(&pdir) else {
            return Ok(Verdict::Allow);
        };

        // If drift is already emitted, no further injection needed.
        if window.drift_emitted {
            return Ok(Verdict::Allow);
        }
        // Already in scope — nothing to warn.
        if is_in_scope(&file_path, &window) {
            return Ok(Verdict::Allow);
        }
        // Forecast: would adding this path push the count to >= threshold?
        let current_len = window.drift.len();
        let normalized = file_path.replace('\\', "/");
        let already_counted = window
            .drift
            .iter()
            .any(|p| p.replace('\\', "/") == normalized);
        let forecast_len = if already_counted {
            current_len
        } else {
            current_len + 1
        };
        let threshold = drift_threshold(&pdir);
        if u32::try_from(forecast_len).unwrap_or(u32::MAX) < threshold {
            return Ok(Verdict::Allow);
        }

        // Derive lang from spec header (best-effort).
        let lang = derive_spec_lang_from_fs(&pdir, &spec_id);
        let n = forecast_len;
        let body = mustard_core::translate("banner.amend.drift", lang);
        let warning = match lang {
            mustard_core::SupportedLocale::PtBr => format!(
                "Você está editando `{file_path}` em outro escopo da spec ativa \
                 `{spec_id}` (pós-CLOSE). Já são {n} arquivos fora do escopo declarado. \
                 {body}",
            ),
            mustard_core::SupportedLocale::EnUs => format!(
                "You're editing `{file_path}` outside the active spec `{spec_id}` scope \
                 (post-CLOSE). {n} files outside declared scope so far. {body}",
            ),
        };
        Ok(Verdict::Inject { context: warning })
    }
}

/// Retrieve the `lang` field from the spec header (`### Lang:`) via filesystem.
/// Fail-open: returns `None` when the spec file is unreadable or lacks the field.
fn derive_spec_lang_from_header(project_dir: &str, spec_id: &str) -> Option<String> {
    let paths = ClaudePaths::for_project(project_dir).ok()?;
    let sp = paths.for_spec(spec_id).ok()?;
    let text = std::fs::read_to_string(sp.spec_md_path()).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("### Lang:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// `derive_spec_lang_from_header` + BCP-47 normalisation. Fail-open: defaults
/// to [`mustard_core::SupportedLocale`] default (`PtBr`) when absent.
fn derive_spec_lang_from_fs(project_dir: &str, spec_id: &str) -> mustard_core::SupportedLocale {
    use std::str::FromStr;
    let raw = derive_spec_lang_from_header(project_dir, spec_id).unwrap_or_default();
    match mustard_core::SupportedLocale::from_str(&raw) {
        Ok(loc) => loc,
        Err(mustard_core::LocaleError::ShortForm(s)) => {
            if s.eq_ignore_ascii_case("pt-br") || s.eq_ignore_ascii_case("pt") {
                mustard_core::SupportedLocale::PtBr
            } else {
                mustard_core::SupportedLocale::EnUs
            }
        }
        Err(_) => mustard_core::SupportedLocale::default(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    // ---- helpers -------------------------------------------------------

    fn make_project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join(".harness")).unwrap();
        dir
    }

    fn seed_window_fs(
        project: &std::path::Path,
        spec_id: &str,
        files: Vec<String>,
        subprojects: Vec<String>,
    ) {
        let spec_dir = project.join(".claude").join("spec").join(spec_id);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let state = WindowState {
            opened_at: "2026-05-20T00:00:00.000Z".to_string(),
            expires_at: "2026-05-23T00:00:00.000Z".to_string(),
            files,
            subprojects,
            ..Default::default()
        };
        let json = serde_json::to_vec_pretty(&state).unwrap();
        let win_path = spec_dir.join(".amend-window.json");
        std::fs::write(&win_path, &json).unwrap();
    }

    fn post_write_input(session_id: &str, cwd: &str, file_path: &str) -> HookInput {
        HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_path }),
            hook_event_name: Some("PostToolUse".to_string()),
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.to_string()),
            raw: json!({ "tool_response": { "exit_code": 0 } }),
        }
    }

    fn post_bash_input(session_id: &str, cwd: &str, command: &str, exit_code: i64) -> HookInput {
        HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": command }),
            hook_event_name: Some("PostToolUse".to_string()),
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.to_string()),
            raw: json!({ "tool_response": { "exit_code": exit_code } }),
        }
    }

    fn prompt_input(session_id: &str, cwd: &str, prompt: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: serde_json::Value::Null,
            hook_event_name: Some("UserPromptSubmit".to_string()),
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.to_string()),
            raw: json!({ "prompt": prompt }),
        }
    }

    fn pre_write_input(session_id: &str, cwd: &str, file_path: &str) -> HookInput {
        HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_path }),
            hook_event_name: Some("PreToolUse".to_string()),
            session_id: Some(session_id.to_string()),
            cwd: Some(cwd.to_string()),
            raw: json!({}),
        }
    }

    fn post_ctx(trigger: Trigger, cwd: &str) -> Ctx {
        Ctx {
            project_dir: cwd.to_string(),
            trigger: Some(trigger),
            workspace_root: None,
        }
    }

    /// Set the active spec via a pipeline-state file so `current_spec` finds it.
    fn set_active_spec(project: &std::path::Path, spec_id: &str) {
        let states = project.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join(format!("{spec_id}.json")), "{}").unwrap();
    }

    // ---- AC-3: in-scope Write → activity written to window ----------------

    #[test]
    fn amend_capture_activity() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let spec_id = "spec-ac3";
        let in_scope_file = "apps/rt/src/hooks/mod.rs";

        set_active_spec(project.path(), spec_id);
        seed_window_fs(
            project.path(),
            spec_id,
            vec![in_scope_file.to_string()],
            vec!["apps/rt/".to_string()],
        );

        let input = post_write_input("session-ac3", cwd, in_scope_file);
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        AmendCapture.observe(&input, &ctx);

        let win = read_window(cwd, spec_id);
        assert!(win.last_activity_at.is_some(), "activity should be recorded");
    }

    // ---- AC-4: build/test success → build_verde_at stamped ----------------

    #[test]
    fn amend_capture_build_verde() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let spec_id = "spec-ac4";

        set_active_spec(project.path(), spec_id);
        seed_window_fs(
            project.path(),
            spec_id,
            vec!["apps/rt/src/lib.rs".to_string()],
            vec!["apps/rt/".to_string()],
        );

        let input = post_bash_input("session-ac4", cwd, "cargo test", 0);
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        AmendCapture.observe(&input, &ctx);

        let win = read_window(cwd, spec_id);
        assert!(win.build_verde_at.is_some());
    }

    // ---- AC-5: UserPromptSubmit when window active (no-panic check) -------

    #[test]
    fn amend_capture_intent_does_not_panic() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let spec_id = "spec-ac5";

        set_active_spec(project.path(), spec_id);
        seed_window_fs(
            project.path(),
            spec_id,
            vec!["apps/cli/src/main.rs".to_string()],
            vec!["apps/cli/".to_string()],
        );

        let input = prompt_input("session-ac5", cwd, "ajuste o follow-up");
        let ctx = post_ctx(Trigger::UserPromptSubmit, cwd);
        // Must not panic — fail-open.
        AmendCapture.observe(&input, &ctx);
    }

    // ---- AC-8: 1 drift file → under threshold, no event ------------------

    #[test]
    fn amend_drift_under_threshold() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let spec_id = "spec-ac8";

        set_active_spec(project.path(), spec_id);
        seed_window_fs(
            project.path(),
            spec_id,
            vec!["apps/rt/src/main.rs".to_string()],
            vec!["apps/rt/".to_string()],
        );

        let input = post_write_input("session-ac8", cwd, "docs/unrelated.md");
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        AmendCapture.observe(&input, &ctx);

        let win = read_window(cwd, spec_id);
        assert_eq!(win.drift.len(), 1);
        assert!(!win.drift_emitted);
    }

    // ---- AC-9: Check injects warning when forecast >= threshold -----------

    #[test]
    fn amend_drift_check_injects_warning() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let spec_id = "spec-ac9-chk";

        // Write mustard.json with threshold=3.
        let mustard_json = project.path().join(".claude").join("mustard.json");
        std::fs::write(&mustard_json, r#"{"amend":{"drift_threshold":3}}"#).unwrap();

        set_active_spec(project.path(), spec_id);
        // Pre-load 2 drift paths so the 3rd triggers the warning.
        let spec_dir = project.path().join(".claude").join("spec").join(spec_id);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let state = WindowState {
            opened_at: "2026-05-20T00:00:00.000Z".to_string(),
            expires_at: "2026-05-23T00:00:00.000Z".to_string(),
            files: vec!["apps/rt/src/main.rs".to_string()],
            subprojects: vec!["apps/rt/".to_string()],
            drift: vec!["docs/file1.md".to_string(), "docs/file2.md".to_string()],
            ..Default::default()
        };
        let json = serde_json::to_vec_pretty(&state).unwrap();
        std::fs::write(spec_dir.join(".amend-window.json"), &json).unwrap();

        let pre_ctx = Ctx {
            project_dir: cwd.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let pre_in = pre_write_input("session-ac9-chk", cwd, "docs/file3.md");
        let verdict = AmendCapture.evaluate(&pre_in, &pre_ctx).unwrap();
        assert!(
            matches!(verdict, Verdict::Inject { .. }),
            "expected Inject, got {verdict:?}"
        );
    }

    // ---- AC-10: file within declared subproject → no drift ---------------

    #[test]
    fn amend_drift_same_subproject_ok() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let spec_id = "spec-ac10";

        set_active_spec(project.path(), spec_id);
        seed_window_fs(
            project.path(),
            spec_id,
            vec!["apps/rt/src/main.rs".to_string()],
            vec!["apps/rt/".to_string()],
        );

        // File NOT in files but within "apps/rt/" subproject.
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        let input = post_write_input("session-ac10", cwd, "apps/rt/src/hooks/new_hook.rs");
        AmendCapture.observe(&input, &ctx);

        let win = read_window(cwd, spec_id);
        assert_eq!(win.drift.len(), 0, "subproject file must not drift");
        assert!(win.last_activity_at.is_some(), "subproject file is activity");
    }

    // ---- atomic write round-trip ------------------------------------------

    #[test]
    fn window_round_trips_via_json() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let spec_id = "my-spec";
        std::fs::create_dir_all(dir.path().join(".claude").join("spec").join(spec_id)).unwrap();
        let state = WindowState {
            opened_at: "2026-05-20T00:00:00.000Z".to_string(),
            expires_at: "2026-05-23T00:00:00.000Z".to_string(),
            files: vec!["src/main.rs".to_string()],
            subprojects: vec!["apps/cli/".to_string()],
            drift: vec!["docs/x.md".to_string()],
            drift_emitted: true,
            last_activity_at: Some("2026-05-20T01:00:00.000Z".to_string()),
            build_verde_at: None,
            closed: false,
        };
        write_window(cwd, spec_id, &state).unwrap();
        let loaded = read_window(cwd, spec_id);
        assert_eq!(loaded.opened_at, state.opened_at);
        assert_eq!(loaded.files, state.files);
        assert_eq!(loaded.drift, state.drift);
        assert!(loaded.drift_emitted);
        assert_eq!(loaded.last_activity_at, state.last_activity_at);
    }

    // ---- missing window file → default (closed) --------------------------

    #[test]
    fn missing_window_returns_default() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        let win = read_window(cwd, "nonexistent-spec");
        assert!(win.opened_at.is_empty());
        assert!(!win.drift_emitted);
    }
}

// ---------------------------------------------------------------------------
// Close helper (used by prompt_gate)
// ---------------------------------------------------------------------------

/// Close all open amendment windows for `session_id` when a new pipeline
/// command is detected. Status is `"resolved"` when the window has activity,
/// `"pending"` otherwise. Emits [`EVENT_PIPELINE_AMEND_CLOSE`] per window.
///
/// Best-effort — all errors are silently dropped.
pub fn close_amend_windows_for_session(project_dir: &str, session_id: &str) {
    let Some((spec_id, mut window)) = active_window(project_dir) else {
        return;
    };
    let status = if window.last_activity_at.is_some() {
        "resolved"
    } else {
        "pending"
    };
    window.closed = true;
    let _ = write_window(project_dir, &spec_id, &window);
    let now = now_iso8601();
    let close_payload = serde_json::to_value(PipelineAmendClosePayload {
        spec_id: spec_id.clone(),
        session_id: session_id.to_string(),
        status: status.to_string(),
        closed_at: Some(now),
        build_verde: Some(window.build_verde_at.is_some()),
        drift_emitted: Some(window.drift_emitted),
    })
    .unwrap_or(serde_json::Value::Null);
    emit(project_dir, session_id, EVENT_PIPELINE_AMEND_CLOSE, close_payload);
}
