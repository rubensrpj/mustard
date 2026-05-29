//! `main_context_counter` — enforces L0 (Universal Delegation) on the
//! orchestrator.
//!
//! Ports `main-context-counter.js`: counts un-delegated main-context work
//! tools between Task dispatches and, in strict mode, denies past `DENY_AT`.
//! A `Check`. Default mode is **`warn`** (advisory), not strict. Shared
//! plumbing lives in [`super::common`].

use super::common;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::io::fs;
use mustard_core::platform::error::Error;
use mustard_core::time::now_iso8601;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::path::Path;

/// Warn / deny thresholds for un-delegated main-context tool calls.
const MAIN_WARN_AT: u32 = 8;
const MAIN_DENY_AT: u32 = 12;
/// Tools that count as main-context "work" (`COUNTED_TOOLS`).
const COUNTED_TOOLS: &[&str] = &[
    "Read", "Edit", "Write", "Bash", "Grep", "Glob", "NotebookEdit",
];
/// The counter file name under `.claude/.agent-state`.
const MAIN_COUNTER_FILE: &str = "main-context.counter.json";

/// The `MUSTARD_MAIN_BUDGET_MODE` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainBudgetMode {
    Off,
    Warn,
    Strict,
}

/// Resolve the main-budget mode in cascade: env var → `mustard.json`
/// (`gates.main_budget`, supplied as `config_override`) → built-in `warn`
/// (this gate is advisory by default, see `settings.json`'s
/// `MUSTARD_MAIN_BUDGET_MODE: "warn"`). An env var set to a non-empty value
/// wins; an absent string OR an unrecognised value resolves to `warn`.
fn main_budget_mode(config_override: Option<&str>) -> MainBudgetMode {
    let s = std::env::var("MUSTARD_MAIN_BUDGET_MODE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| config_override.map(str::to_string));
    match s.unwrap_or_default().to_ascii_lowercase().as_str() {
        "off" => MainBudgetMode::Off,
        "strict" => MainBudgetMode::Strict,
        _ => MainBudgetMode::Warn,
    }
}

/// The persisted main-context counter state.
#[derive(Debug, Clone, Default)]
struct MainState {
    main_count: u32,
    subagent_depth: u32,
}

/// `main-context-counter`: enforces L0 on the orchestrator.
///
/// A `Check`: in strict mode it can `Deny` once `MAIN_DENY_AT` un-delegated
/// tool calls accumulate. The JS hook's warn path prints to stderr; a Rust
/// hook expresses the advisory as a `Verdict::Warn`.
pub struct MainContextCounter;

impl MainContextCounter {
    /// The counter-file path.
    fn counter_path(project_dir: &str) -> std::path::PathBuf {
        ClaudePaths::for_project(project_dir)
            .map(|p| p.agent_state_dir().join(MAIN_COUNTER_FILE))
            .unwrap_or_default()
    }

    /// Read the persisted state. Fail-open: any error → a zeroed state.
    fn read_state(project_dir: &str) -> MainState {
        let Ok(text) = fs::read_to_string(Self::counter_path(project_dir)) else {
            return MainState::default();
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return MainState::default();
        };
        MainState {
            main_count: value
                .get("mainCount")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
            subagent_depth: value
                .get("subagentDepth")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32,
        }
    }

    /// Persist the state. Fail-open.
    fn write_state(project_dir: &str, state: &MainState) {
        let Ok(paths) = ClaudePaths::for_project(Path::new(project_dir)) else {
            return;
        };
        let dir = paths.agent_state_dir();
        let _ = fs::create_dir_all(&dir);
        let body = json!({
            "mainCount": state.main_count,
            "subagentDepth": state.subagent_depth,
            "updatedAt": now_iso8601(),
        });
        let _ = fs::write_atomic(Self::counter_path(project_dir), body.to_string().as_bytes());
    }
}

impl Check for MainContextCounter {
    /// Count an un-delegated main-context tool call and enforce L0.
    ///
    /// `mode` is resolved from `MUSTARD_MAIN_BUDGET_MODE` (default `warn`).
    /// `Off` short-circuits. Lifecycle events keep the `subagentDepth` gauge
    /// honest; a `Task`/`Agent` dispatch resets `mainCount` (work delegated).
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Resolve the project root that owns the counter state file.
        // W5 AC-W5.2: when neither `ctx` nor `input.cwd` carries a valid
        // root, skip the counter machinery entirely — otherwise we leak a
        // `.claude/.agent-state/main-context.counter.json` tree into the
        // process cwd (`apps/rt/` under `cargo test`).
        //
        // Resolved before the mode so the cascade can read the project's
        // `mustard.json` (`gates.main_budget`) override.
        let project = if ctx.project_dir.is_empty() {
            match common::project_dir_opt(input) {
                Some(p) => p,
                None => return Ok(Verdict::Allow),
            }
        } else {
            ctx.project_dir.clone()
        };
        // Cascade: env var → mustard.json (gates.main_budget) → warn.
        let gates = mustard_core::ProjectConfig::load(Path::new(&project)).gates;
        let mode = main_budget_mode(gates.main_budget.as_deref());
        if mode == MainBudgetMode::Off {
            return Ok(Verdict::Allow);
        }
        let mut state = Self::read_state(&project);

        match ctx.trigger {
            Some(Trigger::SessionStart) => {
                Self::write_state(&project, &MainState::default());
                return Ok(Verdict::Allow);
            }
            Some(Trigger::SubagentStart) => {
                state.subagent_depth += 1;
                Self::write_state(&project, &state);
                return Ok(Verdict::Allow);
            }
            Some(Trigger::SubagentStop) => {
                state.subagent_depth = state.subagent_depth.saturating_sub(1);
                Self::write_state(&project, &state);
                return Ok(Verdict::Allow);
            }
            Some(Trigger::PreToolUse) => {}
            _ => return Ok(Verdict::Allow),
        }

        let tool = input.tool_name.as_deref().unwrap_or_default();

        // A Task/Agent dispatch IS delegation — reset the counter.
        if tool == "Task" || tool == "Agent" {
            state.main_count = 0;
            Self::write_state(&project, &state);
            return Ok(Verdict::Allow);
        }

        // Only count main-context work tools, and only outside a subagent.
        if !COUNTED_TOOLS.contains(&tool) {
            return Ok(Verdict::Allow);
        }
        if state.subagent_depth > 0 {
            return Ok(Verdict::Allow);
        }

        state.main_count += 1;
        let count = state.main_count;
        Self::write_state(&project, &state);

        if mode == MainBudgetMode::Strict && count >= MAIN_DENY_AT {
            return Ok(Verdict::Deny {
                reason: format!(
                    "[main-context-counter] {count} tool calls in the main context \
                     without a Task dispatch (L0 Universal Delegation). Stop and \
                     delegate: dispatch a Task agent for this work so the \
                     orchestrator context stays lean. Set \
                     MUSTARD_MAIN_BUDGET_MODE=warn to allow with a warning."
                ),
            });
        }

        // Warn at WARN_AT, then every 4 calls past it.
        if count == MAIN_WARN_AT
            || (count > MAIN_WARN_AT && (count - MAIN_WARN_AT) % 4 == 0)
        {
            return Ok(Verdict::Warn {
                message: format!(
                    "[main-context-counter] {count} tool calls in the main context \
                     without delegating (L0). Consider a Task dispatch — each direct \
                     Read/Edit inflates the orchestrator context."
                ),
            });
        }

        Ok(Verdict::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ctx(trigger: Trigger, dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(trigger),
            workspace_root: None,
        }
    }

    #[test]
    fn main_counter_task_dispatch_resets_count() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        // Seed a non-zero count.
        MainContextCounter::write_state(
            project,
            &MainState {
                main_count: 9,
                subagent_depth: 0,
            },
        );
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Task".to_string()),
            ..HookInput::default()
        };
        let verdict = MainContextCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        assert_eq!(verdict, Verdict::Allow);
        assert_eq!(MainContextCounter::read_state(project).main_count, 0);
    }

    #[test]
    fn main_counter_increments_on_counted_tool() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            ..HookInput::default()
        };
        MainContextCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        assert_eq!(MainContextCounter::read_state(project).main_count, 1);
    }

    #[test]
    fn main_counter_does_not_count_inside_subagent() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        MainContextCounter::write_state(
            project,
            &MainState {
                main_count: 0,
                subagent_depth: 1,
            },
        );
        let input = HookInput {
            hook_event_name: Some("PreToolUse".to_string()),
            tool_name: Some("Read".to_string()),
            ..HookInput::default()
        };
        MainContextCounter
            .evaluate(&input, &ctx(Trigger::PreToolUse, project))
            .unwrap();
        // subagentDepth > 0 → not counted.
        assert_eq!(MainContextCounter::read_state(project).main_count, 0);
    }
}
