//! `mustard-rt run agent-prompt-render` — materialise the agent dispatch
//! prompt server-side.
//!
//! Replaces the orchestrator-side manual interpolation of `{placeholders}`
//! from the legacy `refs/agent-prompt/agent-prompt.md` template. One process
//! call produces a Task-ready prompt string; stdout = the prompt itself
//! (no JSON framing); stderr = warnings about placeholders that could not be
//! filled (graceful degrade — they are still substituted with an empty string).
//!
//! The template is embedded via [`include_str!`] from
//! `agent_prompt_template.md`, so the binary is self-sufficient (no on-disk
//! template dependency).
//!
//! ## Mode selection
//!
//! - `first` → render the Dispatch Template block (`<!-- TEMPLATE: dispatch -->`).
//! - `granular` / `fix-loop` → render the Minimal Retry Template block
//!   (`<!-- TEMPLATE: retry -->`); `{retry_context}` is read from
//!   `--retry-context-file` when provided, else `""`.

use crate::shared::context::project_dir;
use crate::commands::agent::context_inject;
use crate::commands::pipeline::resume_bootstrap::resolve_operational_spec_path;
use crate::commands::spec::spec_sections::is_heading;
use mustard_core::domain::ast::{extract_entities, extract_function_signatures, GrammarLoader};
use mustard_core::io::fs as mfs;
use mustard_core::ClaudePaths;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Render mode — picks which template block (dispatch vs retry) is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    First,
    Granular,
    FixLoop,
}

impl RenderMode {
    /// Parse the `--mode` CLI flag. Defaults to [`RenderMode::First`].
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "granular" => Self::Granular,
            "fix-loop" | "fix_loop" | "fixloop" => Self::FixLoop,
            _ => Self::First,
        }
    }
}

/// Embedded template — contains the Dispatch + Retry blocks delimited by
/// `<!-- TEMPLATE: dispatch -->` / `<!-- TEMPLATE: retry -->` markers.
const TEMPLATE: &str = include_str!("agent_prompt_template.md");

/// Run `mustard-rt run agent-prompt-render`.
///
/// Fail-open contract: every step degrades to an empty placeholder value with
/// a warning on stderr; the process never panics and always exits 0.
///
/// W8.T8.9 — `--budget-tokens <N>` truncates *content* placeholders (the bulky
/// ones: `{task_steps}`, `{context_md}`, `{prior_wave_diff}`,
/// `{cross_wave_memory}`) to keep the final rendered prompt under roughly `N`
/// model tokens. The estimator is the conventional 4-chars-per-token heuristic;
/// placeholders are trimmed least-relevant-first (head-preserving) so the most
/// useful content stays intact while the long tail gets trimmed first. A `None`
/// budget is the historical full-render path.
pub fn run(
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    retry_context_file: Option<&Path>,
    task_filter: Option<&str>,
    task_text: Option<&str>,
    budget_tokens: Option<usize>,
) {
    let project = PathBuf::from(project_dir());
    let rendered = render_prompt_at(
        &project,
        spec,
        wave,
        role,
        subproject,
        mode,
        retry_context_file,
        task_filter,
        task_text,
        budget_tokens,
    );
    // stdout = prompt string (raw, no JSON framing).
    print!("{rendered}");
}

/// Render the dispatch/retry prompt against an explicit `project` root and
/// return the String instead of printing it — the miolo of [`run`], reused
/// in-process by `wave-advance` (which inlines the rendered prompt per
/// dispatch item instead of handing the orchestrator a `prompt_cmd` to shell).
///
/// Fail-open: a missing template block warns on stderr and yields an empty
/// String (the CLI entry then prints nothing, the historical behaviour).
#[allow(clippy::too_many_arguments)] // mirrors the CLI flag surface 1:1
pub(crate) fn render_prompt_at(
    project: &Path,
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    retry_context_file: Option<&Path>,
    task_filter: Option<&str>,
    task_text: Option<&str>,
    budget_tokens: Option<usize>,
) -> String {
    let project = project.to_path_buf();
    // Spec-less paths (the `/scan` guards enrich, `/task` with no scope) pass no
    // `--spec`. They carry no spec directory, no spec memory, and no spec-derived
    // locale — every spec-keyed step below degrades to a project-root fallback.
    // A blank `--spec ""` is treated the same as absent.
    let spec = spec.map(str::trim).filter(|s| !s.is_empty());
    let spec_dir = spec
        .and_then(|s| {
            ClaudePaths::for_project(&project)
                .and_then(|p| p.for_spec(s))
                .map(|sp| sp.dir().to_path_buf())
                .ok()
        })
        .unwrap_or_else(|| project.clone());
    let op_spec_path = resolve_operational_spec_path(&spec_dir, wave);

    // Pick the right template block by mode.
    let block = match mode {
        RenderMode::First => extract_block(TEMPLATE, "dispatch"),
        RenderMode::Granular | RenderMode::FixLoop => extract_block(TEMPLATE, "retry"),
    };
    let Some(mut rendered) = block else {
        eprintln!("agent-prompt-render: WARN: template block missing — emitting empty prompt");
        return String::new();
    };

    // Capture the placeholder tokens the TEMPLATE itself declares, BEFORE any
    // substitution. The unfilled-scan at the end uses this set so a `{token}`
    // that arrives via substituted spec content (e.g. a literal `{entity}` in
    // the wave's `## Tasks`) is never mistaken for an unfilled template
    // placeholder — author text survives verbatim instead of being stripped.
    let template_tokens: std::collections::HashSet<String> =
        scan_unfilled(&rendered).into_iter().collect();

    // ---- Collect placeholder values (fail-open per field). ----

    let subproject_str = subproject.to_string_lossy().to_string();
    let guards_summary = read_guards_block(&project.join(&subproject_str));
    // With a spec, `meta.json`/`### Lang:` is the source of truth. Without one,
    // there is no spec to read — derive the narrative locale from the canonical
    // `mustard.json#specLang` accessor (`ProjectConfig::load(..).i18n()`), the
    // same accessor `build_role_block` already uses for tone. No ad-hoc parse.
    let spec_lang = match spec {
        Some(_) => read_spec_lang(&op_spec_path),
        None => mustard_core::ProjectConfig::load(&project)
            .i18n()
            .lang
            .as_str()
            .to_string(),
    };
    let role_block = build_role_block(role, &project, &subproject_str, &spec_lang);
    let task_steps = {
        let raw = read_task_steps(&op_spec_path);
        let raw = match task_filter {
            Some(pat) => filter_task_lines(&raw, pat),
            None => raw,
        };
        // Spec-less callers (`/scan` guards enrich, `/task` with no scope) have
        // no spec `## Tasks` to read — `--task-text` carries the ad-hoc work so
        // the prompt stays self-contained and verbatim, instead of the
        // orchestrator hand-appending the task after the render.
        if raw.trim().is_empty() {
            task_text.unwrap_or_default().to_string()
        } else {
            raw
        }
    };
    // Spec-keyed scratch lookups. With no spec there is nothing cached; pass an
    // empty key so each helper resolves to a missing path and fail-opens to "".
    let spec_key = spec.unwrap_or("");
    let context_md = read_cached(&project, spec_key, "context-md");
    let prior_wave_diff = wave
        .filter(|&w| w > 1)
        .map(|w| read_prior_wave_diff(&project, spec_key, w - 1))
        .unwrap_or_default();
    // The spec's mid-pipeline change-log (`## CHANGE REQUESTS`) — bullets only,
    // empty (so the heading collapses) for spec-less renders or a spec with none.
    let change_log = spec.map(|_| read_change_log(&spec_dir)).unwrap_or_default();
    let mut cross_wave_memory = cross_wave_pull_pointer(spec_key, wave);
    // Append per-spec memory principles filtered by relevance. T1.5 requires
    // irrelevant principles to NOT enter the prompt — the shared matcher runs
    // an Aho-Corasick scan over the memory-name stems so morphological variants
    // (prompt "routing" → `tabs-routing.md`) are caught. The intent is the
    // role label plus the task block.
    // Spec memory dir; spec-less paths point at the project root so the matcher
    // simply finds no per-spec memory and skips the block (fail-open).
    let memory_dir = spec
        .and_then(|s| {
            ClaudePaths::for_project(&project)
                .and_then(|p| p.for_spec(s))
                .map(|sp| sp.dir().join("memory"))
                .ok()
        })
        .unwrap_or_else(|| project.clone());
    let mem_intent = format!("{role} {task_steps}");
    let spec_memory_block = context_inject::render_spec_memory_block(
        &context_inject::match_spec_memory(&memory_dir, &mem_intent, usize::MAX, true),
    );
    if !spec_memory_block.is_empty() {
        if !cross_wave_memory.is_empty() {
            cross_wave_memory.push_str("\n\n");
        }
        cross_wave_memory.push_str(&spec_memory_block);
    }
    // W5.T5.3 — inject the regression vocabulary so the child agent sees
    // the same Semantic/Pattern term lists the gate will check at Moment 1.
    // This is an INTERNAL agent prompt, so the regression vocabulary is rendered
    // in EN/technical regardless of the project's user-facing locale — agent and
    // subagent prompts stay EN by policy; only user output, specs and waves
    // honour the project locale.
    let locale = mustard_core::SupportedLocale::EnUs;
    let vocab_block = context_inject::vocabulary_inject_block(&project, locale);
    if !vocab_block.is_empty() {
        if !cross_wave_memory.is_empty() {
            cross_wave_memory.push_str("\n\n");
        }
        cross_wave_memory.push_str(&vocab_block);
    }
    // Skills and the entity list were removed from Mustard: the dispatch
    // template no longer carries `## SKILLS` / `## ENTITY`. The subagent's
    // domain context comes from the inline `## GUARDS` block + the grain spec
    // section + its anchors, not from a resolved skill/entity list.

    // Remaining deterministic placeholders the dispatch template carries:
    //   {reference_files}  the spec's `## Files`/`## Arquivos` list + public
    //                      signatures of those files via tree-sitter
    //   {context_extras}   the per-role slice of `.claude/pipeline-config.md`
    let reference_files = build_reference_files(&project, &subproject_str, &op_spec_path);
    let context_extras = build_context_extras(&project, role);
    let retry_context = match (mode, retry_context_file) {
        (RenderMode::First, _) => String::new(),
        (_, Some(path)) => mfs::read_to_string(path).unwrap_or_default(),
        (_, None) => String::new(),
    };

    // W8.T8.9 — apply the token budget by truncating bulky placeholders. The
    // truncation order is least-to-most relevant; skill-resolve has already
    // ordered the skill list, so we keep its head and drop its tail first.
    let mut task_steps = task_steps;
    let mut context_md = context_md;
    let mut prior_wave_diff = prior_wave_diff;
    if let Some(budget) = budget_tokens {
        apply_budget(
            budget,
            &[
                &subproject_str,
                &guards_summary,
                &role_block,
                &spec_lang,
                &retry_context,
                &reference_files,
                &context_extras,
                &rendered,
            ],
            &mut [
                ("prior_wave_diff", &mut prior_wave_diff),
                ("cross_wave_memory", &mut cross_wave_memory),
                ("context_md", &mut context_md),
                ("task_steps", &mut task_steps),
            ],
        );
    }

    // ---- Substitute placeholders. ----
    let substitutions: &[(&str, &str)] = &[
        ("{subproject}", &subproject_str),
        ("{guards_summary}", &guards_summary),
        ("{role_block}", &role_block),
        ("{spec_lang}", &spec_lang),
        ("{task_steps}", &task_steps),
        ("{context_md}", &context_md),
        ("{prior_wave_diff}", &prior_wave_diff),
        ("{change_log}", &change_log),
        ("{cross_wave_memory}", &cross_wave_memory),
        ("{reference_files}", &reference_files),
        ("{context_extras}", &context_extras),
        ("{retry_context}", &retry_context),
    ];
    for (key, value) in substitutions {
        rendered = rendered.replace(key, value);
    }

    // ---- Drop headings whose fail-open body resolved to empty. ----
    // `## GUARDS`, `## SHARED LANGUAGE`, `## REFERENCE`, `## CROSS-WAVE MEMORY`
    // and `## PRIOR WAVE DIFF` all degrade to "" on the spec-less / wave-1 /
    // no-Files paths; a dangling empty heading is negative signal, so collapse it.
    rendered = collapse_empty_sections(&rendered);

    // ---- Blank only the TEMPLATE placeholders left unfilled (warn on each). ----
    // A `{token}` that came in through substituted spec content is author text,
    // not a render gap — `strip_unfilled_template_tokens` leaves it verbatim.
    let (stripped, unfilled) = strip_unfilled_template_tokens(&rendered, &template_tokens);
    rendered = stripped;
    for token in unfilled {
        eprintln!("agent-prompt-render: WARN: unfilled placeholder {token}");
    }

    rendered
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a `<!-- TEMPLATE: <name> -->` ... `<!-- /TEMPLATE: <name> -->`
/// block body from the embedded template.
fn extract_block(template: &str, name: &str) -> Option<String> {
    let open = format!("<!-- TEMPLATE: {name} -->");
    let close = format!("<!-- /TEMPLATE: {name} -->");
    let start = template.find(&open)? + open.len();
    let end = template[start..].find(&close)? + start;
    let mut body = template[start..end].to_string();
    // Trim a single leading/trailing newline added by the markers. CRLF-aware
    // so the binary behaves identically on Windows and POSIX line endings.
    if body.starts_with("\r\n") {
        body.drain(0..2);
    } else if body.starts_with('\n') {
        body.remove(0);
    }
    if body.ends_with("\r\n") {
        body.pop();
        body.pop();
    } else if body.ends_with('\n') {
        body.pop();
    }
    Some(body)
}

/// Read the `## Guards` section body from a subproject's `CLAUDE.md`. Empty
/// when the file or the section is absent.
fn read_guards_block(subproject_dir: &Path) -> String {
    let text = mfs::read_to_string(subproject_dir.join("CLAUDE.md")).unwrap_or_default();
    if text.is_empty() {
        return String::new();
    }
    let mut in_section = false;
    let mut collected = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("## ") {
            if in_section {
                break; // Next `## ` ends the section.
            }
            let after = line.trim_start().trim_start_matches('#').trim();
            if after.eq_ignore_ascii_case("Guards") {
                in_section = true;
                continue;
            }
        }
        if in_section {
            collected.push_str(line);
            collected.push('\n');
        }
    }
    collected.trim().to_string()
}

/// Build the `{role_block}` — the role cue **plus a per-role delivery contract**.
/// Each known role (`guards`, `explore`, `review`, `qa`, and the `impl` default)
/// gets an explicit contract: what to produce and how to deliver it (return text
/// vs. edit, the return-line cap, read-only vs. write). This is what makes the
/// rendered prompt self-restricting — the orchestrator no longer hand-appends the
/// contract, and a read-only role is told (and, via its `subagent_type`, unable)
/// to write. See [`recommended_subagent_type`] for the matching tool-restricted
/// agent per role.
fn build_role_block(role: &str, project: &Path, subproject: &str, spec_lang: &str) -> String {
    match role.trim().to_ascii_lowercase().as_str() {
        "guards" => build_guards_role_block(project, subproject, spec_lang),
        "explore" => format!(
            "ROLE: explore\n\
             You map a slice of {subproject} read-only and return a compact briefing. You \
             write NOTHING — if the task implies a change, report it, do not do it. Start from \
             the anchors you were given; follow import/render chains into child files when the \
             question is about composed behavior (an anchor alone does not show what its \
             children render); never bulk-read. Settle existence/duplication questions by Grep \
             enumeration over the slice FIRST — reading samples never proves absence. Ground \
             every claim in file:line. NEVER assert \"X does not exist\" and never refute a \
             symptom the user observed at runtime — static reading cannot disprove it; say \
             \"not found in the files I read\" instead. Deliver: your final message is a \
             ≤30-line briefing — the pattern to mirror, files to touch, contract wiring — plus \
             a coverage footer (files read / chains not followed), exempt from the cap. No \
             file dumps."
        ),
        "review" => format!(
            "ROLE: review\n\
             You adversarially verify the implementer's work in {subproject}. You are NOT the \
             implementer. Read-only: report findings, never fix. Stay skeptical — the implementer \
             is not authoritative; if you cannot independently confirm a claim, reject it. Run \
             tests with the feature enabled (code presence is not effectiveness). If the prompt \
             carries a `## CHANGE REQUESTS` section, confirm EACH mid-pipeline request was \
             addressed in the code AND is covered by an Acceptance Criterion — flag any that was \
             silently dropped. Deliver: your \
             final message is a ≤60-line verdict — pass/fail per claim, each backed by the command \
             you ran and its real output."
        ),
        "qa" => format!(
            "ROLE: qa\n\
             You run each Acceptance Criterion command in the spec and report pass/fail. You do \
             NOT fix anything. Run the exact `Command:` from each AC and capture its real output. \
             Deliver: per-AC pass/fail + the proving output; overall=pass only if every AC passes."
        ),
        _ => format!(
            "ROLE: {role}\n\
             You implement inside {subproject} ONLY — never touch another subproject, the spec, or \
             .claude/. Before the first Edit/Write, read ONE sibling file to match conventions. \
             Source code stays English; only spec prose follows the project locale. Max 3 build \
             attempts, then STOP and report. Deliver: your final message is a ≤40-line report — \
             files changed + non-obvious decisions + blockers. Do NOT paste file contents."
        ),
    }
}

/// Build the `guards` role block — the Wave-2 enrich instruction. Carries the
/// grounded 3-6 line cap, the project locale + tone (from `mustard.json` via the
/// canonical [`mustard_core::ProjectConfig`] accessor — no ad-hoc parse), the
/// pending block's deterministic facts, and the delivery contract (return the
/// lines as text; never write a file — the caller pipes to `scan-guards-apply`).
fn build_guards_role_block(project: &Path, subproject: &str, spec_lang: &str) -> String {
    let tone = mustard_core::ProjectConfig::load(project).i18n().tone.as_str().to_string();
    let facts = read_guards_facts(&project.join(subproject));
    let facts_line = if facts.is_empty() {
        String::new()
    } else {
        format!("\nFacts (deterministic, from scan): {facts}.")
    };
    format!(
        "ROLE: guards\n\
         Write 3-6 lines of Guards (do/don't) GROUNDED in the deterministic facts \
         and the subproject's real code; include ONLY what is NOT auto-inferable \
         from the manifest/tree. Write in the project locale ({spec_lang}) and tone \
         ({tone}). Be concise; never generic prose. Deliver ONLY the lines as your \
         final message; do NOT write any file — the caller pipes your text to \
         scan-guards-apply.{facts_line}"
    )
}

/// Read the `<!-- facts: ... -->` payload from a subproject's pending `## Guards`
/// block (Wave 1's grounding context: `kind=...; frameworks=...`). Empty when
/// the file or the facts line is absent. Shape-mirrors [`read_guards_block`].
fn read_guards_facts(subproject_dir: &Path) -> String {
    let text = mfs::read_to_string(subproject_dir.join("CLAUDE.md")).unwrap_or_default();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<!-- facts:") {
            return rest.trim_end_matches("-->").trim().to_string();
        }
    }
    String::new()
}

/// Resolve the spec's narrative locale. Defaults to `"en-US"` (BCP-47).
///
/// Resolution — **`meta.json` is the single source of truth**:
/// 1. `meta.json#lang` beside the spec.
/// 2. Legacy fallback: the `### Lang:` header in `spec.md` (first 30 lines)
///    for un-migrated specs.
///
/// Legacy short codes (`pt` / `en`) are tolerated on read and returned
/// verbatim — `mustard_core::SupportedLocale::from_str` is the canonical parser
/// for downstream consumers.
fn read_spec_lang(spec_path: &Path) -> String {
    if let Some(m) = mustard_core::domain::meta::read_meta_beside(spec_path) {
        if let Some(lang) = m.lang.filter(|s| !s.is_empty()) {
            return lang;
        }
    }
    // Legacy fallback: the `### Lang:` header in the markdown.
    let text = mfs::read_to_string(spec_path).unwrap_or_default();
    for line in text.lines().take(30) {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("### ") else {
            continue;
        };
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let key = rest[..colon].trim();
        if key.eq_ignore_ascii_case("lang") {
            let val = rest[colon + 1..].trim();
            if !val.is_empty() {
                return val.to_string();
            }
        }
    }
    "en-US".to_string()
}

/// Cut the `## Tarefas` / `## Tasks` section from a spec file. Empty when
/// neither heading exists.
///
/// Lean bugfix/Light specs carry no `## Tasks` checklist — their work lives in
/// `## Causa raiz` / `## Plano`. When the structured section is missing or has
/// no body, fall back to [`build_task_fallback`] so the dispatched agent still
/// receives a non-empty TASK block (root cause + plan, or — when those are
/// absent too — the spec's Context + Acceptance Criteria sections under an
/// origin header, plus a read-the-spec cue) instead of a blank one. Full specs
/// are unaffected: a present, non-empty `## Tasks` section is always preferred
/// and returned byte-identical.
pub(crate) fn read_task_steps(spec_path: &Path) -> String {
    let text = mfs::read_to_string(spec_path).unwrap_or_default();
    if text.is_empty() {
        return String::new();
    }
    let structured = cut_tasks_section(&text);
    if !structured.is_empty() {
        return structured;
    }
    build_task_fallback(&text, spec_path)
}

/// Extract the `## Tasks` / `## Tarefas` / `## Checklist` section body (heading
/// included). Empty when the heading is absent or carries no content lines.
fn cut_tasks_section(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|l| is_heading(l, "tasks")) else {
        return String::new();
    };
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("## ") {
            end = i;
            break;
        }
    }
    // Require at least one non-blank body line under the heading; an empty
    // `## Tasks` heading must trigger the fallback, not emit a bare heading.
    let has_body = lines[start + 1..end].iter().any(|l| !l.trim().is_empty());
    if !has_body {
        return String::new();
    }
    lines[start..end].join("\n").trim_end().to_string()
}

/// Build a TASK block from the spec body when no structured `## Tasks` section
/// exists. Tier 1: the `## Causa raiz` / `## Root cause` section (when
/// present) plus the `## Plano` / `## Plan` section. Tier 2 (when tier 1 finds
/// nothing — the tactical-fix / drafted-spec shape): the spec's `## Context` /
/// `## Contexto` + `## Acceptance Criteria` / `## Critérios de Aceitação`
/// sections (canonical `is_heading` keys), prefixed with a header naming the
/// origin so the agent knows it is reading narrative, not a checklist. Both
/// tiers append an explicit instruction to read the full spec before editing.
/// Empty only when no narrative section is present at all (the renderer then
/// degrades to a blank TASK as before).
fn build_task_fallback(text: &str, spec_path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(body) = cut_section_by_display(text, &["Root cause", "Causa raiz"]) {
        parts.push(body);
    }
    if let Some(body) = cut_section_by_display(text, &["Plan", "Plano"]) {
        parts.push(body);
    }
    if parts.is_empty() {
        let mut tier2: Vec<String> = Vec::new();
        if let Some(body) = cut_section_by_key(text, "context") {
            tier2.push(body);
        }
        if let Some(body) = cut_section_by_key(text, "acceptance-criteria") {
            tier2.push(body);
        }
        if !tier2.is_empty() {
            parts.push(
                "> TASK fallback: the spec has no `## Tasks` section — the content below \
                 is its Context + Acceptance Criteria sections, verbatim."
                    .to_string(),
            );
            parts.append(&mut tier2);
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    parts.push(format!(
        "Read the full spec at {} before editing.",
        spec_path.display()
    ));
    parts.join("\n\n")
}

/// Cut a `## <name>` section body (heading included) by literal display name,
/// case-insensitively, matching any of `names`. Used for narrative-divider
/// headings (`## Plan`/`## Plano`) that are intentionally absent from the
/// canonical `SECTIONS` table, so `is_heading` does not resolve them. Returns
/// `None` when the heading is absent or carries no body content.
fn cut_section_by_display(text: &str, names: &[&str]) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| {
        let Some(rest) = l.strip_prefix("##") else {
            return false;
        };
        let after = rest.trim_start_matches([' ', '\t']);
        if after.len() == rest.len() {
            return false; // `## ` requires whitespace after the hashes.
        }
        names.iter().any(|n| after.trim_end().eq_ignore_ascii_case(n))
    })?;
    cut_section_at(&lines, start)
}

/// Cut a `## <key>` section body (heading included) by canonical section key
/// via [`is_heading`] — i18n-aware, so `context` matches both `## Context` and
/// `## Contexto`, and `acceptance-criteria` matches `## Acceptance Criteria`
/// and `## Critérios de Aceitação`. Returns `None` when the heading is absent
/// or carries no body content.
fn cut_section_by_key(text: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| is_heading(l, key))?;
    cut_section_at(&lines, start)
}

/// The section slice starting at `start` (the heading line, inclusive) through
/// the line before the next `## ` heading or EOF. `None` when the body is
/// entirely blank (an empty heading must not survive into the TASK block).
fn cut_section_at(lines: &[&str], start: usize) -> Option<String> {
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("## ") {
            end = i;
            break;
        }
    }
    let has_body = lines[start + 1..end].iter().any(|l| !l.trim().is_empty());
    if !has_body {
        return None;
    }
    Some(lines[start..end].join("\n").trim_end().to_string())
}

/// Filter the lines of a task block by a regex-style pattern.
///
/// The heading line (e.g. `## Tarefas`) is always kept. Every subsequent
/// top-level bullet is kept only when its content matches `pattern`.
/// Sub-bullets / blank continuation lines follow the parent bullet's fate.
///
/// Pattern support: literal characters + `\\.` escape + `(a|b|c)` alternation.
/// This covers the common `T0\\.(1|5)` dispatch-slicing use case without
/// pulling in a full regex crate. Patterns that cannot be parsed warn on
/// stderr and leave the block unfiltered.
fn filter_task_lines(raw: &str, pattern: &str) -> String {
    // Expand the pattern into one or more literal alternatives so that
    // `T0\.(1|5)` becomes ["T0.1", "T0.5"].
    let alternatives = expand_pattern(pattern);

    let mut out: Vec<&str> = Vec::new();
    let mut keep_continuation = false;
    for line in raw.lines() {
        // Section headings are always kept.
        if line.starts_with("## ") || line.starts_with("# ") {
            out.push(line);
            keep_continuation = false;
            continue;
        }
        // Top-level bullet (not indented).
        if line.starts_with("- ") {
            // Strip `- [ ] ` / `- [x] ` / `- ` prefix to reach the content.
            let content = line
                .trim_start_matches('-')
                .trim_start()
                .trim_start_matches(['[', 'x', ' ', ']'])
                .trim_start();
            keep_continuation = alternatives.iter().any(|alt| content.contains(alt.as_str()));
            if keep_continuation {
                out.push(line);
            }
        } else {
            // Blank lines and continuation/sub-bullet lines follow parent.
            if keep_continuation {
                out.push(line);
            }
        }
    }
    out.join("\n")
}

/// Expand a simplified pattern into a set of literal strings to match against.
///
/// Rules applied in order:
/// 1. `\\.` → literal `.` (unescape).
/// 2. `(a|b|c)` → cross-product with the prefix/suffix around the group.
/// 3. All other characters are kept as-is.
///
/// If the pattern contains unsupported constructs (nested groups, `*`, `+`,
/// `?`, `^`, `$`, character classes `[...]`), the function logs a warning and
/// returns the raw pattern as a single alternative (substring match fallback).
fn expand_pattern(pattern: &str) -> Vec<String> {
    // Detect unsupported constructs (anything beyond `\.` and `(a|b)`).
    let unsupported = pattern
        .chars()
        .any(|c| matches!(c, '*' | '+' | '?' | '^' | '$' | '[' | ']'));
    if unsupported {
        eprintln!(
            "agent-prompt-render: WARN: --task-filter pattern '{pattern}' \
             contains unsupported regex construct — using as literal substring"
        );
        return vec![pattern.to_string()];
    }

    // Unescape `\.` → `.` first, then expand one `(a|b|c)` group if present.
    let unescaped = pattern.replace("\\.", ".");
    match unescaped.find('(') {
        None => vec![unescaped],
        Some(open) => {
            let close = unescaped[open..].find(')').map(|i| open + i);
            let Some(close) = close else {
                eprintln!(
                    "agent-prompt-render: WARN: --task-filter pattern '{pattern}' \
                     has unmatched '(' — using as literal substring"
                );
                return vec![unescaped];
            };
            let prefix = &unescaped[..open];
            let suffix = &unescaped[close + 1..];
            let inner = &unescaped[open + 1..close];
            inner
                .split('|')
                .map(|alt| format!("{prefix}{alt}{suffix}"))
                .collect()
        }
    }
}

/// Read a cached `.claude/.pipeline-states/{spec}.{name}.md` file. Empty on
/// any IO error. Retained as the lookup for `context-md` and other legacy
/// per-spec scratch files; per-wave `diff.md` now lives under
/// `spec/{spec}/wave-N-{role}/diff.md` and goes through
/// [`read_prior_wave_diff`].
fn read_cached(project: &Path, spec: &str, name: &str) -> String {
    let path = ClaudePaths::for_project(project)
        .map(|p| p.pipeline_states_dir().join(format!("{spec}.{name}.md")))
        .unwrap_or_else(|_| project.join(format!("{spec}.{name}.md")));
    mfs::read_to_string(&path).unwrap_or_default()
}

/// Read the diff captured by wave `wave_num` (per the W2 path catalog: the file
/// at `<root>/.claude/spec/{spec}/wave-{n}-{role}/diff.md`). The role suffix
/// is unknown a priori, so the first matching directory wins.
///
/// Empty on any IO error or when the spec directory does not exist.
fn read_prior_wave_diff(project: &Path, spec: &str, wave_num: u32) -> String {
    let Ok(sp) = ClaudePaths::for_project(project).and_then(|p| p.for_spec(spec))
    else {
        return String::new();
    };
    // Probe both `wave-{n}` and `wave-{n}-*` variants. `for_wave` validates
    // the slug so the malformed inputs early-out.
    let Ok(read) = mfs::read_dir(sp.dir()) else {
        return String::new();
    };
    let prefix = format!("wave-{wave_num}");
    for entry in read {
        let name_str = entry.file_name.as_str();
        let matches = name_str == prefix
            || name_str.starts_with(&format!("{prefix}-"));
        if !matches {
            continue;
        }
        if let Ok(wp) = sp.for_wave(name_str) {
            if let Ok(text) = mfs::read_to_string(wp.diff_md_path()) {
                if !text.is_empty() {
                    return text;
                }
            }
        }
    }
    String::new()
}

/// On-demand POINTER to prior-wave memory — replaces inlining the full summary
/// dump. The agent pulls earlier-wave context ONLY when its task needs it (via
/// `mustard-rt run memory cross-wave`), so a wave-N+1 dispatch pays ~zero tokens
/// for memory it never reads instead of carrying every prior summary. Empty for
/// wave 1 / spec-less (no prior waves to pull). The full summaries still exist in
/// `<spec>/.events`; this just stops force-feeding them into every prompt.
fn cross_wave_pull_pointer(spec: &str, wave: Option<u32>) -> String {
    let Some(w) = wave else {
        return String::new();
    };
    if spec.is_empty() || w <= 1 {
        return String::new();
    }
    format!(
        "Earlier waves (1..{prev}) of `{spec}` captured per-agent summaries. They are NOT \
         inlined here — pull ONLY what you need, on demand:\n\
         `rtk mustard-rt run memory cross-wave --spec {spec} --wave {w}`\n\
         Skip that call unless your task depends on what an earlier wave did.",
        prev = w - 1
    )
}

// ---------------------------------------------------------------------------
// F3-b — deterministic placeholder fillers
// ---------------------------------------------------------------------------

/// Build `{reference_files}` — the spec's `## Files` / `## Arquivos` list plus
/// a compact structural summary (public signatures + declared entities) of the
/// listed files via tree-sitter, never a file dump.
///
/// The `## Files` section drives the list; each path that resolves under the
/// subproject is parsed once through `mustard_core::domain::ast` (AST when a
/// grammar resolves, agnostic fallback otherwise) and reduced to its public
/// function names + entity names. Empty when the spec has no Files section.
fn build_reference_files(project: &Path, subproject: &str, spec_path: &Path) -> String {
    let spec_text = mfs::read_to_string(spec_path).unwrap_or_default();
    if spec_text.is_empty() {
        return String::new();
    }
    let files = files_section_paths(&spec_text);
    if files.is_empty() {
        return String::new();
    }
    let sub_root = project.join(subproject);
    // One shared grammar loader for every file (built once, with builtins so the
    // AST path is available for the common languages; the fallback floor covers
    // everything else). Anchored at the subproject so on-disk grammar overrides
    // resolve.
    let loader = GrammarLoader::with_builtins(&sub_root);

    let mut out = String::from("## Files\n");
    for rel in files.iter().take(20) {
        let _ = writeln!(out, "- `{rel}`");
        let abs = sub_root.join(rel);
        let abs = if abs.is_file() { abs } else { project.join(rel) };
        if !abs.is_file() {
            continue;
        }
        let Ok(source) = mfs::read_to_string(&abs) else {
            continue;
        };
        let lang_id = loader.language_id_for_path(&abs).unwrap_or_default();
        let summary = structural_summary(&loader, &source, &lang_id);
        if !summary.is_empty() {
            let _ = writeln!(out, "  - {summary}");
        }
    }
    out.trim_end().to_string()
}

/// Compact structural summary of one source file: up to a few public function
/// names and declared entity names. Returns `""` when nothing is extracted so
/// the caller omits the sub-bullet.
fn structural_summary(loader: &GrammarLoader, source: &str, lang_id: &str) -> String {
    let mut fns: Vec<String> = extract_function_signatures(loader, source, lang_id)
        .into_iter()
        .map(|s| s.name)
        .collect();
    fns.dedup();
    fns.truncate(6);
    let mut ents: Vec<String> = extract_entities(loader, source, lang_id)
        .into_iter()
        .map(|e| e.name)
        .collect();
    ents.dedup();
    ents.truncate(6);

    let mut parts: Vec<String> = Vec::new();
    if !ents.is_empty() {
        parts.push(format!("types: {}", ents.join(", ")));
    }
    if !fns.is_empty() {
        parts.push(format!("fns: {}", fns.join(", ")));
    }
    parts.join(" | ")
}

/// Extract the file paths listed under a spec's `## Files` / `## Arquivos`
/// section. Each line's first backtick-quoted token (or, failing that, the
/// first path-ish token) is taken as the path. Stops at the next `## ` heading.
pub(crate) fn files_section_paths(spec_text: &str) -> Vec<String> {
    let lines: Vec<&str> = spec_text.lines().collect();
    let Some(start) = lines.iter().position(|l| is_heading(l, "files")) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for line in lines.iter().skip(start + 1) {
        if line.starts_with("## ") {
            break;
        }
        if let Some(path) = first_path_token(line) {
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// First path-like token in a `## Files` bullet: the content of the first
/// backtick pair when present, else the first whitespace-delimited token that
/// looks like a path (contains `/` or a dotted extension).
fn first_path_token(line: &str) -> Option<String> {
    if let Some(open) = line.find('`') {
        if let Some(close_rel) = line[open + 1..].find('`') {
            let inner = line[open + 1..open + 1 + close_rel].trim();
            if !inner.is_empty() {
                return Some(inner.replace('\\', "/"));
            }
        }
    }
    let stripped = line
        .trim_start()
        .trim_start_matches(['-', '*', ' '])
        .trim_start_matches(['[', 'x', ' ', ']'])
        .trim_start();
    let first = stripped.split_whitespace().next()?;
    let looks_pathy = first.contains('/')
        || first
            .rsplit_once('.')
            .is_some_and(|(_, ext)| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()));
    if looks_pathy {
        Some(first.trim_matches(['(', ')', ',']).replace('\\', "/"))
    } else {
        None
    }
}

/// Build `{context_extras}` — the per-role slice of `.claude/pipeline-config.md`.
///
/// Scans `pipeline-config.md` for a `## ` / `### ` heading that names the role
/// (case-insensitive substring, e.g. role `review` matches `## Review Rules`)
/// and returns that section's body up to the next same-or-higher heading.
/// Reuses the same heading-scan shape as [`read_guards_block`]. Empty when the
/// file or a role-specific section is absent (fail-open).
fn build_context_extras(project: &Path, role: &str) -> String {
    let cfg = ClaudePaths::for_project(project)
        .map(|p| p.claude_dir().join("pipeline-config.md"))
        .unwrap_or_else(|_| project.join(".claude").join("pipeline-config.md"));
    let text = mfs::read_to_string(&cfg).unwrap_or_default();
    if text.is_empty() {
        return String::new();
    }
    let role_lc = role.trim().to_ascii_lowercase();
    if role_lc.is_empty() {
        return String::new();
    }
    let mut in_section = false;
    let mut collected = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            if in_section {
                break; // Next heading ends the role slice.
            }
            let heading = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
            if heading.contains(&role_lc) {
                in_section = true;
                collected.push_str(line);
                collected.push('\n');
                continue;
            }
        }
        if in_section {
            collected.push_str(line);
            collected.push('\n');
        }
    }
    collected.trim().to_string()
}

/// Conventional 4-chars-per-token heuristic. Used by [`apply_budget`].
#[must_use]
fn chars_to_tokens(n_chars: usize) -> usize {
    n_chars.div_ceil(4)
}

/// Truncate the bulky placeholder values in `mutable` so the combined token
/// estimate (fixed + mutable + template overhead) stays at or below `budget`.
///
/// `mutable` is ordered least-to-most-relevant: the *first* entry is trimmed
/// first. Each placeholder is trimmed to a per-slot quota; the quota is the
/// remaining budget divided across the surviving slots, but never grows the
/// content (so a small placeholder is left untouched). Trimming preserves the
/// leading content (head-truncation), which keeps skill-resolve's top picks.
///
/// The function emits a single stderr line summarising what was trimmed, so
/// the AC smoke test can confirm the budget had an observable effect.
fn apply_budget(budget: usize, fixed: &[&String], mutable: &mut [(&str, &mut String)]) {
    // 1. Estimate the fixed cost (in tokens) we cannot trim.
    let fixed_tokens: usize = fixed
        .iter()
        .map(|s| chars_to_tokens(s.chars().count()))
        .sum();
    // 2. Reserve ~10% for template scaffolding (markers, headings, etc).
    let reserve = (budget / 10).max(64);
    let remaining = budget.saturating_sub(fixed_tokens + reserve);

    // 3. Trim from the head of the list (least relevant) until we fit.
    let current_total: usize = mutable
        .iter()
        .map(|(_, v)| chars_to_tokens(v.chars().count()))
        .sum();
    if fixed_tokens + reserve + current_total <= budget {
        return; // Already under budget.
    }

    // Per-slot quota: split the remaining budget evenly across mutable slots.
    let per_slot = if mutable.is_empty() {
        remaining
    } else {
        remaining / mutable.len().max(1)
    };
    let mut trimmed_summary: Vec<String> = Vec::new();
    for entry in mutable.iter_mut() {
        let name = entry.0;
        let value: &mut String = entry.1;
        let cur_tokens = chars_to_tokens(value.chars().count());
        if cur_tokens <= per_slot {
            continue;
        }
        // Keep `per_slot` tokens of head content (≈ per_slot*4 chars).
        let keep_chars = per_slot.saturating_mul(4);
        let original_tokens = cur_tokens;
        let trimmed: String = value.chars().take(keep_chars).collect();
        let new_value = format!("{trimmed}\n…[truncated for token budget]");
        *value = new_value;
        let new_tokens = chars_to_tokens(value.chars().count());
        trimmed_summary.push(format!(
            "{name}:{original_tokens}->{new_tokens}"
        ));
    }
    if !trimmed_summary.is_empty() {
        eprintln!(
            "agent-prompt-render: budget {budget}tok ({fixed_tokens}fixed+{reserve}reserve) — trimmed [{}]",
            trimmed_summary.join(", ")
        );
    }
}

/// Find unfilled `{placeholder}` tokens (lowercase + underscore identifiers).
/// Returns each token once, in the order encountered.
fn scan_unfilled(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Find closing `}` without whitespace inside (placeholders never
            // contain whitespace — code-fence blocks like `{ foo }` are ignored).
            let mut j = i + 1;
            let mut all_id = true;
            while j < bytes.len() && bytes[j] != b'}' {
                let c = bytes[j];
                if !(c.is_ascii_lowercase() || c == b'_' || c.is_ascii_digit()) {
                    all_id = false;
                    break;
                }
                j += 1;
            }
            if all_id && j < bytes.len() && j > i + 1 {
                let token = &text[i..=j];
                let owned = token.to_string();
                if !out.contains(&owned) {
                    out.push(owned);
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Blank every **template** placeholder left unfilled after substitution, and
/// return the list that was blanked (for the WARN log).
///
/// `template_tokens` is the set of `{token}`s present in the *original* template
/// block, captured before substitution. A `{token}` in `rendered` that is NOT in
/// that set arrived through substituted spec content — e.g. a literal `{entity}`
/// an author wrote in the wave's `## Tasks` line. That is the author's text, not
/// a render gap: it is left verbatim, never warned-on and never stripped (the
/// old behaviour stripped *any* `{token}`, silently corrupting the task body and
/// emitting a spurious `unfilled placeholder {entity}` warning).
fn strip_unfilled_template_tokens(
    rendered: &str,
    template_tokens: &std::collections::HashSet<String>,
) -> (String, Vec<String>) {
    let mut out = rendered.to_string();
    let mut unfilled = Vec::new();
    for token in scan_unfilled(rendered) {
        if template_tokens.contains(&token) {
            out = out.replace(&token, "");
            unfilled.push(token);
        }
    }
    (out, unfilled)
}

/// Map a pipeline role to the `subagent_type` the orchestrator should dispatch.
///
/// Read-only roles resolve to **tool-restricted** agents so they physically
/// cannot write: `explore` → the built-in `Explore` (no Edit/Write), `plan` →
/// the built-in `Plan` (no Edit/Write), `review`/`qa` → `mustard-review`
/// (Read/Grep/Glob/Bash — Bash for tests only), `guards` → `mustard-guards`
/// (Read/Grep/Glob only). Writing roles (`impl` and any other) stay
/// `general-purpose`: they need Edit/Write and rely on the per-role contract +
/// the `scope_guard` hook instead. Emitted by `dispatch-plan` so the
/// orchestrator never picks the agent by hand.
#[must_use]
pub fn recommended_subagent_type(role: &str) -> &'static str {
    match role.trim().to_ascii_lowercase().as_str() {
        "explore" => "Explore",
        "plan" => "Plan",
        "review" | "qa" => "mustard-review",
        "guards" => "mustard-guards",
        _ => "general-purpose",
    }
}

/// Remove any `## ` heading whose body — every line until the next `## ` heading
/// or end of text — is entirely whitespace. Keeps the dispatched prompt clean
/// when a fail-open placeholder (`{guards_summary}`, `{context_md}`,
/// `{reference_files}`, `{cross_wave_memory}`, `{prior_wave_diff}`) resolves to
/// "". Only `## `-level headings are considered, so the `<!-- PREFIX-STABLE -->`
/// marker and inline prose are never touched. The `## TASK` section always
/// survives: its trailing "Guards carregados …" line is non-blank body.
/// Read the spec's `change-log.md` (mid-pipeline requests) for the prompt's
/// `## CHANGE REQUESTS` section. Keeps only the request bullets (drops the
/// title + explanatory blurb). Fail-open: a missing/unreadable file, or one with
/// no bullets, yields an empty string — which collapses the heading.
fn read_change_log(spec_dir: &Path) -> String {
    mfs::read_to_string(&spec_dir.join("change-log.md"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.trim_start().starts_with("- "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collapse_empty_sections(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("## ") {
            let mut j = i + 1;
            while j < lines.len() && !lines[j].starts_with("## ") {
                j += 1;
            }
            if lines[i + 1..j].iter().all(|l| l.trim().is_empty()) {
                i = j; // Drop the heading and its blank body.
                continue;
            }
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn extract_block_returns_dispatch_body() {
        let body = extract_block(TEMPLATE, "dispatch").expect("dispatch block present");
        assert!(body.starts_with("<!-- PREFIX-STABLE -->"));
        assert!(body.contains("{task_steps}"));
    }

    #[test]
    fn extract_block_returns_retry_body() {
        let body = extract_block(TEMPLATE, "retry").expect("retry block present");
        assert!(body.starts_with("<!-- VARIABLE -->"));
        assert!(body.contains("{retry_context}"));
    }

    #[test]
    fn extract_block_missing_returns_none() {
        assert!(extract_block(TEMPLATE, "nope").is_none());
    }

    #[test]
    fn scan_unfilled_finds_typed_tokens() {
        let text = "hello {foo} and {bar_baz} {already} { skip } not_a_placeholder";
        let tokens = scan_unfilled(text);
        assert_eq!(tokens, vec!["{foo}", "{bar_baz}", "{already}"]);
    }

    #[test]
    fn scan_unfilled_ignores_whitespace_braces() {
        // Code-fence-style `{ ... }` blocks (with whitespace) are not placeholders.
        let text = "fn f() { let x = 1; }";
        assert!(scan_unfilled(text).is_empty());
    }

    /// Regression (#4): only placeholders the TEMPLATE declared are stripped /
    /// warned. A `{entity}` that arrived via substituted spec content (e.g. a
    /// literal `{entity}` in the wave's `## Tasks`) must survive verbatim — the
    /// old code stripped any `{token}`, corrupting the task body and emitting a
    /// spurious `unfilled placeholder {entity}` warning.
    #[test]
    fn strip_unfilled_only_touches_template_tokens() {
        let template_tokens: std::collections::HashSet<String> =
            ["{foo}".to_string()].into_iter().collect();
        // `{foo}` is a genuine unfilled template placeholder → stripped + warned.
        // `{entity}` is author content from the task body → left verbatim.
        let rendered = "task: implement {entity} now\nleftover {foo} here";
        let (out, unfilled) = strip_unfilled_template_tokens(rendered, &template_tokens);
        assert!(out.contains("{entity}"), "author token must survive: {out}");
        assert!(!out.contains("{foo}"), "unfilled template token must be stripped: {out}");
        assert_eq!(unfilled, vec!["{foo}".to_string()]);
    }

    /// `read_change_log` keeps only the request bullets (item #2 — review inject).
    #[test]
    fn read_change_log_keeps_only_bullets() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("change-log.md"),
            "# Change Log — feat\n\n_blurb explicativo_\n\n- **ts1** _(Execute)_ — muda X\n\
             - **ts2** — muda Y\n",
        )
        .unwrap();
        let out = read_change_log(dir.path());
        assert!(out.contains("muda X") && out.contains("muda Y"), "bullets: {out}");
        assert!(!out.contains("blurb"), "blurb dropped: {out}");
        assert!(!out.contains("# Change Log"), "title dropped: {out}");
        // No file → empty (the heading collapses).
        assert!(read_change_log(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn read_guards_block_extracts_section() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("CLAUDE.md"),
            "# Title\n\n## What\n- foo\n\n## Guards\n- rule A\n- rule B\n\n## Stack\nrust\n",
        )
        .unwrap();
        let guards = read_guards_block(dir.path());
        assert!(guards.contains("rule A"));
        assert!(guards.contains("rule B"));
        assert!(!guards.contains("Stack"));
    }

    #[test]
    fn build_role_block_emits_role_cue_and_contract() {
        // Every role block starts with its `ROLE:` cue and now carries a
        // delivery contract (no longer a bare marker). The read-only roles state
        // their write-restriction in prose; their subagent_type enforces it.
        let dir = tempdir().unwrap();
        let impl_block = build_role_block("impl", dir.path(), "api", "en-US");
        assert!(impl_block.starts_with("ROLE: impl"), "cue missing: {impl_block}");
        assert!(impl_block.contains("implement inside api ONLY"), "scope missing: {impl_block}");
        let review_block = build_role_block("review", dir.path(), "api", "en-US");
        assert!(review_block.starts_with("ROLE: review"));
        assert!(review_block.contains("never fix"), "review write-restriction missing");
        let explore_block = build_role_block("explore", dir.path(), "api", "en-US");
        assert!(explore_block.starts_with("ROLE: explore"));
        assert!(explore_block.contains("write NOTHING"), "explore write-restriction missing");
    }

    #[test]
    fn explore_role_block_carries_epistemic_contract() {
        // Field defect: an Explore read sliced anchors and confidently returned
        // "no duplication" — refuting a symptom the user had SEEN rendered (the
        // second <h1> lived in a child component, invisible to sliced anchor
        // reads). The contract must route existence questions to Grep
        // enumeration, demand file:line evidence, forbid unqualified negative
        // verdicts, and keep the coverage footer outside the return cap.
        let dir = tempdir().unwrap();
        let block = build_role_block("explore", dir.path(), "api", "en-US");
        assert!(block.contains("never proves absence"), "grep-first rule missing: {block}");
        assert!(block.contains("file:line"), "evidence rule missing: {block}");
        assert!(
            block.contains("not found in the files I read"),
            "qualified-negative form missing: {block}"
        );
        assert!(block.contains("never refute a symptom"), "symptom rule missing: {block}");
        assert!(block.contains("coverage footer"), "coverage footer missing: {block}");
    }

    #[test]
    fn guards_role_block_carries_delivery_contract() {
        // The guards block must tell the agent to return the lines as text and
        // never write a file — the missing rule that let an agent self-write.
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let block = build_role_block("guards", dir.path(), "apps/rt", "pt-BR");
        assert!(block.contains("scan-guards-apply"), "delivery contract missing: {block}");
        assert!(block.contains("do NOT write any file"), "write-restriction missing: {block}");
    }

    #[test]
    fn recommended_subagent_type_locks_read_only_roles() {
        // Read-only roles map to tool-restricted agents; writing roles stay
        // general-purpose. Case/whitespace-insensitive.
        assert_eq!(recommended_subagent_type("explore"), "Explore");
        assert_eq!(recommended_subagent_type("plan"), "Plan");
        assert_eq!(recommended_subagent_type("review"), "mustard-review");
        assert_eq!(recommended_subagent_type("qa"), "mustard-review");
        assert_eq!(recommended_subagent_type(" Guards "), "mustard-guards");
        assert_eq!(recommended_subagent_type("impl"), "general-purpose");
        assert_eq!(recommended_subagent_type("backend"), "general-purpose");
    }

    #[test]
    fn collapse_empty_sections_drops_blank_keeps_filled() {
        let text = "## A\n\n## B\nbody\n\n## C\n   \n## D\nx";
        let out = collapse_empty_sections(text);
        assert!(!out.contains("## A"), "empty heading A survived: {out}");
        assert!(out.contains("## B\nbody"), "filled heading B dropped: {out}");
        assert!(!out.contains("## C"), "whitespace-only heading C survived: {out}");
        assert!(out.contains("## D\nx"), "filled heading D dropped: {out}");
    }

    #[test]
    fn cross_wave_pull_pointer_is_a_pull_not_a_dump() {
        // Wave 1 / spec-less → no pointer (nothing prior to pull).
        assert!(cross_wave_pull_pointer("", Some(1)).is_empty());
        assert!(cross_wave_pull_pointer("demo", Some(1)).is_empty());
        assert!(cross_wave_pull_pointer("demo", None).is_empty());
        // Wave > 1 with a spec → a one-line pointer to the pull command, NOT the
        // inlined summaries (those stay in `<spec>/.events`, fetched on demand).
        let p = cross_wave_pull_pointer("demo-spec", Some(3));
        assert!(
            p.contains("mustard-rt run memory cross-wave --spec demo-spec --wave 3"),
            "pointer must carry the pull command: {p}"
        );
        assert!(p.contains("on demand"), "must instruct pull-on-demand: {p}");
        assert!(p.contains("1..2"), "must name the prior-wave range: {p}");
    }

    #[test]
    fn guards_prompt_lang_carries_locale_tone_and_facts() {
        // The `guards` role drives the Wave-2 enrich step: its block must name
        // the project locale + tone (from mustard.json) and surface the pending
        // block's deterministic facts so the agent stays grounded.
        let dir = tempdir().unwrap();
        anchor(dir.path());
        // mustard.json declares a non-default tone — the block must echo it.
        std::fs::write(
            dir.path().join("mustard.json"),
            br#"{"specLang":"pt-BR","tone":"technical"}"#,
        )
        .unwrap();
        // A subproject CLAUDE.md with a pending Guards block carrying facts.
        let sub = dir.path().join("apps").join("rt");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("CLAUDE.md"),
            "# Rt\n\n## Guards\n\n<!-- mustard:guards pending -->\n\
             <!-- facts: kind=rust; frameworks=serde, clap -->\n<!-- /mustard:guards -->\n",
        )
        .unwrap();

        let block = build_role_block("guards", dir.path(), "apps/rt", "pt-BR");
        assert!(block.starts_with("ROLE: guards"), "role marker missing: {block}");
        // Locale + tone from mustard.json are surfaced.
        assert!(block.contains("pt-BR"), "locale missing: {block}");
        assert!(block.contains("technical"), "tone missing: {block}");
        // Grounding facts from the pending block are surfaced.
        assert!(block.contains("kind=rust"), "kind fact missing: {block}");
        assert!(block.contains("serde, clap"), "framework facts missing: {block}");
        // The cap (3-6 lines) is named so the agent stays concise.
        assert!(block.contains("3-6"), "line cap not stated: {block}");

        // A non-guards role gets its own contract (no longer a bare marker).
        let backend = build_role_block("backend", dir.path(), "apps/rt", "pt-BR");
        assert!(backend.starts_with("ROLE: backend"), "role marker missing: {backend}");
        assert!(backend.contains("apps/rt"), "subproject scope missing: {backend}");
    }

    #[test]
    fn guards_prompt_lang_specless_derives_locale_from_mustard_json() {
        // The `/scan` enrich path runs spec-less: `run` is invoked with no
        // `--spec`, so there is no spec.md to read `### Lang:` from. The locale
        // must instead come from `mustard.json#specLang` via the canonical
        // `ProjectConfig::load(..).i18n()` accessor — the SAME accessor the
        // guards role already uses for tone — never an ad-hoc parse. This test
        // pins the spec-less branch's locale source feeding into the guards
        // block (locale + tone + the grounded 3-6 line instruction).
        let dir = tempdir().unwrap();
        anchor(dir.path());
        std::fs::write(
            dir.path().join("mustard.json"),
            br#"{"specLang":"pt-BR","tone":"technical"}"#,
        )
        .unwrap();
        let sub = dir.path().join("apps").join("rt");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("CLAUDE.md"),
            "# Rt\n\n## Guards\n\n<!-- mustard:guards pending -->\n\
             <!-- facts: kind=rust; frameworks=serde, clap -->\n<!-- /mustard:guards -->\n",
        )
        .unwrap();

        // Mirror the spec-less locale derivation in `run`: with `spec == None`
        // the narrative locale is `ProjectConfig::load(..).i18n().lang`.
        let spec_lang = mustard_core::ProjectConfig::load(dir.path())
            .i18n()
            .lang
            .as_str()
            .to_string();
        assert_eq!(spec_lang, "pt-BR", "spec-less locale must come from mustard.json#specLang");

        // That derived locale flows into the guards block exactly as the spec
        // path would: locale + tone + the capped, grounded instruction.
        let block = build_role_block("guards", dir.path(), "apps/rt", &spec_lang);
        assert!(block.starts_with("ROLE: guards"), "role marker missing: {block}");
        assert!(block.contains("pt-BR"), "locale missing: {block}");
        assert!(block.contains("technical"), "tone missing: {block}");
        assert!(block.contains("kind=rust"), "kind fact missing: {block}");
        assert!(block.contains("3-6"), "line cap not stated: {block}");

        // A project with no specLang declared falls back to the i18n default
        // locale (never a panic / parse error on the spec-less path).
        let bare = tempdir().unwrap();
        anchor(bare.path()); // anchor writes `{}` mustard.json.
        let default_lang = mustard_core::ProjectConfig::load(bare.path())
            .i18n()
            .lang
            .as_str()
            .to_string();
        assert!(!default_lang.is_empty(), "default locale must be non-empty");
    }

    #[test]
    fn read_spec_lang_defaults_to_en() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, "# Title\n\n## Body\n").unwrap();
        // BCP-47 default per `project_locale_codes` memory.
        assert_eq!(read_spec_lang(&path), "en-US");
    }

    #[test]
    fn read_spec_lang_parses_pt() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        // BCP-47 spelling is the canonical write form.
        std::fs::write(&path, "# Title\n### Lang: pt-BR\n").unwrap();
        assert_eq!(read_spec_lang(&path), "pt-BR");
    }

    #[test]
    fn read_spec_lang_tolerates_legacy_short_form() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        // Legacy short codes are returned verbatim — `SupportedLocale::from_str` rejects
        // them so downstream code must normalise (e.g. via the tolerant path).
        std::fs::write(&path, "# Title\n### Lang: pt\n").unwrap();
        assert_eq!(read_spec_lang(&path), "pt");
    }

    #[test]
    fn read_task_steps_cuts_section() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n## Resumo\nx\n## Tarefas\n- [ ] do a\n- [ ] do b\n## Deps\nz\n",
        )
        .unwrap();
        let steps = read_task_steps(&path);
        assert!(steps.contains("Tarefas"));
        assert!(steps.contains("do a"));
        assert!(!steps.contains("Deps"));
    }

    #[test]
    fn read_task_steps_falls_back_to_body_when_no_tasks_section() {
        // A lean bugfix/Light spec: no `## Tasks`, but `## Causa raiz` +
        // `## Plano` carry the work. The TASK block must be non-empty so the
        // dispatched agent receives a real work description, plus the explicit
        // read-the-spec instruction line.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n## Contexto\nbrief\n## Causa raiz\nrace on shutdown\n## Plano\n\
             - fix the lock ordering\n## Critérios de Aceitação\n- repro exits 0\n",
        )
        .unwrap();
        let steps = read_task_steps(&path);
        assert!(!steps.is_empty(), "TASK block must not be empty for a lean spec");
        assert!(steps.contains("race on shutdown"), "root cause missing: {steps}");
        assert!(steps.contains("fix the lock ordering"), "plan missing: {steps}");
        assert!(
            steps.contains("Read the full spec at"),
            "read-the-spec instruction missing: {steps}"
        );
    }

    #[test]
    fn task_fallback_tf_without_tasks_yields_context_and_ac() {
        // A tactical-fix / drafted spec: no `## Tasks`, no `## Causa raiz` /
        // `## Plano` — only `## Contexto` + `## Critérios de Aceitação`. The
        // TASK block must be non-empty, carry both sections and a header
        // naming the origin, instead of degrading to a blank TASK.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# TF\n## Contexto\nthe digest misses pt intents\n\
             ## Critérios de Aceitação\n- **AC-1** — repro query returns hits\n",
        )
        .unwrap();
        let steps = read_task_steps(&path);
        assert!(!steps.is_empty(), "TASK must not be empty for a TF spec");
        assert!(steps.contains("TASK fallback"), "origin header missing: {steps}");
        assert!(steps.contains("the digest misses pt intents"), "context missing: {steps}");
        assert!(steps.contains("AC-1"), "acceptance criteria missing: {steps}");
        assert!(steps.contains("Read the full spec at"), "read-the-spec cue missing: {steps}");
    }

    #[test]
    fn task_fallback_matches_en_headings_too() {
        // The canonical-key cut is i18n-aware: an EN-authored spec with
        // `## Context` + `## Acceptance Criteria` resolves the same tier.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# TF\n## Context\nwidget cache is stale\n\
             ## Acceptance Criteria\n- **AC-1** — cache invalidates on write\n",
        )
        .unwrap();
        let steps = read_task_steps(&path);
        assert!(steps.contains("widget cache is stale"), "context missing: {steps}");
        assert!(steps.contains("cache invalidates on write"), "AC missing: {steps}");
    }

    #[test]
    fn task_fallback_spec_with_tasks_stays_byte_identical() {
        // A spec WITH `## Tasks` keeps the exact structured cut — no fallback
        // header, no Context/AC leakage, byte-identical to the section slice.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# T\n## Contexto\nctx prose\n## Tasks\n- [ ] do the thing\n\
             ## Critérios de Aceitação\n- **AC-1** — gate passes\n",
        )
        .unwrap();
        let steps = read_task_steps(&path);
        assert_eq!(steps, "## Tasks\n- [ ] do the thing", "structured cut must be byte-identical");
        assert!(!steps.contains("TASK fallback"), "fallback header leaked: {steps}");
    }

    #[test]
    fn task_fallback_root_cause_tier_still_wins_over_context_tier() {
        // Tier order is stable: when `## Causa raiz`/`## Plano` exist, the
        // Context/AC tier (and its origin header) must NOT engage.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# T\n## Contexto\nctx prose\n## Causa raiz\nrace on shutdown\n\
             ## Plano\n- fix lock order\n## Critérios de Aceitação\n- repro exits 0\n",
        )
        .unwrap();
        let steps = read_task_steps(&path);
        assert!(steps.contains("race on shutdown"));
        assert!(steps.contains("fix lock order"));
        assert!(!steps.contains("TASK fallback"), "tier-2 header leaked: {steps}");
        assert!(!steps.contains("ctx prose"), "tier-2 content leaked: {steps}");
    }

    #[test]
    fn read_task_steps_prefers_structured_tasks_over_fallback() {
        // When `## Tasks` is present and non-empty, no fallback content leaks in.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# T\n## Causa raiz\nthe cause\n## Plano\nthe plan\n## Tasks\n- [ ] do the thing\n",
        )
        .unwrap();
        let steps = read_task_steps(&path);
        assert!(steps.contains("do the thing"));
        assert!(!steps.contains("Read the full spec at"), "fallback leaked: {steps}");
        assert!(!steps.contains("the cause"), "root cause leaked: {steps}");
    }

    #[test]
    fn dispatch_render_lean_spec_yields_nonempty_task_block() {
        // End-to-end: render the dispatch block for a spec with no `## Tasks`
        // section and assert the `## TASK` placeholder is filled (non-blank).
        let spec_body = "# T\n## Causa raiz\nnull deref in parse\n## Plano\n- guard the option\n";
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, spec_body).unwrap();

        let task_steps = read_task_steps(&path);
        assert!(!task_steps.is_empty(), "task_steps fell back to empty for a lean spec");
        let mut rendered = extract_block(TEMPLATE, "dispatch").expect("dispatch block");
        rendered = rendered.replace("{task_steps}", &task_steps);

        // The `## TASK` body is the slice between the heading and the trailing
        // `Guards carregados` line. It must carry the lean spec's narrative.
        let after_task = rendered
            .split_once("## TASK")
            .map(|(_, rest)| rest)
            .expect("template has a ## TASK heading");
        let body = after_task
            .split("Guards carregados")
            .next()
            .unwrap_or("")
            .trim();
        assert!(!body.is_empty(), "TASK block is empty for a lean spec");
        assert!(
            body.contains("null deref in parse"),
            "TASK block missing root cause: {body:?}"
        );
        assert!(body.contains("guard the option"), "TASK block missing plan: {body:?}");
    }

    // -----------------------------------------------------------------------
    // F3-b — {entity_info} / {reference_files} / {context_extras} fillers
    // -----------------------------------------------------------------------

    /// Plant a workspace anchor so `ClaudePaths::for_project` accepts the temp dir.
    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

    #[test]
    fn build_reference_files_lists_files_and_signatures() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        // A source file under the subproject with a public fn + struct.
        let sub = dir.path().join("api");
        std::fs::create_dir_all(sub.join("src")).unwrap();
        std::fs::write(
            sub.join("src").join("user.rs"),
            "pub struct User { id: i32 }\npub fn make_user() -> User { User { id: 0 } }\n",
        )
        .unwrap();
        let spec = dir.path().join("spec.md");
        std::fs::write(&spec, "# T\n## Files\n- `src/user.rs` — the user model\n## Tasks\n- x\n").unwrap();
        let refs = build_reference_files(dir.path(), "api", &spec);
        assert!(refs.contains("## Files"));
        assert!(refs.contains("src/user.rs"));
        // Structural summary surfaces the public fn / type name.
        assert!(refs.contains("make_user") || refs.contains("User"), "got: {refs}");
        // No spec Files section → empty.
        let empty_spec = dir.path().join("empty.md");
        std::fs::write(&empty_spec, "# T\n## Tasks\n- x\n").unwrap();
        assert!(build_reference_files(dir.path(), "api", &empty_spec).is_empty());
    }

    #[test]
    fn build_context_extras_slices_role_section() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let cfg = ClaudePaths::for_project(dir.path()).unwrap().claude_dir().join("pipeline-config.md");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            "# Pipeline\n## Review Rules\n- stay skeptical\n- run tests\n## Parallel Rules\n- single message\n",
        )
        .unwrap();
        let extras = build_context_extras(dir.path(), "review");
        assert!(extras.contains("Review Rules"));
        assert!(extras.contains("stay skeptical"));
        assert!(!extras.contains("Parallel Rules"), "slice bled into next heading");
        // Unknown role → empty.
        assert!(build_context_extras(dir.path(), "nonexistent-role").is_empty());
    }

    #[test]
    fn dispatch_render_fills_three_placeholders_and_leaves_no_unfilled() {
        // End-to-end: assemble the dispatch block, substitute the three F3-b
        // placeholders + the rest with realistic values, then assert no
        // `{...}` placeholder remains (the `scan_unfilled` contract).
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let sub = dir.path().join("api");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("widget.rs"), "pub fn build_widget() {}\n").unwrap();
        let spec = dir.path().join("spec.md");
        std::fs::write(
            &spec,
            "# T\n## Files\n- `widget.rs`\n## Tasks\n- [ ] refactor the widget pipeline\n",
        )
        .unwrap();
        let cfg = ClaudePaths::for_project(dir.path()).unwrap().claude_dir().join("pipeline-config.md");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "# P\n## Review Rules\n- skeptical\n## Next\n- x\n").unwrap();

        let task_steps = read_task_steps(&spec);
        let reference_files = build_reference_files(dir.path(), "api", &spec);
        let context_extras = build_context_extras(dir.path(), "review");
        assert!(!reference_files.is_empty(), "reference_files empty");
        assert!(!context_extras.is_empty(), "context_extras empty");

        let mut rendered = extract_block(TEMPLATE, "dispatch").expect("dispatch block");
        // The removed `{entity_info}` / `{recommended_skills}` placeholders are no
        // longer in the template, so they are not substituted here either.
        let subs: &[(&str, &str)] = &[
            ("{subproject}", "api"),
            ("{guards_summary}", "g"),
            ("{role_block}", "ROLE: review"),
            ("{spec_lang}", "en-US"),
            ("{task_steps}", &task_steps),
            ("{context_md}", ""),
            ("{prior_wave_diff}", ""),
            ("{change_log}", ""),
            ("{cross_wave_memory}", ""),
            ("{reference_files}", &reference_files),
            ("{context_extras}", &context_extras),
            ("{retry_context}", ""),
        ];
        for (k, v) in subs {
            rendered = rendered.replace(k, v);
        }
        // Mirror run(): collapse the now-empty sections (SHARED LANGUAGE, CROSS-WAVE
        // MEMORY, PRIOR WAVE DIFF) before the unfilled-placeholder check.
        let rendered = collapse_empty_sections(&rendered);
        assert!(rendered.contains("widget.rs"), "reference_files not rendered");
        assert!(rendered.contains("Review Rules"), "context_extras not rendered");
        assert!(
            scan_unfilled(&rendered).is_empty(),
            "unfilled placeholders remain: {:?}",
            scan_unfilled(&rendered)
        );
    }
}
