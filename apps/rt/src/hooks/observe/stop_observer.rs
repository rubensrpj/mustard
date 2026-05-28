//! `stop_observer` — SubagentStop reinforcement observer (W4B migration).
//!
//! Walks `.claude/memory/agent/*.md` via [`MarkdownStore`] and updates
//! `last_used` in frontmatter for any document whose `summary` is a substring
//! of the subagent's terminal output. Signals "this memory was used in this
//! run" to the W8.T8.6 promotion logic and the W7 lazy-decay model.
//!
//! Pure [`Observer`] — never blocks.

use mustard_core::io::atomic_md::{MarkdownDoc, MarkdownStore};
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct StopObserver;

fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

fn final_output(input: &HookInput) -> String {
    for key in ["result", "final_output", "output", "tool_response", "tool_result"] {
        if let Some(v) = input.raw.get(key) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
            if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    String::new()
}

fn agent_dir(cwd: &str) -> Option<PathBuf> {
    ClaudePaths::for_project(Path::new(cwd))
        .ok()
        .map(|p| p.claude_dir().join("memory").join("agent"))
}

/// Walk `.claude/memory/agent/*.md` and bump `last_used` for every doc whose
/// `summary` is a substring of `text`. Fail-open: per-file errors degrade
/// silently.
fn bump_last_used(cwd: &str, text: &str) {
    let Some(dir) = agent_dir(cwd) else { return };
    if !dir.exists() {
        return;
    }
    let now = mustard_core::time::now_iso8601();
    for doc in MarkdownStore::scan_dir(&dir) {
        let Some(fm) = &doc.frontmatter else { continue };
        let Some(summary) = fm.get_str("summary") else { continue };
        let trimmed = summary.trim();
        if trimmed.len() < 6 {
            continue;
        }
        if !text.contains(trimmed) {
            continue;
        }
        // Load full doc, update last_used, re-write atomically.
        let Ok(mut full) = MarkdownStore::read_one(&doc.path) else { continue };
        if let Some(fm2) = &mut full.frontmatter {
            if let Value::Object(map) = &mut fm2.0 {
                map.insert("last_used".into(), json!(now.clone()));
            }
        }
        let _ = MarkdownStore::write_atomic(&doc.path, &full);
    }
}

fn emit_economy_operation(cwd: &str, operation: &str) {
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: mustard_core::time::now_iso8601(),
        session_id: crate::shared::context::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("stop_observer".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({ "operation": operation, "duration_ms": 0, "tokens_used": 0 }),
        spec: crate::shared::context::current_spec(cwd),
    };
    let _ = crate::shared::events::route::emit(cwd, &event);
}

impl Observer for StopObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let output = final_output(input);
        if output.is_empty() {
            return;
        }
        let cwd = project_dir(input, ctx);
        bump_last_used(&cwd, &output);
        emit_economy_operation(&cwd, "stop_observer.bump_last_used");
    }
}

// ---------------------------------------------------------------------------
// W8.T8.6 — SessionEnd consolidation
// ---------------------------------------------------------------------------

/// SessionEnd consolidation observer.
///
/// Promotes high-confidence (`>= 0.85`) `.claude/memory/agent/*.md` rows
/// captured during the session into permanent decision / lesson markdown
/// files, then flips the source row's `status` to `promoted` so it is not
/// promoted twice.
pub struct SessionEndConsolidate;

pub const PROMOTION_CONFIDENCE_THRESHOLD: f64 = 0.85;

/// Classify a summary as decision or lesson. Imperative verbs → decision.
fn classify(summary: &str) -> &'static str {
    let head = summary
        .trim_start()
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("");
    let head_lower = head.to_ascii_lowercase();
    let decision_verbs = [
        "use", "adopt", "prefer", "reject", "switch", "ban", "require", "enforce",
    ];
    if decision_verbs.iter().any(|v| *v == head_lower) {
        "decisions"
    } else {
        "lessons"
    }
}

fn fnv1a8(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:016x}", h).chars().take(8).collect()
}

fn slug_for(captured_at: &str, content: &str) -> String {
    let ts_compact: String = captured_at
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    format!("{ts_compact}-{}", fnv1a8(content))
}

fn promote_high_confidence(cwd: &str) -> usize {
    let Some(dir) = agent_dir(cwd) else { return 0 };
    if !dir.exists() {
        return 0;
    }
    let Ok(cp) = ClaudePaths::for_project(Path::new(cwd)) else {
        return 0;
    };
    let memory_root = cp.claude_dir().join("memory");
    let now = mustard_core::time::now_iso8601();
    let mut promoted = 0usize;

    for doc in MarkdownStore::scan_dir(&dir) {
        let Some(fm) = &doc.frontmatter else { continue };
        let status = fm
            .get_str("status")
            .map(str::to_string)
            .unwrap_or_else(|| "active".to_string());
        if status != "active" {
            continue;
        }
        let confidence = fm
            .as_object()
            .and_then(|o| o.get("confidence"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if confidence < PROMOTION_CONFIDENCE_THRESHOLD {
            continue;
        }
        let summary = fm.get_str("summary").map(str::to_string).unwrap_or_default();
        let spec = fm.get_str("spec").map(str::to_string);
        // Body of source doc becomes "details".
        let details = MarkdownStore::read_one(&doc.path)
            .map(|d| d.body)
            .unwrap_or_default();
        let table = classify(&summary);
        let content = if details.is_empty() {
            summary.clone()
        } else {
            format!("{summary}\n\n{details}")
        };
        let source = spec.unwrap_or_else(|| "agent_memory_promotion".to_string());
        let dest_dir = memory_root.join(table);
        if std::fs::create_dir_all(&dest_dir).is_err() {
            continue;
        }
        let slug = slug_for(&now, &content);
        let dest_path = dest_dir.join(format!("{slug}.md"));
        let kind = if table == "decisions" { "decision" } else { "lesson" };
        let mut new_fm = serde_json::Map::new();
        new_fm.insert("kind".into(), json!(kind));
        new_fm.insert("captured_at".into(), json!(now.clone()));
        new_fm.insert("source".into(), json!(source));
        new_fm.insert("status".into(), json!("active"));
        let new_doc = MarkdownDoc {
            path: dest_path.clone(),
            frontmatter: Some(mustard_core::io::atomic_md::frontmatter::Frontmatter(
                Value::Object(new_fm),
            )),
            body: format!("{content}\n"),
        };
        if MarkdownStore::write_atomic(&dest_path, &new_doc).is_ok() {
            // Flip source to promoted.
            if let Ok(mut src_doc) = MarkdownStore::read_one(&doc.path) {
                if let Some(src_fm) = &mut src_doc.frontmatter {
                    if let Value::Object(map) = &mut src_fm.0 {
                        map.insert("status".into(), json!("promoted"));
                    }
                }
                let _ = MarkdownStore::write_atomic(&doc.path, &src_doc);
            }
            promoted += 1;
        }
    }
    promoted
}

impl Observer for SessionEndConsolidate {
    fn observe(&self, _input: &HookInput, ctx: &Ctx) {
        let cwd = if ctx.project_dir.is_empty() {
            ".".to_string()
        } else {
            ctx.project_dir.clone()
        };
        let n = promote_high_confidence(&cwd);
        if n > 0 {
            emit_economy_operation(&cwd, "session_end_consolidate.promote");
        }
    }
}

// ---------------------------------------------------------------------------
// W8.T8.7 — PreCompact: surface up to 3 recent memories as injected context
// ---------------------------------------------------------------------------

pub struct PreCompactMemorySnippet;

fn recent_agent_memory(cwd: &str) -> Vec<String> {
    let Some(dir) = agent_dir(cwd) else { return Vec::new() };
    if !dir.exists() {
        return Vec::new();
    }
    let mut rows: Vec<(String, String)> = Vec::new();
    for doc in MarkdownStore::scan_dir(&dir) {
        let Some(fm) = &doc.frontmatter else { continue };
        let status = fm
            .get_str("status")
            .map(str::to_string)
            .unwrap_or_else(|| "active".to_string());
        if status != "active" {
            continue;
        }
        let Some(summary) = fm.get_str("summary").map(str::to_string) else {
            continue;
        };
        let ts = fm
            .get_str("last_used")
            .or_else(|| fm.get_str("at"))
            .map(str::to_string)
            .unwrap_or_default();
        rows.push((ts, summary));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    rows.into_iter().take(3).map(|(_, s)| s).collect()
}

impl mustard_core::domain::model::contract::Check for PreCompactMemorySnippet {
    fn evaluate(
        &self,
        input: &HookInput,
        ctx: &Ctx,
    ) -> Result<mustard_core::domain::model::contract::Verdict, mustard_core::platform::error::Error> {
        use mustard_core::domain::model::contract::{Trigger, Verdict};
        if ctx.trigger != Some(Trigger::PreCompact) {
            return Ok(Verdict::Allow);
        }
        let cwd = project_dir(input, ctx);
        let entries = recent_agent_memory(&cwd);
        if entries.is_empty() {
            return Ok(Verdict::Allow);
        }
        emit_economy_operation(&cwd, "pre_compact_memory_snippet.inject");
        let body: String = entries
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Verdict::Inject {
            context: format!("[Agent memory — recent]\n{body}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::tempdir;

    fn write_memory(dir: &Path, slug: &str, summary: &str, last_used: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{slug}.md"));
        let mut fm = serde_json::Map::new();
        fm.insert("summary".into(), json!(summary));
        fm.insert("confidence".into(), json!(0.5));
        fm.insert("status".into(), json!("active"));
        fm.insert("at".into(), json!(last_used));
        fm.insert("last_used".into(), json!(last_used));
        let doc = MarkdownDoc {
            path: path.clone(),
            frontmatter: Some(mustard_core::io::atomic_md::frontmatter::Frontmatter(
                Value::Object(fm),
            )),
            body: String::new(),
        };
        MarkdownStore::write_atomic(&path, &doc).unwrap();
        path
    }

    #[test]
    fn bump_last_used_updates_matching_row() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();
        let agent = dir.path().join(".claude").join("memory").join("agent");
        let path = write_memory(
            &agent,
            "test",
            "MUSTARD-W8-MARKER-XYZZY-PROOF",
            "2026-05-25T00:00:00.000Z",
        );
        let before = MarkdownStore::read_one(&path)
            .unwrap()
            .frontmatter
            .and_then(|f| f.get_str("last_used").map(str::to_string))
            .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        bump_last_used(
            &dir.path().to_string_lossy(),
            "stuff before MUSTARD-W8-MARKER-XYZZY-PROOF stuff after",
        );

        let after = MarkdownStore::read_one(&path)
            .unwrap()
            .frontmatter
            .and_then(|f| f.get_str("last_used").map(str::to_string))
            .unwrap();
        assert_ne!(after, before, "last_used should have advanced");
    }
}
