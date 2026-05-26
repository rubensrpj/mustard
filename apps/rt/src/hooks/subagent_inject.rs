//! `subagent_inject` — PreToolUse(Task) context injector (W8.T8.3).
//!
//! For every `Task` dispatch that does NOT already declare a `SKILL:` block in
//! its `prompt`, we resolve a minimal slice of:
//!
//! - the project's `CONTEXT.md` (when present), keyed against the spec slug
//!   the dispatch carries via env (`MUSTARD_ACTIVE_SPEC`), and
//! - the top-K skills returned by [`crate::run::skill_resolve::resolve`] for
//!   the prompt + role + active-phase.
//!
//! The slice is surfaced as a [`Verdict::Inject`]. The orchestrator-side
//! `agent-prompt-render` already handles fully-formed dispatches; this hook
//! covers the ad-hoc `Task(general-purpose)` calls that bypass the renderer
//! (the L0 path from CLAUDE.md).
//!
//! ## T8.10 — selective spec-memory load
//!
//! `SessionStart` no longer auto-injects the active spec's `memory/`. Per the
//! deep-refactor budget, spec-memory is loaded **per dispatch**: this hook
//! consults `skill_resolve` and picks at most three `memory/*.md` principles
//! whose name tokens overlap the resolved skill list or the prompt verbs.
//!
//! ## Fail-open contract
//!
//! Every IO step degrades to an empty fragment. The hook never blocks — its
//! decisive verdict is always either `Inject` (when something was resolved)
//! or `Allow` (when nothing was).

use mustard_core::error::Error;
use mustard_core::fs;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use std::path::{Path, PathBuf};

/// Char cap for the injected slice — keeps a high enough ceiling for a useful
/// snippet without ballooning the parent context budget.
const INJECT_MAX_CHARS: usize = 1500;

/// Top-K skills surfaced to the dispatch. Matches the
/// `recommended_skills_via_resolve` cap in `agent_prompt_render`.
const TOP_K_SKILLS: usize = 4;

/// Max spec-memory principles loaded per dispatch (T8.10).
const SPEC_MEMORY_MAX: usize = 3;

/// The W8 subagent-inject hook.
pub struct SubagentInject;

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

/// `true` when the dispatch prompt already declares a SKILL block, in which
/// case we trust the caller (typically `agent-prompt-render`) and stay out.
fn prompt_declares_skill(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    // Accept either the canonical heading or an inline marker.
    lower.contains("\nskill:")
        || lower.contains("recommended skills")
        || lower.starts_with("skill:")
}

/// Pick the role from a Task input. The harness passes `subagent_type`; if
/// missing, fall back to `description` or `"general-purpose"`.
fn role_from_input(input: &HookInput) -> String {
    let tool_input = &input.tool_input;
    tool_input
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .map_or_else(|| "general-purpose".to_string(), str::to_string)
}

/// Read at most `INJECT_MAX_CHARS` of the project's top-level CONTEXT.md.
/// Returns an empty string when the file is missing.
fn read_context_md_slice(project: &Path) -> String {
    let path = project.join("CONTEXT.md");
    let Ok(text) = fs::read_to_string(&path) else {
        return String::new();
    };
    if text.chars().count() <= INJECT_MAX_CHARS {
        text
    } else {
        let trimmed: String = text.chars().take(INJECT_MAX_CHARS).collect();
        format!("{trimmed}\n...[truncated CONTEXT.md slice]")
    }
}

/// Pull the spec-memory principle files most relevant to the dispatch.
/// Mirrors the matching used by `agent_prompt_render::filtered_spec_memory`,
/// but capped at [`SPEC_MEMORY_MAX`] and scoped to the active spec only.
fn spec_memory_block(project: &Path, spec: &str, prompt: &str, role: &str) -> String {
    let memory_dir = project
        .join(".claude")
        .join("spec")
        .join(spec)
        .join("memory");
    let Ok(entries) = fs::read_dir(&memory_dir) else {
        return String::new();
    };

    let mut tokens: Vec<String> = role
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_string)
        .filter(|s| s.len() >= 3)
        .collect();
    for w in prompt.split(|c: char| !c.is_ascii_alphanumeric()) {
        let w = w.to_ascii_lowercase();
        if w.len() >= 3 {
            tokens.push(w);
        }
    }

    let mut matched: Vec<String> = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        if !entry.file_name.ends_with(".md") || entry.file_name.starts_with('_') {
            continue;
        }
        let name = entry.file_name.trim_end_matches(".md").to_ascii_lowercase();
        let name_tokens: Vec<&str> = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|s| s.len() >= 3)
            .collect();
        let relevant = name_tokens
            .iter()
            .any(|nt| tokens.iter().any(|t| t == nt));
        if !relevant {
            continue;
        }
        matched.push(name.clone());
        if matched.len() >= SPEC_MEMORY_MAX {
            break;
        }
    }
    if matched.is_empty() {
        return String::new();
    }
    let mut out = String::from("## SPEC MEMORY\n");
    for name in matched {
        out.push_str("- [[");
        out.push_str(&name);
        out.push_str("]]\n");
    }
    out
}

/// Build the recommended-skills block via [`crate::run::skill_resolve::resolve`].
fn recommended_skills_block(
    project: &Path,
    intent: &str,
    subproject: Option<&str>,
    role: &str,
) -> String {
    // Map role → phase the same way `agent_prompt_render` does.
    let phase = match role.trim().to_ascii_lowercase().as_str() {
        "review" => "REVIEW",
        "explore" => "ANALYZE",
        "plan" => "PLAN",
        "qa" => "QA",
        _ => "EXECUTE",
    };
    let resolved =
        crate::run::skill_resolve::resolve(project, intent, subproject, Some(phase), TOP_K_SKILLS);
    if resolved.is_empty() {
        return String::new();
    }
    let mut out = String::from("## RECOMMENDED SKILLS\n");
    for s in &resolved {
        out.push_str("- ");
        out.push_str(&s.name);
        out.push('\n');
    }
    out
}

/// The dispatch prompt — `tool_input.prompt` for a Task call.
fn dispatch_prompt(input: &HookInput) -> String {
    input
        .tool_input
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_default()
}

impl Check for SubagentInject {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() != Some("Task")
            && input.tool_name.as_deref() != Some("Agent")
        {
            return Ok(Verdict::Allow);
        }
        let prompt = dispatch_prompt(input);
        if prompt_declares_skill(&prompt) {
            // Trust agent-prompt-render — do nothing.
            return Ok(Verdict::Allow);
        }
        let cwd = project_dir(input, ctx);
        let project = PathBuf::from(&cwd);
        let role = role_from_input(input);

        let mut sections: Vec<String> = Vec::new();
        let skills = recommended_skills_block(&project, &prompt, None, &role);
        if !skills.is_empty() {
            sections.push(skills);
        }
        let ctx_md = read_context_md_slice(&project);
        if !ctx_md.is_empty() {
            sections.push(format!("## CONTEXT.md (slice)\n{ctx_md}"));
        }
        if let Some(spec) = crate::run::env::current_spec(&cwd) {
            if !spec.is_empty() {
                let mem = spec_memory_block(&project, &spec, &prompt, &role);
                if !mem.is_empty() {
                    sections.push(mem);
                }
            }
        }
        if sections.is_empty() {
            return Ok(Verdict::Allow);
        }
        // Emit telemetry — fail-open.
        emit_economy_operation(&cwd, "subagent_inject.dispatch");
        let combined = sections.join("\n\n");
        let capped = if combined.chars().count() > INJECT_MAX_CHARS {
            let mut s: String = combined.chars().take(INJECT_MAX_CHARS).collect();
            s.push_str("\n...[truncated subagent_inject slice]");
            s
        } else {
            combined
        };
        Ok(Verdict::Inject { context: capped })
    }
}

/// Emit `pipeline.economy.operation.invoked` for a W8 in-binary operation.
/// Fail-open. Routes through `event_route::emit` for uniformity — every
/// pipeline/economy event in the runtime crate goes through the documented
/// router, never directly through `SqliteEventStore::append`.
fn emit_economy_operation(cwd: &str, operation: &str) {
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;

    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: crate::util::now_iso8601(),
        session_id: crate::run::env::session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("subagent_inject".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({ "operation": operation, "duration_ms": 0, "tokens_used": 0 }),
        spec: crate::run::env::current_spec(cwd),
    };
    let _ = crate::run::event_route::emit(cwd, &event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx_for(dir: &Path) -> Ctx {
        Ctx {
            project_dir: dir.to_string_lossy().to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        }
    }

    fn task_input(prompt: &str, role: &str) -> HookInput {
        HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({ "prompt": prompt, "subagent_type": role }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        }
    }

    #[test]
    fn skip_when_skill_already_declared() {
        let dir = tempdir().unwrap();
        let input = task_input("Do this.\nSKILL: foo\n", "general-purpose");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        assert_eq!(v, Verdict::Allow);
    }

    #[test]
    fn skip_for_non_task_tools() {
        let dir = tempdir().unwrap();
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "ls" }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        assert_eq!(
            SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap(),
            Verdict::Allow
        );
    }

    #[test]
    fn injects_context_md_when_present() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CONTEXT.md"), "## Domain\nrelevant.").unwrap();
        let input = task_input("refactor the user module", "general-purpose");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        match v {
            Verdict::Inject { context } => {
                assert!(context.contains("CONTEXT.md"));
                assert!(context.contains("Domain"));
            }
            other => panic!("expected Inject, got {other:?}"),
        }
    }
}
