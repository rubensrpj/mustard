//! `pre_compact_memory_inject` — PreCompact agent-memory injection (W8.T8.7).
//!
//! On `PreCompact`, surfaces up to 3 recent active `.claude/memory/agent/*.md`
//! summaries as injected context so the post-compaction window keeps the most
//! relevant memories. A `Check` whose only verdict is `Allow` or `Inject`.

use super::agent_memory::agent_dir;
use crate::shared::events::economy;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::domain::model::event::ActorKind;
use mustard_core::io::atomic_md::MarkdownStore;
use mustard_core::platform::error::Error;
use serde_json::json;

pub struct PreCompactMemoryInject;

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

impl Check for PreCompactMemoryInject {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreCompact) {
            return Ok(Verdict::Allow);
        }
        let cwd = ctx.project_dir_or_cwd(input);
        let entries = recent_agent_memory(&cwd);
        if entries.is_empty() {
            return Ok(Verdict::Allow);
        }
        economy::emit(&cwd, ActorKind::Hook, "stop_observer", "pipeline.economy.operation.invoked", None, json!({"operation": "pre_compact_memory_snippet.inject", "duration_ms": 0, "tokens_used": 0}));
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
