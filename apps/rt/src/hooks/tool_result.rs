//! `tool_result` — PostToolUse `Observer` that captures the rich tool output
//! (stdout, stderr, file diffs, content excerpts) into a `tool.result` event.
//!
//! ## Why this exists (followup-2 § Trace rico)
//!
//! The existing `metrics-tracker` (in [`crate::hooks::tracker`]) emits a
//! `tool.use` heartbeat with the PreToolUse *intent* — command string, file
//! path, description. The dashboard `<ExecutionTrace>` rendered this raw JSON
//! because the *result* side (Bash stdout, Edit diff, Read content) never
//! reached the harness `events` table.
//!
//! This module is the missing PostToolUse half: per supported tool it extracts
//! the salient slice of `tool_response`, truncates it to the configured cap,
//! and appends a `tool.result` event the dashboard joins with the matching
//! `tool.use` (by `tool_use_id` when forwarded by Claude Code, else by
//! chronological order).
//!
//! ## Scope and fail-open
//!
//! Pure `Observer` — never returns a verdict. Any extraction or DB-write
//! failure is swallowed (the user's tool call must complete regardless).
//! Truncation caps: 2 KB stdout, 1 KB stderr, 4 KB Read content; Edit
//! before/after pieces are bounded to 4 KB each so a multi-megabyte file
//! replacement does not bloat the event row.

use crate::run::current_spec;
use crate::util::now_iso8601;
use mustard_core::model::contract::{Ctx, HookInput, Observer, Trigger};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::Path;

/// Truncation cap for Bash stdout slice.
const STDOUT_CAP: usize = 2 * 1024;
/// Truncation cap for Bash stderr slice.
const STDERR_CAP: usize = 1024;
/// Truncation cap for Read content excerpts.
const CONTENT_CAP: usize = 4 * 1024;
/// Truncation cap for Edit/Write file_before / file_after slices.
const FILE_CHUNK_CAP: usize = 4 * 1024;

/// The wire payload emitted on `tool.result` events.
///
/// Field shape matches the dashboard `ToolResultPayload` consumer in
/// `apps/dashboard/src-tauri/src/telemetry.rs` (followup-2 § 4c). All inner
/// fields are optional — different tools populate different subsets.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolResultPayload {
    /// Correlates this result with the matching `tool.use` event (when the
    /// harness forwards `tool_use_id`). Falls back to chronological pairing
    /// downstream when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// Tool name verbatim (`"Bash"`, `"Edit"`, `"Write"`, `"MultiEdit"`, `"Read"`).
    pub tool: String,
    /// The target file path for Edit/Write/MultiEdit/Read; `None` for Bash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Bash stdout, capped at [`STDOUT_CAP`] bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_excerpt: Option<String>,
    /// Bash stderr, capped at [`STDERR_CAP`] bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_excerpt: Option<String>,
    /// Bash exit code when reported by the harness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    /// For Edit: the pre-replacement string (`tool_input.old_string`). For
    /// MultiEdit: every edit's `old_string` joined with a marker. Capped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_before: Option<String>,
    /// For Edit: the post-replacement string (`tool_input.new_string`). For
    /// Write: the full content. For MultiEdit: every edit's `new_string`
    /// joined with a marker. Capped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_after: Option<String>,
    /// For Read: an excerpt of the file the LLM saw, capped at [`CONTENT_CAP`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_excerpt: Option<String>,
}

/// Truncate `s` to `max` bytes (char-boundary safe), appending a `[truncated,
/// N bytes more]` suffix when the slice was cut. Returns the input unchanged
/// when it already fits.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Find the last char boundary <= max so the suffix never splits a UTF-8
    // codepoint.
    let mut boundary = max;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let extra = s.len().saturating_sub(boundary);
    let mut out = String::with_capacity(boundary + 40);
    out.push_str(&s[..boundary]);
    let _ = write!(out, "... [truncated, {extra} bytes more]");
    out
}

/// Resolve the project dir the same way [`crate::hooks::tracker`] does — the
/// harness `cwd`, falling back to `"."`.
fn project_dir(input: &HookInput) -> String {
    match input.cwd.as_deref() {
        Some(cwd) if !cwd.is_empty() => cwd.to_string(),
        _ => ".".to_string(),
    }
}

/// Extract `tool_use_id` from any of the places Claude Code may put it. The
/// wire protocol is not fully stabilised; checking both the root and the
/// nested `tool_response` keeps the hook resilient.
fn extract_tool_use_id(input: &HookInput) -> Option<String> {
    if let Some(s) = input.raw.get("tool_use_id").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    if let Some(s) = input
        .raw
        .get("tool_response")
        .and_then(|v| v.get("tool_use_id"))
        .and_then(|v| v.as_str())
    {
        return Some(s.to_string());
    }
    None
}

/// Read a string field from either a nested object (e.g. `tool_response.output`)
/// or accept the response when the harness ships it as a bare string.
fn response_string_field(resp: &Value, field: &str) -> Option<String> {
    match resp {
        Value::Object(_) => resp.get(field).and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            other => Some(other.to_string()),
        }),
        Value::String(s) if field == "output" || field == "stdout" => Some(s.clone()),
        _ => None,
    }
}

/// Build the [`ToolResultPayload`] for a given tool. Every tool produces a
/// payload (the `_ =>` branch emits minimal identification fields). Call sites
/// use `Option` for uniform pattern-matching with the observer's emit path.
#[allow(clippy::unnecessary_wraps)] // Option kept for uniform call-site pattern
fn build_payload(tool: &str, input: &HookInput) -> Option<ToolResultPayload> {
    let tool_input = &input.tool_input;
    let tool_response = input.raw.get("tool_response").cloned().unwrap_or(Value::Null);
    let mut payload = ToolResultPayload {
        tool_use_id: extract_tool_use_id(input),
        tool: tool.to_string(),
        ..Default::default()
    };

    match tool {
        "Bash" => {
            // `tool_response.output` is the combined stdout+stderr stream the
            // LLM observed. When the harness splits the streams we honour
            // `stdout` / `stderr`; otherwise everything lands in stdout_excerpt.
            let stdout_raw = response_string_field(&tool_response, "stdout")
                .or_else(|| response_string_field(&tool_response, "output"));
            let stderr_raw = response_string_field(&tool_response, "stderr");
            payload.stdout_excerpt = stdout_raw
                .filter(|s| !s.is_empty())
                .map(|s| truncate(&s, STDOUT_CAP));
            payload.stderr_excerpt = stderr_raw
                .filter(|s| !s.is_empty())
                .map(|s| truncate(&s, STDERR_CAP));
            payload.exit_code = tool_response
                .get("exit_code")
                .or_else(|| tool_response.get("exitCode"))
                .and_then(serde_json::Value::as_i64);
        }
        "Edit" => {
            payload.file_path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            payload.file_before = tool_input
                .get("old_string")
                .and_then(|v| v.as_str())
                .map(|s| truncate(s, FILE_CHUNK_CAP));
            payload.file_after = tool_input
                .get("new_string")
                .and_then(|v| v.as_str())
                .map(|s| truncate(s, FILE_CHUNK_CAP));
        }
        "MultiEdit" => {
            payload.file_path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let edits = tool_input
                .get("edits")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            const SEP: &str = "\n--- edit boundary ---\n";
            let mut before = String::new();
            let mut after = String::new();
            for (i, e) in edits.iter().enumerate() {
                if i > 0 {
                    before.push_str(SEP);
                    after.push_str(SEP);
                }
                if let Some(s) = e.get("old_string").and_then(|v| v.as_str()) {
                    before.push_str(s);
                }
                if let Some(s) = e.get("new_string").and_then(|v| v.as_str()) {
                    after.push_str(s);
                }
            }
            if !before.is_empty() {
                payload.file_before = Some(truncate(&before, FILE_CHUNK_CAP));
            }
            if !after.is_empty() {
                payload.file_after = Some(truncate(&after, FILE_CHUNK_CAP));
            }
        }
        "Write" => {
            payload.file_path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            payload.file_after = tool_input
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| truncate(s, FILE_CHUNK_CAP));
        }
        "Read" => {
            payload.file_path = tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            // `tool_response` for Read is typically the file content as a
            // string, but some harness versions wrap it in `{ file: { content
            // } }` or `{ content }`. Try each shape.
            let content = response_string_field(&tool_response, "content")
                .or_else(|| {
                    tool_response
                        .get("file")
                        .and_then(|f| f.get("content"))
                        .and_then(|c| c.as_str())
                        .map(str::to_string)
                })
                .or_else(|| match &tool_response {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });
            payload.content_excerpt = content
                .filter(|s| !s.is_empty())
                .map(|s| truncate(&s, CONTENT_CAP));
        }
        _ => {
            // Unmodelled tool — emit only the identification fields. The
            // dashboard ignores empty payloads gracefully.
        }
    }

    Some(payload)
}

/// Emit one `tool.result` event, best-effort. Mirrors the `tracker::emit_event`
/// pattern: open via [`SqliteEventStore::for_project`], append, discard error.
fn emit_event(project_dir: &str, payload: ToolResultPayload) {
    let value = match serde_json::to_value(&payload) {
        Ok(v) => v,
        Err(_) => json!({}),
    };
    let harness_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: "unknown".to_string(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("tool_result".to_string()),
            actor_type: None,
        },
        event: "tool.result".to_string(),
        payload: value,
        spec: current_spec(project_dir),
    };
    // `tool.result` is non-pipeline → per-spec NDJSON via the W5 router.
    let _ = crate::run::event_route::emit(project_dir, &harness_event);
}

/// The PostToolUse `Observer` family member that emits `tool.result` events.
pub struct ToolResult;

impl Observer for ToolResult {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        let Some(tool) = input.tool_name.as_deref() else {
            return;
        };
        // Only emit for tools we actually model — avoids polluting the events
        // table with empty payloads for every Skill / Task invocation.
        if !matches!(
            tool,
            "Bash" | "Edit" | "MultiEdit" | "Write" | "Read"
        ) {
            return;
        }
        let project = if ctx.project_dir.is_empty() {
            project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        // Project dir must exist for SqliteEventStore — fall back to "." if
        // the harness reported a missing path.
        let project = if Path::new(&project).exists() {
            project
        } else {
            ".".to_string()
        };
        if let Some(payload) = build_payload(tool, input) {
            emit_event(&project, payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // SQLite store is reused for the legacy assertion shape "no tool.result
    // row landed under PreToolUse". The W5 split sends `tool.result` to the
    // NDJSON sink, so the positive `observe_emits_tool_result_for_bash` test
    // reads the NDJSON file directly instead.
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::PostToolUse),
        }
    }

    #[test]
    fn truncate_short_string_is_passthrough() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn truncate_long_string_is_cut_with_suffix() {
        let big = "a".repeat(5000);
        let cut = truncate(&big, 100);
        assert!(cut.starts_with(&"a".repeat(100)));
        assert!(cut.contains("[truncated"));
        assert!(cut.contains("4900 bytes more"));
    }

    #[test]
    fn truncate_respects_utf8_boundary() {
        // Multi-byte chars at the edge of the cap — must not panic.
        let s = "á".repeat(50); // each 'á' is 2 bytes
        let cut = truncate(&s, 9);
        assert!(cut.contains("[truncated"));
    }

    #[test]
    fn bash_payload_extracts_stdout_and_exit_code() {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "ls" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({
                "tool_response": { "output": "file1\nfile2\n", "exit_code": 0 }
            }),
            ..HookInput::default()
        };
        let payload = build_payload("Bash", &input).expect("payload");
        assert_eq!(payload.tool, "Bash");
        assert_eq!(payload.stdout_excerpt.as_deref(), Some("file1\nfile2\n"));
        assert_eq!(payload.exit_code, Some(0));
        assert!(payload.file_path.is_none());
    }

    #[test]
    fn bash_payload_truncates_large_stdout() {
        let big = "x".repeat(STDOUT_CAP * 3);
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "yes" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": { "output": big } }),
            ..HookInput::default()
        };
        let payload = build_payload("Bash", &input).expect("payload");
        let out = payload.stdout_excerpt.expect("stdout");
        assert!(out.len() < STDOUT_CAP + 64);
        assert!(out.contains("[truncated"));
    }

    #[test]
    fn edit_payload_carries_before_and_after() {
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({
                "file_path": "/tmp/f.rs",
                "old_string": "fn foo() {}",
                "new_string": "fn foo() -> i32 { 1 }",
            }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": {} }),
            ..HookInput::default()
        };
        let payload = build_payload("Edit", &input).expect("payload");
        assert_eq!(payload.file_path.as_deref(), Some("/tmp/f.rs"));
        assert_eq!(payload.file_before.as_deref(), Some("fn foo() {}"));
        assert_eq!(payload.file_after.as_deref(), Some("fn foo() -> i32 { 1 }"));
    }

    #[test]
    fn multiedit_payload_concatenates_edits() {
        let input = HookInput {
            tool_name: Some("MultiEdit".to_string()),
            tool_input: json!({
                "file_path": "/tmp/m.rs",
                "edits": [
                    { "old_string": "a", "new_string": "A" },
                    { "old_string": "b", "new_string": "B" },
                ]
            }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let payload = build_payload("MultiEdit", &input).expect("payload");
        assert_eq!(payload.file_path.as_deref(), Some("/tmp/m.rs"));
        let before = payload.file_before.expect("before");
        let after = payload.file_after.expect("after");
        assert!(before.contains('a') && before.contains('b'));
        assert!(before.contains("edit boundary"));
        assert!(after.contains('A') && after.contains('B'));
    }

    #[test]
    fn write_payload_uses_content_as_file_after() {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({
                "file_path": "/tmp/new.txt",
                "content": "hello world",
            }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let payload = build_payload("Write", &input).expect("payload");
        assert_eq!(payload.file_path.as_deref(), Some("/tmp/new.txt"));
        assert_eq!(payload.file_after.as_deref(), Some("hello world"));
        assert!(payload.file_before.is_none());
    }

    #[test]
    fn read_payload_extracts_content_excerpt() {
        let input = HookInput {
            tool_name: Some("Read".to_string()),
            tool_input: json!({ "file_path": "/tmp/r.md" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": "# Title\n\nbody" }),
            ..HookInput::default()
        };
        let payload = build_payload("Read", &input).expect("payload");
        assert_eq!(payload.file_path.as_deref(), Some("/tmp/r.md"));
        assert_eq!(payload.content_excerpt.as_deref(), Some("# Title\n\nbody"));
    }

    #[test]
    fn tool_use_id_extracted_from_root_or_nested() {
        let root = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "ls" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_use_id": "tu_123", "tool_response": {} }),
            ..HookInput::default()
        };
        let nested = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "ls" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({ "tool_response": { "tool_use_id": "tu_456" } }),
            ..HookInput::default()
        };
        assert_eq!(extract_tool_use_id(&root).as_deref(), Some("tu_123"));
        assert_eq!(extract_tool_use_id(&nested).as_deref(), Some("tu_456"));
    }

    #[test]
    fn observe_skips_non_post_tool_use_trigger() {
        // Smoke test: pre-tool-use must short-circuit without emitting.
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "ls" }),
            hook_event_name: Some("PreToolUse".to_string()),
            raw: json!({ "tool_response": { "output": "x" } }),
            ..HookInput::default()
        };
        let pre = Ctx {
            project_dir: project.to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        ToolResult.observe(&input, &pre);
        // No event should land — open the store and check.
        let events = SqliteEventStore::for_project(project)
            .and_then(|s| s.replay())
            .unwrap_or_default();
        assert!(events.iter().all(|e| e.event != "tool.result"));
    }

    #[test]
    fn observe_emits_tool_result_for_bash() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "ls -la" }),
            hook_event_name: Some("PostToolUse".to_string()),
            raw: json!({
                "tool_response": { "output": "file1\nfile2\n", "exit_code": 0 }
            }),
            ..HookInput::default()
        };
        ToolResult.observe(&input, &ctx(project));

        // W5: `tool.result` is non-pipeline → lives in the NDJSON sink under
        // `<project>/.claude/.session/<slug>/events/` (no spec resolves in
        // this test). Read the first file and confirm the payload landed.
        let events_root = dir.path().join(".claude").join(".session");
        // Walk for the first .ndjson file under any session slug.
        let mut found_payload = None;
        'outer: for entry in std::fs::read_dir(&events_root).expect("session dir") {
            let sess = entry.unwrap().path();
            let events_dir = sess.join("events");
            if !events_dir.exists() {
                continue;
            }
            for f in std::fs::read_dir(&events_dir).unwrap() {
                let p = f.unwrap().path();
                let body = std::fs::read_to_string(&p).unwrap_or_default();
                for line in body.lines() {
                    let v: serde_json::Value = match serde_json::from_str(line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if v["event"] == "tool.result" {
                        found_payload = Some(v["payload"].clone());
                        break 'outer;
                    }
                }
            }
        }
        let payload = found_payload.expect("tool.result NDJSON line present");
        assert_eq!(payload.get("tool").and_then(|v| v.as_str()), Some("Bash"));
        assert_eq!(
            payload.get("stdout_excerpt").and_then(|v| v.as_str()),
            Some("file1\nfile2\n")
        );
        assert_eq!(
            payload.get("exit_code").and_then(serde_json::Value::as_i64),
            Some(0)
        );
    }

    #[test]
    fn observe_ignores_unmodelled_tool() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "description": "x" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        ToolResult.observe(&input, &ctx(project));
        let events = SqliteEventStore::for_project(project)
            .and_then(|s| s.replay())
            .unwrap_or_default();
        assert!(events.iter().all(|e| e.event != "tool.result"));
    }
}
