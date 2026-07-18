//! Section cutting and prompt cleanup: read a subproject's `## Guards`, cut the
//! spec's `## Tasks` (with narrative fallbacks), resolve the spec locale, and
//! the two post-substitution passes that keep the rendered prompt clean
//! ([`collapse_empty_sections`], [`strip_unfilled_template_tokens`]).

use crate::commands::spec::spec_sections::is_heading;
use mustard_core::io::fs as mfs;
use std::path::Path;

/// Read the `## Guards` section body from a subproject's `CLAUDE.md`. Empty
/// when the file or the section is absent.
pub(crate) fn read_guards_block(subproject_dir: &Path) -> String {
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
pub(crate) fn read_spec_lang(spec_path: &Path) -> String {
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
pub(crate) fn filter_task_lines(raw: &str, pattern: &str) -> String {
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

/// Find unfilled `{placeholder}` tokens (lowercase + underscore identifiers).
/// Returns each token once, in the order encountered.
pub(crate) fn scan_unfilled(text: &str) -> Vec<String> {
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
pub(crate) fn strip_unfilled_template_tokens(
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

/// Remove any `## ` heading whose body — every line until the next `## ` heading
/// or end of text — is entirely whitespace. Keeps the dispatched prompt clean
/// when a fail-open placeholder (`{guards_summary}`, `{context_md}`,
/// `{reference_files}`, `{cross_wave_memory}`, `{prior_wave_diff}`) resolves to
/// "". Only `## `-level headings are considered, so the `<!-- PREFIX-STABLE -->`
/// marker and inline prose are never touched. The `## TASK` section always
/// survives: its trailing "Guards carregados …" line is non-blank body.
pub(crate) fn collapse_empty_sections(text: &str) -> String {
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
    fn collapse_empty_sections_drops_blank_keeps_filled() {
        let text = "## A\n\n## B\nbody\n\n## C\n   \n## D\nx";
        let out = collapse_empty_sections(text);
        assert!(!out.contains("## A"), "empty heading A survived: {out}");
        assert!(out.contains("## B\nbody"), "filled heading B dropped: {out}");
        assert!(!out.contains("## C"), "whitespace-only heading C survived: {out}");
        assert!(out.contains("## D\nx"), "filled heading D dropped: {out}");
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
}
