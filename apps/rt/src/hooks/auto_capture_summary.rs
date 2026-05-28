//! `auto_capture_summary` — PostToolUse(Task) memory writer (W8.T8.4).
//!
//! Every time a `Task` (subagent) returns, we scan its output for either:
//!
//! - a `<MEMORY>…</MEMORY>` block (preferred — the explicit form), or
//! - a `Resumo:` / `Summary:` line/section (fallback — informal returns)
//!
//! and persist what we find as an `agent_memory` row via the W7 helper
//! `crate::run::knowledge::memory::insert_agent_memory`. The hook never blocks — it is
//! a pure [`Observer`].
//!
//! ## W3C → W4B migration
//!
//! `emit_economy_operation` routes economy events via
//! `crate::shared::events::route::emit` (NDJSON path). W4B then moved the
//! `agent_memory` write-path off SQLite onto markdown rows under
//! `.claude/memory/agent/` via `crate::run::knowledge::memory::persist_agent_memory_md`,
//! so no `rusqlite` connection is opened from this module.
//!
//! ## Fail-open
//!
//! Every IO step degrades to a no-op. Telemetry is not load-bearing.

use mustard_core::model::contract::{Ctx, HookInput, Observer};

/// The W8 auto-capture hook.
pub struct AutoCaptureSummary;

/// Resolve the project dir for an invocation.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// Extract a `<MEMORY>...</MEMORY>` block body, trimmed. `None` when absent or
/// empty.
fn extract_memory_block(text: &str) -> Option<String> {
    let open = text.find("<MEMORY>")?;
    let rest = &text[open + "<MEMORY>".len()..];
    let close = rest.find("</MEMORY>")?;
    let body = rest[..close].trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

/// Extract a `Resumo:` / `Summary:` line as a single-line summary. Picks the
/// first paragraph after the keyword, capped at 240 chars.
fn extract_resumo(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let key_idx = lower
        .find("resumo:")
        .or_else(|| lower.find("summary:"))?;
    // Step past the keyword + colon. Use byte offsets — they are identical to
    // char offsets for ASCII keywords.
    let after = &text[key_idx..];
    let colon = after.find(':')?;
    let body = after[colon + 1..].trim_start();
    // Take until the next blank line.
    let mut end = body.len();
    for (i, line) in body.lines().enumerate() {
        if i > 0 && line.trim().is_empty() {
            // Compute the byte offset of this blank line in `body`.
            let mut off = 0;
            for (j, l) in body.lines().enumerate() {
                if j == i {
                    end = off;
                    break;
                }
                off += l.len() + 1;
            }
            break;
        }
    }
    let raw = body[..end].trim();
    if raw.is_empty() {
        return None;
    }
    let summary: String = raw.chars().take(240).collect();
    Some(summary)
}

/// Pull the role for an `agent_memory` row from the Task input.
fn role_from_input(input: &HookInput) -> Option<String> {
    input
        .tool_input
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Pull the Task output text. The harness layers vary; try common locations.
fn task_output(input: &HookInput) -> String {
    // The PostToolUse payload typically lands under `tool_response` /
    // `tool_result` / `output`. Probe all three.
    for key in ["tool_response", "tool_result", "output", "result"] {
        if let Some(v) = input.raw.get(key) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
            // Sometimes the harness nests the text under `.text`.
            if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    String::new()
}

/// Persist a single captured summary as an `agent_memory` markdown row.
/// Fail-open: a write error degrades silently.
///
/// W4B migration: persistence moved off SQLite entirely. The summary lands as
/// a markdown file under `.claude/memory/agent/` via
/// [`crate::run::knowledge::memory::persist_agent_memory_md`]. No `rusqlite::Connection`
/// is opened from this path.
fn persist(
    cwd: &str,
    session_id: Option<&str>,
    spec: Option<&str>,
    role: Option<&str>,
    summary: &str,
    details: Option<&str>,
) {
    // W4B migration: persistence moved off SQLite — write a markdown row in
    // `.claude/memory/agent/` via the shared helper.
    let _ = crate::run::knowledge::memory::persist_agent_memory_md(
        cwd,
        session_id,
        spec,
        None,
        role,
        summary,
        details,
        0.7,
        Some("active"),
    );
}

/// Emit `pipeline.economy.operation.invoked` via the NDJSON event route.
/// Fail-open: any error degrades to a no-op.
fn emit_economy_operation(cwd: &str, operation: &str) {
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: crate::util::now_iso8601(),
        session_id: crate::shared::context::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("auto_capture_summary".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({ "operation": operation, "duration_ms": 0, "tokens_used": 0 }),
        spec: crate::shared::context::current_spec(cwd),
    };
    let _ = crate::shared::events::route::emit(cwd, &event);
}

impl Observer for AutoCaptureSummary {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        // Only Task PostToolUse — the registry already constrains us, but
        // belt-and-braces.
        let output = task_output(input);
        if output.is_empty() {
            return;
        }
        let cwd = project_dir(input, ctx);

        let memory_body = extract_memory_block(&output);
        let resumo = extract_resumo(&output);

        // Prefer the explicit MEMORY block; fall back to Resumo:.
        let (summary, details) = match (memory_body.as_deref(), resumo.as_deref()) {
            (Some(body), _) => {
                // First non-empty line is the summary; remainder = details.
                let mut lines = body.lines();
                let summary = lines.find(|l| !l.trim().is_empty()).unwrap_or("").trim();
                let rest: String = lines.collect::<Vec<_>>().join("\n");
                let rest = rest.trim();
                (
                    summary.to_string(),
                    if rest.is_empty() {
                        None
                    } else {
                        Some(rest.to_string())
                    },
                )
            }
            (None, Some(r)) => (r.to_string(), None),
            (None, None) => return,
        };
        if summary.is_empty() {
            return;
        }

        let role = role_from_input(input);
        let spec = crate::shared::context::current_spec(&cwd);
        let session_id = input
            .session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        persist(
            &cwd,
            session_id.as_deref(),
            spec.as_deref(),
            role.as_deref(),
            &summary,
            details.as_deref(),
        );
        emit_economy_operation(&cwd, "auto_capture_summary.persist");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_memory_block_round_trip() {
        let text = "blurb\n<MEMORY>\nKey insight here.\nLine two.\n</MEMORY>\nmore";
        let body = extract_memory_block(text).unwrap();
        assert!(body.contains("Key insight"));
        assert!(body.contains("Line two"));
    }

    #[test]
    fn extract_memory_block_absent_returns_none() {
        assert!(extract_memory_block("no marker here").is_none());
    }

    #[test]
    fn extract_resumo_picks_first_paragraph() {
        let text = "blah blah\n\nResumo: this is the takeaway from the run.\n\nNext section\n";
        let r = extract_resumo(text).unwrap();
        assert!(r.contains("takeaway"));
    }

    #[test]
    fn extract_summary_keyword_also_works() {
        let text = "Summary: short one-liner.\n";
        let r = extract_resumo(text).unwrap();
        assert!(r.starts_with("short"));
    }
}
