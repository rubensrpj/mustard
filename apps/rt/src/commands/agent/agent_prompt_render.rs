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
use crate::commands::review::gate_regression_check;
use crate::commands::knowledge::memory_cross_wave;
use crate::commands::pipeline::resume_bootstrap::{read_wave_model, resolve_operational_spec_path};
use crate::commands::skill::skill_resolve;
use crate::commands::spec::spec_sections::is_heading;
use mustard_core::io::fs as mfs;
use mustard_core::platform::i18n;
use mustard_core::ClaudePaths;
use std::cell::RefCell;
use std::collections::HashMap;
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
/// `{cross_wave_memory}`, `{recommended_skills}`) to keep the final rendered
/// prompt under roughly `N` model tokens. The estimator is the conventional
/// 4-chars-per-token heuristic; placeholders are ranked by relevance (the
/// skill-resolve signal already orders skills) so the most useful content
/// stays intact while the long tail gets trimmed first. A `None` budget is the
/// historical full-render path.
pub fn run(
    spec: &str,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    retry_context_file: Option<&Path>,
    task_filter: Option<&str>,
    budget_tokens: Option<usize>,
) {
    let project = PathBuf::from(project_dir());
    let spec_dir = ClaudePaths::for_project(&project)
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.dir().to_path_buf())
        .unwrap_or_else(|_| project.clone());
    let op_spec_path = resolve_operational_spec_path(&spec_dir, wave);

    // Pick the right template block by mode.
    let block = match mode {
        RenderMode::First => extract_block(TEMPLATE, "dispatch"),
        RenderMode::Granular | RenderMode::FixLoop => extract_block(TEMPLATE, "retry"),
    };
    let Some(mut rendered) = block else {
        eprintln!("agent-prompt-render: WARN: template block missing — emitting empty prompt");
        return;
    };

    // ---- Collect placeholder values (fail-open per field). ----

    let subproject_str = subproject.to_string_lossy().to_string();
    let guards_summary = read_guards_block(&project.join(&subproject_str));
    let role_block = build_role_block(&project.join(&subproject_str), role);
    let spec_lang = read_spec_lang(&op_spec_path);
    let task_steps = {
        let raw = read_task_steps(&op_spec_path);
        match task_filter {
            Some(pat) => filter_task_lines(&raw, pat),
            None => raw,
        }
    };
    let context_md = read_cached(&project, spec, "context-md");
    let prior_wave_diff = wave
        .filter(|&w| w > 1)
        .map(|w| read_prior_wave_diff(&project, spec, w - 1))
        .unwrap_or_default();
    let mut cross_wave_memory = render_cross_wave(&project, spec, wave);
    // Append per-spec memory principles filtered by cluster relevance. T1.5
    // requires irrelevant principles to NOT enter the prompt — filtered_spec_memory
    // uses skill-resolve's intent tokens to match against principle names.
    let spec_memory_block = filtered_spec_memory(&project, spec, &task_steps, role);
    if !spec_memory_block.is_empty() {
        if !cross_wave_memory.is_empty() {
            cross_wave_memory.push_str("\n\n");
        }
        cross_wave_memory.push_str(&spec_memory_block);
    }
    // W5.T5.3 — inject the regression vocabulary so the child agent sees
    // the same Semantic/Pattern term lists the gate will check at Moment 1.
    // Locale resolved per-project; fail-open to PtBr.
    let locale = i18n::project_locale(&project);
    let vocab_block = vocabulary_inject_block(&project, locale);
    if !vocab_block.is_empty() {
        if !cross_wave_memory.is_empty() {
            cross_wave_memory.push_str("\n\n");
        }
        cross_wave_memory.push_str(&vocab_block);
    }
    let mut recommended_skills = recommended_skills_via_resolve(
        &project,
        spec,
        wave,
        role,
        &subproject_str,
        &task_steps,
    );

    let entity_info = String::new();
    let reference_files = String::new();
    let context_extras = String::new();
    let wave_model = wave
        .and_then(|w| read_wave_model(&spec_dir, w))
        .unwrap_or_default();
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
                &wave_model,
                &retry_context,
                &entity_info,
                &reference_files,
                &context_extras,
                &rendered,
            ],
            &mut [
                ("recommended_skills", &mut recommended_skills),
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
        ("{cross_wave_memory}", &cross_wave_memory),
        ("{recommended_skills}", &recommended_skills),
        ("{entity_info}", &entity_info),
        ("{reference_files}", &reference_files),
        ("{context_extras}", &context_extras),
        ("{wave_model}", &wave_model),
        ("{retry_context}", &retry_context),
    ];
    for (key, value) in substitutions {
        rendered = rendered.replace(key, value);
    }

    // ---- Warn about any remaining `{placeholder}` tokens. ----
    for token in scan_unfilled(&rendered) {
        eprintln!("agent-prompt-render: WARN: unfilled placeholder {token}");
        rendered = rendered.replace(&token, "");
    }

    // stdout = prompt string (raw, no JSON framing).
    print!("{rendered}");
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

/// Decide `{role_block}` content based on whether the subproject ships a
/// custom `{role}-impl.md` agent definition.
///
/// When `{subproject}/.claude/agents/{role}-impl.md` exists, the file already
/// declares role/boundary/validate/return — so the orchestrator passes an
/// empty block (per the legacy ref's contract).
fn build_role_block(subproject_dir: &Path, role: &str) -> String {
    let custom = ClaudePaths::for_project(subproject_dir)
        .map(|p| p.agents_dir().join(format!("{role}-impl.md")))
        .unwrap_or_else(|_| subproject_dir.join(format!("{role}-impl.md")));
    if custom.exists() {
        return String::new();
    }
    // Fallback: synthesise a minimal role line so the section is not empty.
    format!("ROLE: {role}")
}

/// Extract the `### Lang:` header value from a spec file. Defaults to
/// `"en-US"` (BCP-47). Legacy short codes (`pt` / `en`) are tolerated on read
/// and returned verbatim — `mustard_core::SupportedLocale::from_str` is the canonical
/// parser for downstream consumers.
fn read_spec_lang(spec_path: &Path) -> String {
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
fn read_task_steps(spec_path: &Path) -> String {
    let text = mfs::read_to_string(spec_path).unwrap_or_default();
    if text.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| is_heading(l, "tasks"));
    let Some(start) = start else {
        return String::new();
    };
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("## ") {
            end = i;
            break;
        }
    }
    lines[start..end].join("\n").trim_end().to_string()
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
    let Ok(read) = std::fs::read_dir(sp.dir()) else {
        return String::new();
    };
    let prefix = format!("wave-{wave_num}");
    for entry in read.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
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

/// Render the prior-wave memory block via `memory_cross_wave::render`. Empty
/// when the wave is 1 / the DB is absent / no memory rows match.
fn render_cross_wave(project: &Path, spec: &str, wave: Option<u32>) -> String {
    let Some(w) = wave else {
        return String::new();
    };
    if w <= 1 {
        return String::new();
    }
    let spec_dir = ClaudePaths::for_project(project)
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.dir().to_path_buf())
        .unwrap_or_else(|_| project.to_path_buf());
    let plan_text = mfs::read_to_string(spec_dir.join("wave-plan.md")).unwrap_or_default();
    let mut names = memory_cross_wave::parse_wave_names(&plan_text);
    if names.is_empty() {
        names = memory_cross_wave::parse_wave_dirs_from_fs(&spec_dir);
    }
    let n_prior = (w as usize).saturating_sub(1).min(names.len());
    let prior: Vec<String> = names.into_iter().take(n_prior).collect();
    memory_cross_wave::render(&prior, project, spec)
}

// Per-wave skill-resolve cache: byte-stable, scoped to one process.
// Keys are `(spec, wave, role, subproject)`. Avoids re-running the resolver
// when `agent-prompt-render` is invoked multiple times within the same wave.
thread_local! {
    static SKILL_CACHE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Resolve `{recommended_skills}` by calling [`skill_resolve::resolve`] in
/// process. The intent fed to the resolver is the wave's task block plus the
/// role label — gives the resolver verbs (`refactor`, `add`, ...) plus a
/// signal of which agent role is acting.
fn recommended_skills_via_resolve(
    project: &Path,
    spec: &str,
    wave: Option<u32>,
    role: &str,
    subproject: &str,
    task_steps: &str,
) -> String {
    let key = format!("{spec}|{}|{role}|{subproject}", wave.unwrap_or(0));
    if let Some(cached) = SKILL_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return cached;
    }

    let phase = match role.trim().to_ascii_lowercase().as_str() {
        "review" => "REVIEW",
        "explore" => "ANALYZE",
        "plan" => "PLAN",
        "qa" => "QA",
        _ => "EXECUTE",
    };
    // Intent = role + first 800 chars of task block. Resolver tokenises.
    let mut intent = role.to_string();
    intent.push(' ');
    intent.push_str(&task_steps.chars().take(800).collect::<String>());

    let resolved = skill_resolve::resolve(
        project,
        &intent,
        if subproject.is_empty() { None } else { Some(subproject) },
        Some(phase),
        4,
    );
    let names: Vec<String> = resolved.iter().map(|r| r.name.clone()).collect();
    // Always ensure karpathy-guidelines is present for code-editing phases
    // (no-op when the resolver already picked it).
    let mut final_list = names.clone();
    if matches!(phase, "EXECUTE" | "REVIEW") && !final_list.iter().any(|n| n == "karpathy-guidelines") {
        final_list.insert(0, "karpathy-guidelines".to_string());
    }
    let joined = final_list.join(", ");
    SKILL_CACHE.with(|c| {
        c.borrow_mut().insert(key, joined.clone());
    });
    joined
}

/// Read `memory/{name}.md` files under the active spec and keep only those
/// whose name tokens overlap the task block. Returns a markdown block ready
/// to append to `{cross_wave_memory}`.
fn filtered_spec_memory(project: &Path, spec: &str, task_steps: &str, role: &str) -> String {
    let memory_dir = ClaudePaths::for_project(project)
        .and_then(|p| p.for_spec(spec))
        .map(|sp| sp.dir().join("memory"))
        .unwrap_or_else(|_| project.to_path_buf());
    let Ok(entries) = mfs::read_dir(&memory_dir) else {
        return String::new();
    };

    // Tokenise task body + role for matching against memory file names.
    let mut tokens: Vec<String> = role
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_string)
        .filter(|s| s.len() >= 3)
        .collect();
    for word in task_steps.split(|c: char| !c.is_ascii_alphanumeric()) {
        let w = word.to_ascii_lowercase();
        if w.len() >= 3 {
            tokens.push(w);
        }
    }

    let mut matched: Vec<(String, String)> = Vec::new();
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
        // Match: ANY name token appears in the task tokens — keep.
        let relevant = name_tokens
            .iter()
            .any(|nt| tokens.iter().any(|t| t == nt));
        if !relevant {
            continue;
        }
        let Ok(text) = mfs::read_to_string(&entry.path) else {
            continue;
        };
        let summary = extract_memory_summary(&text);
        matched.push((entry.file_name, summary));
    }
    if matched.is_empty() {
        return String::new();
    }
    let mut out = String::from("## SPEC MEMORY\n");
    for (file, summary) in matched {
        let _ = writeln!(out, "- [[{}]] — {summary}", file.trim_end_matches(".md"));
    }
    out
}

/// First non-empty body line (skipping frontmatter + headings) — used as the
/// inline summary for a memory entry.
fn extract_memory_summary(text: &str) -> String {
    let mut in_fm = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            in_fm = !in_fm;
            continue;
        }
        if in_fm || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Return the first meaningful body line, capped to 120 chars.
        let cap: String = trimmed.chars().take(120).collect();
        return cap;
    }
    String::new()
}

/// W5.T5.3 — Render the regression-vocabulary block for injection into the
/// rendered prompt. Reuses [`gate_regression_check::build_vocab_matcher`] so
/// the inject path agrees with the gate's Moment-1 scan on which terms get
/// surfaced. Empty when the project has no resolvable vocabulary
/// (fail-open).
fn vocabulary_inject_block(project: &Path, locale: i18n::Locale) -> String {
    let Some(_matcher) = gate_regression_check::build_vocab_matcher(project) else {
        return String::new();
    };
    let (semantic, pattern) = read_vocab_layers_for_inject(project);
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

/// Resolve the (semantic, pattern) layer term lists for the project. Reads
/// `.claude/vocab/regression.toml` and falls back to the gate's in-memory
/// defaults when the file is absent. Best-effort TOML walk — full parsing is
/// covered by [`gate_regression_check::build_vocab_matcher`] on the scan
/// side; this helper only needs the term names for display.
fn read_vocab_layers_for_inject(project: &Path) -> (Vec<String>, Vec<String>) {
    // W5#2: dedup'd via `VocabularyDoc::layer_terms` shared with
    // `subagent_inject::read_vocab_layers`.
    let toml_path = project
        .join(".claude")
        .join("vocab")
        .join("regression.toml");
    let (mut semantic, mut pattern) =
        match mustard_core::domain::vocabulary::VocabularyDoc::load_from_file(&toml_path) {
            Ok(doc) => (
                doc.layer_terms(mustard_core::domain::vocabulary::Layer::Semantic)
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect::<Vec<String>>(),
                doc.layer_terms(mustard_core::domain::vocabulary::Layer::Pattern)
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
    fn build_role_block_empty_when_custom_agent_exists() {
        let dir = tempdir().unwrap();
        let agents = ClaudePaths::for_project(dir.path()).unwrap().agents_dir();
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(agents.join("ui-impl.md"), "x").unwrap();
        assert!(build_role_block(dir.path(), "ui").is_empty());
        // Without the file, a synthesised ROLE: line is returned.
        let other = tempdir().unwrap();
        assert_eq!(build_role_block(other.path(), "ui"), "ROLE: ui");
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
}
