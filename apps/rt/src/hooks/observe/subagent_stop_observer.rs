//! `subagent_stop_observer` — SubagentStop reinforcement observer (W4B migration).
//!
//! Walks `.claude/memory/agent/*.md` via [`MarkdownStore`] and updates
//! `last_used` in frontmatter for any document whose `summary` is a substring
//! of the subagent's terminal output. Signals "this memory was used in this
//! run" to the promotion logic ([`super::memory_promote_observer`]) and the W7
//! lazy-decay model.
//!
//! Pure [`Observer`] — never blocks.

use super::agent_memory::agent_dir;
use crate::shared::events::economy;
use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use mustard_core::domain::model::event::ActorKind;
use mustard_core::io::atomic_md::MarkdownStore;
use serde_json::{json, Value};

pub struct SubagentStopObserver;

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

impl Observer for SubagentStopObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let output = final_output(input);
        if output.is_empty() {
            return;
        }
        let cwd = ctx.project_dir_or_cwd(input);
        bump_last_used(&cwd, &output);
        economy::emit(&cwd, ActorKind::Hook, "stop_observer", "pipeline.economy.operation.invoked", None, json!({"operation": "stop_observer.bump_last_used", "duration_ms": 0, "tokens_used": 0}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::atomic_md::MarkdownDoc;
    use std::path::Path;
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
