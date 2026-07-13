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
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::domain::model::event::ActorKind;
use mustard_core::domain::model::knowledge::{Kind, Knowledge, Origin, Scope, Status};
use mustard_core::io::knowledge_store::KnowledgeStore;
use mustard_core::io::atomic_md::MarkdownStore;
use mustard_core::ClaudePaths;
use serde_json::json;
use std::path::Path;

pub struct MemoryPromoteObserver;

pub(crate) const PROMOTION_CONFIDENCE_THRESHOLD: f64 = 0.85;

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
    let Some(agent) = agent_dir(cwd) else { return 0 };
    if !agent.exists() {
        return 0;
    }
    let Ok(cp) = ClaudePaths::for_project(Path::new(cwd)) else {
        return 0;
    };
    // One store, rooted at `.claude/`: it owns BOTH the read of the agent
    // summaries and the write of the promoted decision/lesson + the in-place
    // status flip. The agent dir is `memory/agent/` → Summary records.
    //
    // The read is SCOPED to `memory/agent/` (one `scan_dir` over that dir), not
    // a whole-`.claude/` `read_all`: a recursive read of the root would also
    // hydrate `spec.md` / `wave-plan.md` and every other frontmatter-less `.md`
    // as a `Kind::Summary` (no `kind:` key → Summary), polluting the promotion
    // set. The agent dir holds only agent summaries, so it is the correct reach.
    let store = KnowledgeStore::new(cp.claude_dir());
    let now = mustard_core::time::now_iso8601();
    let mut promoted = 0usize;

    let source_paths: Vec<_> = MarkdownStore::scan_dir(&agent).into_iter().map(|d| d.path).collect();
    for path in source_paths {
        let Ok(src) = store.read(&path) else { continue };
        if src.kind != Kind::Summary || src.status != Status::Active {
            continue;
        }
        if f64::from(src.confidence) < PROMOTION_CONFIDENCE_THRESHOLD {
            continue;
        }
        let summary = src.label.clone();
        let details = src.content.clone();
        let table = classify(&summary);
        let content = if details.is_empty() {
            summary.clone()
        } else {
            format!("{summary}\n\n{details}")
        };
        let kind = if table == "decisions" {
            Kind::Decision
        } else {
            Kind::Lesson
        };
        // Provenance: keep the source spec when present; else mark the synthetic
        // promotion origin.
        let source_spec = src
            .scope
            .spec()
            .map(str::to_string)
            .unwrap_or_else(|| "agent_memory_promotion".to_string());
        let promoted_record = Knowledge {
            kind,
            scope: Scope::Spec {
                spec: source_spec.clone(),
            },
            label: content.chars().take(200).collect(),
            content,
            origin: Origin {
                spec: Some(source_spec),
                captured_at: now.clone(),
                ..Origin::default()
            },
            confidence: 0.0,
            status: Status::Active,
        };
        // Only proceed when the promotion actually persisted. The store's
        // quality gate may skip a non-substantive record (`Ok(None)`); in that
        // case we must NOT flip the source to `promoted` or count it.
        if matches!(store.write(&promoted_record), Ok(Some(_))) {
            // Flip the source summary to `promoted` in place. The slug is
            // content-addressed and `status` is NOT part of the seed, so the
            // re-write lands on the SAME agent file (idempotent overwrite).
            let mut flipped = src;
            flipped.status = Status::Promoted;
            let _ = store.write(&flipped);
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
