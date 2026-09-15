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
//! ## Layout
//!
//! [`render_prompt_at`] is the compositor: it collects each placeholder's value
//! (fail-open per field) from the cohesive sub-engines and substitutes them into
//! the picked template block. The sub-engines are:
//!
//! - [`prompt_ref`] — the `--emit ref` stub, its deterministic path, the FNV key;
//! - [`role`] — the per-role delivery contracts + `recommended_subagent_type`;
//! - [`sections`] — spec section cutting, task steps, the per-wave cut of the
//!   parent spec's conversation material, and the cleanup passes;
//! - [`retry`] — `## RETRY CONTEXT` composition;
//! - [`capabilities`] — the durable BM25 capability injector;
//! - [`skills`] — the subproject skill shelf;
//! - [`reference`] — `{reference_files}` via tree-sitter.
//!
//! ## Mode selection
//!
//! - `first` → render the Dispatch Template block (`<!-- TEMPLATE: dispatch -->`).
//! - `granular` / `fix-loop` → render the Minimal Retry Template block
//!   (`<!-- TEMPLATE: retry -->`); `{retry_context}` is read from
//!   `--retry-context-file` when provided, else composed by
//!   [`compose_retry_context`](retry::compose_retry_context) from what the spec
//!   already recorded (the last review verdict + persisted findings + the
//!   prior-wave diff and change requests), so a rejected wave is re-dispatched
//!   with the WHY rather than a blank prompt. Empty only when the spec recorded
//!   none of those.

use crate::commands::agent::context_inject;
use crate::commands::pipeline::resume_bootstrap::{
    find_wave_spec_path, resolve_operational_spec_path,
};
use crate::shared::context::project_dir;
use mustard_core::io::fs as mfs;
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

mod capabilities;
// `pub(crate)` para o resumo da spec carimbar o documento com o mesmo
// `fnv1a64` que nomeia o arquivo de despacho — um hash estável só, no crate.
pub(crate) mod prompt_ref;
// `pub(crate)` so the `/mustard:pr` door's review step reads a spec's declared
// files through the SAME parser the dispatch prompt uses — a second reader of
// `## Files` is a second spelling of the section, and the two would drift.
pub(crate) mod reference;
mod retry;
mod role;
// `pub` (not `mod`) only so the `dispatch_warns_on_uncurated_rules` integration
// test can call `read_guards_block` through the lib face; every other item in it
// stays `pub(crate)`.
pub mod sections;
// `pub(crate)` for the same reason as `reference` above: the PR review step
// hands the reviewer the shelf the IMPLEMENTER was dispatched with, which only
// stays true while both read it from here.
pub(crate) mod skills;

// Re-exports that preserve the historical public surface so the compatibility
// façade (`agent::agent_prompt_render`) and every in-crate consumer keep
// resolving unchanged. `PROMPT_REF_MARKER`, `EPISTEMIC_FLOOR` and
// `recommended_subagent_type` stay fully public (the hook + integration tests
// reach them); the crate-internal helpers keep their `pub(crate)` reach.
pub use prompt_ref::PROMPT_REF_MARKER;
pub use role::{recommended_subagent_type, EPISTEMIC_FLOOR};
pub(crate) use prompt_ref::render_prompt_ref_at;
pub(crate) use sections::read_task_steps;
// Surfaced for the compatibility façade's consumers (`wave_scaffold` tests and
// `wave_done`, which parses the same `## Files` section); the compositor calls
// `build_reference_files`, not this directly. NOT `#[cfg(test)]` — a runtime
// caller exists, and gating it to tests made the bin build fail to resolve it.
pub(crate) use reference::files_section_paths;

// Sub-engine helpers the compositor calls directly.
use capabilities::capability_block;
use prompt_ref::prompt_ref_stub;
use reference::build_reference_files;
use retry::compose_retry_context;
use role::build_role_block;
use sections::{
    build_conversation_material, build_why_block, collapse_empty_sections, filter_task_lines,
    read_guards_block, read_reality_obligations, read_wave_acceptance,
    scan_unfilled, strip_unfilled_template_tokens, MaterialCensus,
};
use skills::{build_mold_pointer, build_skills_list};

/// The placeholder keys this renderer substitutes into the embedded template,
/// in template order.
///
/// It IS the substitution list, not a copy of one: [`render_prompt_at`] zips it
/// with the collected values, and the value array's length is pinned to
/// `TEMPLATE_PLACEHOLDERS.len()`, so a key added without its value fails to
/// compile.
///
/// Public because this set is the contract the SHIPPED reference
/// (`plugin/refs/agent-prompt/agent-prompt.md`) documents for whoever plans a
/// wave. That file is prose: a placeholder added here and not there breaks
/// nothing in this workspace and is never seen by a compiler — the same silent
/// failure mode the rest of `tests/plugin_agents.rs` ratchets. The
/// `agent_prompt_ref_documents_every_placeholder` guard reads THIS constant, so
/// the ref's table is checked as a SET; its size is a consequence of the set and
/// never a claim the guard asserts.
pub const TEMPLATE_PLACEHOLDERS: &[&str] = &[
    "{subproject}",
    "{guards_file}",
    "{guards_summary}",
    "{role_block}",
    "{spec_lang}",
    "{task_steps}",
    "{context_md}",
    "{prior_wave_diff}",
    "{change_log}",
    "{reality_obligations}",
    "{conversation_material}",
    "{cross_wave_memory}",
    "{reference_files}",
    "{skills_list}",
    "{mold_pointer}",
    "{why_block}",
    "{acceptance_block}",
    "{retry_context}",
];

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

    /// The `--mode` spelling that selects this variant, so a refusal can quote
    /// the caller's own flag back at it.
    #[must_use]
    fn flag(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Granular => "granular",
            Self::FixLoop => "fix-loop",
        }
    }

    /// `true` for the two RE-dispatch modes. A retry re-runs ONE agent that
    /// already ran, so it always has a wave — [`wave_flag_refusal`] is the rule
    /// that says so out loud.
    #[must_use]
    fn is_retry(self) -> bool {
        matches!(self, Self::Granular | Self::FixLoop)
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

/// Embedded template — contains the Dispatch + Retry blocks delimited by
/// `<!-- TEMPLATE: dispatch -->` / `<!-- TEMPLATE: retry -->` markers.
const TEMPLATE: &str = include_str!("../agent_prompt_template.md");

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
    // A `--spec` that does not name a real spec directory is a CALL error, and
    // it must be refused BEFORE anything is rendered or written. Fail-open is
    // the renderer's contract for a placeholder it could not fill; it was never
    // meant to cover an argument that names nothing. Measured in the field: a
    // `--spec <slug>/wave-1-backend` (the wave folded into the slug) failed the
    // separator check in `ClaudePaths::for_spec`, degraded the spec dir to the
    // project root, and emitted a prompt with an empty `## TASK` and zero
    // material — with exit 0, a written file, and no warning.
    if let Some(refusal) = spec_refusal(&project, spec) {
        eprintln!("agent-prompt-render: REFUSED: {refusal}");
        std::process::exit(2);
    }
    // A RETRY of a wave plan that names no wave. Same refusal, same reason: an
    // argument that names nothing is not a placeholder to fail open on.
    if let Some(refusal) = wave_flag_refusal(&project, spec, wave, mode) {
        eprintln!("agent-prompt-render: REFUSED: {refusal}");
        std::process::exit(2);
    }
    let rendered = render_prompt_with_census(
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
    // What this wave's prompt actually carries, on stderr.
    //
    // stdout is the prompt itself and must stay raw, so the measurement rides
    // the diagnostic channel. Without it a hollow wave is invisible until the
    // agent returns something thin: the operator reported exactly that, and the
    // pipeline had no number to show.
    //
    // `held-back` is the other half of the same sentence. A bare total on a spec
    // holding more items reads as a truncation — the operator measured 28 of 35
    // and concluded the renderer caps the material. It does not (see the "No
    // size budget" note below); the difference is the per-wave cut, and a count
    // that cannot say so invents a defect while hiding a real one.
    let task_chars = rendered.task_chars;
    eprintln!(
        "agent-prompt-render: wave={} role={role} material={} held-back={} (other wave) task={task_chars} chars",
        wave.map_or_else(|| "-".to_string(), |w| w.to_string()),
        material_line_count(&rendered.text),
        rendered.material.other_wave,
    );
    // A prompt with an empty `## TASK` is the definition of a useless dispatch:
    // the agent is handed guards, references and material, and nothing to do.
    // Emitting it costs a wave; refusing costs one re-run.
    if task_chars == 0 {
        eprintln!(
            "agent-prompt-render: REFUSED: the rendered prompt has an EMPTY ## TASK — there is \
             nothing to dispatch. For a wave, pass `--spec <slug> --wave <n>`; without a spec, \
             pass `--task-text \"<the work>\"`."
        );
        std::process::exit(2);
    }
    let out = match emit {
        EmitMode::Inline => rendered.text,
        EmitMode::Ref => prompt_ref_stub(
            &project,
            spec,
            wave,
            role,
            subproject,
            mode,
            task_filter,
            task_text,
            &rendered.text,
            Some(rendered.material),
        ),
    };
    // stdout = prompt string or dispatch stub (raw, no JSON framing).
    print!("{out}");
}

/// Why a `--spec` cannot be honoured, or `None` when it can (including the
/// spec-less dispatch, which passes no `--spec` at all).
///
/// Two shapes are refused, and each names the form that works — a refusal that
/// does not teach the call costs the same round-trip it saved:
///
/// - a value `ClaudePaths::for_spec` rejects (a path separator, a `..`): almost
///   always the WAVE folded into the slug;
/// - a well-formed slug whose directory does not exist: a typo, or the wrong
///   checkout — the case that used to render an empty prompt at the repo root.
fn spec_refusal(project: &Path, spec: Option<&str>) -> Option<String> {
    let slug = spec.map(str::trim).filter(|s| !s.is_empty())?;
    let Ok(paths) = ClaudePaths::for_project(project) else {
        // No workspace anchor to resolve against — the historical fail-open
        // path. Nothing here can be established, so nothing is refused.
        return None;
    };
    match paths.for_spec(slug) {
        Err(e) => Some(format!(
            "--spec '{slug}' is not a spec name ({e}). The wave is its OWN flag: \
             pass `--spec <slug> --wave <n>`."
        )),
        Ok(sp) if !sp.dir().exists() => Some(format!(
            "--spec '{slug}' names no spec directory ({}). Check the slug the base gate minted, \
             or run from the project root.",
            sp.dir().display()
        )),
        Ok(_) => None,
    }
}

/// Why a RETRY render (`--mode granular` / `--mode fix-loop`) of a WAVE PLAN
/// cannot be honoured without `--wave N`, or `None` when it can.
///
/// A retry re-dispatches ONE agent that already ran, and that agent ran on a
/// wave. Omitting the flag does not degrade — it silently renders the WRONG
/// prompt: `resolve_operational_spec_path` falls back to the parent `spec.md`,
/// so `## TASK` becomes the parent's checklist, and the ruler is cut with no
/// `satisfies:` filter, handing the agent EVERY sibling's criteria under "these
/// are the JUDGE of this wave" — the exact noise the per-wave cut exists to
/// remove. The prose prescribed the flagless form for the fix loop; the
/// dispatch path always passed `--wave`, so the two disagreed and only the
/// prose's readers paid.
///
/// Only a wave plan is refused: a Light / tactical-fix spec has no wave to
/// name, and its retry is the same spec-level render it always was. Fail-open
/// on everything unresolvable (no `--spec`, no workspace anchor, no index) —
/// this refuses a call it can PROVE is wrong, never one it cannot read.
fn wave_flag_refusal(
    project: &Path,
    spec: Option<&str>,
    wave: Option<u32>,
    mode: RenderMode,
) -> Option<String> {
    if !mode.is_retry() || wave.is_some() {
        return None;
    }
    let slug = spec.map(str::trim).filter(|s| !s.is_empty())?;
    let paths = ClaudePaths::for_project(project).ok()?;
    let sp = paths.for_spec(slug).ok()?;
    // The plan INDEX is the witness: `plan-materialize` writes it for every
    // wave layout and nothing else writes it.
    if !sp.dir().join("wave-plan.md").is_file() {
        return None;
    }
    Some(format!(
        "`--mode {}` on the wave plan '{slug}' names no wave — `--wave <n>` is MISSING. A retry \
         re-dispatches ONE wave, and without the flag the prompt reads the PARENT's `## TASK` and \
         carries every sibling's criteria under \"these are the JUDGE of this wave\". Pass \
         `--wave <n>` for the wave being retried; `.claude/spec/{slug}/wave-plan.md` lists them.",
        mode.flag()
    ))
}

/// Bullet lines under `## CONVERSATION MATERIAL`, or 0 when the section
/// collapsed. Counting lines, not parsing: the question is "did any of what the
/// conversation settled reach this wave", and a count answers it.
fn material_line_count(prompt: &str) -> usize {
    section_lines(prompt, "## CONVERSATION MATERIAL")
        .filter(|l| l.starts_with('-') || l.starts_with('*'))
        .count()
}

/// Characters of the TASK body — measured on `task_steps` BEFORE substitution,
/// never re-parsed out of the rendered prompt.
///
/// Re-parsing was wrong, and silently so. `read_task_steps` returns the wave's
/// section WITH its own `## Tasks` heading, so the substituted body opens with a
/// `## ` line — and a section scan that stops at the next `## ` stopped on that
/// very line and reported ZERO characters for every wave that had a real task
/// list. The number existed to make a hollow wave visible and answered `0` for
/// the healthy ones instead. Measuring the source has no such ambiguity.
fn task_body_chars(task_steps: &str) -> usize {
    task_steps
        .lines()
        .map(str::trim)
        // The block's own heading is not work — it is the label on the work.
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.len() + 1)
        .sum()
}

/// Trimmed lines between a heading and the next `## `.
///
/// The heading is matched at the START OF A LINE, never anywhere in the text.
/// A bare `prompt.find("## TASK")` matched the literal inside the EFFICIENCY
/// block ("the anchors already handed to you above (`## REFERENCE`, `## TASK`)")
/// and inside the `patterns` role body — both of which precede the real
/// section — so the measurement reported the tail of another block.
fn section_lines<'a>(prompt: &'a str, heading: &str) -> impl Iterator<Item = &'a str> {
    // Walk lines instead of searching the flat text: a heading is a LINE that
    // equals the heading, which no substring match can express without an
    // off-by-one on the anchor. Collecting is fine — a prompt is one document,
    // not a stream.
    let mut body: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in prompt.lines() {
        if inside {
            if line.starts_with("## ") {
                break;
            }
            let t = line.trim();
            if !t.is_empty() {
                body.push(t);
            }
        } else if line.trim_end() == heading {
            inside = true;
        }
    }
    body.into_iter()
}

/// Render the dispatch/retry prompt against an explicit `project` root and
/// return the String instead of printing it — the miolo of [`run`], reused
/// in-process by `wave-advance` (which inlines the rendered prompt per
/// dispatch item instead of handing the orchestrator a `prompt_cmd` to shell).
///
/// Fail-open: a missing template block warns on stderr and yields an empty
/// String (the CLI entry then prints nothing, the historical behaviour).
// Test-only since the census landed: every runtime caller now takes
// [`render_prompt_with_census`], because the counts the dispatch reports are
// part of the answer. The remaining callers are all `#[cfg(test)]` modules
// asserting the prompt TEXT, so gating keeps the bin build warning-free — the
// same treatment `read_task_steps` / `files_section_paths` already get.
#[cfg(test)]
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
    render_prompt_with_census(
        project,
        spec,
        wave,
        role,
        subproject,
        mode,
        retry_context_file,
        task_filter,
        task_text,
    )
    .text
}

/// A rendered prompt plus the census of what the per-wave material cut did to
/// it. [`render_prompt_at`] is the text-only face for the many callers that do
/// not report; `run` takes this one, because reporting what was held back IS
/// its job.
pub(crate) struct RenderedPrompt {
    /// The prompt text — byte-identical to what [`render_prompt_at`] returns.
    pub text: String,
    /// What the per-wave cut carried and what it held back.
    pub material: MaterialCensus,
    /// Size of the TASK body, from the source — see [`task_body_chars`].
    pub task_chars: usize,
}

/// [`render_prompt_at`] with the material census kept instead of discarded.
#[allow(clippy::too_many_arguments)] // mirrors the CLI flag surface 1:1
pub(crate) fn render_prompt_with_census(
    project: &Path,
    spec: Option<&str>,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    retry_context_file: Option<&Path>,
    task_filter: Option<&str>,
    task_text: Option<&str>,
) -> RenderedPrompt {
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
        return RenderedPrompt { text: String::new(), material: MaterialCensus::default(), task_chars: 0 };
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
    // The prompt TELLS the agent which instruction file to open, and under a
    // private install that is not the same file the Guards were read from: the
    // scan writes `CLAUDE.local.md` beside the client's `CLAUDE.md` and never
    // into it. A literal in the template is still a call site choosing a
    // filename — the defect this unit removed from every reader — so the name
    // comes from the same resolver they use, and resolves to `CLAUDE.md` on a
    // shared install, leaving the cached prefix byte-identical to today's.
    let guards_file = crate::shared::context::guards_file_name(&project).to_string();
    let guards_summary = read_guards_block(&project, &project.join(&subproject_str));
    // The narrative locale is the project's text language, with or without a
    // spec: a spec is written in the project's language, so there is no second
    // one to read beside it.
    let spec_lang = mustard_core::ProjectConfig::load(&project)
        .language()
        .text_or_default()
        .as_str()
        .to_string();
    let role_block = build_role_block(role, &project, &subproject_str, &spec_lang);
    // The RULER this wave is measured by — the criteria QA will EXECUTE,
    // verbatim, `Command:` and all, cut by the `satisfies:` line the wave's own
    // `spec.md` carries. The source is the file the JUDGE reads (the
    // `wave-plan.md` union, and the parent's section only for a spec no wave
    // plan materialised — see `sections::ruler_source`), so reader and judge
    // cannot name different commands. Read at render time, never from a copy:
    // the layout is frozen after approval, so a copy would never see an
    // `ac-amend`. Empty for a wave whose line names none (heading collapses),
    // which is the same silence the prompt had before.
    //
    // A WAVE render filters by the WAVE's own spec, never `op_spec_path`: that
    // path falls back to the PARENT `spec.md` whenever the wave directory cannot
    // be found (unmaterialised, a number past `total_waves`, a renamed folder),
    // and the fallback would render the union of EVERY wave's criteria under
    // "these are the JUDGE of this wave" — the exact noise the per-wave cut
    // exists to remove. No wave directory means no `satisfies:` line, and the
    // honest answer is the empty section. A spec-level render (no `--wave`) is
    // the unit itself, so there the parent's whole section IS its ruler.
    //
    // A SPEC-LESS render carries none: with no `--spec`, `spec_dir` is the
    // PROJECT ROOT and `op_spec_path` a root `spec.md`. A repository that
    // happens to keep one at its root — or a `wave-N-*` directory — would have
    // another project's criteria rendered under "these are the JUDGE of this
    // wave", for a `/scan` guards enrich or a scopeless `/task`, which are
    // judged by no criterion at all.
    //
    // So "is there a spec directory at all" is resolved ONCE, as an Option,
    // before either block, and both blocks derive from it. The parent path is
    // handed to both unfiltered — each resolves its own source from it:
    // `## WHY` cuts the PARENT whole (`read_parent_spec` owns the `spec.md` /
    // `spec.original.md` fallback a rewave's archiving needs), `## ACCEPTANCE`
    // cuts the union QA executes and filters it by the wave's own `satisfies:`
    // frontmatter (the wave's spec, found by `find_wave_spec_path` — `None`
    // when unmaterialised, which renders no ruler; absent on a spec-level
    // render, which renders the whole section). Neither arm carries its own
    // guard. The `--wave N` arm used to check nothing and scanned the project
    // root for `wave-N-*` on a spec-less render.
    let spec_root: Option<&Path> = spec.map(|_| spec_dir.as_path());
    let parent_spec: Option<PathBuf> = spec_root.map(|d| d.join("spec.md"));
    let acceptance_block = match (spec_root, wave) {
        (Some(d), Some(w)) => find_wave_spec_path(d, w)
            .map(|ws| read_wave_acceptance(&d.join("spec.md"), Some(&ws)))
            .unwrap_or_default(),
        (Some(d), None) => read_wave_acceptance(&d.join("spec.md"), None),
        (None, _) => String::new(),
    };
    // WHY the work exists, and the ground the unit deliberately does not cover —
    // the parent spec's `## Context` + `## Non-Goals`. It rides from the PARENT
    // (never the wave, which carries neither) through the same path already open
    // for the material cut below, so it costs one more read of a file this
    // function already resolves. Spec-less renders have no parent and carry none.
    let why_block = parent_spec
        .as_deref()
        .map(build_why_block)
        .unwrap_or_default();
    // Both blocks are composed BEFORE the TASK, and that order is load-bearing:
    // the TASK's tier-2 fallback is a POINTER at these two sections, and it is
    // written from the blocks themselves rather than from a second reading of
    // some spec. The second reading was the defect — it asked the WAVE's spec
    // whether the prompt carries a narrative, while `## WHY` is cut from the
    // PARENT's, so a wave could be handed the section and told it did not have
    // it in the same breath.
    let task_steps = {
        let raw = read_task_steps(&op_spec_path, &why_block, &acceptance_block);
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
    // The duties this wave owes the WORLD, declared in the plan and materialised
    // into the wave's own `spec.md`. They ride as their OWN section rather than
    // inside `## TASK`, because a duty to check something outside the repository
    // is not a step of the work — it is a precondition for it, and the one time
    // it caught a provider-semantics inversion in the field it was prose someone
    // happened to write. Empty for a wave that declares none (heading collapses)
    // and for spec-less renders, which have no wave spec to read.
    let reality_obligations = read_reality_obligations(&op_spec_path);
    // `acceptance_block` and `why_block` were composed above, before the TASK
    // that points at them.
    //
    // What the CONVERSATION established, carried in by `spec-draft --material`
    // and living ONCE in the PARENT spec (`## Definitions` / `## Decisions` /
    // `## Evidence`). A per-wave copy would drift, so the cut happens HERE:
    // definitions and decisions bind every wave; a finding rides only to the
    // wave whose declared `## Files` contains its file — see
    // [`build_conversation_material`]. Spec-less renders carry none, and a spec
    // that carried nothing yields "" so the heading collapses and the prompt is
    // byte-identical to one rendered before the channel existed. It sits in the
    // VARIABLE tail of the template (after `## EFFICIENCY`), never in the
    // prefix-stable head — carrying context is worthless if it breaks the
    // prompt cache on every dispatch. The parent path is the SAME one `## WHY`
    // and `## ACCEPTANCE` hand over: the helper resolves `spec.md` →
    // `spec.original.md` itself, so a rewave's archiving cannot starve this
    // section while the other two keep reading the archive.
    let (conversation_material, material_census) = parent_spec
        .as_deref()
        .map(|p| build_conversation_material(p, &op_spec_path))
        .unwrap_or_default();
    // A task that NAMES a material item gets that item echoed right under it.
    // The material still lives once, in its own section — this resolves the
    // pointer where the work happens, so the agent is not left matching 28
    // context items against 13 tasks by itself.
    let task_steps = sections::echo_cited_material(&task_steps, &conversation_material);
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
    // Fold in `decision` events captured from prior waves' `<MEMORY>` blocks
    // (see `hooks::task::subagent_inject::capture_memory_decision`) — the
    // durable cross-wave lesson channel. No relevance filter and no count
    // cap, UNLIKE capabilities/spec-memory above: emission is already
    // gated at the SOURCE (the role's strict "real choice + a future agent
    // would decide worse" bar), so volume stays small by construction —
    // adding a second filter here would just re-litigate a decision the
    // role contract already made.
    let decisions_block = decision_events_block(&project, spec_key);
    if !decisions_block.is_empty() {
        if !cross_wave_memory.is_empty() {
            cross_wave_memory.push_str("\n\n");
        }
        cross_wave_memory.push_str(&decisions_block);
    }
    // Inject the regression vocabulary so the child agent sees
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
    // The subproject's skill shelf rides in the prompt DETERMINISTICALLY —
    // names + trigger descriptions, never bodies, so the section stays
    // PREFIX-STABLE (see refs/agent-prompt/agent-prompt.md). Field evaluation
    // proved the pattern: artifacts pushed into context (Guards) get used;
    // artifacts waiting to be retrieved idle. No scoring, no LLM — the flat
    // list scoped to the dispatch's subproject. The `patterns` role is
    // deliberately excluded: it AUTHORS the molds, and seeing the previous
    // generation would bias the fresh re-author the sweep just enabled.
    let skills_list = if role.trim().eq_ignore_ascii_case("patterns") {
        String::new()
    } else {
        build_skills_list(&project, &subproject_str)
    };

    // The shelf's other half: WHICH of those molds govern the files this wave
    // declares. Same exclusion as the shelf — the `patterns` role authors the
    // molds, so it is never prescribed its own previous generation.
    let mold_pointer = if role.trim().eq_ignore_ascii_case("patterns") {
        String::new()
    } else {
        build_mold_pointer(&project, &subproject_str, &op_spec_path)
    };

    // Remaining deterministic placeholders the dispatch template carries:
    //   {reference_files}  the spec's `## Files`/`## Arquivos` list + public
    //                      signatures of those files via tree-sitter
    let reference_files = build_reference_files(&project, &subproject_str, &op_spec_path);
    // The retry prompt (granular / fix-loop) carries the WHY a wave was
    // rejected. An explicit `--retry-context-file` still wins verbatim (the
    // historical contract); otherwise compose the context from what the spec
    // already recorded — the review verdict + persisted findings + the
    // prior-wave diff and change requests already built above — so the
    // re-dispatched implementer is not sent back in blind. First mode never
    // carries retry context.
    let retry_context = match (mode, retry_context_file) {
        (RenderMode::First, _) => String::new(),
        (_, Some(path)) => mfs::read_to_string(path).unwrap_or_default(),
        (_, None) => compose_retry_context(
            &project,
            spec,
            &spec_dir,
            Some(subproject_str.as_str()),
            &prior_wave_diff,
            &change_log,
        ),
    };

    // No size budget: every placeholder rides in full. Relevance is the only
    // filter on what enters the prompt — the spec-memory gate and the
    // relevance-filtered context slice decide membership; nothing is trimmed by
    // token count.

    // ---- Substitute placeholders. ----
    // Values in TEMPLATE_PLACEHOLDERS order — the array length is pinned to the
    // constant, so adding a key without its value fails to compile.
    let values: [&str; TEMPLATE_PLACEHOLDERS.len()] = [
        &subproject_str,
        &guards_file,
        &guards_summary,
        &role_block,
        &spec_lang,
        &task_steps,
        &context_md,
        &prior_wave_diff,
        &change_log,
        &reality_obligations,
        &conversation_material,
        &cross_wave_memory,
        &reference_files,
        &skills_list,
        &mold_pointer,
        &why_block,
        &acceptance_block,
        &retry_context,
    ];
    for (key, value) in TEMPLATE_PLACEHOLDERS.iter().zip(values.iter()) {
        rendered = rendered.replace(key, value);
    }

    // ---- Drop headings whose fail-open body resolved to empty. ----
    // `## GUARDS`, `## SHARED LANGUAGE`, `## REFERENCE`, `## WHY`,
    // `## ACCEPTANCE`, `## CONVERSATION MATERIAL`, `## CROSS-WAVE MEMORY` and
    // `## PRIOR WAVE DIFF` all degrade to "" on the spec-less / wave-1 /
    // no-Files / no-material / no-criteria paths; a dangling empty heading is
    // negative signal, so collapse it.
    rendered = collapse_empty_sections(&rendered);

    // ---- Blank only the TEMPLATE placeholders left unfilled (warn on each). ----
    // A `{token}` that came in through substituted spec content is author text,
    // not a render gap — `strip_unfilled_template_tokens` leaves it verbatim.
    let (stripped, unfilled) = strip_unfilled_template_tokens(&rendered, &template_tokens);
    rendered = stripped;
    for token in unfilled {
        eprintln!("agent-prompt-render: WARN: unfilled placeholder {token}");
    }

    // Git boundary: when the target subproject is its OWN nested git repository
    // (a submodule), the implementer must know it commits in a SEPARATE history
    // and must not bump the superproject's gitlink pointer itself — the `/git`
    // parent step owns that sync (the prohibition alone left the pointer stale
    // forever, since nobody was named as its owner). Appended after the
    // template (not a placeholder) so it rides in EVERY mode / role; EN by the
    // agent-prompt policy regardless of the project locale.
    let boundary = git_boundary_block(&project, &subproject_str);
    if !boundary.is_empty() {
        rendered.push_str("\n\n");
        rendered.push_str(&boundary);
    }

    RenderedPrompt { text: rendered, material: material_census, task_chars: task_body_chars(&task_steps) }
}

// ---------------------------------------------------------------------------
// Compositor-private helpers
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

/// Read the diff captured by wave `wave_num` (per the canonical path catalog: the file
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
        if let Ok(wp) = sp.for_wave(name_str)
            && let Ok(text) = mfs::read_to_string(wp.diff_md_path())
                && !text.is_empty() {
                    return text;
                }
    }
    String::new()
}

/// Fold the spec's captured `decision` events into a compact `## DECISIONS`
/// block — the durable home for `<MEMORY>` blocks a prior wave's `impl`
/// agent emitted (harvested automatically at `SubagentStop`, see
/// `hooks::task::subagent_inject::capture_memory_decision`). Reads the
/// spec's OWN per-spec NDJSON event log (never the whole project's), keeps
/// only `event == "decision"` rows, dedupes exact repeats. Empty when the
/// spec has none — the `""` collapses the section via
/// `collapse_empty_sections`, so a spec with no captured decisions yet is
/// silent, not a dangling empty heading. Fail-open: a missing/unreadable
/// spec dir degrades to no events.
fn decision_events_block(project: &Path, spec: &str) -> String {
    if spec.is_empty() {
        return String::new();
    }
    let Some(events_dir) = ClaudePaths::for_project(project)
        .and_then(|p| p.for_spec(spec))
        .ok()
        .map(|sp| sp.events_dir())
    else {
        return String::new();
    };
    let events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(&events_dir);
    let mut lines: Vec<String> = events
        .iter()
        .filter(|e| e.event == "decision")
        .filter_map(|e| e.payload.get("title").and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("- {t}"))
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    lines.dedup();
    format!("## DECISIONS\n{}", lines.join("\n"))
}

/// The fixed `## GIT BOUNDARY` block for a submodule subproject — appended to a
/// rendered prompt when `subproject`'s own directory is a nested git repository
/// root (`.git` dir or pointer file). Empty for the superproject root (`"."` /
/// empty) or a plain subproject, so the block only appears where the boundary is
/// real. Reuses the single git-root probe
/// [`mustard_core::io::workspace::is_git_repo_root`] — the SAME fact the scan
/// census records as `own_git_root` and dispatch threads onto the item — so the
/// three surfaces can never disagree. EN by policy (agent prompts stay English).
fn git_boundary_block(project: &Path, subproject: &str) -> String {
    let sub = subproject.trim().trim_end_matches('/');
    if sub.is_empty() || sub == "." {
        return String::new();
    }
    if !mustard_core::io::workspace::is_git_repo_root(&project.join(sub)) {
        return String::new();
    }
    format!(
        "## GIT BOUNDARY\n\nThis subproject (`{sub}`) is its OWN git repository — it has a \
         SEPARATE commit history. Commit INSIDE this subproject only; never bump the \
         superproject's gitlink pointer YOURSELF — the `/git` parent step owns that sync, right \
         after your commit lands."
    )
}

/// Read the spec's `change-log.md` (mid-pipeline requests) for the prompt's
/// `## CHANGE REQUESTS` section. Keeps only the request bullets (drops the
/// title + explanatory blurb). Fail-open: a missing/unreadable file, or one with
/// no bullets, yields an empty string — which collapses the heading.
fn read_change_log(spec_dir: &Path) -> String {
    mfs::read_to_string(spec_dir.join("change-log.md"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.trim_start().starts_with("- "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    /// The render measures what each wave's prompt carries.
    ///
    /// A hollow wave used to be invisible until its agent came back with
    /// something thin. These two counts make it visible BEFORE the dispatch.
    #[test]
    fn render_reports_material_counts_per_wave() {
        // Written line by line on purpose: an earlier version of this literal
        // used string continuations, which left the headings INDENTED. It
        // passed only because the search was loose enough to match a heading
        // anywhere in the text — the very defect this section-cutter now
        // refuses. A heading is a line, so the fixture must contain lines.
        let prompt = concat!(
            "## CONVERSATION MATERIAL\n",
            "\n",
            "- **termo** — o que significa\n",
            "- decisao com razao\n",
            "\n",
            "## TASK\n",
            "\n",
            "faca a coisa\n",
            "em duas linhas\n",
            "\n",
            "## EFFICIENCY\n",
            "\n",
            "x\n",
        );
        assert_eq!(super::material_line_count(prompt), 2, "both bullets counted");

        // A collapsed material section answers zero — the number worth seeing.
        let hollow = "## TASK\n\nfaca\n";
        assert_eq!(super::material_line_count(hollow), 0);
    }

    /// The TASK size is measured on the SOURCE, and a wave's own `## Tasks`
    /// heading is not work — it is the label on the work.
    ///
    /// This is the shape that made the old measurement lie. `read_task_steps`
    /// returns the section WITH its heading, so the substituted body opens with
    /// a `## ` line; re-parsing the rendered prompt stopped on that line and
    /// reported ZERO for every wave that had a real task list — a number whose
    /// whole job was to expose a hollow wave, answering `0` for healthy ones.
    #[test]
    fn task_size_is_measured_on_the_source_and_skips_the_heading() {
        let steps = "## Tasks\n\n- [ ] faca a coisa\n- [ ] em duas linhas\n";
        assert_eq!(
            super::task_body_chars(steps),
            "- [ ] faca a coisa".len() + "- [ ] em duas linhas".len() + 2,
            "the `## Tasks` heading and the blank lines are not the body",
        );
        // A wave with no tasks at all is the one case that must read zero.
        assert_eq!(super::task_body_chars(""), 0);
        assert_eq!(super::task_body_chars("## Tasks\n\n"), 0, "a bare heading is not work");
    }

    use super::*;
    use tempfile::tempdir;

    /// Plant a workspace anchor so `ClaudePaths::for_project` accepts the temp dir.
    fn anchor(dir: &Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("mustard.json"), b"{}").unwrap();
    }

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
    fn dispatch_render_lean_spec_yields_nonempty_task_block() {
        // End-to-end: render the dispatch block for a spec with no `## Tasks`
        // section and assert the `## TASK` placeholder is filled (non-blank).
        let spec_body = "# T\n## Causa raiz\nnull deref in parse\n## Plano\n- guard the option\n";
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, spec_body).unwrap();

        let task_steps = read_task_steps(
            &path,
            &sections::build_why_block(&path),
            &sections::read_wave_acceptance(&path, None),
        );
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

    /// The retry template carries a `## RETRY CONTEXT` heading that collapses
    /// when the body is empty and survives (with its body) when filled — the fix
    /// for the bare `{retry_context}` that had no heading at all.
    #[test]
    fn retry_template_retry_context_heading_collapses_when_empty() {
        let body = extract_block(TEMPLATE, "retry").expect("retry block present");
        assert!(body.contains("## RETRY CONTEXT"), "retry heading missing: {body}");
        // Empty retry_context → the heading collapses.
        let empty = collapse_empty_sections(&body.replace("{retry_context}", ""));
        assert!(!empty.contains("## RETRY CONTEXT"), "empty heading must collapse: {empty}");
        // Non-empty retry_context → the heading and its body survive.
        let filled = collapse_empty_sections(
            &body.replace("{retry_context}", "### Review findings\n- x"),
        );
        assert!(filled.contains("## RETRY CONTEXT"), "filled heading must survive: {filled}");
        assert!(filled.contains("Review findings"), "filled body present: {filled}");
    }

    /// Guard 3 of `## CONTEXT` splits the language rule in two, and the split is
    /// what the whole rule means: a comment follows the project locale (like the
    /// spec narrative), everything else in the code stays English. Asserted on
    /// the two SIDES of the "stays English" sentence rather than on the phrasing,
    /// so a reword keeps passing while moving a term across the divide fails.
    #[test]
    fn the_rendered_guard_puts_comments_under_the_project_language() {
        let body = extract_block(TEMPLATE, "dispatch").expect("dispatch block present");
        let rendered = body.replace("{spec_lang}", "pt-BR");
        let guard = rendered
            .lines()
            .find(|l| l.starts_with("3. Spec language is"))
            .expect("guard 3 present in the dispatch template");
        assert!(guard.contains("pt-BR"), "guard 3 must name the rendered locale: {guard}");

        let (locale_side, english_side) = guard
            .split_once("stays English")
            .expect("guard 3 must still name an English-only side");
        // Every comment form sits on the locale side, none on the English side.
        for form in [
            "`//`",
            "`#`",
            "`/* */`",
            "`///`",
            "`'''`",
            "`\"\"\"`",
            "doc-comments",
            "`<!-- -->`",
        ] {
            assert!(
                locale_side.contains(form),
                "comment form {form} must sit on the project-language side: {guard}"
            );
            assert!(
                !english_side.contains(form),
                "comment form {form} must not be listed as English-only: {guard}"
            );
        }
        // What is code identity, not commentary, stays English.
        for term in ["identifiers", "file paths", "shell commands", "`Command:`", "log"] {
            assert!(
                english_side.contains(term),
                "{term} must stay on the English side: {guard}"
            );
        }
        // The surgical clause survives the rewrite: no mass translation pass.
        assert!(
            guard.contains("never translate pre-existing comments"),
            "the no-mass-translation clause must survive: {guard}"
        );
    }

    #[test]
    fn dispatch_render_fills_placeholders_and_leaves_no_unfilled() {
        // End-to-end: assemble the dispatch block, substitute the deterministic
        // placeholders with realistic values, then assert no `{...}` placeholder
        // remains (the `scan_unfilled` contract).
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

        let task_steps = read_task_steps(
            &spec,
            &sections::build_why_block(&spec),
            &sections::read_wave_acceptance(&spec, None),
        );
        let reference_files = build_reference_files(dir.path(), "api", &spec);
        assert!(!reference_files.is_empty(), "reference_files empty");

        let mut rendered = extract_block(TEMPLATE, "dispatch").expect("dispatch block");
        // The removed `{entity_info}` / `{recommended_skills}` / `{context_extras}`
        // placeholders are no longer in the template, so they are not substituted
        // here either.
        let subs: &[(&str, &str)] = &[
            ("{subproject}", "api"),
            ("{guards_file}", "CLAUDE.md"),
            ("{guards_summary}", "g"),
            ("{role_block}", "ROLE: review"),
            ("{spec_lang}", "en-US"),
            ("{task_steps}", &task_steps),
            ("{context_md}", ""),
            ("{prior_wave_diff}", ""),
            ("{change_log}", ""),
            ("{reality_obligations}", ""),
            ("{conversation_material}", ""),
            ("{cross_wave_memory}", ""),
            ("{reference_files}", &reference_files),
            ("{skills_list}", ""),
            ("{mold_pointer}", ""),
            ("{why_block}", ""),
            ("{acceptance_block}", ""),
            ("{retry_context}", ""),
        ];
        for (k, v) in subs {
            rendered = rendered.replace(k, v);
        }
        // Mirror run(): collapse the now-empty sections (SHARED LANGUAGE, CROSS-WAVE
        // MEMORY, PRIOR WAVE DIFF) before the unfilled-placeholder check.
        let rendered = collapse_empty_sections(&rendered);
        assert!(rendered.contains("widget.rs"), "reference_files not rendered");
        assert!(
            scan_unfilled(&rendered).is_empty(),
            "unfilled placeholders remain: {:?}",
            scan_unfilled(&rendered)
        );
    }

    // --- decision_events_block: cross-wave `<MEMORY>` delivery --------------

    /// Append one `decision` event to `spec`'s own NDJSON log — mirrors
    /// exactly the shape `hooks::task::subagent_inject::capture_memory_decision`
    /// writes in production, so this test proves the SAME reader the render
    /// uses can fold back what that hook emits (not a parallel fixture format
    /// that could silently drift from the real writer).
    fn seed_decision_event(project: &Path, spec: &str, title: &str) {
        use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
        let event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: mustard_core::time::now_iso8601(),
            session_id: "test-session".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Hook,
                id: Some("subagent_inject".to_string()),
                actor_type: None,
            },
            event: "decision".to_string(),
            payload: serde_json::json!({ "title": title, "role": "impl", "source": "memory-block" }),
            spec: Some(spec.to_string()),
        };
        let _ = crate::shared::events::route::emit(&project.to_string_lossy(), &event);
    }

    #[test]
    fn decision_events_block_empty_when_no_decisions_captured() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        std::fs::create_dir_all(dir.path().join(".claude/spec/fresh-spec")).unwrap();
        assert_eq!(decision_events_block(dir.path(), "fresh-spec"), "");
        // Empty spec key (spec-less render) is also empty — never panics.
        assert_eq!(decision_events_block(dir.path(), ""), "");
    }

    #[test]
    fn decision_events_block_renders_captured_titles_only() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        std::fs::create_dir_all(dir.path().join(".claude/spec/rbac-spec")).unwrap();
        seed_decision_event(dir.path(), "rbac-spec", "chose X over Y because Z");
        // A non-decision event in the SAME log must not leak in.
        seed_decision_event(dir.path(), "rbac-spec", "");
        let block = decision_events_block(dir.path(), "rbac-spec");
        assert!(block.starts_with("## DECISIONS\n"), "{block}");
        assert!(block.contains("- chose X over Y because Z"), "{block}");
        // The blank-title seed contributes nothing (filtered, not a blank bullet).
        assert!(!block.contains("- \n") && !block.trim_end().ends_with("- "), "{block}");
    }

    /// End-to-end: a decision captured for wave 1 shows up in the FULL
    /// rendered dispatch prompt for wave 2 of the SAME spec — the actual
    /// consumer path (`render_prompt_at`), not just the block builder in
    /// isolation. This is the fix for the traced gap: `<MEMORY>` used to
    /// evaporate after a wave; now it survives into the next wave's prompt.
    #[test]
    fn captured_decision_flows_into_next_wave_dispatch_prompt() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "cross-wave-memory-spec";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# T\n## Tasks\n- [ ] wave 2 task\n",
        )
        .unwrap();
        seed_decision_event(
            dir.path(),
            spec,
            "Chose atomic_md write over direct fs::write — a mid-write crash corrupts the file",
        );

        let rendered = render_prompt_at(
            dir.path(),
            Some(spec),
            None,
            "backend",
            Path::new("."),
            RenderMode::First,
            None,
            None,
            None,
        );
        assert!(
            rendered.contains("## DECISIONS"),
            "decisions heading missing from rendered prompt: {rendered}"
        );
        assert!(
            rendered.contains("Chose atomic_md write over direct fs::write"),
            "captured decision text missing from rendered prompt: {rendered}"
        );
    }

    /// A spec with zero captured decisions renders with NO `## DECISIONS`
    /// heading at all — `collapse_empty_sections` must drop it, not leave a
    /// dangling empty section (the negative-signal noise the render
    /// deliberately avoids everywhere else).
    #[test]
    fn no_captured_decisions_leaves_no_decisions_heading() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "no-decisions-spec";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# T\n## Tasks\n- [ ] a task\n").unwrap();

        let rendered = render_prompt_at(
            dir.path(), Some(spec), None, "backend", Path::new("."),
            RenderMode::First, None, None, None,
        );
        assert!(!rendered.contains("## DECISIONS"), "{rendered}");
    }

    // --- conversation material: the per-wave cut -----------------------------

    /// Plant a parent spec carrying the conversation material plus two waves
    /// with DISJOINT `## Files` lists — the shape the per-wave cut is defined
    /// against. `evidence` is spliced verbatim so a test can vary the findings
    /// without touching anything else.
    fn seed_material_spec(project: &Path, spec: &str, evidence: &str) {
        let spec_dir = project.join(".claude/spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            format!(
                "# T\n\n## Files\n\n- `src/alpha.rs`\n- `src/beta.rs`\n\n\
                 ## Definitions\n\n- **wave** — one level of the plan\n\n\
                 ## Decisions\n\n- everything branches off dev\n  Reason: the release train\n\n\
                 ## Evidence\n\n{evidence}"
            ),
        )
        .unwrap();
        for (dir, file, task) in [
            ("wave-1-alpha", "src/alpha.rs", "do alpha"),
            ("wave-2-beta", "src/beta.rs", "do beta"),
        ] {
            let wd = spec_dir.join(dir);
            std::fs::create_dir_all(&wd).unwrap();
            std::fs::write(
                wd.join("spec.md"),
                format!("# W\n\n## Files\n\n- `{file}`\n\n## Tasks\n\n- [ ] {task}\n"),
            )
            .unwrap();
        }
    }

    fn render_wave(project: &Path, spec: &str, wave: u32) -> String {
        render_prompt_at(
            project, Some(spec), Some(wave), "impl", Path::new("."),
            RenderMode::First, None, None, None,
        )
    }

    /// A finding reaches ONLY the wave whose declared `## Files` contains its
    /// file — asserted in BOTH directions, because a cut that lets everything
    /// through is the same as no cut at all. Definitions and decisions are the
    /// shared vocabulary and the law of the work, so they reach every wave.
    #[test]
    fn findings_reach_only_the_wave_that_declares_the_file() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "carry-material-spec";
        seed_material_spec(
            dir.path(),
            spec,
            "- alpha parses the header twice\n  Evidence: `src/alpha.rs:12`\n\
             - beta swallows the error\n  Evidence: `src/beta.rs:30`\n",
        );

        let w1 = render_wave(dir.path(), spec, 1);
        let w2 = render_wave(dir.path(), spec, 2);

        // Wave 1 declares only `src/alpha.rs`.
        assert!(w1.contains("alpha parses the header twice"), "own finding missing: {w1}");
        assert!(!w1.contains("beta swallows the error"), "sibling finding leaked: {w1}");
        // Wave 2 declares only `src/beta.rs` — the mirror image.
        assert!(w2.contains("beta swallows the error"), "own finding missing: {w2}");
        assert!(!w2.contains("alpha parses the header twice"), "sibling finding leaked: {w2}");

        // Definitions and decisions are uncut: both waves carry both.
        for (label, rendered) in [("wave 1", &w1), ("wave 2", &w2)] {
            assert!(
                rendered.contains("## CONVERSATION MATERIAL"),
                "{label} lost the material heading: {rendered}"
            );
            assert!(
                rendered.contains("**wave** — one level of the plan"),
                "{label} lost the definitions: {rendered}"
            );
            assert!(
                rendered.contains("everything branches off dev"),
                "{label} lost the decisions: {rendered}"
            );
        }
    }

    /// The material rides in the VARIABLE tail: two renders of the same spec
    /// with DIFFERENT findings must leave the prefix-stable head byte-identical,
    /// or every dispatch pays full price instead of the cached-prefix price.
    #[test]
    fn carried_material_does_not_break_the_stable_prompt_head() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "stable-head-spec";

        seed_material_spec(
            dir.path(),
            spec,
            "- alpha parses the header twice\n  Evidence: `src/alpha.rs:12`\n",
        );
        let first = render_wave(dir.path(), spec, 1);
        seed_material_spec(
            dir.path(),
            spec,
            "- alpha leaks the file handle on the error path\n  Evidence: `src/alpha.rs:88`\n",
        );
        let second = render_wave(dir.path(), spec, 1);

        // Both renders carry material, so the marker is present in both and the
        // split is the real boundary between the stable head and the tail.
        let head = |r: &str| {
            r.split_once("## CONVERSATION MATERIAL")
                .map(|(h, _)| h.to_string())
                .expect("material section rendered")
        };
        assert_eq!(head(&first), head(&second), "the prefix-stable head must not vary");
        // ...and the change really did land, in the tail.
        assert!(first.contains("parses the header twice"), "{first}");
        assert!(second.contains("leaks the file handle"), "{second}");
        assert!(!second.contains("parses the header twice"), "{second}");
    }

    /// A spec with no material is INVISIBLE to this wave: no heading, no
    /// placeholder, no blank section — the prompt is exactly what it was before
    /// the channel existed.
    #[test]
    fn spec_without_material_renders_no_conversation_section() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "no-material-spec";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# T\n\n## Tasks\n\n- [ ] a task\n").unwrap();

        let rendered = render_wave(dir.path(), spec, 1);
        assert!(!rendered.contains("## CONVERSATION MATERIAL"), "{rendered}");
        assert!(!rendered.contains("{conversation_material}"), "{rendered}");
        // Spec-less renders never look for material either.
        let spec_less = render_prompt_at(
            dir.path(), None, None, "impl", Path::new("."),
            RenderMode::First, None, None, Some("ad-hoc task"),
        );
        assert!(!spec_less.contains("## CONVERSATION MATERIAL"), "{spec_less}");
    }

    /// The three sections cut from the PARENT survive its archiving TOGETHER. A
    /// rewave renames `spec.md` to `spec.original.md` in step 9, and `## WHY`
    /// and `## ACCEPTANCE` already followed it there — the material arm did
    /// not, so a wave rendered after the archiving carried the story and the
    /// ruler and an EMPTY material section. All three read the parent by the
    /// one rule (`read_parent_spec`), so they cannot disagree on which file it
    /// is; the per-wave cut of the material is untouched by the fallback.
    #[test]
    fn conversation_material_survives_the_parent_archiving() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "archived-parent-spec";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(spec_dir.join("wave-1-impl")).unwrap();
        std::fs::create_dir_all(spec_dir.join("wave-2-impl")).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# T\n\n## Contexto\n\no despacho chega sem o porquê\n\n\
             ## Files\n\n- `src/alpha.rs`\n- `src/beta.rs`\n\n\
             ## Tasks\n\n- [ ] parent task\n\n\
             ## Acceptance Criteria\n\n\
             - **AC-1** — alpha holds.\n  Command: `cargo test alpha`\n\
             - **AC-2** — beta holds.\n  Command: `cargo test beta`\n\n\
             ## Definitions\n\n- **wave** — one level of the plan\n\n\
             ## Decisions\n\n- everything branches off dev\n  Reason: the release train\n\n\
             ## Evidence\n\n\
             - alpha parses the header twice\n  Evidence: `src/alpha.rs:12`\n\
             - beta swallows the error\n  Evidence: `src/beta.rs:30`\n",
        )
        .unwrap();
        for (wave, file, ac) in [("wave-1-impl", "src/alpha.rs", "AC-1"), ("wave-2-impl", "src/beta.rs", "AC-2")] {
            std::fs::write(
                spec_dir.join(wave).join("spec.md"),
                format!(
                    "---\nid: wave.{spec}.{wave}\nsatisfies: [{ac}]\n---\n\n\
                     # W\n\n## Files\n\n- `{file}`\n\n## Tasks\n\n- [ ] do it\n"
                ),
            )
            .unwrap();
        }

        // The archiving a rewave performs: the parent moves, nothing else does.
        std::fs::rename(spec_dir.join("spec.md"), spec_dir.join("spec.original.md")).unwrap();

        let rendered = render_wave(dir.path(), spec, 1);
        assert!(rendered.contains("## WHY"), "o arquivamento matou o porquê: {rendered}");
        assert!(rendered.contains("o despacho chega sem o porquê"), "{rendered}");
        assert!(rendered.contains("## ACCEPTANCE"), "o arquivamento matou a régua: {rendered}");
        assert!(rendered.contains("Command: `cargo test alpha`"), "{rendered}");
        assert!(
            rendered.contains("## CONVERSATION MATERIAL"),
            "o arquivamento matou o material — o canal que só este braço lia direto: {rendered}"
        );
        assert!(rendered.contains("**wave** — one level of the plan"), "definitions lost: {rendered}");
        assert!(rendered.contains("everything branches off dev"), "decisions lost: {rendered}");
        // The per-wave cut is unchanged by the fallback: own finding rides, the
        // sibling's stays home — same as when the parent is still `spec.md`.
        assert!(rendered.contains("alpha parses the header twice"), "own finding lost: {rendered}");
        assert!(!rendered.contains("beta swallows the error"), "sibling finding leaked: {rendered}");
        assert!(!rendered.contains("cargo test beta"), "sibling criterion leaked: {rendered}");
    }

    /// The wave's prompt carries the RULER it will be judged by — the criteria
    /// QA will EXECUTE, verbatim, `Command:` included, filtered by the
    /// `satisfies:` line the wave's own `spec.md` carries — and only those: a
    /// criterion belonging to another wave must not ride.
    ///
    /// This is the whole path the field report named: the prompt had 15 fields
    /// and none of them was a criterion, so the executor was told where and what
    /// and never how it would be measured.
    ///
    /// The criteria live in `wave-plan.md` and NOT in the parent, which is the
    /// half the fixture is shaped to prove: the union is the file the judge
    /// reads, so it is the file the prompt cuts. Reading the parent instead put
    /// reader and judge on different documents wherever the two differ — a
    /// plan-local criterion the parent never defined, and a rewave whose parent
    /// was archived out from under the amendment doors.
    #[test]
    fn wave_prompt_carries_its_acceptance() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "ruler-spec";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(spec_dir.join("wave-1-impl")).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# T\n\n## Tasks\n\n- [ ] parent task\n").unwrap();
        std::fs::write(
            spec_dir.join("wave-plan.md"),
            "# Plan\n\n## Acceptance Criteria\n\n\
             - **AC-1** — alpha holds.\n  Command: `cargo test alpha`\n  Expect: `1 passed`\n\
             - **AC-2** — beta holds.\n  Command: `cargo test beta`\n",
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("wave-1-impl").join("spec.md"),
            "---\nid: wave.ruler-spec.1-impl\nsatisfies: [AC-1]\n---\n\n\
             # W\n\n## Tasks\n\n- [ ] do alpha\n",
        )
        .unwrap();

        let rendered = render_wave(dir.path(), spec, 1);
        assert!(rendered.contains("## ACCEPTANCE"), "the ruler section is missing: {rendered}");
        assert!(rendered.contains("**AC-1**"), "{rendered}");
        assert!(
            rendered.contains("Command: `cargo test alpha`"),
            "the judging command must ride verbatim: {rendered}"
        );
        assert!(rendered.contains("Expect: `1 passed`"), "{rendered}");
        // …and ONLY the criteria this wave satisfies: the sibling's stays home.
        assert!(!rendered.contains("AC-2") && !rendered.contains("cargo test beta"), "{rendered}");
        // The section says what it is: a judge, not a suggestion.
        assert!(rendered.contains("JUDGE of this wave"), "{rendered}");
        // The parent's `## Acceptance Criteria` heading is demoted, so it
        // nests under `## ACCEPTANCE` instead of terminating it.
        assert!(rendered.contains("### Acceptance Criteria"), "{rendered}");

        // A wave that declares none renders NO such section — the heading
        // collapses like every other fail-open placeholder.
        std::fs::create_dir_all(spec_dir.join("wave-2-impl")).unwrap();
        std::fs::write(
            spec_dir.join("wave-2-impl").join("spec.md"),
            "---\nid: wave.ruler-spec.2-impl\n---\n\n# W\n\n## Tasks\n\n- [ ] do beta\n",
        )
        .unwrap();
        let bare = render_wave(dir.path(), spec, 2);
        assert!(!bare.contains("## ACCEPTANCE"), "empty heading survived: {bare}");
        assert!(!bare.contains("AC-1"), "another wave's criterion leaked: {bare}");

        // Uma onda cujo diretório NÃO existe não empresta a régua do pai. O
        // caminho operacional cai para o `spec.md` do pai quando não acha a
        // onda, e renderizar a UNIÃO dos critérios sob "estes são o JUIZ desta
        // onda" é exatamente o ruído que o recorte por onda existe para tirar.
        let ghost = render_wave(dir.path(), spec, 9);
        assert!(
            !ghost.contains("## ACCEPTANCE"),
            "uma onda inexistente herdou a régua do pai: {ghost}"
        );

        // E o RE-DESPACHO carrega as duas seções. O agente de `fix-loop` é
        // re-despachado justamente por ter falhado um critério, então era o
        // único modo que nunca via o critério: os dois valores eram calculados e
        // substituídos num texto que não tinha nenhum dos dois marcadores.
        let retry = render_prompt_at(
            dir.path(), Some(spec), Some(1), "impl", Path::new("."),
            RenderMode::FixLoop, None, None, None,
        );
        assert!(retry.contains("## ACCEPTANCE"), "o re-despacho perdeu a régua: {retry}");
        assert!(
            retry.contains("Command: `cargo test alpha`"),
            "e o comando que a julga: {retry}"
        );
    }

    /// Um RE-DESPACHO de um plano de ondas que não nomeia onda é RECUSADO, com
    /// o flag que falta dito por nome.
    ///
    /// A prosa do fix-loop prescrevia a forma sem `--wave`, e o caminho de
    /// despacho sempre passou o flag: as duas discordavam e só quem lia a prosa
    /// pagava. Sem onda, o `## TASK` vira a checklist do PAI e a régua sai sem
    /// filtro — todos os critérios das irmãs sob "estes são o JUIZ desta onda",
    /// que é exatamente o ruído que o recorte por onda existe para tirar.
    ///
    /// Uma spec SEM plano de ondas (light, tactical-fix) não tem onda para
    /// nomear: o re-despacho dela é o mesmo render de nível-spec de sempre.
    #[test]
    fn a_retry_of_a_wave_plan_must_name_its_wave() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "retry-needs-wave";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(spec_dir.join("wave-1-impl")).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# T\n\n## Tasks\n\n- [ ] parent\n").unwrap();
        std::fs::write(spec_dir.join("wave-plan.md"), "# Plan\n").unwrap();

        for mode in [RenderMode::FixLoop, RenderMode::Granular] {
            let refusal = wave_flag_refusal(dir.path(), Some(spec), None, mode)
                .unwrap_or_else(|| panic!("{mode:?} sem --wave tem de ser recusado"));
            assert!(refusal.contains("--wave <n>"), "o flag que falta é dito: {refusal}");
            assert!(refusal.contains(mode.flag()), "e o modo do chamador: {refusal}");
            // Com a onda nomeada, nada é recusado.
            assert!(wave_flag_refusal(dir.path(), Some(spec), Some(1), mode).is_none());
        }
        // O primeiro despacho não é um re-despacho: ele nomeia a onda por outro
        // caminho e o nível-spec dele é legítimo.
        assert!(wave_flag_refusal(dir.path(), Some(spec), None, RenderMode::First).is_none());

        // Uma spec SEM `wave-plan.md` não é um plano de ondas, e o re-despacho
        // dela segue passando.
        let light = "light-spec";
        let light_dir = dir.path().join(".claude/spec").join(light);
        std::fs::create_dir_all(&light_dir).unwrap();
        std::fs::write(light_dir.join("spec.md"), "# TF\n\n## Tasks\n\n- [ ] fix\n").unwrap();
        assert!(wave_flag_refusal(dir.path(), Some(light), None, RenderMode::FixLoop).is_none());
        // E um render SEM spec nenhum também não (o `/task` sem escopo).
        assert!(wave_flag_refusal(dir.path(), None, None, RenderMode::FixLoop).is_none());
    }

    /// Um render SEM spec não tem régua nenhuma, e o bloco de aceitação tem de
    /// se calar — a MESMA curto-circuitação que o `{why_block}` faz uma
    /// instrução adiante, com o mesmo `spec.is_some()`.
    ///
    /// A regressão que isto tranca: o braço `None` chamava
    /// `read_wave_acceptance(&op_spec_path)` sempre. Sem `--spec` (o enriquecer
    /// do `/scan`, o `/task` sem escopo) o `spec_dir` cai para a RAIZ do projeto
    /// e o caminho operacional para um `spec.md` da raiz — então um repositório
    /// que por acaso carrega um renderizava os critérios de OUTRO projeto sob
    /// "estes são o JUIZ desta onda", para um trabalho que critério nenhum
    /// julga.
    #[test]
    fn a_spec_less_render_does_not_borrow_a_root_spec_as_its_ruler() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        // O `spec.md` que o repositório por acaso carrega na raiz.
        std::fs::write(
            dir.path().join("spec.md"),
            "# Outro projeto\n\n## Acceptance Criteria\n\n\
             - **AC-7** — a régua de outra unidade.\n  Command: `cargo test alheio`\n",
        )
        .unwrap();

        let spec_less = render_prompt_at(
            dir.path(), None, None, "guards", Path::new("."),
            RenderMode::First, None, None, Some("ad-hoc task"),
        );
        assert!(
            !spec_less.contains("## ACCEPTANCE"),
            "um render sem spec não tem régua: {spec_less}"
        );
        assert!(
            !spec_less.contains("AC-7") && !spec_less.contains("cargo test alheio"),
            "e nada do `spec.md` da raiz pode viajar nele: {spec_less}"
        );

        // A metade que não pode quebrar junto: COM spec, a régua continua
        // chegando exatamente como antes.
        let spec = "ruler-still-rides";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(spec_dir.join("wave-1-impl")).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# T\n\n## Tasks\n\n- [ ] parent task\n\n## Acceptance Criteria\n\n\
             - **AC-1** — alpha holds.\n  Command: `cargo test alpha`\n",
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("wave-1-impl").join("spec.md"),
            "---\nid: wave.ruler-still-rides.1-impl\nsatisfies: [AC-1]\n---\n\n\
             # W\n\n## Tasks\n\n- [ ] do alpha\n",
        )
        .unwrap();
        let with_spec = render_wave(dir.path(), spec, 1);
        assert!(with_spec.contains("## ACCEPTANCE"), "{with_spec}");
        assert!(with_spec.contains("Command: `cargo test alpha`"), "{with_spec}");
    }

    /// O IRMÃO do teste acima, no outro braço: `--wave N` SEM `--spec`.
    ///
    /// A regressão que isto tranca: o braço `None` do bloco de aceitação
    /// curto-circuitava sem spec e o braço `Some(wave)` NÃO — ele chamava
    /// `find_wave_spec_path(&spec_dir, w)` sempre, e sem `--spec` o `spec_dir`
    /// é a RAIZ do projeto. Um repositório com um diretório `wave-1-*/spec.md`
    /// na raiz renderizava a régua dele para um trabalho que critério nenhum
    /// julga. Os dois braços agora derivam do MESMO `Option`, resolvido uma
    /// vez, e nenhum carrega guarda própria.
    #[test]
    fn a_spec_less_wave_render_does_not_scan_the_project_root_for_a_wave() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        // O `wave-1-*` que o repositório por acaso carrega na raiz, e o
        // `spec.md` de raiz que a linha dele apontaria.
        std::fs::create_dir_all(dir.path().join("wave-1-alheio")).unwrap();
        std::fs::write(
            dir.path().join("wave-1-alheio").join("spec.md"),
            "---\nid: wave.alheio.1-alheio\nsatisfies: [AC-7]\n---\n\n# Outra onda\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("spec.md"),
            "# Outro projeto\n\n## Acceptance Criteria\n\n\
             - **AC-7** — a régua de outra unidade.\n  Command: `cargo test alheio`\n",
        )
        .unwrap();

        let spec_less = render_prompt_at(
            dir.path(), None, Some(1), "guards", Path::new("."),
            RenderMode::First, None, None, Some("ad-hoc task"),
        );
        assert!(
            !spec_less.contains("## ACCEPTANCE"),
            "um render sem spec não tem régua, com ou sem `--wave`: {spec_less}"
        );
        assert!(
            !spec_less.contains("AC-7") && !spec_less.contains("cargo test alheio"),
            "e nada de um `wave-1-*` da raiz pode viajar nele: {spec_less}"
        );
    }

    /// A REGRESSÃO que este teste tranca: o ponteiro do fallback de TASK negava
    /// uma seção que ESTÁ no prompt.
    ///
    /// A configuração é a de campo: a onda não tem `## Tasks` (então o fallback
    /// dispara), não tem `## Contexto` próprio (o `spec.md` dela nunca tem — o
    /// `wave-scaffold` não escreve narrativa), e o PAI tem os dois. O ponteiro
    /// perguntava ao texto da ONDA se o prompt carrega narrativa, e o `## WHY` é
    /// recortado do PAI: o prompt saía com a seção renderizada e, logo abaixo
    /// dela, a frase dizendo que ele não diz por que o trabalho existe.
    ///
    /// As duas seções são medidas no MESMO prompt renderizado — é o par que a
    /// contradição exige, e nenhuma metade sozinha a enxerga.
    #[test]
    fn the_task_pointer_agrees_with_the_sections_the_prompt_carries() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "pointer-spec";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(spec_dir.join("wave-1-impl")).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# T\n\n## Contexto\n\no despacho chega sem o porquê\n\n\
             ## Não-Objetivos\n\n- não endurecer o portão de resíduo\n\n\
             ## Tasks\n\n- [ ] parent task\n\n## Acceptance Criteria\n\n\
             - **AC-1** — alpha holds.\n  Command: `cargo test alpha`\n",
        )
        .unwrap();
        // A onda: sem `## Tasks` (o fallback dispara) e sem narrativa própria.
        std::fs::write(
            spec_dir.join("wave-1-impl").join("spec.md"),
            "---\nid: wave.pointer-spec.1-impl\nsatisfies: [AC-1]\n---\n\n\
             # W\n\n## Files\n\n- `src/alpha.rs`\n",
        )
        .unwrap();

        let rendered = render_wave(dir.path(), spec, 1);
        // As duas seções ESTÃO no prompt…
        assert!(rendered.contains("## WHY"), "o `## WHY` do pai tem de renderizar: {rendered}");
        assert!(
            rendered.contains("o despacho chega sem o porquê"),
            "com o texto dele: {rendered}"
        );
        assert!(rendered.contains("## ACCEPTANCE"), "e a régua da onda: {rendered}");
        // …e o ponteiro concorda com as duas, em vez de negar uma delas.
        assert!(
            rendered.contains("TASK fallback"),
            "precondição: a onda sem `## Tasks` cai no tier 2: {rendered}",
        );
        assert!(
            !rendered.contains("no narrative section reached it"),
            "o prompt carrega o `## WHY` e diz ao agente que não carrega — o ponteiro \
             respondeu do arquivo errado: {rendered}",
        );
        assert!(
            !rendered.contains("no acceptance criteria reached it"),
            "e o mesmo para a régua, que também está no prompt: {rendered}",
        );
        assert!(
            rendered.contains("`## WHY` above") && rendered.contains("`## ACCEPTANCE`"),
            "o ponteiro nomeia as duas seções que o prompt carrega: {rendered}",
        );
    }

    /// The wave's prompt carries the PARENT's `## Contexto` + `## Não-Objetivos`
    /// — why the work exists and what the unit deliberately does not cover.
    ///
    /// The parent is the only place either lives, and before this the sole path
    /// to a wave was the TASK fallback, which fires ONLY for a spec with no
    /// `## Tasks` — and a full-scope wave always has one. So the assertion is
    /// made against a wave WITH tasks, which is the case that used to lose it.
    #[test]
    fn wave_prompt_carries_parent_why() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "why-spec";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(spec_dir.join("wave-1-impl")).unwrap();
        std::fs::write(
            spec_dir.join("spec.md"),
            "# T\n\n## Contexto\n\no despacho chega sem o porquê\n\n\
             ## Não-Objetivos\n\n- não endurecer o portão de resíduo\n\n\
             ## Tasks\n\n- [ ] parent task\n",
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("wave-1-impl").join("spec.md"),
            "# W\n\n## Tasks\n\n- [ ] do alpha\n",
        )
        .unwrap();

        let rendered = render_wave(dir.path(), spec, 1);
        assert!(rendered.contains("## WHY"), "{rendered}");
        assert!(rendered.contains("o despacho chega sem o porquê"), "context lost: {rendered}");
        assert!(
            rendered.contains("não endurecer o portão de resíduo"),
            "non-goals lost: {rendered}"
        );
        // The cut sections nest under `## WHY` — a `## ` line here would end the
        // section and `collapse_empty_sections` would then drop the heading.
        assert!(rendered.contains("### Contexto"), "heading not demoted: {rendered}");
        assert!(rendered.contains("### Não-Objetivos"), "heading not demoted: {rendered}");

        // A spec that declares neither renders no WHY section at all.
        let bare_spec = "no-why-spec";
        let bare_dir = dir.path().join(".claude/spec").join(bare_spec);
        std::fs::create_dir_all(&bare_dir).unwrap();
        std::fs::write(bare_dir.join("spec.md"), "# T\n\n## Tasks\n\n- [ ] a task\n").unwrap();
        let bare = render_prompt_at(
            dir.path(), Some(bare_spec), None, "impl", Path::new("."),
            RenderMode::First, None, None, None,
        );
        assert!(!bare.contains("## WHY"), "empty heading survived: {bare}");

        // O `## WHY` sobrevive ao ARQUIVAMENTO do spec do pai: um rewave renomeia
        // `spec.md` para `spec.original.md` no passo 9, e sem o fallback este
        // canal morre exatamente para a população que TEM ondas.
        std::fs::rename(spec_dir.join("spec.md"), spec_dir.join("spec.original.md")).unwrap();
        let after_rewave = render_wave(dir.path(), spec, 1);
        assert!(
            after_rewave.contains("o despacho chega sem o porquê"),
            "o arquivamento do spec do pai não pode matar o canal: {after_rewave}"
        );

        // E o RE-DESPACHO carrega o porquê também — ver
        // `wave_prompt_carries_its_acceptance` para a outra metade.
        let retry = render_prompt_at(
            dir.path(), Some(spec), Some(1), "impl", Path::new("."),
            RenderMode::Granular, None, None, None,
        );
        assert!(retry.contains("## WHY"), "o re-despacho perdeu o porquê: {retry}");
        assert!(retry.contains("o despacho chega sem o porquê"), "{retry}");
    }

    /// Um render de nível-spec SEM `## Tasks` — a forma tactical-fix / spec
    /// recém-rascunhada — imprime o `## Contexto` UMA vez.
    ///
    /// É a única forma em que os dois recortes saem do MESMO arquivo: o `## WHY`
    /// lê o spec do pai e o fallback de TASK lê o spec operacional, que aqui é o
    /// mesmo. Enquanto o tier 2 copiava o Contexto, o prompt o trazia duas
    /// vezes — uma sob `## WHY` e outra dentro do `## TASK`.
    #[test]
    fn a_no_tasks_spec_renders_its_context_exactly_once() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        let spec = "tf-sem-tasks";
        let spec_dir = dir.path().join(".claude/spec").join(spec);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let story = "o digest nao acha intents em pt";
        std::fs::write(
            spec_dir.join("spec.md"),
            format!(
                "# TF\n\n## Contexto\n\n{story}\n\n\
                 ## Critérios de Aceitação\n\n- **AC-1** — a consulta volta com acertos.\n"
            ),
        )
        .unwrap();

        let rendered = render_prompt_at(
            dir.path(), Some(spec), None, "impl", Path::new("."),
            RenderMode::First, None, None, None,
        );
        assert_eq!(
            rendered.matches(story).count(),
            1,
            "o Contexto tem de sair uma vez só: {rendered}"
        );
        // …e ele sai pelo canal que é dele, com o TASK apontando para lá.
        assert!(rendered.contains("## WHY"), "{rendered}");
        assert!(rendered.contains("TASK fallback"), "{rendered}");
        assert!(rendered.contains("**AC-1**"), "a régua continua chegando: {rendered}");
    }

    /// When the target subproject is its OWN nested git repository (`.git`
    /// FILE — the submodule shape), the rendered prompt states the git boundary
    /// (separate commit history; do not bump the superproject gitlink yourself —
    /// the `/git` parent step owns that sync). A plain subproject and the
    /// superproject root (`.`) get no such block.
    #[test]
    fn render_appends_own_git_root_boundary_for_nested_repo() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        // Nested submodule at apps/sub (`.git` FILE).
        let sub = dir.path().join("apps").join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), b"gitdir: ../../.git/modules/sub\n").unwrap();

        let rendered = render_prompt_at(
            dir.path(), None, None, "impl", Path::new("apps/sub"),
            RenderMode::First, None, None, None,
        );
        assert!(rendered.contains("## GIT BOUNDARY"), "boundary heading present: {rendered}");
        assert!(rendered.contains("its OWN git repository"), "boundary sentence present: {rendered}");
        assert!(
            rendered.contains("never bump the superproject's gitlink pointer YOURSELF"),
            "gitlink warning present: {rendered}"
        );
        assert!(
            rendered.contains("`/git` parent step owns that sync"),
            "the warning names WHO re-points the parent (the prohibition is not the whole rule): {rendered}"
        );

        // A plain subproject (no `.git`) gets NO boundary block.
        let plain = render_prompt_at(
            dir.path(), None, None, "impl", Path::new("apps/other"),
            RenderMode::First, None, None, None,
        );
        assert!(!plain.contains("## GIT BOUNDARY"), "no boundary for a plain subproject: {plain}");

        // The superproject root (`.`) is never a nested boundary, even though it
        // is itself a git repository.
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let root = render_prompt_at(
            dir.path(), None, None, "impl", Path::new("."),
            RenderMode::First, None, None, None,
        );
        assert!(!root.contains("## GIT BOUNDARY"), "root `.` is never a nested boundary: {root}");
    }
}
