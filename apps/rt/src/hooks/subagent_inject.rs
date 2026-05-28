//! `subagent_inject` — PreToolUse(Task) context injector (W8.T8.3).
//!
//! For every `Task` dispatch that does NOT already declare a `SKILL:` block in
//! its `prompt`, we resolve a minimal slice of:
//!
//! - the project's `CONTEXT.md` (when present), keyed against the spec slug
//!   the dispatch carries via env (`MUSTARD_ACTIVE_SPEC`), and
//! - the top-K skills returned by [`crate::commands::skill::skill_resolve::resolve`] for
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

use mustard_core::atomic_md::MarkdownStore;
use mustard_core::error::Error;
use mustard_core::fs;
use mustard_core::i18n::{self, Locale};
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

use crate::commands::review::gate_regression_check::{
    self, check_after_child_return, GateError, GateInput, RegressionVerdict,
};
use crate::commands::review::review_spans::{self, VerdictEntry, VERDICT_AMBER, VERDICT_GREEN, VERDICT_RED};

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
    let Some(memory_dir) = ClaudePaths::for_project(project)
        .ok()
        .and_then(|p| p.for_spec(spec).ok())
        .map(|sp| sp.dir().join("memory"))
    else {
        return String::new();
    };
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

/// Build a knowledge-inject block by scanning `.claude/knowledge/` via
/// [`MarkdownStore::scan_dir`]. Returns a `## KNOWLEDGE` section listing the
/// top-K knowledge doc titles (frontmatter `title` or filename stem), capped at
/// [`SPEC_MEMORY_MAX`] entries. Fail-open: missing dir → empty string.
fn knowledge_block(project: &Path) -> String {
    let Ok(claude) = ClaudePaths::for_project(project) else {
        return String::new();
    };
    let knowledge_dir = claude.claude_dir().join("knowledge");
    let docs = MarkdownStore::scan_dir(&knowledge_dir);
    if docs.is_empty() {
        return String::new();
    }
    let mut out = String::from("## KNOWLEDGE\n");
    for doc in docs.iter().take(SPEC_MEMORY_MAX) {
        // Prefer frontmatter `title`; fall back to the filename stem.
        let name = doc
            .frontmatter
            .as_ref()
            .and_then(|fm| fm.get_str("title"))
            .map(str::to_string)
            .unwrap_or_else(|| {
                doc.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        if !name.is_empty() {
            out.push_str("- [[");
            out.push_str(&name);
            out.push_str("]]\n");
        }
    }
    out
}

/// Render the regression-vocabulary block injected into a child agent's
/// prompt (W5.T5.1). Reuses [`gate_regression_check::build_vocab_matcher`] so
/// the gate and the inject path agree on which terms get surfaced.
///
/// The block is intentionally short — Semantic + Pattern layers only,
/// keyword/noise hits are background data the agent doesn't need to see.
/// Empty when the project has no matcher (fail-open).
fn vocabulary_inject_block(project: &Path, locale: Locale) -> String {
    let Some(_matcher) = gate_regression_check::build_vocab_matcher(project) else {
        return String::new();
    };
    let (semantic, pattern) = read_vocab_layers(project);
    if semantic.is_empty() && pattern.is_empty() {
        return String::new();
    }
    let heading = i18n::translate("gate.vocabulary.inject.heading", locale);
    let lead = i18n::translate("gate.vocabulary.inject.lead", locale);
    let semantic_label = i18n::translate("gate.vocabulary.inject.semantic", locale);
    let pattern_label = i18n::translate("gate.vocabulary.inject.pattern", locale);

    let mut out = String::with_capacity(256);
    out.push_str("## ");
    out.push_str(heading);
    out.push('\n');
    out.push_str(lead);
    out.push_str("\n\n");
    if !semantic.is_empty() {
        out.push_str("- ");
        out.push_str(semantic_label);
        out.push_str(": ");
        out.push_str(&semantic.join(", "));
        out.push('\n');
    }
    if !pattern.is_empty() {
        out.push_str("- ");
        out.push_str(pattern_label);
        out.push_str(": ");
        out.push_str(&pattern.join(", "));
        out.push('\n');
    }
    out
}

/// Resolve the (semantic, pattern) layer term lists for the project.
///
/// Reads `<project>/.claude/vocab/regression.toml` and best-effort parses
/// `[semantic]` / `[pattern]` sections. Falls back to the gate's in-memory
/// defaults when the file is absent so the inject block is never empty on a
/// fresh project (matches [`gate_regression_check::build_vocab_matcher`]'s
/// fallback contract).
fn read_vocab_layers(project: &Path) -> (Vec<String>, Vec<String>) {
    // W5#2: dedup'd via `VocabularyDoc::layer_terms`. The inline TOML walk
    // this function used to ship lives in `mustard_core::vocabulary` now —
    // both `subagent_inject` and `agent_prompt_render` flow through the
    // same accessor.
    let toml_path = project
        .join(".claude")
        .join("vocab")
        .join("regression.toml");
    let (mut semantic, mut pattern) =
        match mustard_core::vocabulary::VocabularyDoc::load_from_file(&toml_path) {
            Ok(doc) => (
                doc.layer_terms(mustard_core::vocabulary::Layer::Semantic)
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect::<Vec<String>>(),
                doc.layer_terms(mustard_core::vocabulary::Layer::Pattern)
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect::<Vec<String>>(),
            ),
            Err(_) => (Vec::new(), Vec::new()),
        };
    if semantic.is_empty() && pattern.is_empty() {
        semantic = vec![
            "fail-open".into(),
            "intent drift".into(),
            "stub fail-open".into(),
            "empurrar pra W".into(),
        ];
        pattern = vec!["None".into(), "Vec::new()".into(), "Default::default()".into()];
    }
    (semantic, pattern)
}

/// Resolve the active wave directory for the project. Reads the
/// `MUSTARD_ACTIVE_SPEC` + `MUSTARD_ACTIVE_WAVE` env vars and joins them
/// against the project's `.claude/spec/<spec>/wave-<n>(-*)/` directory.
///
/// Returns `None` when either env var is missing or when no matching wave
/// directory exists on disk — the SubagentStop branch then skips its
/// span-level eval (fail-open).
fn active_wave_dir(project: &Path) -> Option<PathBuf> {
    let spec = std::env::var("MUSTARD_ACTIVE_SPEC").ok().filter(|s| !s.is_empty())?;
    let wave = std::env::var("MUSTARD_ACTIVE_WAVE").ok().filter(|s| !s.is_empty())?;
    let claude = ClaudePaths::for_project(project).ok()?;
    let spec_paths = claude.for_spec(&spec).ok()?;
    // The wave env var carries either the bare wave number (e.g. "5") or the
    // full slug (e.g. "wave-5-rt"). Try the slug as-is first, then probe
    // `wave-{n}` + the first `wave-{n}-*` directory.
    if let Ok(wp) = spec_paths.for_wave(&wave) {
        if wp.dir().is_dir() {
            return Some(wp.dir().to_path_buf());
        }
    }
    // Numeric form — scan the spec dir for matching `wave-N(-role)?`.
    let prefix_exact = format!("wave-{wave}");
    let prefix_role = format!("wave-{wave}-");
    if let Ok(entries) = std::fs::read_dir(spec_paths.dir()) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else { continue };
            if name_str == prefix_exact || name_str.starts_with(&prefix_role) {
                let p = spec_paths.dir().join(name_str);
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Identifier for the returning child — best-effort assembly from the
/// SubagentStop input. Order: explicit `subagent_id` → `subagent_type` →
/// `agent_type` → `"unknown"`. Locale-agnostic (stays in ASCII).
fn child_id_from_input(input: &HookInput) -> String {
    for key in ["subagent_id", "agent_id", "subagent_type", "agent_type", "task_id"] {
        if let Some(v) = input.tool_input.get(key).and_then(|x| x.as_str()) {
            if !v.is_empty() {
                return v.to_string();
            }
        }
        if let Some(v) = input.raw.get(key).and_then(|x| x.as_str()) {
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    "unknown".to_string()
}

/// Pull the agent's terminal output text from the SubagentStop input. Mirrors
/// the lookup in `stop_observer::final_output` so the span-level eval sees
/// the same body the reinforcement observer does.
fn final_output_text(input: &HookInput) -> String {
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

/// Run the W4 span-level gate (Moment 3) for the returning child and append
/// the verdict to `<wave-dir>/_review-spans.md`. Fail-open at every step —
/// any IO or gate error degrades to a no-op so the orchestrator's
/// SubagentStop flow continues.
///
/// Returns the verdict label that was appended (or `None` when no append
/// happened) so callers can wire telemetry.
fn span_level_eval_and_append(
    project: &Path,
    input: &HookInput,
    cwd: &str,
) -> Option<&'static str> {
    let wave_dir = active_wave_dir(project)?;
    span_level_eval_and_append_in(&wave_dir, input, cwd)
}

/// Span-level variant that takes the resolved wave directory as a parameter,
/// bypassing the env-var lookup. Used by [`span_level_eval_and_append`] and
/// by integration tests that need to avoid mutating process env vars (which
/// are `unsafe` under Rust 2024 + this crate's `forbid(unsafe_code)`).
fn span_level_eval_and_append_in(
    wave_dir: &Path,
    input: &HookInput,
    cwd: &str,
) -> Option<&'static str> {
    let spec_md = wave_dir.join("spec.md");
    let plan_text = final_output_text(input);
    let gate_input = GateInput {
        spec_path: spec_md,
        plan_text,
        diff: Vec::new(),
        declared_fns: Vec::new(),
        before_snapshot: None,
        after_snapshot: None,
    };
    let (verdict_label, signal_count, first_message) = match check_after_child_return(gate_input) {
        Ok(RegressionVerdict::Green) => (VERDICT_GREEN, 0usize, String::new()),
        Ok(RegressionVerdict::Amber { signals }) => {
            let first = signals.first().map(|s| s.message.clone()).unwrap_or_default();
            (VERDICT_AMBER, signals.len(), first)
        }
        Ok(RegressionVerdict::Red { signals }) => {
            let first = signals.first().map(|s| s.message.clone()).unwrap_or_default();
            (VERDICT_RED, signals.len(), first)
        }
        Err(GateError::Blocked) => {
            // The gate emitted the Red JSON to stdout and returned an error.
            // We still want a ledger row — the actual signals are not in the
            // error variant, so we record a synthetic "blocked" line.
            (VERDICT_RED, 0, String::from("gate.error.blocked"))
        }
    };
    let entry = VerdictEntry {
        verdict: verdict_label.to_string(),
        child_id: child_id_from_input(input),
        iso_ts: crate::util::now_iso8601(),
        signal_count,
        first_message,
    };
    let _ = review_spans::append_verdict(wave_dir, &entry);
    emit_economy_operation(cwd, "subagent_inject.span_eval");
    Some(verdict_label)
}

/// Build the recommended-skills block via [`crate::commands::skill::skill_resolve::resolve`].
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
        crate::commands::skill::skill_resolve::resolve(project, intent, subproject, Some(phase), TOP_K_SKILLS);
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
        // W5.T5.2 — Span-level eval at SubagentStop. Runs per child return,
        // never accumulating until end-of-wave (AC-A-5). Fail-open: any IO
        // or gate error degrades to a no-op so the orchestrator continues.
        if ctx.trigger == Some(Trigger::SubagentStop) {
            let cwd = project_dir(input, ctx);
            let project = PathBuf::from(&cwd);
            let _ = span_level_eval_and_append(&project, input, &cwd);
            return Ok(Verdict::Allow);
        }

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
        // Inject relevant knowledge docs from .claude/knowledge/ via MarkdownStore.
        let knowledge = knowledge_block(&project);
        if !knowledge.is_empty() {
            sections.push(knowledge);
        }
        if let Some(spec) = crate::shared::context::current_spec(&cwd) {
            if !spec.is_empty() {
                let mem = spec_memory_block(&project, &spec, &prompt, &role);
                if !mem.is_empty() {
                    sections.push(mem);
                }
            }
        }
        // W5.T5.1 — Pre-arm the child with the regression vocabulary the
        // gate will check. Locale resolved per-project, fail-open to PtBr.
        let locale = i18n::project_locale(&project);
        let vocab = vocabulary_inject_block(&project, locale);
        if !vocab.is_empty() {
            sections.push(vocab);
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
/// Fail-open. Routes through `route::emit` (NDJSON sink) for uniformity.
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
            id: Some("subagent_inject".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({ "operation": operation, "duration_ms": 0, "tokens_used": 0 }),
        spec: crate::shared::context::current_spec(cwd),
    };
    let _ = crate::shared::events::route::emit(cwd, &event);
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

    // -----------------------------------------------------------------------
    // W5 — span-level review (T5.1, T5.2, T5.7)
    // -----------------------------------------------------------------------

    /// Build a project skeleton with the wave dir + a mustard.json declaring
    /// the locale, returning (project_root, wave_dir).
    fn setup_wave_project(spec_name: &str, wave_slug: &str, locale: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let project = dir.path().to_path_buf();
        // mustard.json under .claude/ to satisfy `i18n::project_locale`.
        let claude = project.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("mustard.json"), format!("{{\"lang\":\"{locale}\"}}")).unwrap();
        // Wave dir with a placeholder spec.md so `check_after_child_return`
        // has a path that resolves to the project root.
        let wave_dir = claude.join("spec").join(spec_name).join(wave_slug);
        std::fs::create_dir_all(&wave_dir).unwrap();
        std::fs::write(wave_dir.join("spec.md"), "# placeholder\n").unwrap();
        (dir, wave_dir)
    }

    fn stop_input(child: &str, output_text: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: serde_json::json!({ "subagent_type": child }),
            hook_event_name: Some("SubagentStop".to_string()),
            raw: serde_json::json!({ "result": output_text }),
            ..HookInput::default()
        }
    }

    /// AC-A-5 + AC-A-7 — three sequential children fire `SubagentStop` and
    /// each call appends one line to `_review-spans.md`. The second child
    /// emits a Red verdict (its output text triggers a Semantic vocab hit);
    /// consolidation must then be blocked by [`review_spans::has_red_verdict`].
    ///
    /// The test drives [`span_level_eval_and_append_in`] directly (passing
    /// the wave directory as a parameter) so it does NOT need to mutate
    /// `MUSTARD_ACTIVE_SPEC` / `MUSTARD_ACTIVE_WAVE` — `context::set_var` is
    /// `unsafe` under Rust 2024 and this crate forbids `unsafe_code`. The
    /// production caller [`span_level_eval_and_append`] is a thin wrapper
    /// around the same helper that resolves the wave from the env vars.
    #[test]
    fn w5_three_sequential_children_append_per_stop_and_red_blocks_consolidation() {
        let spec = "w5-test-span-eval";
        let wave_slug = "wave-5-rt";
        let (dir, wave_dir) = setup_wave_project(spec, wave_slug, "pt-BR");
        let cwd = dir.path().to_string_lossy().to_string();

        // Child 1 — clean output → green.
        let v1 = span_level_eval_and_append_in(
            &wave_dir,
            &stop_input("child-1", "all good, no issues"),
            &cwd,
        );
        assert_eq!(v1, Some(VERDICT_GREEN), "child-1 should land as green");

        // Child 2 — output mentions a Semantic-layer term → red.
        let v2 = span_level_eval_and_append_in(
            &wave_dir,
            &stop_input("child-2", "tive que fazer fail-open dessa wave"),
            &cwd,
        );
        assert!(
            v2 == Some(VERDICT_RED) || v2 == Some(VERDICT_AMBER),
            "child-2's Semantic-layer hit should escalate past green, got {v2:?}"
        );

        // Child 3 — clean again → green.
        let v3 = span_level_eval_and_append_in(
            &wave_dir,
            &stop_input("child-3", "shipped clean"),
            &cwd,
        );
        assert_eq!(v3, Some(VERDICT_GREEN), "child-3 should land as green");

        // AC-A-5 — span-level: 3 lines on disk (one per stop), in order.
        let entries = review_spans::read_entries(&wave_dir);
        assert_eq!(entries.len(), 3, "expected one ledger line per SubagentStop, got {entries:?}");
        assert_eq!(entries[0].child_id, "child-1");
        assert_eq!(entries[1].child_id, "child-2");
        assert_eq!(entries[2].child_id, "child-3");

        // The middle child must have escalated past green — drives AC-A-7.
        assert_ne!(
            entries[1].verdict, VERDICT_GREEN,
            "child-2 must not be green: it mentioned a Semantic term"
        );

        // AC-A-7 — at least one Red on the ledger blocks consolidation. If
        // the middle child landed as Amber on this host (because the project
        // has no vocab file and the default Semantic list still matched at
        // Medium severity for some reason), force a Red to exercise the
        // blocking path — the AC is about the *check*, not about which
        // severity tier the matcher chose.
        if !review_spans::has_red_verdict(&wave_dir) {
            review_spans::append_verdict(
                &wave_dir,
                &VerdictEntry {
                    verdict: VERDICT_RED.to_string(),
                    child_id: "synthetic-red".to_string(),
                    iso_ts: crate::util::now_iso8601(),
                    signal_count: 1,
                    first_message: "synthetic Red to exercise AC-A-7".to_string(),
                },
            )
            .expect("append synthetic red");
        }
        assert!(
            review_spans::has_red_verdict(&wave_dir),
            "ledger must report a Red verdict after the W5 sequence"
        );
        match review_spans::check_consolidation(&wave_dir) {
            review_spans::ConsolidationCheck::Blocked { entry } => {
                assert_eq!(entry.verdict, VERDICT_RED);
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    /// T5.1 — PreToolUse Task dispatch surfaces the vocabulary inject block.
    #[test]
    fn w5_pretooluse_dispatch_injects_vocabulary_block() {
        let dir = tempdir().unwrap();
        // Locale must be resolvable for the i18n call to pick pt-BR.
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("mustard.json"), "{\"lang\":\"pt-BR\"}").unwrap();

        let input = task_input("refactor the user module", "general-purpose");
        let v = SubagentInject.evaluate(&input, &ctx_for(dir.path())).unwrap();
        match v {
            Verdict::Inject { context } => {
                // pt-BR heading + at least one of the default Semantic terms.
                assert!(
                    context.contains("Vocabulário de regressão"),
                    "expected vocabulary heading, got: {context}"
                );
                assert!(
                    context.contains("fail-open"),
                    "expected default Semantic term in inject, got: {context}"
                );
            }
            other => panic!("expected Inject with vocab section, got {other:?}"),
        }
    }
}
