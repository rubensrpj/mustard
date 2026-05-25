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

use crate::run::env::project_dir;
use crate::run::memory_cross_wave;
use crate::run::resume_bootstrap::{read_wave_model, resolve_operational_spec_path};
use crate::run::spec_sections::is_heading;
use mustard_core::fs as mfs;
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
pub fn run(
    spec: &str,
    wave: Option<u32>,
    role: &str,
    subproject: &Path,
    mode: RenderMode,
    retry_context_file: Option<&Path>,
    task_filter: Option<&str>,
) {
    let project = PathBuf::from(project_dir());
    let spec_dir = project.join(".claude").join("spec").join(spec);
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
        .map(|w| read_cached(&project, spec, &format!("wave-{}.diff", w - 1)))
        .unwrap_or_default();
    let cross_wave_memory = render_cross_wave(&project, spec, wave);
    let recommended_skills = guess_recommended_skills(role);
    let recipe_context = String::new();
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
        ("{recipe_context}", &recipe_context),
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
    // Trim a single leading/trailing newline added by the markers.
    if body.starts_with('\n') {
        body.remove(0);
    }
    if body.ends_with('\n') {
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
    let custom = subproject_dir
        .join(".claude")
        .join("agents")
        .join(format!("{role}-impl.md"));
    if custom.exists() {
        return String::new();
    }
    // Fallback: synthesise a minimal role line so the section is not empty.
    format!("ROLE: {role}")
}

/// Extract the `### Lang:` header value from a spec file. Defaults to `"en"`.
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
    "en".to_string()
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
/// any IO error.
fn read_cached(project: &Path, spec: &str, name: &str) -> String {
    let path = project
        .join(".claude")
        .join(".pipeline-states")
        .join(format!("{spec}.{name}.md"));
    mfs::read_to_string(&path).unwrap_or_default()
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
    let spec_dir = project.join(".claude").join("spec").join(spec);
    let plan_text = mfs::read_to_string(spec_dir.join("wave-plan.md")).unwrap_or_default();
    let mut names = memory_cross_wave::parse_wave_names(&plan_text);
    if names.is_empty() {
        names = memory_cross_wave::parse_wave_dirs_from_fs(&spec_dir);
    }
    let n_prior = (w as usize).saturating_sub(1).min(names.len());
    let prior: Vec<String> = names.into_iter().take(n_prior).collect();
    memory_cross_wave::render(&prior, project, spec)
}

/// Heuristic skill list per role — mirrors the table in the legacy ref. Kept
/// minimal here; the orchestrator can override by overwriting `{recommended_skills}`.
fn guess_recommended_skills(role: &str) -> String {
    match role.trim().to_ascii_lowercase().as_str() {
        "ui" | "frontend" | "mobile" => {
            "karpathy-guidelines, design-craft, react-best-practices".to_string()
        }
        "backend" | "api" | "general" => "karpathy-guidelines".to_string(),
        "review" => "diagnose".to_string(),
        "explore" => String::new(),
        _ => "karpathy-guidelines".to_string(),
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
        let agents = dir.path().join(".claude").join("agents");
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
        assert_eq!(read_spec_lang(&path), "en");
    }

    #[test]
    fn read_spec_lang_parses_pt() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
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
