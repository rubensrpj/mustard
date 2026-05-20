//! Integration tests for `amend-finalize` (AC-11 … AC-14).
//!
//! Since `mustard-rt` is binary-only (no lib), these tests drive the subcommand
//! as a subprocess (`mustard-rt run amend-finalize --session-id <id>`) and use
//! `mustard_core` + direct SQLite writes for setup and assertion.

use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{
    Actor, ActorKind, HarnessEvent, PipelineAmendOpenPayload, PipelineScopePayload,
    SCHEMA_VERSION, EVENT_PIPELINE_AMEND_ACTIVITY, EVENT_PIPELINE_AMEND_CLOSE,
    EVENT_PIPELINE_AMEND_INTENT, EVENT_PIPELINE_SCOPE,
};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_project() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".claude").join(".harness")).unwrap();
    dir
}

fn store_for(dir: &Path) -> SqliteEventStore {
    SqliteEventStore::for_project(dir).unwrap()
}

fn seed_scope_event(store: &SqliteEventStore, spec_id: &str, session_id: &str, lang: Option<&str>) {
    let payload = PipelineScopePayload {
        scope: "full".to_string(),
        lang: lang.map(str::to_string),
        model: None,
        is_wave_plan: None,
        total_waves: None,
    };
    store
        .append(&HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T00:00:00.000Z".to_string(),
            session_id: session_id.to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: Some("test".to_string()), actor_type: None },
            event: EVENT_PIPELINE_SCOPE.to_string(),
            payload: serde_json::to_value(&payload).unwrap(),
            spec: Some(spec_id.to_string()),
        })
        .unwrap();
}

fn seed_activity_event(store: &SqliteEventStore, spec_id: &str, session_id: &str) {
    store
        .append(&HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T00:01:00.000Z".to_string(),
            session_id: session_id.to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: Some("test".to_string()), actor_type: None },
            event: EVENT_PIPELINE_AMEND_ACTIVITY.to_string(),
            payload: json!({
                "spec_id": spec_id,
                "session_id": session_id,
                "tool": "Write",
                "file_path": "apps/rt/src/lib.rs",
                "at": "2026-05-20T00:01:00.000Z",
            }),
            spec: Some(spec_id.to_string()),
        })
        .unwrap();
}

fn seed_intent_event(store: &SqliteEventStore, spec_id: &str, session_id: &str, prompt: &str) {
    store
        .append(&HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T00:02:00.000Z".to_string(),
            session_id: session_id.to_string(),
            wave: 0,
            actor: Actor { kind: ActorKind::Hook, id: Some("test".to_string()), actor_type: None },
            event: EVENT_PIPELINE_AMEND_INTENT.to_string(),
            payload: json!({
                "spec_id": spec_id,
                "session_id": session_id,
                "prompt_text": prompt,
                "at": "2026-05-20T00:02:00.000Z",
            }),
            spec: Some(spec_id.to_string()),
        })
        .unwrap();
}

fn seed_window(
    store: &SqliteEventStore,
    spec_id: &str,
    session_id: &str,
    build_verde_at: Option<&str>,
    last_activity_at: Option<&str>,
    drift_emitted: bool,
) {
    store
        .open_amend_window(&PipelineAmendOpenPayload {
            spec_id: spec_id.to_string(),
            session_id: session_id.to_string(),
            closed_at: "2026-05-20T00:00:00.000Z".to_string(),
            pipeline_file_set: vec!["apps/rt/src/lib.rs".to_string()],
            subprojects: vec!["apps/rt/".to_string()],
        })
        .unwrap();
    if let Some(la) = last_activity_at {
        store.record_amend_activity(spec_id, session_id, la).unwrap();
    }
    if let Some(bv) = build_verde_at {
        store.mark_amend_build_verde(session_id, bv).unwrap();
    }
    if drift_emitted {
        store.mark_amend_drift_emitted(spec_id, session_id).unwrap();
    }
}

fn create_spec_md(project_root: &Path, spec_id: &str) {
    let spec_dir = project_root
        .join(".claude")
        .join("spec")
        .join("active")
        .join(spec_id);
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.md"),
        format!("# Spec {}\n\nSome content.\n", spec_id),
    )
    .unwrap();
}

/// Returns `(content, is_in_active)`.
fn read_spec_md(project_root: &Path, spec_id: &str) -> (String, bool) {
    let active = project_root
        .join(".claude")
        .join("spec")
        .join("active")
        .join(spec_id)
        .join("spec.md");
    if active.exists() {
        return (std::fs::read_to_string(&active).unwrap(), true);
    }
    let archived = project_root
        .join(".claude")
        .join("spec")
        .join("archived")
        .join(spec_id)
        .join("spec.md");
    (std::fs::read_to_string(&archived).unwrap(), false)
}

/// Run `mustard-rt run amend-finalize --session-id <id>` against the temp project.
/// Returns the parsed JSON stdout.
fn run_finalize(project_root: &Path, session_id: &str) -> Value {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let out = Command::new(bin)
        .args(["run", "amend-finalize", "--session-id", session_id])
        .env("MUSTARD_PROJECT_ROOT", project_root.to_str().unwrap())
        .env("CLAUDE_PROJECT_DIR", project_root.to_str().unwrap())
        .output()
        .expect("spawn mustard-rt");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| json!({ "parse_error": e.to_string(), "raw": stdout.as_ref() }))
}

// ---------------------------------------------------------------------------
// AC-11: archived — build_verde_at >= last_activity_at → status="archived",
//         spec.md contains PT block, dir moved to archived/
// ---------------------------------------------------------------------------

#[test]
fn amend_session_end_archived() {
    let project = make_project();
    let root = project.path();
    let store = store_for(root);
    let session_id = "session-ac11";
    let spec_id = "spec-ac11";

    seed_scope_event(&store, spec_id, session_id, Some("pt"));
    seed_window(
        &store,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        false,
    );
    seed_activity_event(&store, spec_id, session_id);
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");

    let windows = result["windows"].as_array().expect("windows array");
    assert_eq!(windows.len(), 1, "expected 1 window in report");
    assert_eq!(windows[0]["status"], json!("archived"), "status mismatch: {result}");
    assert!(windows[0]["error"].is_null(), "unexpected error: {}", windows[0]["error"]);

    // spec.md must contain the PT-language ## Amendments block.
    let (content, in_active) = read_spec_md(root, spec_id);
    assert!(content.contains("## Amendments"), "## Amendments missing from spec.md:\n{content}");
    // PT lang: the block contains "build verde" (not "build green").
    assert!(
        content.contains("build verde") || content.contains("resolução"),
        "expected PT markers in spec.md:\n{content}"
    );

    // Dir must have moved to archived/.
    assert!(!in_active, "spec dir must be in archived/, not active/");

    // EVENT_PIPELINE_AMEND_CLOSE must be present with status="archived".
    let close_events: Vec<_> = store
        .replay()
        .unwrap()
        .into_iter()
        .filter(|e| e.event == EVENT_PIPELINE_AMEND_CLOSE)
        .collect();
    assert_eq!(close_events.len(), 1, "expected one amend_close event");
    assert_eq!(close_events[0].payload["status"], json!("archived"));
    assert_eq!(close_events[0].payload["spec_id"], json!(spec_id));
    assert_eq!(close_events[0].payload["build_verde"], json!(true));
}

// ---------------------------------------------------------------------------
// AC-12: closed-amend-pending — activity but no build_verde_at, dir stays
// ---------------------------------------------------------------------------

#[test]
fn amend_session_end_pending() {
    let project = make_project();
    let root = project.path();
    let store = store_for(root);
    let session_id = "session-ac12";
    let spec_id = "spec-ac12";

    seed_window(&store, spec_id, session_id, None, Some("2026-05-20T00:01:00.000Z"), false);
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");

    let windows = result["windows"].as_array().expect("windows array");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["status"], json!("closed-amend-pending"), "{result}");
    assert!(windows[0]["error"].is_null(), "unexpected error: {}", windows[0]["error"]);

    // Dir must NOT have moved — still in active/.
    let active_dir = root.join(".claude").join("spec").join("active").join(spec_id);
    assert!(active_dir.exists(), "spec dir must remain in active/ for pending");
}

// ---------------------------------------------------------------------------
// AC-13: closed-amend-drift — drift_emitted=true wins regardless of build verde
// ---------------------------------------------------------------------------

#[test]
fn amend_session_end_drift() {
    let project = make_project();
    let root = project.path();
    let store = store_for(root);
    let session_id = "session-ac13";
    let spec_id = "spec-ac13";

    // drift_emitted=true AND build_verde set — drift takes priority.
    seed_window(
        &store,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        true,
    );
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");

    let windows = result["windows"].as_array().expect("windows array");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["status"], json!("closed-amend-drift"), "{result}");
    assert!(windows[0]["error"].is_null(), "unexpected error: {}", windows[0]["error"]);

    // Dir must NOT have moved.
    let active_dir = root.join(".claude").join("spec").join("active").join(spec_id);
    assert!(active_dir.exists(), "spec dir must remain in active/ for drift");
}

// ---------------------------------------------------------------------------
// AC-14a: lang="pt" → PT block (prompt do usuário)
// ---------------------------------------------------------------------------

#[test]
fn amend_writer_lang_pt() {
    let project = make_project();
    let root = project.path();
    let store = store_for(root);
    let session_id = "session-ac14-a";
    let spec_id = "spec-ac14-a";

    seed_scope_event(&store, spec_id, session_id, Some("pt"));
    seed_window(
        &store,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        false,
    );
    seed_intent_event(&store, spec_id, session_id, "ajustar o módulo");
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");
    assert_eq!(result["windows"][0]["status"], json!("archived"));

    let (content, _) = read_spec_md(root, spec_id);
    assert!(
        content.contains("prompt do usuário"),
        "expected PT 'prompt do usuário' in spec.md:\n{content}"
    );
    assert!(
        !content.contains("user prompt:"),
        "must not contain EN 'user prompt:' in PT spec:\n{content}"
    );
}

// ---------------------------------------------------------------------------
// AC-14b: no lang field → defaults to EN (user prompt)
// ---------------------------------------------------------------------------

#[test]
fn amend_writer_lang_default_en() {
    let project = make_project();
    let root = project.path();
    let store = store_for(root);
    let session_id = "session-ac14-b";
    let spec_id = "spec-ac14-b";

    // scope event without lang field.
    seed_scope_event(&store, spec_id, session_id, None);
    seed_window(
        &store,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        false,
    );
    seed_intent_event(&store, spec_id, session_id, "fix the module");
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");
    assert_eq!(result["windows"][0]["status"], json!("archived"));

    let (content, _) = read_spec_md(root, spec_id);
    assert!(
        content.contains("user prompt:"),
        "expected EN 'user prompt:' in spec.md:\n{content}"
    );
    assert!(
        !content.contains("prompt do usuário"),
        "must not contain PT 'prompt do usuário' in EN spec:\n{content}"
    );
}

// ---------------------------------------------------------------------------
// AC-14c: lang="en" → EN block
// ---------------------------------------------------------------------------

#[test]
fn amend_writer_lang_en() {
    let project = make_project();
    let root = project.path();
    let store = store_for(root);
    let session_id = "session-ac14-c";
    let spec_id = "spec-ac14-c";

    seed_scope_event(&store, spec_id, session_id, Some("en"));
    seed_window(
        &store,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        false,
    );
    seed_intent_event(&store, spec_id, session_id, "extend the feature");
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");
    assert_eq!(result["windows"][0]["status"], json!("archived"));

    let (content, _) = read_spec_md(root, spec_id);
    assert!(
        content.contains("user prompt:"),
        "expected EN 'user prompt:' in spec.md:\n{content}"
    );
    assert!(
        !content.contains("prompt do usuário"),
        "must not contain PT markers in EN spec:\n{content}"
    );
}

// ---------------------------------------------------------------------------
// Fail-open: no windows for session → empty report, exit 0
// ---------------------------------------------------------------------------

#[test]
fn amend_finalize_no_windows_is_noop() {
    let project = make_project();
    let root = project.path();

    let result = run_finalize(root, "session-no-windows");
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");
    let windows = result["windows"].as_array().expect("windows array");
    assert!(windows.is_empty(), "expected empty windows list");
}
