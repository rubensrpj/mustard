//! `size_gate` — the consolidated Write/Edit size & skill-validation module.
//!
//! ## Scope (b3 Wave 4, Write/Edit family)
//!
//! This module ports the **structural-gate** concerns of three JavaScript
//! hooks, all `PreToolUse(Write|Edit)` gates:
//!
//! - `spec-size-gate.js` — warns/blocks an oversized spec `.md` file
//!   (warn 200 / strict-warn 400 / block 500 lines). Its second concern, the
//!   advisory **AC quality audit**, is also ported — it never blocks.
//! - `skill-size-gate.js` — the same three-tier line check for `SKILL.md`
//!   files, skipping generated skills in warn mode.
//! - `skill-validate-gate.js` — validates `SKILL.md` YAML frontmatter
//!   (kebab-case `name`, `description` with trigger words, `source: scan|manual`).
//!
//! Consolidation **regroups, it does not re-decide** — every verdict is a 1:1
//! port of the JS decision logic. The parity tests at the bottom mirror
//! `__tests__/size-gates.test.js` and `__tests__/skill-validate-gate.test.js`.
//!
//! ## Modes
//!
//! Each gate resolves its **own** `MUSTARD_*_MODE` env var, independent of the
//! dispatcher's module-level mode:
//! - `spec-size-gate` → `MUSTARD_SPEC_SIZE_MODE` (default `warn`).
//! - `skill-size-gate` → `MUSTARD_SKILL_SIZE_MODE` (default `warn`).
//! - `skill-validate-gate` → `MUSTARD_SKILL_VALIDATE_GATE_MODE` (default `warn`).
//! - the AC audit → `MUSTARD_AC_QUALITY_MODE` (default `warn`) — advisory only.
//!
//! All three size/validate gates default to `warn`: the dominant verdict is an
//! advisory (`Warn`), never a `Deny` unless the env var is explicitly `strict`.
//! Because the gate computes its own verdict, the dispatcher repasses it
//! without downgrade.

use mustard_core::error::Error;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};

use crate::util::format_gate_message;

// ---------------------------------------------------------------------------
// Shared: mode resolution + content extraction
// ---------------------------------------------------------------------------

/// A three-state gate mode, `off` / `warn` / `strict`. The JS `resolveMode`
/// lowercases the env var and falls back to the default for any unrecognised
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateMode {
    Off,
    Warn,
    Strict,
}

/// Resolve a `MUSTARD_*_MODE` env var into a [`GateMode`]. `default` is the
/// fallback both for an absent variable and for an unrecognised value —
/// matching `resolveMode` / `getMode` in the JS hooks.
fn resolve_mode(env_var: &str, default: GateMode) -> GateMode {
    match std::env::var(env_var)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => GateMode::Off,
        "warn" => GateMode::Warn,
        "strict" => GateMode::Strict,
        _ => default,
    }
}

/// The line-count thresholds shared by the spec and skill size gates.
const WARN_LINES: usize = 200;
const STRICT_WARN_LINES: usize = 400;
const BLOCK_LINES: usize = 500;

/// Count newline-separated segments — JS `content.split('\n').length`.
fn count_lines(content: &str) -> usize {
    content.split('\n').count()
}

/// Resolve the post-edit content of a Write/Edit invocation.
///
/// - `Write` uses `tool_input.content`.
/// - `Edit` simulates the edit against the file on disk (the JS `simulateEdit`):
///   read the current file (empty string when absent), apply `old_string` →
///   `new_string` once (or every occurrence with `replace_all`).
///
/// Returns `None` for any other tool, or when an `Edit` has no `file_path`.
fn resolve_content(input: &HookInput) -> Option<String> {
    let tool = input.tool_name.as_deref().unwrap_or_default();
    let tool_input = &input.tool_input;
    match tool {
        "Write" => Some(
            tool_input
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        ),
        "Edit" => {
            let file_path = tool_input.get("file_path").and_then(|v| v.as_str())?;
            let current = std::fs::read_to_string(file_path).unwrap_or_default();
            let old_str = tool_input
                .get("old_string")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let new_str = tool_input
                .get("new_string")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let replace_all = tool_input
                .get("replace_all")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if replace_all {
                Some(current.split(old_str).collect::<Vec<_>>().join(new_str))
            } else {
                match current.find(old_str) {
                    Some(idx) => {
                        let mut out = String::with_capacity(current.len());
                        out.push_str(&current[..idx]);
                        out.push_str(new_str);
                        out.push_str(&current[idx + old_str.len()..]);
                        Some(out)
                    }
                    None => Some(current),
                }
            }
        }
        _ => None,
    }
}

/// The `file_path` of a Write/Edit invocation (also accepts the legacy `path`
/// key — `tool_input.file_path || tool_input.path`).
fn file_path_of(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// spec-size-gate — PreToolUse(Write|Edit) size gate for spec files
// ---------------------------------------------------------------------------

/// `true` when `file_path` is a spec markdown file. Flat layout:
/// `.claude/spec/{name}/.+\.md` or any `/spec/.+\.md`.
fn is_spec_path(file_path: &str) -> bool {
    let p = file_path.replace('\\', "/");
    // Flat layout: `.claude/spec/{name}/*.md` — requires at least one char
    // between the prefix and `.md` (mirrors the JS regex).
    if p.contains(".claude/spec/") && std::path::Path::new(&p)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md")) {
        if let Some(rest) = p.split(".claude/spec/").nth(1) {
            if rest.len() > ".md".len() {
                return true;
            }
        }
    }
    // Generic: any `/spec/` segment followed by a non-empty `.md` file.
    if std::path::Path::new(&p)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md")) {
        if let Some(idx) = p.find("/spec/") {
            let rest = &p[idx + "/spec/".len()..];
            if rest.len() > ".md".len() {
                return true;
            }
        }
    }
    false
}

/// The spec-size-gate verdict for a spec file of `lines` lines under `mode`.
///
/// 1:1 with `spec-size-gate.js#delegateSizeGate`:
/// - strict mode + `lines >= 500` → `Deny`;
/// - warn mode (any tier ≥ 200) → `Warn` advisory, never `Deny`;
/// - under 200 lines → `Allow`.
fn spec_size_verdict(lines: usize, mode: GateMode) -> Verdict {
    if mode == GateMode::Strict && lines >= BLOCK_LINES {
        return Verdict::Deny {
            reason: format_gate_message(
                "Spec Size",
                &format!("spec exceeds the {BLOCK_LINES}-line hard limit ({lines} lines)"),
                "oversized specs are hard to read and review",
                "split into references/{section}.md (see feature/SKILL.md § Spec Layout), \
                 or set MUSTARD_SPEC_SIZE_MODE=warn",
            ),
        };
    }
    if lines >= WARN_LINES {
        let msg = if lines >= BLOCK_LINES {
            format_gate_message(
                "Spec Size",
                &format!("spec has {lines} lines — over the {BLOCK_LINES}-line hard limit"),
                "warn mode is active so this is advisory only",
                "split into references/{section}.md (see feature/SKILL.md § Spec Layout), \
                 or set MUSTARD_SPEC_SIZE_MODE=strict to enforce",
            )
        } else if lines >= STRICT_WARN_LINES {
            format_gate_message(
                "Spec Size",
                &format!(
                    "spec has {lines} lines — past the strict threshold \
                     ({STRICT_WARN_LINES}), approaching the {BLOCK_LINES}-line block"
                ),
                "oversized specs are hard to read and review",
                "split into references/{section}.md (see feature/SKILL.md § Spec Layout)",
            )
        } else {
            format_gate_message(
                "Spec Size",
                &format!(
                    "spec has {lines} lines (warn at {WARN_LINES}, strict at \
                     {STRICT_WARN_LINES}, block at {BLOCK_LINES})"
                ),
                "approaching the size where specs become hard to review",
                "consider splitting into references/{section}.md",
            )
        };
        return Verdict::Warn { message: msg };
    }
    Verdict::Allow
}

// ---------------------------------------------------------------------------
// AC quality audit — advisory-only, runs alongside spec-size-gate
// ---------------------------------------------------------------------------

/// One AC-quality audit result. Mirrors `auditAC`'s return shape.
#[derive(Debug, Default)]
struct AcAudit {
    total: usize,
    rich: usize,
    poor: usize,
    non_binary: usize,
    non_binary_reasons: Vec<&'static str>,
}

/// Extract the body of the `## Acceptance Criteria` (or PT `## Critérios de
/// Aceitação`) section — the text between that heading and the next `## `.
fn extract_ac_section(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if is_ac_heading(line) {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start) {
        if is_h2(line) {
            end = i;
            break;
        }
    }
    Some(lines[start..end].join("\n"))
}

/// `true` if `line` is the EN or PT Acceptance-Criteria H2 heading.
fn is_ac_heading(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower == "## acceptance criteria" || lower == "## critérios de aceitação"
}

/// `true` if `line` is a `## ` markdown heading.
fn is_h2(line: &str) -> bool {
    line.starts_with("## ") || line == "##"
}

/// Audit the AC section text for empirical-rigor signals. Port of `auditAC`.
fn audit_ac(ac_text: &str) -> AcAudit {
    // Group lines into AC items: a line matching `- [ ] AC-N` starts an item;
    // an indented line continues it.
    let mut items: Vec<String> = Vec::new();
    let mut curr: Option<String> = None;
    for line in ac_text.split('\n') {
        if is_ac_item_start(line) {
            if let Some(c) = curr.take() {
                items.push(c);
            }
            curr = Some(line.to_string());
        } else if curr.is_some() && line.starts_with(char::is_whitespace) {
            if let Some(c) = curr.as_mut() {
                c.push('\n');
                c.push_str(line);
            }
        }
    }
    if let Some(c) = curr {
        items.push(c);
    }

    let mut rich = 0;
    let mut poor = 0;
    let mut non_binary = 0;
    let mut reasons: Vec<&'static str> = Vec::new();
    for ac in &items {
        let lower = ac.to_ascii_lowercase();
        let is_rich = ac_is_rich(&lower);
        let is_poor = !is_rich && ac_is_poor(&lower);
        if is_rich {
            rich += 1;
        } else if is_poor {
            poor += 1;
        }
        // Non-binary detection.
        if !lower.contains("command:") && !lower.contains("command :") {
            non_binary += 1;
            push_reason(&mut reasons, "missing-command");
        } else if let Some(reason) = ac_non_binary_reason(&lower) {
            non_binary += 1;
            push_reason(&mut reasons, reason);
        }
    }
    AcAudit {
        total: items.len(),
        rich,
        poor,
        non_binary,
        non_binary_reasons: reasons,
    }
}

/// Add `reason` to `reasons` only once (the JS uses a `Set`).
fn push_reason(reasons: &mut Vec<&'static str>, reason: &'static str) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

/// `true` if a line starts a new AC item: `- [ ] AC-N` / `- [x] AC-N`.
fn is_ac_item_start(line: &str) -> bool {
    let t = line.trim_start();
    let Some(rest) = t.strip_prefix('-') else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix('[') else {
        return false;
    };
    // `[ ]` or `[x]`/`[X]`.
    let mut chars = rest.chars();
    let marker = chars.next();
    if !matches!(marker, Some(' ' | 'x' | 'X')) {
        return false;
    }
    let rest = chars.as_str();
    let Some(rest) = rest.strip_prefix(']') else {
        return false;
    };
    let rest = rest.trim_start().to_ascii_lowercase();
    // `AC-\d+`.
    let Some(rest) = rest.strip_prefix("ac-") else {
        return false;
    };
    rest.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// `true` if an AC item's Command clause inspects real payload/state
/// (`node -e`, `bash -c`, `bun -e`, `grep`, `jq`, `curl`, `sqlite`, `cat …|`).
/// `ac_lower` is the lowercased item text.
fn ac_is_rich(ac_lower: &str) -> bool {
    // Each rich pattern is `Command:` somewhere followed (anywhere after) by a
    // signal token. The JS regexes are `Command:.*\bTOKEN\b`.
    let Some(cmd_idx) = ac_lower.find("command:") else {
        return false;
    };
    let after = &ac_lower[cmd_idx..];
    has_word_pair_loose(after, "node", "-e")
        || has_word_pair_loose(after, "bash", "-c")
        || has_word_pair_loose(after, "bun", "-e")
        || contains_word(after, "grep")
        || contains_word(after, "jq")
        || contains_word(after, "curl")
        || contains_word(after, "sqlite")
        || contains_word(after, "sqlite3")
        || (contains_word(after, "cat") && after.contains('|'))
}

/// `true` if the AC's Command clause is exclusively a build/test wrapper —
/// `Command: [`]?(bun|npm|pnpm|yarn) (run )?(test|build|lint|tsc|type-check)`.
fn ac_is_poor(ac_lower: &str) -> bool {
    for line in ac_lower.split('\n') {
        let line = line.trim();
        let Some(rest) = line.find("command:").map(|i| &line[i + "command:".len()..]) else {
            continue;
        };
        let rest = rest.trim().trim_start_matches('`').trim();
        let mut tokens = rest.split_whitespace();
        let Some(runner) = tokens.next() else {
            continue;
        };
        if !matches!(runner, "bun" | "npm" | "pnpm" | "yarn") {
            continue;
        }
        let mut next = tokens.next();
        if next == Some("run") {
            next = tokens.next();
        }
        let Some(script) = next else { continue };
        let script = script.trim_end_matches('`');
        if matches!(script, "test" | "build" | "lint" | "tsc" | "type-check")
            && tokens.next().map_or("", |t| t.trim_end_matches('`')).is_empty()
        {
            return true;
        }
    }
    false
}

/// The non-binary reason label for an AC item, if any. `ac_lower` is lowercased.
fn ac_non_binary_reason(ac_lower: &str) -> Option<&'static str> {
    let cmd_idx = ac_lower.find("command:")?;
    let after = ac_lower[cmd_idx + "command:".len()..].trim_start();
    // pt: "já validado" / "já validada".
    if after.starts_with("já validad") || after.starts_with("ja validad") {
        return Some("past-tense");
    }
    // en: "validated" / "(validated".
    let after_paren = after.trim_start_matches('(').trim_start();
    if after_paren.starts_with("validated") {
        return Some("past-tense-en");
    }
    // "same as AC-".
    if after.starts_with("same as ac-") {
        return Some("same-as");
    }
    // empty: "(nenhum|none|n/a|—|-)" optionally bracketed.
    let stripped = after
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    if matches!(stripped, "nenhum" | "none" | "n/a" | "—" | "-" | "") {
        return Some("empty");
    }
    // hard-coded spec path (flat layout: .claude/spec/{name}/).
    if after.contains(".claude/spec/") {
        return Some("active-path");
    }
    None
}

/// Whitespace-tolerant "word A followed by word B" check.
fn has_word_pair_loose(s: &str, a: &str, b: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = s[from..].find(a) {
        let start = from + rel;
        let end = start + a.len();
        let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
        let rest = &s[end..];
        let trimmed = rest.trim_start();
        let had_ws = trimmed.len() < rest.len();
        if left_ok && had_ws && trimmed.starts_with(b) {
            return true;
        }
        from = end;
    }
    false
}

/// `true` if `s` contains `word` with word boundaries on both sides.
fn contains_word(s: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = s[from..].find(word) {
        let start = from + rel;
        let end = start + word.len();
        let left_ok = start == 0 || !is_word_byte(s.as_bytes()[start - 1]);
        let right_ok = s.as_bytes().get(end).is_none_or(|&b| !is_word_byte(b));
        if left_ok && right_ok {
            return true;
        }
        from = end;
    }
    false
}

/// `true` for an ASCII word byte (alphanumeric or `_`).
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The AC-quality advisory text for a spec file, or `None` when the section is
/// healthy / absent. Advisory only — the caller never turns this into a `Deny`.
fn ac_quality_advisory(content: &str) -> Option<String> {
    let ac_text = extract_ac_section(content)?;
    let audit = audit_ac(&ac_text);
    let mut parts: Vec<String> = Vec::new();
    if audit.total >= 3 && audit.rich == 0 && audit.poor > 0 {
        parts.push(format_gate_message(
            "AC Quality",
            &format!(
                "{}/{} AC use only build/test commands (no node -e / bash -c / \
                 grep / jq verifying real payload)",
                audit.poor, audit.total
            ),
            "specs with only \"build passes\" AC do not prove the feature works \
             end-to-end",
            "add an AC that asserts real payload — see refs/feature/ac-cross-shell.md",
        ));
    }
    if audit.non_binary > 0 {
        parts.push(format_gate_message(
            "AC Quality",
            &format!(
                "{}/{} AC are non-binary ({}) — past-tense validation, moveable \
                 path (spec/), or no Command",
                audit.non_binary,
                audit.total,
                audit.non_binary_reasons.join(", ")
            ),
            "re-running QA after CLOSE will fail on these",
            "use 'Command: bash -c ...' with an executable assertion over current state",
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// skill-size-gate — PreToolUse(Write|Edit) size gate for SKILL.md files
// ---------------------------------------------------------------------------

/// `true` when `file_path` is a `SKILL.md` file at any depth. Mirrors
/// `isSkillPath`: `/SKILL\.md$` or the bare `SKILL.md`.
fn is_skill_path(file_path: &str) -> bool {
    let p = file_path.replace('\\', "/");
    p == "SKILL.md" || p.ends_with("/SKILL.md")
}

/// The skill-size-gate verdict for a `SKILL.md` of `lines` lines under `mode`.
///
/// 1:1 with `_lib/size-gate.js#run` driven by `skill-size-gate.js`:
/// - in warn mode, a generated skill (content starting with the
///   `<!-- mustard:generated -->` header) is skipped — `Allow`;
/// - strict mode + `lines >= 500` → `Deny`;
/// - warn mode at any tier ≥ 200 → `Warn` advisory;
/// - under 200 lines → `Allow`.
fn skill_size_verdict(content: &str, lines: usize, mode: GateMode) -> Verdict {
    // skipWhen: warn mode + generated header → skip silently.
    if mode == GateMode::Warn && content.trim_start().starts_with("<!-- mustard:generated -->") {
        return Verdict::Allow;
    }
    if lines >= BLOCK_LINES {
        let msg = format!(
            "[skill-size-gate] SKILL.md exceeds 500 lines ({lines} lines) — \
             split verbose sections into references/examples.md"
        );
        if mode == GateMode::Strict {
            return Verdict::Deny { reason: msg };
        }
        return Verdict::Warn { message: msg };
    }
    if lines >= STRICT_WARN_LINES {
        return Verdict::Warn {
            message: format!(
                "[skill-size-gate] WARN: {lines} lines (strict-warn threshold \
                 {STRICT_WARN_LINES})"
            ),
        };
    }
    if lines >= WARN_LINES {
        return Verdict::Warn {
            message: format!(
                "[skill-size-gate] ADVISORY: {lines} lines (warn threshold {WARN_LINES})"
            ),
        };
    }
    Verdict::Allow
}

// ---------------------------------------------------------------------------
// skill-validate-gate — PreToolUse(Write|Edit) SKILL.md frontmatter validator
// ---------------------------------------------------------------------------

/// Validate a `SKILL.md` body against the structural rules in
/// `scripts/skills.js#validateSkill`. Returns the list of error strings — an
/// empty list means the skill is valid.
fn validate_skill(content: &str) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();
    let normalized = content.replace("\r\n", "\n");
    // Frontmatter: `^---\n([\s\S]*?)\n---`.
    let Some(body) = extract_frontmatter(&normalized) else {
        errors.push("missing YAML frontmatter".to_string());
        return errors;
    };

    // `name:` — must be present and kebab-case (`^[a-z][a-z0-9-]+$`).
    match yaml_field(&body, "name") {
        None => errors.push("frontmatter: missing \"name\"".to_string()),
        Some(name) => {
            let name = name.trim();
            if !is_kebab_case(name) {
                errors.push(format!("name not kebab-case: {name}"));
            }
        }
    }

    // `description:` — present, 50..=600 chars (whitespace-collapsed), with a
    // trigger word.
    match yaml_description(&body) {
        None => errors.push("frontmatter: missing \"description\"".to_string()),
        Some(desc) => {
            let collapsed = collapse_ws(&desc);
            let len = collapsed.chars().count();
            if len < 50 {
                errors.push(format!("description too short ({len} chars, min 50)"));
            }
            if len > 600 {
                errors.push(format!("description too long ({len} chars, max 600)"));
            }
            if !has_trigger_word(&collapsed) {
                errors.push(
                    "description lacks trigger words (use when / when / add / create / ...)"
                        .to_string(),
                );
            }
        }
    }

    // `source:` — must be exactly `scan` or `manual`.
    match yaml_field(&body, "source") {
        Some(s) if matches!(s.trim(), "scan" | "manual") => {}
        _ => errors.push("frontmatter: missing \"source\" (expected scan|manual)".to_string()),
    }

    errors
}

/// Extract the YAML frontmatter body — the text between the opening `---\n` and
/// the next `\n---`. The content must *start* with `---\n`.
fn extract_frontmatter(normalized: &str) -> Option<String> {
    let rest = normalized.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(rest[..end].to_string())
}

/// Read a single-line `^key:\s*(.+)$` field from a YAML body.
fn yaml_field(body: &str, key: &str) -> Option<String> {
    for line in body.split('\n') {
        if let Some(rest) = line.strip_prefix(key) {
            if let Some(value) = rest.strip_prefix(':') {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Read the `description:` field — supports a quoted single-line value and a
/// multi-line value (subsequent indented lines). Mirrors the JS regex
/// `^description:\s*(?:"([\s\S]+?)"|([^\n]+(?:\n\s+[^\n]+)*))$`.
fn yaml_description(body: &str) -> Option<String> {
    let lines: Vec<&str> = body.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix("description:") else {
            continue;
        };
        let first = rest.trim_start();
        if first.is_empty() {
            return None;
        }
        // Quoted form: `"..."` possibly spanning lines.
        if let Some(quoted) = first.strip_prefix('"') {
            if let Some(end) = quoted.find('"') {
                return Some(quoted[..end].to_string());
            }
            // Multi-line quoted — collect until the closing quote.
            let mut acc = quoted.to_string();
            for next in &lines[i + 1..] {
                acc.push('\n');
                if let Some(end) = next.find('"') {
                    acc.push_str(&next[..end]);
                    return Some(acc);
                }
                acc.push_str(next);
            }
            return Some(acc);
        }
        // Unquoted form: the value plus any following indented continuation
        // lines.
        let mut acc = first.to_string();
        for next in &lines[i + 1..] {
            if next.starts_with(char::is_whitespace) && !next.trim().is_empty() {
                acc.push('\n');
                acc.push_str(next);
            } else {
                break;
            }
        }
        return Some(acc);
    }
    None
}

/// `true` if `name` is kebab-case: `^[a-z][a-z0-9-]+$` (at least 2 chars).
fn is_kebab_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut count = 1;
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return false;
        }
        count += 1;
    }
    count >= 2
}

/// Collapse every run of whitespace to a single space and trim — JS
/// `.replace(/\s+/g, ' ').trim()`.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `true` if `desc` (already whitespace-collapsed) carries a trigger word.
/// Mirrors `\b(use when|when the user|add|create|new|detect|check|write|even if)\b`.
fn has_trigger_word(desc: &str) -> bool {
    let lower = desc.to_ascii_lowercase();
    if lower.contains("use when") || lower.contains("when the user") || lower.contains("even if") {
        return true;
    }
    ["add", "create", "new", "detect", "check", "write"]
        .iter()
        .any(|w| contains_word(&lower, w))
}

/// The skill-validate-gate verdict for a `SKILL.md` body under `mode`.
///
/// 1:1 with `skill-validate-gate.js`: structural errors → `Deny` in strict
/// mode, `Warn` advisory otherwise. A valid skill → `Allow`.
fn skill_validate_verdict(content: &str, mode: GateMode) -> Verdict {
    let errors = validate_skill(content);
    if errors.is_empty() {
        return Verdict::Allow;
    }
    let error_list = errors
        .iter()
        .map(|e| format!("  - {e}"))
        .collect::<Vec<_>>()
        .join("\n");
    let reason = format!(
        "[skill-validate-gate] SKILL.md fails structural validation:\n{error_list}\n\
         Run `bun .claude/scripts/skills.js validate` for details."
    );
    if mode == GateMode::Strict {
        Verdict::Deny { reason }
    } else {
        Verdict::Warn { message: reason }
    }
}

// ---------------------------------------------------------------------------
// Contract impl
// ---------------------------------------------------------------------------

/// The consolidated Write/Edit size & skill-validation module.
pub struct SizeGate;

impl Check for SizeGate {
    /// Run the three ported `PreToolUse(Write|Edit)` gates.
    ///
    /// Each gate self-resolves its own `MUSTARD_*_MODE`. The first gate to
    /// reach a `Deny` wins (`Deny` dominates the fold anyway); `Warn`
    /// advisories from multiple gates are concatenated into one message so
    /// none is dropped.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        let tool = input.tool_name.as_deref().unwrap_or_default();
        if tool != "Write" && tool != "Edit" {
            return Ok(Verdict::Allow);
        }
        let Some(file_path) = file_path_of(input) else {
            return Ok(Verdict::Allow);
        };

        let mut warnings: Vec<String> = Vec::new();

        // ── spec-size-gate (+ AC quality audit) ───────────────────────────
        if is_spec_path(&file_path) {
            let spec_mode = resolve_mode("MUSTARD_SPEC_SIZE_MODE", GateMode::Warn);
            let ac_mode = resolve_mode("MUSTARD_AC_QUALITY_MODE", GateMode::Warn);
            if spec_mode != GateMode::Off || ac_mode != GateMode::Off {
                if let Some(content) = resolve_content(input) {
                    // AC audit first (advisory only — never a Deny).
                    if ac_mode != GateMode::Off {
                        if let Some(advisory) = ac_quality_advisory(&content) {
                            warnings.push(advisory);
                        }
                    }
                    if spec_mode != GateMode::Off {
                        match spec_size_verdict(count_lines(&content), spec_mode) {
                            Verdict::Deny { reason } => return Ok(Verdict::Deny { reason }),
                            Verdict::Warn { message } => warnings.push(message),
                            _ => {}
                        }
                    }
                }
            }
        }

        // ── skill-size-gate + skill-validate-gate ─────────────────────────
        if is_skill_path(&file_path) {
            let size_mode = resolve_mode("MUSTARD_SKILL_SIZE_MODE", GateMode::Warn);
            let validate_mode =
                resolve_mode("MUSTARD_SKILL_VALIDATE_GATE_MODE", GateMode::Warn);
            if size_mode != GateMode::Off || validate_mode != GateMode::Off {
                if let Some(content) = resolve_content(input) {
                    if size_mode != GateMode::Off {
                        match skill_size_verdict(&content, count_lines(&content), size_mode) {
                            Verdict::Deny { reason } => return Ok(Verdict::Deny { reason }),
                            Verdict::Warn { message } => warnings.push(message),
                            _ => {}
                        }
                    }
                    if validate_mode != GateMode::Off {
                        match skill_validate_verdict(&content, validate_mode) {
                            Verdict::Deny { reason } => return Ok(Verdict::Deny { reason }),
                            Verdict::Warn { message } => warnings.push(message),
                            _ => {}
                        }
                    }
                }
            }
        }

        if warnings.is_empty() {
            Ok(Verdict::Allow)
        } else {
            Ok(Verdict::Warn {
                message: warnings.join("\n"),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_input(file_path: &str, content: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_path, "content": content }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    /// A string of `n` newline-separated lines.
    fn make_content(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
    }

    // --- spec-size-gate parity (size-gates.test.js) ------------------------

    #[test]
    fn spec_150_lines_warn_mode_allows() {
        assert_eq!(spec_size_verdict(150, GateMode::Warn), Verdict::Allow);
    }

    #[test]
    fn spec_250_lines_warn_mode_warns() {
        assert!(matches!(
            spec_size_verdict(250, GateMode::Warn),
            Verdict::Warn { .. }
        ));
    }

    #[test]
    fn spec_450_lines_warn_mode_strict_warn_tier() {
        match spec_size_verdict(450, GateMode::Warn) {
            Verdict::Warn { message } => assert!(message.contains("strict")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn spec_550_lines_warn_mode_advises_never_denies() {
        let v = spec_size_verdict(550, GateMode::Warn);
        assert!(!v.is_blocking());
        assert!(matches!(v, Verdict::Warn { .. }));
    }

    #[test]
    fn spec_550_lines_strict_mode_denies() {
        assert!(spec_size_verdict(550, GateMode::Strict).is_blocking());
    }

    #[test]
    fn spec_499_lines_strict_mode_no_deny() {
        assert!(!spec_size_verdict(499, GateMode::Strict).is_blocking());
    }

    #[test]
    fn non_spec_md_path_is_silent() {
        let (input, ctx) = write_input("/project/README.md", &make_content(550));
        assert_eq!(
            SizeGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn spec_path_recognition() {
        // Flat layout (no buckets).
        assert!(is_spec_path("/project/.claude/spec/my-epic/spec.md"));
        assert!(is_spec_path(
            "C:\\proj\\.claude\\spec\\done-spec\\spec.md"
        ));
        // Generic /spec/ segment (unchanged).
        assert!(is_spec_path("/repo/spec/feature/x.md"));
        assert!(!is_spec_path("/project/README.md"));
        assert!(!is_spec_path("/project/.claude/skills/x/SKILL.md"));
    }

    // --- skill-size-gate parity --------------------------------------------

    #[test]
    fn skill_550_lines_warn_mode_advises() {
        let v = skill_size_verdict(&make_content(550), 550, GateMode::Warn);
        assert!(matches!(v, Verdict::Warn { .. }));
        assert!(!v.is_blocking());
    }

    #[test]
    fn skill_550_lines_strict_mode_denies() {
        assert!(skill_size_verdict(&make_content(550), 550, GateMode::Strict).is_blocking());
    }

    #[test]
    fn skill_generated_warn_mode_skipped() {
        let content = format!("<!-- mustard:generated -->\n{}", make_content(549));
        // 550 lines, generated, warn mode → skipped.
        assert_eq!(
            skill_size_verdict(&content, 550, GateMode::Warn),
            Verdict::Allow
        );
    }

    #[test]
    fn skill_generated_strict_mode_still_denies() {
        let content = format!("<!-- mustard:generated -->\n{}", make_content(549));
        assert!(skill_size_verdict(&content, 550, GateMode::Strict).is_blocking());
    }

    #[test]
    fn skill_path_recognition() {
        assert!(is_skill_path("/project/.claude/skills/my-skill/SKILL.md"));
        assert!(is_skill_path("SKILL.md"));
        assert!(!is_skill_path("/project/.claude/skills/my-skill/README.md"));
    }

    // --- skill-validate-gate parity (skill-validate-gate.test.js) ----------

    const VALID_SKILL: &str = "---\nname: my-skill\ndescription: Comprehensive helper. \
        Use when the user wants to do something specific that requires guidance and \
        triggers automatic activation reliably.\nsource: manual\n---\n\n# My Skill\n\n\
        Body content here.\n";

    const INVALID_NO_SOURCE: &str = "---\nname: my-skill\ndescription: Comprehensive helper. \
        Use when the user wants to do something specific that requires guidance and \
        triggers automatic activation reliably.\n---\n\n# My Skill\n\nBody content here.\n";

    #[test]
    fn valid_skill_passes_validation() {
        assert!(validate_skill(VALID_SKILL).is_empty());
    }

    #[test]
    fn invalid_skill_missing_source_flagged() {
        let errors = validate_skill(INVALID_NO_SOURCE);
        assert!(errors.iter().any(|e| e.contains("source")));
    }

    #[test]
    fn valid_skill_warn_mode_allows() {
        assert_eq!(
            skill_validate_verdict(VALID_SKILL, GateMode::Warn),
            Verdict::Allow
        );
    }

    #[test]
    fn invalid_skill_warn_mode_warns() {
        match skill_validate_verdict(INVALID_NO_SOURCE, GateMode::Warn) {
            Verdict::Warn { message } => {
                assert!(message.contains("skill-validate-gate"));
                assert!(message.contains("source"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn invalid_skill_strict_mode_denies() {
        match skill_validate_verdict(INVALID_NO_SOURCE, GateMode::Strict) {
            Verdict::Deny { reason } => assert!(reason.contains("source")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn missing_frontmatter_flagged() {
        let errors = validate_skill("# No frontmatter\n\nbody");
        assert!(errors.iter().any(|e| e.contains("frontmatter")));
    }

    #[test]
    fn non_kebab_name_flagged() {
        let bad = "---\nname: MySkill\ndescription: Comprehensive helper. Use when the \
            user wants guidance and reliable triggering across sessions reliably.\n\
            source: manual\n---\nbody";
        let errors = validate_skill(bad);
        assert!(errors.iter().any(|e| e.contains("kebab-case")));
    }

    // --- mode resolution ----------------------------------------------------

    #[test]
    fn resolve_mode_defaults_and_parses() {
        // Unset env var → default.
        assert_eq!(
            resolve_mode("MUSTARD_SIZE_GATE_TEST_UNSET_VAR", GateMode::Warn),
            GateMode::Warn
        );
    }

    // --- Edit simulation ----------------------------------------------------

    #[test]
    fn edit_on_missing_file_simulates_empty_base() {
        // An Edit against a non-existent file: current = "", apply old→new.
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({
                "file_path": "/nonexistent/.claude/spec/x/spec.md",
                "old_string": "",
                "new_string": make_content(250),
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let content = resolve_content(&input).expect("Edit resolves content");
        assert_eq!(count_lines(&content), 250);
    }

    // --- gate routing -------------------------------------------------------

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        let (input, _) = write_input("/p/.claude/spec/x/spec.md", &make_content(999));
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        };
        assert_eq!(
            SizeGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_write_edit_tool_allows() {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: String::new(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        assert_eq!(
            SizeGate.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    // --- AC quality audit ---------------------------------------------------

    #[test]
    fn ac_audit_flags_weak_build_only_criteria() {
        let content = "## Acceptance Criteria\n\n\
            - [ ] AC-1: Build — Command: `npm run build`\n\
            - [ ] AC-2: Test — Command: `bun test`\n\
            - [ ] AC-3: Lint — Command: `pnpm lint`\n";
        let advisory = ac_quality_advisory(content).expect("weak AC flagged");
        assert!(advisory.contains("AC Quality"));
        assert!(advisory.contains("build/test"));
    }

    #[test]
    fn ac_audit_passes_rich_criteria() {
        let content = "## Acceptance Criteria\n\n\
            - [ ] AC-1: payload — Command: `node -e \"assert(x)\"`\n\
            - [ ] AC-2: state — Command: `bash -c 'grep foo bar'`\n";
        // Rich criteria, all have a Command → no advisory.
        assert!(ac_quality_advisory(content).is_none());
    }
}
