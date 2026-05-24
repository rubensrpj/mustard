//! `amend_capture` — session-bound amendment window enforcement module.
//!
//! ## Scope (Wave 2 — 2026-05-20-session-bound-amendments)
//!
//! Fires on `PostToolUse(Bash|Write|Edit)` and `UserPromptSubmit` to track
//! in-session edits that happen after a pipeline is closed. When the number of
//! edits that fall outside the original pipeline scope (`drift`) exceeds the
//! configured threshold, an advisory is injected into the agent's context.
//!
//! ## Dual-face design
//!
//! [`AmendCapture`] implements both [`Observer`] (side-effects, no decision)
//! and [`Check`] (look-ahead drift injection, `Verdict::Allow` with optional
//! `Inject`). The `Check` side never writes to the database — it only
//! forecasts; the `Observer` side owns all mutations.
//!
//! ## Fail-open guarantee
//!
//! All errors inside `observe` are swallowed (`let _ = …`). `evaluate` returns
//! `Ok(Verdict::Allow)` on any failure to open the store or query the window.

use crate::run::current_spec;
use crate::util::now_iso8601;
use mustard_core::error::Error;
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::{AmendWindow, SqliteEventStore};
use mustard_core::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, PipelineAmendActivityPayload, PipelineAmendClosePayload,
    PipelineAmendDriftPayload, PipelineAmendIntentPayload, PipelineAmendOpenPayload,
    SCHEMA_VERSION, EVENT_PIPELINE_AMEND_ACTIVITY, EVENT_PIPELINE_AMEND_CLOSE,
    EVENT_PIPELINE_AMEND_DRIFT, EVENT_PIPELINE_AMEND_INTENT, EVENT_PIPELINE_AMEND_OPEN,
};
use std::path::Path;

// ---------------------------------------------------------------------------
// Default drift threshold — may be overridden by mustard.json
// ---------------------------------------------------------------------------

/// Default number of out-of-scope edits before a drift warning fires.
const DEFAULT_DRIFT_THRESHOLD: u32 = 3;

/// Read `amend.drift_threshold` from `{project_dir}/.claude/mustard.json`,
/// falling back to [`DEFAULT_DRIFT_THRESHOLD`] on any error.
fn drift_threshold(project_dir: &str) -> u32 {
    let path = Path::new(project_dir)
        .join(".claude")
        .join("mustard.json");
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
// Shared helpers
// ---------------------------------------------------------------------------

/// Resolve the project directory from context, falling back to `.`.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

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
/// In-scope means: the path is in `pipeline_file_set` OR it starts with any
/// of the `subprojects` prefixes (using forward-slash normalization).
fn is_in_scope(file_path: &str, window: &AmendWindow) -> bool {
    let normalized = file_path.replace('\\', "/");
    if window
        .pipeline_file_set
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

/// Emit a harness event best-effort; failures are silently dropped.
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
    let _ = SqliteEventStore::for_project(project_dir)
        .and_then(|store| store.append(&ev));
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
        let pdir = project_dir(input, ctx);
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
                                let _ = SqliteEventStore::for_project(&pdir)
                                    .and_then(|s| s.mark_amend_build_verde(&session_id, &now));
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
    let Ok(store) = SqliteEventStore::for_project(project_dir) else {
        return;
    };
    let Ok(file_set) = store.amend_window_pipeline_file_set(spec_id) else {
        return;
    };
    let subprojects = derive_subprojects(&file_set);
    let now = now_iso8601();
    let payload = PipelineAmendOpenPayload {
        spec_id: spec_id.to_string(),
        session_id: session_id.to_string(),
        closed_at: now,
        pipeline_file_set: file_set,
        subprojects,
    };
    let _ = store.open_amend_window(&payload);
    let event_payload = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
    emit(project_dir, session_id, EVENT_PIPELINE_AMEND_OPEN, event_payload);
}

/// Handle PostToolUse(Write|Edit) — record activity or accumulate drift.
fn observe_write_edit(project_dir: &str, session_id: &str, tool: &str, file_path: &str) {
    let Ok(store) = SqliteEventStore::for_project(project_dir) else {
        return;
    };
    let Ok(Some(window)) = store.amend_window_for_session(session_id) else {
        return;
    };

    if is_in_scope(file_path, &window) {
        let now = now_iso8601();
        let _ = store.record_amend_activity(&window.spec_id, session_id, &now);
        let act_payload = serde_json::to_value(PipelineAmendActivityPayload {
            spec_id: window.spec_id.clone(),
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
        let threshold = drift_threshold(project_dir);
        let Ok(new_len) = store.add_amend_drift_path(&window.spec_id, session_id, file_path) else {
            return;
        };
        if u32::try_from(new_len).unwrap_or(u32::MAX) >= threshold {
            let _ = store.mark_amend_drift_emitted(&window.spec_id, session_id);
            // Re-read the updated drift paths for the event payload.
            let unrelated_paths = match store.amend_window_for_session(session_id) {
                Ok(Some(w)) => w.drift_unrelated_paths,
                _ => vec![file_path.to_string()],
            };
            let drift_payload = serde_json::to_value(PipelineAmendDriftPayload {
                spec_id: window.spec_id.clone(),
                session_id: session_id.to_string(),
                unrelated_paths,
                threshold: Some(threshold),
            })
            .unwrap_or(serde_json::Value::Null);
            emit(project_dir, session_id, EVENT_PIPELINE_AMEND_DRIFT, drift_payload);
        }
    }
}

/// Handle `UserPromptSubmit` — emit intent event when a window is open.
fn observe_user_prompt(input: &HookInput, _ctx: &Ctx, project_dir: &str, session_id: &str) {
    let Ok(store) = SqliteEventStore::for_project(project_dir) else {
        return;
    };
    let Ok(Some(window)) = store.amend_window_for_session(session_id) else {
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
        spec_id: window.spec_id.clone(),
        session_id: session_id.to_string(),
        prompt_text,
        at: Some(now),
    })
    .unwrap_or(serde_json::Value::Null);
    emit(project_dir, session_id, EVENT_PIPELINE_AMEND_INTENT, intent_payload);
}

// ---------------------------------------------------------------------------
// Check — look-ahead drift injection, no database writes
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
        let session_id = match input.session_id.as_deref() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return Ok(Verdict::Allow),
        };
        let pdir = project_dir(input, ctx);
        let Ok(store) = SqliteEventStore::for_project(&pdir) else {
            return Ok(Verdict::Allow);
        };
        let Ok(Some(window)) = store.amend_window_for_session(&session_id) else {
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
        let current_len = window.drift_unrelated_paths.len();
        let already_counted = window
            .drift_unrelated_paths
            .iter()
            .any(|p| p.replace('\\', "/") == file_path.replace('\\', "/"));
        let forecast_len = if already_counted {
            current_len
        } else {
            current_len + 1
        };
        let threshold = drift_threshold(&pdir);
        if u32::try_from(forecast_len).unwrap_or(u32::MAX) < threshold {
            return Ok(Verdict::Allow);
        }

        // Derive lang from the spec's pipeline.scope event (best-effort).
        let lang = derive_spec_lang(&store, &window.spec_id).unwrap_or_else(|| "en".to_string());
        let n = forecast_len;
        let warning = if lang == "pt" {
            format!(
                "Você está editando `{file_path}` em outro escopo da spec ativa \
                 `{spec_id}` (pós-CLOSE). Já são {n} arquivos fora do escopo declarado. \
                 Considere abrir `/mustard:feature` ou `/mustard:task` separado — a sessão \
                 continua, mas o drift não é absorvido pela spec original.",
                spec_id = window.spec_id,
            )
        } else {
            format!(
                "You're editing `{file_path}` outside the active spec `{spec_id}` scope \
                 (post-CLOSE). {n} files outside declared scope so far. Consider opening a \
                 separate `/mustard:feature` or `/mustard:task` — the session continues, but \
                 drift is not absorbed by the original spec.",
                spec_id = window.spec_id,
            )
        };
        Ok(Verdict::Inject { context: warning })
    }
}

/// Retrieve the `lang` field from the most recent `pipeline.scope` event for
/// `spec_id`. Returns `None` if the store cannot be queried or the field is absent.
fn derive_spec_lang(store: &SqliteEventStore, spec_id: &str) -> Option<String> {
    use mustard_core::model::event::{PipelineScopePayload, EVENT_PIPELINE_SCOPE};
    let events = store.query(Some(spec_id)).ok()?;
    events
        .into_iter()
        .rfind(|e| e.event == EVENT_PIPELINE_SCOPE)
        .and_then(|e| serde_json::from_value::<PipelineScopePayload>(e.payload).ok())
        .and_then(|p| p.lang)
}

// ---------------------------------------------------------------------------
// Tests (AC-3 … AC-10)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use mustard_core::model::event::{
        PipelineAmendOpenPayload, EVENT_PIPELINE_AMEND_ACTIVITY, EVENT_PIPELINE_AMEND_DRIFT,
        EVENT_PIPELINE_AMEND_INTENT,
    };
    use serde_json::json;
    use tempfile::tempdir;

    // ---- helpers -------------------------------------------------------

    fn make_project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude").join(".harness")).unwrap();
        dir
    }

    fn store_for(dir: &std::path::Path) -> SqliteEventStore {
        SqliteEventStore::for_project(dir).unwrap()
    }

    fn seed_window(
        store: &SqliteEventStore,
        spec_id: &str,
        session_id: &str,
        pipeline_file_set: Vec<String>,
        subprojects: Vec<String>,
    ) {
        let payload = PipelineAmendOpenPayload {
            spec_id: spec_id.to_string(),
            session_id: session_id.to_string(),
            closed_at: "2026-05-20T00:00:00.000Z".to_string(),
            pipeline_file_set,
            subprojects,
        };
        store.open_amend_window(&payload).unwrap();
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
        }
    }

    fn events_of_kind(store: &SqliteEventStore, kind: &str) -> Vec<HarnessEvent> {
        store
            .replay()
            .unwrap()
            .into_iter()
            .filter(|e| e.event == kind)
            .collect()
    }

    // ---- AC-3: in-scope Write → AMEND_ACTIVITY -------------------------

    #[test]
    fn amend_capture_activity() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let store = store_for(project.path());
        let session_id = "session-ac3";
        let in_scope_file = "apps/rt/src/hooks/mod.rs";

        seed_window(
            &store,
            "spec-ac3",
            session_id,
            vec![in_scope_file.to_string()],
            vec!["apps/rt/".to_string()],
        );

        let input = post_write_input(session_id, cwd, in_scope_file);
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        AmendCapture.observe(&input, &ctx);

        let events = events_of_kind(&store, EVENT_PIPELINE_AMEND_ACTIVITY);
        assert_eq!(events.len(), 1, "expected one amend_activity event");
        assert_eq!(events[0].payload["file_path"], json!(in_scope_file));
        assert_eq!(events[0].payload["spec_id"], json!("spec-ac3"));
    }

    // ---- AC-4: build/test success → build_verde_at stamped -------------

    #[test]
    fn amend_capture_build_verde() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let store = store_for(project.path());
        let session_id = "session-ac4";

        seed_window(
            &store,
            "spec-ac4",
            session_id,
            vec!["apps/rt/src/lib.rs".to_string()],
            vec!["apps/rt/".to_string()],
        );

        let input = post_bash_input(session_id, cwd, "cargo test", 0);
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        AmendCapture.observe(&input, &ctx);

        let window = store
            .amend_window_for_session(session_id)
            .unwrap()
            .unwrap();
        assert!(window.build_verde_at.is_some());
    }

    // ---- AC-5: UserPromptSubmit → AMEND_INTENT with prompt_text --------

    #[test]
    fn amend_capture_intent() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let store = store_for(project.path());
        let session_id = "session-ac5";
        let prompt = "ajuste o follow-up";

        seed_window(
            &store,
            "spec-ac5",
            session_id,
            vec!["apps/cli/src/main.rs".to_string()],
            vec!["apps/cli/".to_string()],
        );

        let input = prompt_input(session_id, cwd, prompt);
        let ctx = post_ctx(Trigger::UserPromptSubmit, cwd);
        AmendCapture.observe(&input, &ctx);

        let events = events_of_kind(&store, EVENT_PIPELINE_AMEND_INTENT);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["prompt_text"], json!(prompt));
        assert_eq!(events[0].payload["session_id"], json!(session_id));
    }

    // ---- AC-6: session isolation ----------------------------------------

    #[test]
    fn amend_capture_session_isolation() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let store = store_for(project.path());
        let session_a = "session-ac6-a";
        let session_b = "session-ac6-b";
        let in_scope_file = "apps/rt/src/main.rs";

        // Only session A has a window.
        seed_window(
            &store,
            "spec-ac6",
            session_a,
            vec![in_scope_file.to_string()],
            vec!["apps/rt/".to_string()],
        );

        // PostToolUse from session B — should be a no-op.
        let input = post_write_input(session_b, cwd, in_scope_file);
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        AmendCapture.observe(&input, &ctx);

        // No events emitted.
        let all_events = store.replay().unwrap();
        assert!(all_events.is_empty());

        // Session A's window unchanged.
        let window = store.amend_window_for_session(session_a).unwrap().unwrap();
        assert!(window.last_activity_at.is_none());
    }

    // ---- AC-8: 1 drift file → no AMEND_DRIFT ---------------------------

    #[test]
    fn amend_drift_under_threshold() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let store = store_for(project.path());
        let session_id = "session-ac8";

        seed_window(
            &store,
            "spec-ac8",
            session_id,
            vec!["apps/rt/src/main.rs".to_string()],
            vec!["apps/rt/".to_string()],
        );

        let input = post_write_input(session_id, cwd, "docs/unrelated.md");
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        AmendCapture.observe(&input, &ctx);

        let window = store.amend_window_for_session(session_id).unwrap().unwrap();
        assert_eq!(window.drift_unrelated_paths.len(), 1);
        assert!(!window.drift_emitted);
        assert!(events_of_kind(&store, EVENT_PIPELINE_AMEND_DRIFT).is_empty());
    }

    // ---- AC-9: 3 drift files → AMEND_DRIFT + Check injects warning -----

    #[test]
    fn amend_drift_triggers_warning() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let session_id = "session-ac9";

        // Write mustard.json with threshold=3.
        let mustard_json = project.path().join(".claude").join("mustard.json");
        std::fs::write(&mustard_json, r#"{"amend":{"drift_threshold":3}}"#).unwrap();

        let store = store_for(project.path());
        seed_window(
            &store,
            "spec-ac9",
            session_id,
            vec!["apps/rt/src/main.rs".to_string()],
            vec!["apps/rt/".to_string()],
        );

        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        for i in 1..=3u32 {
            let f = format!("docs/file{i}.md");
            let inp = post_write_input(session_id, cwd, &f);
            AmendCapture.observe(&inp, &ctx);
        }

        let drift_events = events_of_kind(&store, EVENT_PIPELINE_AMEND_DRIFT);
        assert_eq!(drift_events.len(), 1, "drift event must fire exactly once");
        assert_eq!(drift_events[0].payload["threshold"], json!(3u32));

        // Check side: with 2 pre-existing drift paths, 3rd triggers Inject.
        let session_chk = "session-ac9-chk";
        seed_window(
            &store,
            "spec-ac9-chk",
            session_chk,
            vec!["apps/rt/src/main.rs".to_string()],
            vec!["apps/rt/".to_string()],
        );
        store.add_amend_drift_path("spec-ac9-chk", session_chk, "docs/file1.md").unwrap();
        store.add_amend_drift_path("spec-ac9-chk", session_chk, "docs/file2.md").unwrap();

        let pre_ctx = Ctx {
            project_dir: cwd.to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        let pre_in = pre_write_input(session_chk, cwd, "docs/file3.md");
        let verdict = AmendCapture.evaluate(&pre_in, &pre_ctx).unwrap();
        assert!(
            matches!(verdict, Verdict::Inject { .. }),
            "expected Inject, got {verdict:?}"
        );
    }

    // ---- AC-10: file within declared subproject → no drift --------------

    #[test]
    fn amend_drift_same_subproject_ok() {
        let project = make_project();
        let cwd = project.path().to_str().unwrap();
        let store = store_for(project.path());
        let session_id = "session-ac10";

        seed_window(
            &store,
            "spec-ac10",
            session_id,
            vec!["apps/rt/src/main.rs".to_string()],
            vec!["apps/rt/".to_string()],
        );

        // File NOT in pipeline_file_set but within "apps/rt/" subproject.
        let ctx = post_ctx(Trigger::PostToolUse, cwd);
        let input = post_write_input(session_id, cwd, "apps/rt/src/hooks/new_hook.rs");
        AmendCapture.observe(&input, &ctx);

        let window = store.amend_window_for_session(session_id).unwrap().unwrap();
        assert_eq!(window.drift_unrelated_paths.len(), 0, "subproject file must not drift");
        assert!(events_of_kind(&store, EVENT_PIPELINE_AMEND_DRIFT).is_empty());
        // It was treated as activity.
        assert!(window.last_activity_at.is_some());
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
    let Ok(store) = SqliteEventStore::for_project(project_dir) else {
        return;
    };
    let Ok(Some(window)) = store.amend_window_for_session(session_id) else {
        return;
    };
    let status = if window.last_activity_at.is_some() {
        "resolved"
    } else {
        "pending"
    };
    let _ = store.close_amend_window(&window.spec_id, session_id, status);
    let now = now_iso8601();
    let close_payload = serde_json::to_value(PipelineAmendClosePayload {
        spec_id: window.spec_id.clone(),
        session_id: session_id.to_string(),
        status: status.to_string(),
        closed_at: Some(now),
        build_verde: Some(window.build_verde_at.is_some()),
        drift_emitted: Some(window.drift_emitted),
    })
    .unwrap_or(serde_json::Value::Null);
    emit(project_dir, session_id, EVENT_PIPELINE_AMEND_CLOSE, close_payload);
}
