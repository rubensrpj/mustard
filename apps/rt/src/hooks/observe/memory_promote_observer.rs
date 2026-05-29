//! `memory_promote_observer` — SessionEnd consolidation observer (W8.T8.6).
//!
//! Promotes high-confidence (`>= 0.85`) `.claude/memory/agent/*.md` rows
//! captured during the session into permanent decision / lesson markdown
//! files, then flips the source row's `status` to `promoted` so it is not
//! promoted twice.
//!
//! Pure [`Observer`] — never blocks.

use super::agent_memory::agent_dir;
use crate::shared::events::economy;
use crate::util::slug;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::domain::model::event::ActorKind;
use mustard_core::io::atomic_md::{MarkdownDoc, MarkdownStore};
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::Path;

pub struct MemoryPromoteObserver;

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
        let slug = slug::slug_for(&now, &content);
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

impl Observer for MemoryPromoteObserver {
    fn observe(&self, _input: &HookInput, ctx: &Ctx) {
        let cwd = if ctx.project_dir.is_empty() {
            ".".to_string()
        } else {
            ctx.project_dir.clone()
        };
        let n = promote_high_confidence(&cwd);
        if n > 0 {
            economy::emit(&cwd, ActorKind::Hook, "stop_observer", "pipeline.economy.operation.invoked", None, json!({"operation": "session_end_consolidate.promote", "duration_ms": 0, "tokens_used": 0}));
        }
    }
}
