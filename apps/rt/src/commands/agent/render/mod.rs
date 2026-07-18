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
//! - [`sections`] — spec section cutting, task steps, and the cleanup passes;
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
use crate::commands::pipeline::resume_bootstrap::resolve_operational_spec_path;
use crate::shared::context::project_dir;
use mustard_core::io::fs as mfs;
use mustard_core::ClaudePaths;
use std::path::{Path, PathBuf};

mod capabilities;
mod prompt_ref;
mod reference;
mod retry;
mod role;
mod sections;
mod skills;

// Re-exports that preserve the historical public surface so the compatibility
// façade (`agent::agent_prompt_render`) and every in-crate consumer keep
// resolving unchanged. `PROMPT_REF_MARKER`, `EPISTEMIC_FLOOR` and
// `recommended_subagent_type` stay fully public (the hook + integration tests
// reach them); the crate-internal helpers keep their `pub(crate)` reach.
pub use prompt_ref::PROMPT_REF_MARKER;
pub use role::{recommended_subagent_type, EPISTEMIC_FLOOR};
pub(crate) use prompt_ref::render_prompt_ref_at;
pub(crate) use sections::read_task_steps;
// Surfaced only for the compatibility façade's test-gated consumers
// (`wave_scaffold` tests); the compositor calls `build_reference_files`, not
// this directly, so the bin build never references it.
#[cfg(test)]
pub(crate) use reference::files_section_paths;

// Sub-engine helpers the compositor calls directly.
use capabilities::capability_block;
use prompt_ref::prompt_ref_stub;
use reference::build_reference_files;
use retry::compose_retry_context;
use role::{build_role_block, patterns_task_block};
use sections::{
    collapse_empty_sections, filter_task_lines, read_guards_block, read_spec_lang, scan_unfilled,
    strip_unfilled_template_tokens,
};
use skills::build_skills_list;

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
    // `--role patterns` (the `/scan` mold enrich) is spec-less and its work is
    // DATA, not prose: the mold worklist `scan-patterns-list` computes. Embed
    // that SAME worklist (single source — `scan_patterns::list::collect`),
    // filtered to this subproject, into the TASK body so the mustard-patterns
    // agent can work from the rendered prompt alone. An empty worklist renders
    // an explicit "no candidates — author nothing" TASK (never the silent
    // guards-reminder-only TASK that forced agents to demand a re-dispatch).
    let task_steps = if role.trim().eq_ignore_ascii_case("patterns") {
        patterns_task_block(&project, &subproject_str, &task_steps)
    } else {
        task_steps
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
        (_, None) => {
            compose_retry_context(&project, spec, &spec_dir, &prior_wave_diff, &change_log)
        }
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
        ("{skills_list}", &skills_list),
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

#[cfg(test)]
mod tests {
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

    /// End-to-end regression for the /scan patterns dispatch defect: rendering
    /// `--role patterns` for a subproject with candidates must embed the SAME
    /// worklist `scan-patterns-list` computes — slug, moldPath and exemplar
    /// paths — in the TASK body, filtered to that subproject, so the
    /// mustard-patterns agent can work from the rendered prompt alone.
    #[test]
    fn patterns_render_embeds_subproject_worklist() {
        let dir = tempdir().unwrap();
        anchor(dir.path());
        std::fs::write(
            dir.path().join(".claude").join("grain.model.json"),
            r#"{
              "projects": [{"name":"api","dir":"apps/api"},{"name":"web","dir":"apps/web"}],
              "roles": [
                {"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services","decl_kind":"class","implements":"BaseService"},
                {"affix":"Widget","kind":"suffix","count":4,"common_dir":"apps/web/widgets"}
              ],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"},
                {"path":"apps/web/widgets/ChartWidget.tsx"},
                {"path":"apps/web/widgets/TableWidget.tsx"}
              ]
            }"#,
        )
        .unwrap();

        let render = || {
            render_prompt_at(
                dir.path(),
                None,
                None,
                "patterns",
                Path::new("apps/api"),
                RenderMode::First,
                None,
                None,
                Some("Extra orchestrator note."),
            )
        };
        let rendered = render();
        // The worklist entry rides inside the ## TASK section with every field
        // the agent needs: slug, label, affix(+kind), declKind, count,
        // implements, moldPath and the exemplar file paths.
        assert!(rendered.contains("ROLE: patterns"), "role block missing: {rendered}");
        assert!(rendered.contains("slug: api-service"), "slug missing: {rendered}");
        assert!(rendered.contains("label: service"), "label missing: {rendered}");
        assert!(rendered.contains("affix: Service (suffix)"), "affix missing: {rendered}");
        assert!(rendered.contains("declKind: class"), "declKind missing: {rendered}");
        assert!(rendered.contains("implements: BaseService"), "implements missing: {rendered}");
        // The role tallies 5 repo-wide, but THIS subproject holds 2 — the agent
        // is told what its own house has, never the global figure.
        assert!(rendered.contains("count: 2"), "local count missing: {rendered}");
        assert!(
            rendered.contains("moldPath: apps/api/.claude/skills/api-service-pattern/SKILL.md"),
            "moldPath missing: {rendered}"
        );
        assert!(
            rendered.contains("apps/api/services/UserService.ts")
                && rendered.contains("apps/api/services/OrderService.ts"),
            "exemplar paths missing: {rendered}"
        );
        // Filtered to the requested subproject — the web cluster never leaks in.
        assert!(!rendered.contains("web-widget"), "other subproject leaked: {rendered}");
        // The worklist lands in the ## TASK section (before the guards line).
        let task_body = rendered
            .split_once("## TASK")
            .map(|(_, rest)| rest)
            .expect("## TASK heading present");
        assert!(task_body.contains("slug: api-service"), "worklist not in TASK: {task_body}");
        // `--task-text` rides after the worklist instead of being swallowed.
        assert!(rendered.contains("Extra orchestrator note."), "task-text dropped: {rendered}");
        // Deterministic: two renders produce identical bytes.
        assert_eq!(rendered, render(), "patterns render must be byte-stable");
    }

    /// Empty worklist (no model / all molds exist): the TASK must explicitly
    /// state there is nothing to author — the silent guards-reminder-only TASK
    /// (the /scan dispatch defect) must be impossible. Exit stays 0 per the
    /// renderer's fail-open contract; the loud part is the explicit no-op TASK
    /// plus the stderr WARN.
    #[test]
    fn patterns_render_empty_worklist_states_no_candidates() {
        let dir = tempdir().unwrap();
        anchor(dir.path()); // no grain.model.json → collect() fail-opens to [].
        let rendered = render_prompt_at(
            dir.path(),
            None,
            None,
            "patterns",
            Path::new("apps/api"),
            RenderMode::First,
            None,
            None,
            None,
        );
        assert!(rendered.contains("NO CANDIDATES"), "explicit no-op missing: {rendered}");
        assert!(
            rendered.contains("Do NOT author anything"),
            "author-nothing instruction missing: {rendered}"
        );
        // The TASK section survived with the explicit body (not collapsed, not blank).
        let task_body = rendered
            .split_once("## TASK")
            .map(|(_, rest)| rest)
            .expect("## TASK heading present");
        assert!(task_body.contains("NO CANDIDATES"), "no-op not in TASK: {task_body}");
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

    /// B1: the retry template carries a `## RETRY CONTEXT` heading that collapses
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

        let task_steps = read_task_steps(&spec);
        let reference_files = build_reference_files(dir.path(), "api", &spec);
        assert!(!reference_files.is_empty(), "reference_files empty");

        let mut rendered = extract_block(TEMPLATE, "dispatch").expect("dispatch block");
        // The removed `{entity_info}` / `{recommended_skills}` / `{context_extras}`
        // placeholders are no longer in the template, so they are not substituted
        // here either.
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
            ("{skills_list}", ""),
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
}
