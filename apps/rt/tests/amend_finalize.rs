// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args
)]

//! Integration tests for `amend-finalize` (AC-11 … AC-14).
//!
//! W8A-3 (no-sqlite Wave 8): the seed path migrated from
//! `SqliteEventStore::open_amend_window` + `record_amend_activity` etc. to
//! direct filesystem writes:
//!
//! - the amend window state lands in `.claude/spec/{id}/.amend-window.json`
//!   (W3C atomic write — same schema the production `amend_capture` hook
//!   produces);
//! - the `pipeline.scope` / `pipeline.amend_activity` / `pipeline.amend_intent`
//!   events the finalize reader resolves are seeded as NDJSON lines under
//!   `.claude/spec/{id}/.events/<session>.ndjson`.
//!
//! The subprocess invocation and JSON output assertions stay byte-identical.

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

/// Write `<root>/.claude/spec/<spec_id>/.amend-window.json` matching the
/// shape that `amend_finalize::WindowState` deserialises.
fn write_amend_window(
    root: &Path,
    spec_id: &str,
    session_id: &str,
    build_verde_at: Option<&str>,
    last_activity_at: Option<&str>,
    drift_emitted: bool,
) {
    let spec_dir = root.join(".claude").join("spec").join(spec_id);
    std::fs::create_dir_all(&spec_dir).unwrap();
    let window = json!({
        "opened_at": "2026-05-20T00:00:00.000Z",
        "expires_at": "2026-05-20T01:00:00.000Z",
        "files": ["apps/rt/src/lib.rs"],
        "subprojects": ["apps/rt/"],
        "drift": [],
        "drift_emitted": drift_emitted,
        "last_activity_at": last_activity_at,
        "build_verde_at": build_verde_at,
        "closed": false,
        "session_id": session_id,
    });
    std::fs::write(
        spec_dir.join(".amend-window.json"),
        serde_json::to_string_pretty(&window).unwrap(),
    )
    .unwrap();
}

/// Append one NDJSON event to `<root>/.claude/spec/<spec_id>/.events/<session>.ndjson`.
/// Mirrors the shape `event_writer_ndjson` produces (raw flatten with `event`
/// + `kind` + `ts` + `spec` + `session_id` + `payload`).
fn append_event(
    root: &Path,
    spec_id: &str,
    session_id: &str,
    event_name: &str,
    kind: &str,
    ts: &str,
    payload: Value,
) {
    let events_dir = root
        .join(".claude")
        .join("spec")
        .join(spec_id)
        .join(".events");
    std::fs::create_dir_all(&events_dir).unwrap();
    let line = json!({
        "event": event_name,
        "kind": kind,
        "ts": ts,
        "v": 1,
        "spec": spec_id,
        "session_id": session_id,
        "wave": 0,
        "actor": "test",
        "payload": payload,
    });
    let mut content = String::new();
    let path = events_dir.join(format!("{session_id}.ndjson"));
    if path.exists() {
        content = std::fs::read_to_string(&path).unwrap();
    }
    content.push_str(&line.to_string());
    content.push('\n');
    std::fs::write(&path, content).unwrap();
}

fn seed_scope_event(root: &Path, spec_id: &str, session_id: &str, lang: Option<&str>) {
    let payload = json!({
        "scope": "full",
        "lang": lang,
        "model": null,
        "isWavePlan": null,
        "totalWaves": null,
    });
    append_event(
        root,
        spec_id,
        session_id,
        "pipeline.scope",
        "pipeline",
        "2026-05-20T00:00:00.000Z",
        payload,
    );
}

fn seed_intent_event(root: &Path, spec_id: &str, session_id: &str, prompt: &str) {
    let payload = json!({
        "spec_id": spec_id,
        "session_id": session_id,
        "prompt_text": prompt,
        "at": "2026-05-20T00:02:00.000Z",
    });
    append_event(
        root,
        spec_id,
        session_id,
        "pipeline.amend_intent",
        "pipeline",
        "2026-05-20T00:02:00.000Z",
        payload,
    );
}

fn create_spec_md(project_root: &Path, spec_id: &str) {
    // Wave-2 flat layout: specs live at .claude/spec/{spec_id}/ for their
    // entire lifetime; no active/ or archived/ buckets.
    let spec_dir = project_root.join(".claude").join("spec").join(spec_id);
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(
        spec_dir.join("spec.md"),
        format!("# Spec {spec_id}\n\nSome content.\n"),
    )
    .unwrap();
}

/// Returns the spec.md content from the flat layout path.
fn read_spec_md(project_root: &Path, spec_id: &str) -> String {
    let path = project_root
        .join(".claude")
        .join("spec")
        .join(spec_id)
        .join("spec.md");
    std::fs::read_to_string(&path).unwrap()
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
//         spec.md contains PT block
// ---------------------------------------------------------------------------

#[test]
fn amend_session_end_archived() {
    let project = make_project();
    let root = project.path();
    let session_id = "session-ac11";
    let spec_id = "spec-ac11";

    seed_scope_event(root, spec_id, session_id, Some("pt"));
    write_amend_window(
        root,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        false,
    );
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");

    let windows = result["windows"].as_array().expect("windows array");
    assert_eq!(windows.len(), 1, "expected 1 window in report");
    assert_eq!(windows[0]["status"], json!("archived"), "status mismatch: {result}");
    assert!(windows[0]["error"].is_null(), "unexpected error: {}", windows[0]["error"]);

    // spec.md must contain the PT-language ## Amendments block.
    let content = read_spec_md(root, spec_id);
    assert!(content.contains("## Amendments"), "## Amendments missing from spec.md:\n{content}");
    // PT lang: the block contains "build verde" (not "build green").
    assert!(
        content.contains("build verde") || content.contains("resolução"),
        "expected PT markers in spec.md:\n{content}"
    );

    // Flat layout: spec dir stays at .claude/spec/{spec_id}/ — no move.
    let flat_dir = root.join(".claude").join("spec").join(spec_id);
    assert!(flat_dir.exists(), "spec dir must remain at flat path .claude/spec/{spec_id}/");

    // AC-13 contract: either `.amend-window.json` is removed OR a
    // `pipeline.amend_close` event lands in the per-spec NDJSON sink.
    // W8A-3 keeps the window file (closed=true) for audit, so we assert the
    // event-side branch of the OR — the close event is the durable signal.
    let window_path = flat_dir.join(".amend-window.json");
    let close_event_present = scan_close_event(root, spec_id, session_id);
    assert!(
        !window_path.exists() || close_event_present,
        "AC-13: either `.amend-window.json` must be removed OR pipeline.amend_close event must be present in NDJSON"
    );
    assert!(
        close_event_present,
        "pipeline.amend_close event missing from .claude/spec/{spec_id}/.events/*.ndjson"
    );
}

/// Scan `<root>/.claude/spec/<spec>/.events/*.ndjson` for a `pipeline.amend_close`
/// event matching `session_id`. Returns `true` on first match.
fn scan_close_event(root: &Path, spec_id: &str, session_id: &str) -> bool {
    let events_dir = root.join(".claude").join("spec").join(spec_id).join(".events");
    let Ok(entries) = std::fs::read_dir(&events_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ndjson") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else { continue };
        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(value): Result<Value, _> = serde_json::from_str(line) else { continue };
            if value.get("event").and_then(Value::as_str) == Some("pipeline.amend_close")
                && value.get("session_id").and_then(Value::as_str) == Some(session_id)
            {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// AC-12: closed-amend-pending — activity but no build_verde_at
// ---------------------------------------------------------------------------

#[test]
fn amend_session_end_pending() {
    let project = make_project();
    let root = project.path();
    let session_id = "session-ac12";
    let spec_id = "spec-ac12";

    write_amend_window(root, spec_id, session_id, None, Some("2026-05-20T00:01:00.000Z"), false);
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");

    let windows = result["windows"].as_array().expect("windows array");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["status"], json!("closed-amend-pending"), "{result}");
    assert!(windows[0]["error"].is_null(), "unexpected error: {}", windows[0]["error"]);

    let flat_dir = root.join(".claude").join("spec").join(spec_id);
    assert!(flat_dir.exists(), "spec dir must remain at flat path .claude/spec/{spec_id}/ for pending");
}

// ---------------------------------------------------------------------------
// AC-13: closed-amend-drift — drift_emitted=true wins regardless of build verde
// ---------------------------------------------------------------------------

#[test]
fn amend_session_end_drift() {
    let project = make_project();
    let root = project.path();
    let session_id = "session-ac13";
    let spec_id = "spec-ac13";

    write_amend_window(
        root,
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

    let flat_dir = root.join(".claude").join("spec").join(spec_id);
    assert!(flat_dir.exists(), "spec dir must remain at flat path .claude/spec/{spec_id}/ for drift");
}

// ---------------------------------------------------------------------------
// AC-14a: lang="pt" → PT block (prompt do usuário)
// ---------------------------------------------------------------------------

#[test]
fn amend_writer_lang_pt() {
    let project = make_project();
    let root = project.path();
    let session_id = "session-ac14-a";
    let spec_id = "spec-ac14-a";

    seed_scope_event(root, spec_id, session_id, Some("pt"));
    write_amend_window(
        root,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        false,
    );
    seed_intent_event(root, spec_id, session_id, "ajustar o módulo");
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");
    assert_eq!(result["windows"][0]["status"], json!("archived"));

    let content = read_spec_md(root, spec_id);
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
    let session_id = "session-ac14-b";
    let spec_id = "spec-ac14-b";

    // scope event without lang field.
    seed_scope_event(root, spec_id, session_id, None);
    write_amend_window(
        root,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        false,
    );
    seed_intent_event(root, spec_id, session_id, "fix the module");
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");
    assert_eq!(result["windows"][0]["status"], json!("archived"));

    let content = read_spec_md(root, spec_id);
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
    let session_id = "session-ac14-c";
    let spec_id = "spec-ac14-c";

    seed_scope_event(root, spec_id, session_id, Some("en"));
    write_amend_window(
        root,
        spec_id,
        session_id,
        Some("2026-05-20T00:02:00.000Z"),
        Some("2026-05-20T00:01:00.000Z"),
        false,
    );
    seed_intent_event(root, spec_id, session_id, "extend the feature");
    create_spec_md(root, spec_id);

    let result = run_finalize(root, session_id);
    assert!(result.get("parse_error").is_none(), "JSON parse failed: {result}");
    assert_eq!(result["windows"][0]["status"], json!("archived"));

    let content = read_spec_md(root, spec_id);
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
