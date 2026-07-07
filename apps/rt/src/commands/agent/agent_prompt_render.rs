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

/// Emit selector for the `--emit` CLI flag. `Inline` prints the full rendered
/// prompt on stdout (the historical contract). `Ref` writes the prompt to a
/// deterministic file under `.claude/` and prints a 2-line dispatch stub
/// instead — the orchestrator passes the stub VERBATIM as the Task prompt and
/// the `subagent_inject` PreToolUse hook expands it back to the full text
/// inside the dispatch. The full prompt then never transits the
/// orchestrator's context (historically it was paid twice: once as command
/// stdout, once again in the Task dispatch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitMode {
    Inline,
    Ref,
}

impl EmitMode {
    /// Parse the `--emit` CLI flag. Defaults to [`EmitMode::Inline`].
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "ref" => Self::Ref,
            _ => Self::Inline,
        }
    }
}

/// Marker prefix of the stub's first line. `subagent_inject` greps the Task
/// prompt for this exact prefix to locate the file to expand.
pub const PROMPT_REF_MARKER: &str = "MUSTARD-PROMPT-REF:";

/// Embedded template — contains the Dispatch + Retry blocks delimited by
/// `<!-- TEMPLATE: dispatch -->` / `<!-- TEMPLATE: retry -->` markers.
const TEMPLATE: &str = include_str!("agent_prompt_template.md");

/// Run `mustard-rt run agent-prompt-render`.
///
/// Fail-open contract: every step degrades to an empty placeholder value with
/// a warning on stderr; the process never panics and always exits 0.
#[allow(clippy::too_many_arguments)] // mirrors the CLI flag surface 1:1
pub fn run(
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    retry_context_file: Option<&Path>,
    task_filter: Option<&str>,
    task_text: Option<&str>,
    emit: EmitMode,
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
    );
    let out = match emit {
        EmitMode::Inline => rendered,
        EmitMode::Ref => prompt_ref_stub(
            &project, spec, wave, role, subproject, mode, task_filter, task_text, &rendered,
        ),
    };
    // stdout = prompt string or dispatch stub (raw, no JSON framing).
    print!("{out}");
}

/// Write `rendered` to its deterministic dispatch file and return the 2-line
/// stub that stands in for it. Fail-open both ways: an empty render returns
/// the empty string (the historical print-nothing behaviour), and a write
/// failure degrades to the full inline prompt — the dispatch must never be
/// lost to a missing directory or a locked file.
#[allow(clippy::too_many_arguments)] // mirrors render_prompt_at's surface
fn prompt_ref_stub(
    project: &Path,
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    task_filter: Option<&str>,
    task_text: Option<&str>,
    rendered: &str,
) -> String {
    let rel = prompt_ref_rel_path(spec, wave, role, subproject, mode, task_filter, task_text);
    write_prompt_ref(project, &rel, rendered)
}

/// Write `rendered` to project-relative `rel` and return the 2-line dispatch
/// stub that stands in for it. The shared primitive behind every `--emit ref`,
/// so the full prompt never transits the orchestrator's context. Fail-open both
/// ways: an empty render returns the empty string (the historical
/// print-nothing behaviour), and a write failure degrades to the full inline
/// prompt — the dispatch must never be lost to a missing directory or a locked
/// file.
pub(crate) fn write_prompt_ref(project: &Path, rel: &str, rendered: &str) -> String {
    if rendered.is_empty() {
        return String::new();
    }
    if mfs::write_atomic(project.join(rel), rendered.as_bytes()).is_err() {
        eprintln!("--emit ref: WARN: could not write {rel} — falling back to inline prompt");
        return rendered.to_string();
    }
    format!(
        "{PROMPT_REF_MARKER} {rel}\nDispatch stub ({} chars rendered) — pass this stub VERBATIM as the Task prompt; the PreToolUse hook expands it to the full prompt. Subagent fallback: if you are reading this line, the hook did not expand it — Read the file above and follow its content as your prompt.\n",
        rendered.chars().count()
    )
}

/// Compose [`render_prompt_at`] + [`prompt_ref_stub`] — the ref-mode miolo
/// reused in-process by `wave-advance`, so its dispatch items carry the cheap
/// stub instead of the full prompt (which the orchestrator would pay once in
/// the command output and again in the Task dispatch).
pub(crate) fn render_prompt_ref_at(
    project: &Path,
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
) -> String {
    let rendered =
        render_prompt_at(project, spec, wave, role, subproject, mode, None, None, None);
    prompt_ref_stub(project, spec, wave, role, subproject, mode, None, None, &rendered)
}

/// Deterministic project-relative path (forward slashes — survives Git Bash
/// and stays byte-stable across hosts) for a rendered dispatch prompt.
///
/// - Spec'd renders live beside the spec (cleaned up with it):
///   `.claude/spec/{spec}/.dispatch/wave-{n}-{role}[-{subproject}].{mode}.prompt.md`
///   (`n` = 0 for wave-less renders; the subproject slug disambiguates the
///   per-subproject review round, which renders the same spec/wave/role once
///   per subproject).
/// - Spec-less renders (`/task`, ANALYZE/DIAGNOSE explores) key on an FNV-1a
///   hash of the distinguishing inputs:
///   `.claude/.dispatch/{role}-{hash:016x}.prompt.md`.
fn prompt_ref_rel_path(
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    task_filter: Option<&str>,
    task_text: Option<&str>,
) -> String {
    let mode_tag = match mode {
        RenderMode::First => "first",
        RenderMode::Granular => "granular",
        RenderMode::FixLoop => "fix-loop",
    };
    let sub = subproject.to_string_lossy();
    match spec {
        Some(s) => {
            let sub_slug = path_slug(&sub);
            let sub_part = if sub_slug.is_empty() { String::new() } else { format!("-{sub_slug}") };
            format!(
                ".claude/spec/{s}/.dispatch/wave-{}-{role}{sub_part}.{mode_tag}.prompt.md",
                wave.unwrap_or(0)
            )
        }
        None => {
            let hash = fnv1a64(&[
                role,
                &sub,
                mode_tag,
                task_filter.unwrap_or(""),
                task_text.unwrap_or(""),
            ]);
            format!(".claude/.dispatch/{role}-{hash:016x}.prompt.md")
        }
    }
}

/// Filename-safe slug of a subproject path: alphanumerics and `-` kept,
/// everything else folded to `-`; the root (`.` / empty) yields the empty
/// slug (no suffix).
fn path_slug(path: &str) -> String {
    let slug: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() { String::new() } else { trimmed.to_string() }
}

/// FNV-1a 64-bit over `parts` with a separator fold between them — pure and
/// deterministic (no clock, no randomness), so the same render inputs always
/// map to the same dispatch file.
pub(crate) fn fnv1a64(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for part in parts {
        for b in part.as_bytes() {
            eat(*b);
        }
        eat(0x1f); // unit separator — "ab","c" never collides with "a","bc"
    }
    h
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
    // The `{cross_wave_memory}` body accumulates the relevance-gated blocks
    // below (capabilities, spec memory, vocabulary). An empty result collapses
    // the section (`collapse_empty_sections`). The query is the role + task
    // text — the same intent the spec-memory gate already keys on.
    let recall_intent = format!("{role} {task_steps}");
    let mut cross_wave_memory = String::new();
    // Durable capabilities relevant to the task — the "what the system already
    // does" context at ANALYZE. Ranked by the SAME BM25 arithmetic the knowledge
    // recall uses, but through a SEPARATE injector: capabilities are durable and
    // must never decay/prune, so they never enter the `Knowledge` recall path
    // (no `last_used` write-back, nothing mutated). Folded into the same block;
    // collapses when nothing clears the relevance floor.
    let capabilities = capability_block(&project, &recall_intent);
    if !capabilities.is_empty() {
        if !cross_wave_memory.is_empty() {
            cross_wave_memory.push_str("\n\n");
        }
        cross_wave_memory.push_str(&capabilities);
    }
    // Append the spec-memory principles through the relevance gate. The shared
    // `resolve_spec_memory` is the single home for the tri-state — the gate's
    // approved set (`<spec>/.memory-approved`, written by the orchestration-layer
    // Haiku judge) when it ran, else the deterministic recall fallback. Both
    // injection paths call it; no duplicated branching. Relevance is the only
    // filter (never a count, never a size); spec-less renders fail-open to empty.
    let spec_memory_block = context_inject::render_spec_memory_block(
        &context_inject::resolve_spec_memory(&spec_dir, &recall_intent, true),
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

    // No size budget: every placeholder rides in full. Relevance is the only
    // filter on what enters the prompt — the spec-memory gate and the
    // relevance-filtered context slice decide membership; nothing is trimmed by
    // token count.

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

/// The epistemic floor for read-only investigative dispatches (the `explore`
/// role). Settle existence by enumeration, never claim absence from sampled
/// reads, and never refute a symptom the user observed at runtime — static
/// reading cannot disprove it. This single definition feeds BOTH the rendered
/// explore contract (in [`build_role_block`]) AND the `subagent_inject` floor
/// that re-asserts it for Explores dispatched OUTSIDE the renderer (ad-hoc /
/// cross-repo), so the discipline never drifts between the two and is never
/// lost to the dispatch route.
pub const EPISTEMIC_FLOOR: &str = "Settle existence/duplication questions by Grep \
     enumeration over the slice FIRST — reading samples never proves absence. Ground \
     every claim in file:line. NEVER assert \"X does not exist\" and never refute a \
     symptom the user observed at runtime — static reading cannot disprove it; say \
     \"not found in the files I read\" instead.";

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
        "explore" => {
            // The epistemic discipline lives in one place (EPISTEMIC_FLOOR) so
            // the rendered contract and the `subagent_inject` floor never drift.
            let floor = EPISTEMIC_FLOOR;
            format!(
                "ROLE: explore\n\
                 You map a slice of {subproject} read-only and return a compact briefing. You \
                 write NOTHING — if the task implies a change, report it, do not do it. Start from \
                 the anchors you were given; when the question is about composed behavior, follow \
                 the anchor's references into the files it pulls in (an anchor alone does not show \
                 what those files contribute); never bulk-read. {floor} Deliver: your final message is a \
                 ≤30-line briefing — the pattern to mirror, files to touch, contract wiring — plus \
                 a coverage footer (files read / chains not followed), exempt from the cap. No \
                 file dumps."
            )
        },
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
             files changed + non-obvious decisions + blockers. Do NOT paste file contents.\n\
             MEMORY: when you finish, emit ONE `<MEMORY>one-line decision/lesson + why in ≤2 \
             sentences</MEMORY>` block ONLY if BOTH hold: (a) there was a REAL choice — alternatives \
             existed and you could have gone the other way (not the only option, not the obvious \
             default); AND (b) a future agent on this project would decide WORSE without knowing it. \
             Obvious / a recap of what you did / only-true-for-this-task / context you read / guards / \
             a file list / 'interrupted' → emit NO `<MEMORY>`. \
             Good: `<MEMORY>Chose atomic_md write over direct fs::write — a mid-write crash corrupts \
             the file</MEMORY>`. Bad: `<MEMORY>Fixed the bug in foo.rs</MEMORY>` (a recap)."
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
         from the manifest/tree. Do not RESTATE a fact — author the non-obvious \
         RULE it implies: a `scripts=` codegen step ⇒ \"its output is generated — \
         regenerate via that script, never hand-edit it\"; a detected stack ⇒ the \
         convention that stack enforces here. Write in the project locale \
         ({spec_lang}) and tone ({tone}). Be concise; never generic prose. Deliver \
         ONLY the lines as your final message; do NOT write any file — the caller \
         pipes your text to scan-guards-apply.{facts_line}"
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

// ---------------------------------------------------------------------------
// Capabilities — durable "what the system already does" injection.
//
// REUSES only the BM25 ranking arithmetic (`domain::ranking`), so there is no
// second ranker. It does NOT touch the `Knowledge` store: capabilities are
// DURABLE — they must never decay or be pruned (no `last_used` write-back, no
// confidence decay, no mutation). This block only ranks + cuts capability docs
// for relevance and renders them; it reads the filesystem and writes nothing.
// ---------------------------------------------------------------------------

/// Relevance floor as a fraction of the best score (×1024) — the same anti-bloat
/// bound `context_inject::MEMORY_RELEVANCE_FLOOR_FRACTION` expresses, mirrored
/// here in fixed-point so capabilities get the identical relevance discipline as
/// the memory blocks (an off-topic capability never enters).
const CAPABILITY_RELEVANCE_FLOOR_FRACTION: u64 = 348; // 0.34 ×1024 (rounded)

/// Top-K capabilities injected once the relevance floor has cut the weak tail.
const CAPABILITY_TOP_K: usize = 3;

/// Minimum content-token length: shorter tokens
/// (`the`, `a`, `is`) match too broadly to discriminate, so both the intent query
/// and a capability's searchable text are tokenised on ≥4-char lowercase
/// alphanumeric runs.
const CAPABILITY_MIN_TERM_LEN: usize = 4;

/// Active PUSH of the RELEVANT durable capabilities — the "what the system already
/// does" context at ANALYZE. Loads every `.claude/capabilities/*.md`, parses each
/// into a [`Capability`] (reusing `capability::parse`; unparseable docs are skipped
/// — fail-open), ranks them against `intent` with the core's byte-stable BM25
/// (`domain::ranking`, the SAME arithmetic the knowledge recall uses), cuts the
/// weak tail with the SAME relevance floor, takes the top-K (rank desc, then id
/// asc for byte-stability), and renders them under a `## CAPABILITIES` heading.
///
/// Spec-agnostic: it ranks ALL active capabilities against the request intent, so
/// it works for a fresh feature at ANALYZE (no spec, no wave). Empty (so the
/// heading collapses) when there are no capabilities or none clears the floor.
///
/// DURABILITY: this never mutates anything — no `last_used` write-back, no decay,
/// no prune. It does not touch the `Knowledge` store.
fn capability_block(project: &Path, intent: &str) -> String {
    let Ok(dir) = ClaudePaths::for_project(project).map(|p| p.capabilities_dir()) else {
        return String::new();
    };
    let caps = load_capabilities(&dir);
    let selected = rank_capabilities(&caps, intent, CAPABILITY_TOP_K);
    let bullets = render_capability_bullets(&selected);
    if bullets.is_empty() {
        return String::new();
    }
    // Folded into the `{cross_wave_memory}` body, so it carries its OWN heading —
    // a distinct sub-section beside PROJECT KNOWLEDGE / CROSS-WAVE MEMORY.
    format!("## CAPABILITIES\n{bullets}")
}

/// Load + parse every `.claude/capabilities/*.md` into a [`Capability`], skipping
/// unreadable / unparseable docs (fail-open) and any non-active capability (a
/// deprecated capability is durable history, not current context). The directory
/// listing is sorted by file name so the corpus order is deterministic; never
/// panics.
fn load_capabilities(dir: &Path) -> Vec<mustard_core::domain::capability::Capability> {
    let Ok(mut entries) = mfs::read_dir(dir) else {
        return Vec::new();
    };
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    let mut out = Vec::new();
    for entry in entries {
        if entry.is_dir || !entry.file_name.ends_with(".md") {
            continue;
        }
        let Ok(md) = mfs::read_to_string(&entry.path) else {
            continue;
        };
        let cap = crate::commands::capability::parse(&md);
        // A capability with no id is a parse miss (no frontmatter) — skip it.
        // A deprecated capability is excluded: ANALYZE wants what the system
        // does NOW, and `is_active` is lenient (empty/unset status counts live).
        if cap.id.trim().is_empty() || !cap.is_active() {
            continue;
        }
        out.push(cap);
    }
    out
}

/// Rank `caps` against `intent` with the core's fixed-point BM25 and return the
/// top-`max` survivors of the relevance floor.
///
/// The rankable text of a capability is its searchable content — the title plus
/// every requirement statement plus every scenario's when/then. The corpus stats
/// (`avgdl`) and per-term `bm25_x1024_default` are the SAME `domain::ranking`
/// arithmetic (no second ranker). The cut is
/// score-desc with id-asc tiebreak (byte-stable),
/// then keep only documents within [`CAPABILITY_RELEVANCE_FLOOR_FRACTION`] of the
/// top score, then truncate to `max`. NO decay weight — capabilities are durable,
/// so unlike the knowledge recall there is no confidence/age attenuation. Pure +
/// deterministic; never panics; never mutates.
fn rank_capabilities<'a>(
    caps: &'a [mustard_core::domain::capability::Capability],
    intent: &str,
    max: usize,
) -> Vec<&'a mustard_core::domain::capability::Capability> {
    use mustard_core::domain::ranking::{avgdl_x1024, bm25_x1024_default};

    if max == 0 || caps.is_empty() {
        return Vec::new();
    }
    let terms = capability_query_terms(intent);
    if terms.is_empty() {
        return Vec::new();
    }

    // Tokenise each capability's searchable text once; corpus stats feed the
    // shared `avgdl` so BM25's length normalisation is corpus-aware (same as
    // recall over the knowledge corpus).
    let docs: Vec<Vec<String>> = caps.iter().map(capability_doc_terms).collect();
    let total_len: usize = docs.iter().map(Vec::len).sum();
    let avgdl = avgdl_x1024(total_len, docs.len());

    // Score every capability: sum the per-term BM25 over the query terms. A term
    // absent from the doc contributes 0 (bm25 of tf=0), so a capability with no
    // intent overlap scores 0 and is dropped. NO confidence/decay weight — the
    // durable injector ranks on textual relevance alone.
    let mut scored: Vec<(u64, &mustard_core::domain::capability::Capability)> =
        Vec::with_capacity(caps.len());
    for (doc, cap) in docs.iter().zip(caps) {
        let dl = doc.len();
        let mut bm25 = 0_u64;
        for term in &terms {
            let tf = doc.iter().filter(|t| *t == term).count();
            bm25 = bm25.saturating_add(bm25_x1024_default(tf, dl, avgdl));
        }
        if bm25 > 0 {
            scored.push((bm25, cap));
        }
    }
    if scored.is_empty() {
        return Vec::new();
    }

    // Best-first; id-asc breaks ties for a byte-stable order independent of the
    // directory enumeration (the same tiebreak shape recall uses on slug).
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));

    // Relevance floor (anti-bloat): keep only capabilities within
    // CAPABILITY_RELEVANCE_FLOOR_FRACTION of the top score. Fixed-point: `score *
    // frac` ×1024 vs `top * SCALE` — no float enters the cut (mirrors recall).
    if let Some(&(top, _)) = scored.first() {
        let floor_x1024 = top.saturating_mul(CAPABILITY_RELEVANCE_FLOOR_FRACTION);
        scored.retain(|(score, _)| {
            score.saturating_mul(mustard_core::domain::ranking::SCALE) >= floor_x1024
        });
    }
    scored.truncate(max);
    scored.into_iter().map(|(_, cap)| cap).collect()
}

/// The distinct intent query terms: ≥[`CAPABILITY_MIN_TERM_LEN`]-char lowercase
/// alphanumeric runs, deduplicated, order-preserving.
fn capability_query_terms(intent: &str) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for tok in intent
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if tok.len() >= CAPABILITY_MIN_TERM_LEN && seen.insert(tok.to_string()) {
            out.push(tok.to_string());
        }
    }
    out
}

/// Tokenise a capability's searchable text into its term bag — the title, every
/// requirement statement, and every scenario when/then, as ≥
/// [`CAPABILITY_MIN_TERM_LEN`]-char lowercase alphanumeric runs WITH duplicates
/// (term frequency is the BM25 signal). The status and the opaque link ids
/// (`covers`/`specs`/`related`) are NOT searchable content — they carry no
/// intent signal and would only add noise.
fn capability_doc_terms(cap: &mustard_core::domain::capability::Capability) -> Vec<String> {
    let mut combined = cap.title.clone();
    for req in &cap.requirements {
        combined.push(' ');
        combined.push_str(&req.statement);
        for sc in &req.scenarios {
            combined.push(' ');
            combined.push_str(&sc.when);
            combined.push(' ');
            combined.push_str(&sc.then);
        }
    }
    let mut out: Vec<String> = Vec::new();
    for tok in combined
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if tok.len() >= CAPABILITY_MIN_TERM_LEN {
            out.push(tok.to_string());
        }
    }
    out
}

/// Render the already-selected (see [`rank_capabilities`]) capabilities as
/// `- **{title}** ([[{id}]]) — {first requirement statement, trimmed}` bullets.
/// A capability with no requirement degrades to `- **{title}** ([[{id}]])` (the
/// link still resolves). Empty (no heading, no bullets) when the slice is empty
/// so the caller can collapse. Byte-stable: the slice order is fixed by
/// [`rank_capabilities`] and nothing here reads a clock or a path.
fn render_capability_bullets(
    caps: &[&mustard_core::domain::capability::Capability],
) -> String {
    if caps.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for cap in caps {
        let summary = cap
            .requirements
            .first()
            .map(|r| r.statement.trim())
            .unwrap_or("")
            .chars()
            .take(160)
            .collect::<String>();
        if summary.is_empty() {
            let _ = writeln!(out, "- **{}** ([[{}]])", cap.title, cap.id);
        } else {
            let _ = writeln!(out, "- **{}** ([[{}]]) — {summary}", cap.title, cap.id);
        }
    }
    out.trim_end().to_string()
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
    fn emit_ref_rel_paths_are_deterministic_and_collision_free() {
        // Spec'd render: stable name from spec/wave/role/mode; the subproject
        // slug disambiguates the per-subproject review round.
        let p = prompt_ref_rel_path(
            Some("demo"), Some(2), "rt", Path::new("."), RenderMode::First, None, None,
        );
        assert_eq!(p, ".claude/spec/demo/.dispatch/wave-2-rt.first.prompt.md");
        let a = prompt_ref_rel_path(
            Some("demo"), None, "review", Path::new("apps/rt"), RenderMode::First, None, None,
        );
        let b = prompt_ref_rel_path(
            Some("demo"), None, "review", Path::new("apps/cli"), RenderMode::First, None, None,
        );
        assert_eq!(a, ".claude/spec/demo/.dispatch/wave-0-review-apps-rt.first.prompt.md");
        assert_ne!(a, b, "review round across subprojects must not collide");

        // Spec-less render: hashed on the distinguishing inputs — same input
        // → same path (resumable), different task text → different path.
        let x = prompt_ref_rel_path(
            None, None, "explore", Path::new("."), RenderMode::First, None, Some("map the slice"),
        );
        let y = prompt_ref_rel_path(
            None, None, "explore", Path::new("."), RenderMode::First, None, Some("map the slice"),
        );
        let z = prompt_ref_rel_path(
            None, None, "explore", Path::new("."), RenderMode::First, None, Some("other task"),
        );
        assert_eq!(x, y, "deterministic for identical inputs");
        assert_ne!(x, z, "task text distinguishes spec-less dispatches");
        assert!(x.starts_with(".claude/.dispatch/explore-"), "spec-less prefix: {x}");
    }

    #[test]
    fn emit_ref_writes_prompt_file_and_returns_stub() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rendered = "ROLE: impl\nfull rendered body";
        let stub = prompt_ref_stub(
            dir.path(), Some("demo"), Some(1), "rt", Path::new("."), RenderMode::First,
            None, None, rendered,
        );
        let first = stub.lines().next().expect("stub first line");
        let rel = first.strip_prefix(PROMPT_REF_MARKER).expect("marker prefix").trim();
        let on_disk = std::fs::read_to_string(dir.path().join(rel)).expect("stub file");
        assert_eq!(on_disk, rendered, "file holds the full render verbatim");
        assert!(stub.contains("VERBATIM"), "stub instructs verbatim dispatch: {stub}");
        assert!(stub.contains("Read the file"), "stub carries the subagent fallback: {stub}");

        // Empty render → empty stub (the historical print-nothing contract).
        let empty = prompt_ref_stub(
            dir.path(), Some("demo"), Some(1), "rt", Path::new("."), RenderMode::First,
            None, None, "",
        );
        assert!(empty.is_empty(), "empty render must not produce a stub: {empty}");
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

    /// Producing roles (impl/plan → the default arm) carry the intentional-
    /// `<MEMORY>` instruction; read-only roles (explore/review/qa/guards) do NOT
    /// — they are not knowledge producers, so the contract stays off their prompt.
    #[test]
    fn producing_roles_carry_memory_contract_readonly_do_not() {
        let dir = tempdir().unwrap();
        let impl_block = build_role_block("impl", dir.path(), "api", "en-US");
        assert!(impl_block.contains("<MEMORY>"), "impl must carry MEMORY contract: {impl_block}");
        // The sharpened contract: an operational two-part test (a real choice AND
        // transferable) rather than the vague "non-obvious decision".
        assert!(
            impl_block.contains("REAL choice") && impl_block.contains("decide WORSE"),
            "MEMORY contract must carry the operational (a)+(b) test: {impl_block}"
        );
        // `plan` falls into the same default arm → also carries it.
        let plan_block = build_role_block("plan", dir.path(), "api", "en-US");
        assert!(plan_block.contains("<MEMORY>"), "plan must carry MEMORY contract: {plan_block}");
        // Read-only roles must NOT carry it.
        for role in ["explore", "review", "qa"] {
            let block = build_role_block(role, dir.path(), "api", "en-US");
            assert!(
                !block.contains("<MEMORY>"),
                "read-only role {role} must not carry the MEMORY contract: {block}"
            );
        }
    }

    #[test]
    fn explore_role_block_carries_epistemic_contract() {
        // Field defect: an Explore read sliced anchors and confidently returned
        // "no duplication" — refuting a symptom the user had observed at
        // runtime (the duplicate lived in a referenced file, invisible to
        // sliced anchor reads). The contract must route existence questions to Grep
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

    // --- capability_block: durable, relevance-gated, never mutates -----------

    /// Write a capability doc under `<project>/.claude/capabilities/{slug}.md`
    /// via the canonical renderer, so the parse the injector runs is exercised.
    fn write_capability(
        project: &Path,
        slug: &str,
        title: &str,
        requirement: &str,
        when: &str,
        then: &str,
    ) {
        use mustard_core::domain::capability::{Capability, Requirement, Scenario};
        let dir = project.join(".claude").join("capabilities");
        std::fs::create_dir_all(&dir).unwrap();
        let cap = Capability {
            id: format!("cap.{slug}"),
            title: title.into(),
            status: "active".into(),
            requirements: vec![Requirement {
                statement: requirement.into(),
                scenarios: vec![Scenario {
                    name: "s".into(),
                    when: when.into(),
                    then: then.into(),
                    command: None,
                }],
            }],
            ..Capability::default()
        };
        std::fs::write(
            dir.join(format!("{slug}.md")),
            crate::commands::capability::render(&cap),
        )
        .unwrap();
    }

    /// Two capabilities, an intent that matches ONE: the matching capability is
    /// injected, the off-topic one is cut by the relevance floor.
    #[test]
    fn capability_block_injects_relevant_and_cuts_off_topic() {
        let dir = tempdir().unwrap();
        write_capability(
            dir.path(),
            "token-savings",
            "Token savings recorder",
            "The system SHALL record token cost and savings per command.",
            "a command runs through the harness",
            "its token cost and savings are recorded",
        );
        write_capability(
            dir.path(),
            "kubernetes-deploy",
            "Kubernetes deployment manifests",
            "The system SHALL render kubernetes ingress manifests for deployment.",
            "a deployment is requested",
            "ingress manifests are produced",
        );

        let block = capability_block(dir.path(), "improve how we record token savings");
        assert!(block.starts_with("## CAPABILITIES"), "heading present: {block}");
        // The relevant capability is injected, with its `[[id]]` and first req.
        assert!(block.contains("Token savings recorder"), "relevant injected: {block}");
        assert!(block.contains("[[cap.token-savings]]"), "id link present: {block}");
        assert!(
            block.contains("SHALL record token cost and savings"),
            "first requirement rendered: {block}"
        );
        // The off-topic capability is cut by the relevance floor.
        assert!(
            !block.contains("Kubernetes"),
            "off-topic capability must not enter: {block}"
        );
    }

    /// Zero capabilities (no directory) → empty block (the heading collapses).
    #[test]
    fn capability_block_empty_when_no_capabilities() {
        let dir = tempdir().unwrap();
        // No `.claude/capabilities/` at all.
        assert!(capability_block(dir.path(), "anything at all here").is_empty());
        // And with an empty directory present, still empty.
        std::fs::create_dir_all(dir.path().join(".claude").join("capabilities")).unwrap();
        assert!(capability_block(dir.path(), "anything at all here").is_empty());
    }

    /// The injector mutates NOTHING on disk: the capability files (and the whole
    /// `.claude/` tree) are byte-identical before and after a `capability_block`
    /// call — no `last_used` write-back, no decay, no prune.
    #[test]
    fn capability_block_mutates_nothing_on_disk() {
        let dir = tempdir().unwrap();
        write_capability(
            dir.path(),
            "token-savings",
            "Token savings recorder",
            "The system SHALL record token cost and savings per command.",
            "a command runs",
            "savings are recorded",
        );
        let caps = dir.path().join(".claude").join("capabilities");
        let snapshot = |root: &Path| -> Vec<(String, String)> {
            let mut v: Vec<(String, String)> = std::fs::read_dir(root)
                .unwrap()
                .map(|e| {
                    let p = e.unwrap().path();
                    (
                        p.file_name().unwrap().to_string_lossy().into_owned(),
                        std::fs::read_to_string(&p).unwrap(),
                    )
                })
                .collect();
            v.sort();
            v
        };
        let before = snapshot(&caps);
        // A run that injects (matching intent) and one that injects nothing.
        let _ = capability_block(dir.path(), "record token savings");
        let _ = capability_block(dir.path(), "completely unrelated query terms");
        let after = snapshot(&caps);
        assert_eq!(before, after, "capability docs must be byte-identical after injection");
    }

    /// Output is byte-stable across calls, and id-asc breaks ties for an order
    /// independent of directory enumeration.
    #[test]
    fn capability_block_is_byte_stable() {
        let dir = tempdir().unwrap();
        // Two equally-relevant capabilities (identical searchable text) → the
        // id-asc tiebreak fixes their order deterministically.
        write_capability(
            dir.path(), "bbb", "Beta", "The system SHALL handle caching relevance ranking.",
            "x", "y",
        );
        write_capability(
            dir.path(), "aaa", "Alpha", "The system SHALL handle caching relevance ranking.",
            "x", "y",
        );
        let a = capability_block(dir.path(), "caching relevance ranking");
        let b = capability_block(dir.path(), "caching relevance ranking");
        assert_eq!(a, b, "deterministic across calls");
        // id-asc: cap.aaa precedes cap.bbb regardless of file enumeration order.
        let pos_a = a.find("[[cap.aaa]]").expect("aaa present");
        let pos_b = a.find("[[cap.bbb]]").expect("bbb present");
        assert!(pos_a < pos_b, "id-asc tiebreak orders aaa before bbb: {a}");
    }

    /// Deprecated capabilities are excluded — ANALYZE wants what the system does
    /// NOW. A doc with no frontmatter id (parse miss) is also skipped.
    #[test]
    fn capability_block_skips_deprecated_and_unparseable() {
        let dir = tempdir().unwrap();
        let caps = dir.path().join(".claude").join("capabilities");
        std::fs::create_dir_all(&caps).unwrap();
        // Deprecated capability — durable history, not current context.
        use mustard_core::domain::capability::{Capability, Requirement};
        let dep = Capability {
            id: "cap.legacy".into(),
            title: "Legacy caching path".into(),
            status: "deprecated".into(),
            requirements: vec![Requirement {
                statement: "The system SHALL use the old caching path.".into(),
                scenarios: vec![],
            }],
            ..Capability::default()
        };
        std::fs::write(caps.join("legacy.md"), crate::commands::capability::render(&dep)).unwrap();
        // Unparseable garbage (no frontmatter id) — fail-open skip, never panic.
        std::fs::write(caps.join("junk.md"), "not a capability at all, just prose about caching\n").unwrap();

        let block = capability_block(dir.path(), "caching path query");
        assert!(block.is_empty(), "deprecated + unparseable yield nothing: {block}");
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
