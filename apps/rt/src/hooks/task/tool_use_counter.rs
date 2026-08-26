//! `tool_use_counter` — caps tool uses per active Explore subagent.
//!
//! Ports `tool-use-counter.js`: a **`Check`** that denies at the Explore limit
//! (15) and warns at 12, owning the per-agent counter files under
//! `.claude/.agent-state/*.counter.json`. The other three events it handles
//! (`SubagentStart`, `SubagentStop`, `SessionStart`) are pure file-state side
//! effects that resolve to `Allow`. Shared plumbing lives in [`super::common`].

use super::common;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::io::fs;
use mustard_core::platform::error::Error;
use mustard_core::time::now_iso8601;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};

/// Hard tool-use cap and warn threshold for a generic enforced agent type.
const HARD_LIMIT: u32 = 20;
const WARN_THRESHOLD: u32 = 15;
/// Explore agents get a tighter budget — deny at 15, warn at 12.
const EXPLORE_LIMIT: u32 = 15;
const EXPLORE_WARN: u32 = 12;
/// A counter file older than this is stale and is deleted on read.
const COUNTER_STALE_MS: u128 = 10 * 60 * 1000;

/// `tool-use-counter`: caps tool uses per active Explore subagent.
///
/// This is a `Check` because [`Self::handle_pre_tool_use`] can return a
/// blocking `Deny`. The other three events it handles (`SubagentStart`,
/// `SubagentStop`, `SessionStart`) are pure file-state side effects that
/// resolve to `Allow`.
pub struct ToolUseCounter;

/// One on-disk tool-use counter, `<agent_id>.counter.json`.
///
/// The `createdAt` field is *not* modelled here: it is carried verbatim as an
/// ISO string by [`Counter::to_json`], and staleness is computed inline from
/// the parsed string in [`ToolUseCounter::handle_pre_tool_use`].
#[derive(Debug)]
struct Counter {
    /// The enforced agent type, e.g. `"Explore"`.
    agent_type: String,
    /// Hard deny limit.
    limit: u32,
    /// Warn threshold.
    warn_at: u32,
    /// Current tool-use count.
    count: u32,
}

impl Counter {
    /// Serialise to the JSON shape `tool-use-counter.js` writes. The `createdAt`
    /// field round-trips as an ISO string only when the counter was created in
    /// this process; on disk a counter created elsewhere keeps its own string.
    /// To stay faithful, the counter stores `created_at_iso` verbatim.
    fn to_json(&self, created_at_iso: &str) -> Value {
        json!({
            "type": self.agent_type,
            "limit": self.limit,
            "warnAt": self.warn_at,
            "count": self.count,
            "createdAt": created_at_iso,
        })
    }
}

impl ToolUseCounter {
    /// The `.claude/.agent-state` directory for a project.
    fn state_dir(project_dir: &str) -> std::path::PathBuf {
        ClaudePaths::for_project(project_dir)
            .map(|p| p.agent_state_dir())
            .unwrap_or_default()
    }

    /// `SubagentStart`: create a counter file for an enforced agent type
    /// (`Explore`). Returns the budget-reminder advisory, or `Allow` for a
    /// non-enforced type.
    fn handle_start(input: &HookInput, project_dir: &str) -> Verdict {
        let agent_id = input
            .raw
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let agent_type = input
            .raw
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Only `Explore` is an enforced type (`ENFORCED_TYPES`).
        if agent_type != "Explore" {
            return Verdict::Allow;
        }
        let agent_id = if agent_id.is_empty() {
            // The JS uses `unknown-${Date.now()}`; a deterministic-enough id.
            &format!("unknown-{}", mustard_core::time::now_unix_millis() as u128)
        } else {
            agent_id
        };

        let dir = Self::state_dir(project_dir);
        let _ = fs::create_dir_all(&dir);

        let limit = EXPLORE_LIMIT; // agent_type == "Explore"
        let warn_at = EXPLORE_WARN;
        let counter = Counter {
            agent_type: agent_type.to_string(),
            limit,
            warn_at,
            count: 0,
        };
        let iso = now_iso8601();
        let file = dir.join(format!("{agent_id}.counter.json"));
        let _ = fs::write_atomic(
            &file,
            serde_json::to_string_pretty(&counter.to_json(&iso)).unwrap_or_default().as_bytes(),
        );

        Verdict::Inject {
            context: format!(
                "[Tool Budget] This agent has a {limit}-tool-use budget. \
                 Use Grep over Read where possible. Return findings as soon as \
                 root cause is clear."
            ),
        }
    }

    /// `SubagentStop`: remove the stopped agent's counter file.
    fn handle_stop(input: &HookInput, project_dir: &str) -> Verdict {
        let agent_id = input
            .raw
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !agent_id.is_empty() {
            let file = Self::state_dir(project_dir).join(format!("{agent_id}.counter.json"));
            let _ = fs::remove_file(&file);
        }
        Verdict::Allow
    }

    /// `SessionStart`: delete every `*.counter.json` — a fresh session starts
    /// with clean counters.
    fn handle_session_start(project_dir: &str) -> Verdict {
        let dir = Self::state_dir(project_dir);
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries {
                if entry.file_name.ends_with(".counter.json") {
                    let _ = fs::remove_file(&entry.path);
                }
            }
        }
        Verdict::Allow
    }

    /// `PreToolUse`: increment every active counter, enforce the limits.
    ///
    /// A counter that reaches its `limit` denies (deny dominates). The first
    /// counter to hit `warn_at` warns. A stale counter is deleted and skipped.
    fn handle_pre_tool_use(project_dir: &str) -> Verdict {
        let dir = Self::state_dir(project_dir);
        let Ok(entries) = fs::read_dir(&dir) else {
            return Verdict::Allow; // no state dir → no active Explore agents
        };
        let counter_files: Vec<std::path::PathBuf> = entries
            .into_iter()
            .filter(|e| e.file_name.ends_with(".counter.json"))
            .map(|e| e.path)
            .collect();
        if counter_files.is_empty() {
            return Verdict::Allow;
        }

        let now = mustard_core::time::now_unix_millis() as u128;
        let mut deny: Option<Verdict> = None;
        let mut warn: Option<Verdict> = None;

        for file in counter_files {
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue; // corrupt counter — skip
            };
            let created_at_iso = value
                .get("createdAt")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let created_at_ms = mustard_core::time::parse_iso_millis(created_at_iso).unwrap_or(0) as u128;

            // Staleness: delete and skip.
            if now.saturating_sub(created_at_ms) > COUNTER_STALE_MS {
                let _ = fs::remove_file(&file);
                continue;
            }

            let count = value
                .get("count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32
                + 1;
            let limit = value
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .map_or(HARD_LIMIT, |n| n as u32);
            let warn_at = value
                .get("warnAt")
                .and_then(serde_json::Value::as_u64)
                .map_or(WARN_THRESHOLD, |n| n as u32);
            let agent_type = value
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            // Persist the incremented count, preserving the original
            // `createdAt` string.
            let updated = Counter {
                agent_type,
                limit,
                warn_at,
                count,
            };
            let _ = fs::write_atomic(
                &file,
                serde_json::to_string_pretty(&updated.to_json(created_at_iso))
                    .unwrap_or_default()
                    .as_bytes(),
            );

            if count >= limit {
                deny = Some(Verdict::Deny {
                    reason: format!(
                        "[Tool Budget] Explore agent reached {limit} tool uses \
                         (limit). Wrap up your findings."
                    ),
                });
                // Deny dominates — stop scanning the remaining counters.
                break;
            }
            if count == warn_at && warn.is_none() {
                warn = Some(Verdict::Warn {
                    message: format!(
                        "[Tool Budget] {count}/{limit} tool uses. Begin wrapping \
                         up — return findings after completing current \
                         investigation."
                    ),
                });
            }
        }

        deny.or(warn).unwrap_or(Verdict::Allow)
    }
}

impl Check for ToolUseCounter {
    /// Dispatch by trigger to the matching handler. The JS hook runs on four
    /// events (`SubagentStart` / `SubagentStop` / `PreToolUse` /
    /// `SessionStart`); any other trigger self-allows.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        let project = if ctx.project_dir.is_empty() {
            common::project_dir(input)
        } else {
            ctx.project_dir.clone()
        };
        let verdict = match ctx.trigger {
            Some(Trigger::SubagentStart) => Self::handle_start(input, &project),
            Some(Trigger::SubagentStop) => Self::handle_stop(input, &project),
            Some(Trigger::PreToolUse) => Self::handle_pre_tool_use(&project),
            Some(Trigger::SessionStart) => Self::handle_session_start(&project),
            _ => Verdict::Allow,
        };
        Ok(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx(trigger: Trigger, dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(trigger),
            workspace_root: None,
            inject_only: None,
        }
    }

    // --- tool-use-counter parity (hooks.test.js "tool-use-counter.js") -----

    #[test]
    fn start_creates_counter_for_explore_with_15_budget() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            hook_event_name: Some("SubagentStart".to_string()),
            raw: json!({ "agent_id": "explore-123", "agent_type": "Explore" }),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::SubagentStart, project))
            .unwrap();
        // The budget reminder is injected.
        match verdict {
            Verdict::Inject { context } => {
                assert!(context.contains("Tool Budget"));
                assert!(context.contains("15"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
        // The counter file exists with the Explore budget.
        let f = dir
            .path()
            .join(".claude")
            .join(".agent-state")
            .join("explore-123.counter.json");
        let counter: Value =
            serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        assert_eq!(counter["type"], json!("Explore"));
        assert_eq!(counter["limit"], json!(15));
        assert_eq!(counter["warnAt"], json!(12));
        assert_eq!(counter["count"], json!(0));
    }

    #[test]
    fn start_does_not_create_counter_for_non_explore() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            hook_event_name: Some("SubagentStart".to_string()),
            raw: json!({ "agent_id": "impl-1", "agent_type": "general-purpose" }),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::SubagentStart, project))
            .unwrap();
        assert_eq!(verdict, Verdict::Allow);
        let f = dir
            .path()
            .join(".claude")
            .join(".agent-state")
            .join("impl-1.counter.json");
        assert!(!f.exists());
    }

    #[test]
    fn pre_tool_use_with_no_counters_allows() {
        let dir = tempdir().unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, dir.path().to_str().unwrap()))
            .unwrap();
        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn pre_tool_use_denies_when_counter_reaches_limit() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        // A counter at count=14, limit=15 — the next PreToolUse hits 15.
        let counter = json!({
            "type": "Explore",
            "limit": 15,
            "warnAt": 12,
            "count": 14,
            "createdAt": now_iso8601(),
        });
        std::fs::write(
            state_dir.join("explore-x.counter.json"),
            counter.to_string(),
        )
        .unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Grep".to_string()),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        match verdict {
            Verdict::Deny { reason } => assert!(reason.contains("Tool Budget")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn pre_tool_use_warns_at_threshold() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        // count=11, warnAt=12 — the next PreToolUse hits exactly 12.
        let counter = json!({
            "type": "Explore", "limit": 15, "warnAt": 12,
            "count": 11, "createdAt": now_iso8601(),
        });
        std::fs::write(
            state_dir.join("explore-w.counter.json"),
            counter.to_string(),
        )
        .unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Grep".to_string()),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        match verdict {
            Verdict::Warn { message } => assert!(message.contains("12/15")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn pre_tool_use_deletes_stale_counter() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        // createdAt well over 10 minutes ago → stale.
        let counter = json!({
            "type": "Explore", "limit": 15, "warnAt": 12, "count": 14,
            "createdAt": "2000-01-01T00:00:00.000Z",
        });
        let file = state_dir.join("explore-stale.counter.json");
        std::fs::write(&file, counter.to_string()).unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Grep".to_string()),
            ..HookInput::default()
        };
        let verdict = ToolUseCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        // Stale counter is skipped → no deny — and the file is gone.
        assert_eq!(verdict, Verdict::Allow);
        assert!(!file.exists());
    }

    #[test]
    fn session_start_clears_counters() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("a.counter.json"), "{}").unwrap();
        std::fs::write(state_dir.join("b.counter.json"), "{}").unwrap();
        ToolUseCounter
            .evaluate(
                &HookInput {
                    hook_event_name: Some("SessionStart".to_string()),
                    ..HookInput::default()
                },
                &ctx(Trigger::SessionStart, project),
            )
            .unwrap();
        assert!(!state_dir.join("a.counter.json").exists());
        assert!(!state_dir.join("b.counter.json").exists());
    }

    #[test]
    fn stop_removes_counter_file() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let state_dir = dir.path().join(".claude").join(".agent-state");
        std::fs::create_dir_all(&state_dir).unwrap();
        let file = state_dir.join("explore-s.counter.json");
        std::fs::write(&file, "{}").unwrap();
        ToolUseCounter
            .evaluate(
                &HookInput {
                    hook_event_name: Some("SubagentStop".to_string()),
                    raw: json!({ "agent_id": "explore-s" }),
                    ..HookInput::default()
                },
                &ctx(Trigger::SubagentStop, project),
            )
            .unwrap();
        assert!(!file.exists());
    }
}
