//! `skills_audit` — the recommended-skills count advisory.
//!
//! ## Scope (b3 Wave 3, Task family)
//!
//! Ports `recommended-skills-audit.js` 1:1: a `PreToolUse(Task)` advisory that
//! counts the skills a dispatch lists in its `recommended_skills` hint and
//! warns when that count exceeds 10. It is **advisory only** — it never
//! blocks. The JS hook prints a stderr WARN above the threshold; a Rust hook
//! expresses that as a non-blocking [`Verdict::Warn`].
//!
//! The JS hook also emits a `recommended-skills` metric (skill count, resolved
//! bytes). That metric write is a side effect with no verdict to *change*; the
//! parity surface the oracle exercises is the count + the warn boundary, so
//! this port keeps the metric emission verdict-free and focuses the parity
//! tests on `extract_skills` / the warn threshold.
//!
//! ## Contract
//!
//! [`SkillsAudit`] implements [`Check`] — but it can only ever return `Allow`
//! or `Warn`, never `Deny`. It is a `Check` (not an `Observer`) because the
//! warn surfaces in the agent's context, which only a `Verdict` can carry.

use mustard_core::error::Error;
use mustard_core::metrics::{MetricLine, emit_metric};
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;

use crate::util::now_iso8601;

/// Skill count above which the audit warns. Port of `WARN_THRESHOLD`.
const WARN_THRESHOLD: usize = 10;

/// The recommended-skills audit module.
pub struct SkillsAudit;

/// Extract skill names from a Task prompt. Port of `extractSkills`.
///
/// Two shapes are tolerated:
/// 1. `recommended_skills: [alpha, beta, gamma]` — an array literal, tolerant
///    of spacing / underscore / dash in the key.
/// 2. a markdown section headed "recommended skills" with bulleted items,
///    until the next `##` heading or end of text.
///
/// Returns a de-duplicated, sorted set of lowercase skill names.
fn extract_skills(prompt: &str) -> Vec<String> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    let lower = prompt.to_ascii_lowercase();

    // ── Shape 1: recommended_skills: [a, b, c] ─────────────────────────────
    // Find each `recommended[_\s-]?skills?` key followed by `[ ... ]`.
    extract_array_skills(&lower, &mut found);

    // ── Shape 2: a "## Recommended Skills" section with bullet items ───────
    extract_section_skills(prompt, &mut found);

    found.into_iter().collect()
}

/// Shape 1: scan for `recommended[_\s-]?skills?[:\s]*[ ... ]` array literals.
/// Operates on the already-lowercased prompt.
fn extract_array_skills(lower: &str, found: &mut BTreeSet<String>) {
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let Some(rel) = lower[i..].find("recommended") else {
            break;
        };
        let start = i + rel;
        let mut cursor = start + "recommended".len();
        // Optional single `_`, ` ` or `-` separator.
        if matches!(bytes.get(cursor), Some(b'_' | b' ' | b'-')) {
            cursor += 1;
        }
        // `skill` then an optional `s`.
        if lower[cursor..].starts_with("skill") {
            cursor += "skill".len();
            if bytes.get(cursor) == Some(&b's') {
                cursor += 1;
            }
            // Skip `[:\s]*` then expect `[`.
            while matches!(bytes.get(cursor), Some(b':' | b' ' | b'\t' | b'\n' | b'\r')) {
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&b'[') {
                cursor += 1;
                if let Some(end_rel) = lower[cursor..].find(']') {
                    let inner = &lower[cursor..cursor + end_rel];
                    for raw in inner.split([',', '\n']) {
                        let name: String = raw
                            .chars()
                            .filter(|c| !matches!(c, '"' | '\'' | '`'))
                            .collect();
                        let name = name.trim();
                        if !name.is_empty() {
                            found.insert(name.to_string());
                        }
                    }
                    cursor += end_rel + 1;
                }
            }
        }
        i = cursor.max(start + 1);
    }
}

/// Shape 2: a `#`/`##` heading whose text is "recommended skill(s)", followed
/// by bullet lines, until the next heading or EOF. Operates on the
/// original-case prompt; names are lowercased on insert.
fn extract_section_skills(prompt: &str, found: &mut BTreeSet<String>) {
    let lines: Vec<&str> = prompt.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if is_recommended_skills_heading(lines[i]) {
            // Collect bullets until the next heading / EOF.
            let mut j = i + 1;
            while j < lines.len() && !is_markdown_heading(lines[j]) {
                if let Some(name) = bullet_skill_name(lines[j]) {
                    found.insert(name.to_ascii_lowercase());
                }
                j += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

/// `true` if `line` is a `#`/`##` heading reading "recommended skill(s)".
fn is_recommended_skills_heading(line: &str) -> bool {
    let t = line.trim();
    let body = t.strip_prefix("##").or_else(|| t.strip_prefix('#'));
    let Some(body) = body else {
        return false;
    };
    let body = body.trim().to_ascii_lowercase();
    body == "recommended skill" || body == "recommended skills"
}

/// `true` if `line` is any markdown heading (`#`/`##`).
fn is_markdown_heading(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("# ") || t.starts_with("## ") || t == "#" || t == "##"
}

/// Extract a skill name from a bullet line: `- name`, `* name`, `1. name`,
/// optionally backtick-wrapped. Mirrors the JS bullet regex.
fn bullet_skill_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    // Strip the bullet marker: `-`, `*`, or `<digits>.`.
    let rest = if let Some(r) = t.strip_prefix('-') {
        r
    } else if let Some(r) = t.strip_prefix('*') {
        r
    } else {
        // `\d+\.`
        let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        t[digits.len()..].strip_prefix('.')?
    };
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start().trim_start_matches('`');
    // The name: `[a-z0-9][a-z0-9._/:-]+` (case-insensitive), at least 2 chars.
    let mut chars = rest.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphanumeric() {
        return None;
    }
    let mut name = String::new();
    name.push(first);
    for c in chars {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | ':' | '-') {
            name.push(c);
        } else {
            break;
        }
    }
    // The JS regex requires at least 2 chars total (`[a-z0-9]` + `[...]+`).
    if name.len() >= 2 { Some(name) } else { None }
}

/// Resolve a skill name to its `SKILL.md` byte size, if it exists.
/// Port of `resolveSkill`: checks `.claude/skills/{name}/SKILL.md`, then a
/// shallow one-level subproject scan. Returns the byte size or `None`.
fn resolve_skill_bytes(project_dir: &str, name: &str) -> Option<u64> {
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    if safe.is_empty() {
        return None;
    }
    let primary = Path::new(project_dir)
        .join(".claude")
        .join("skills")
        .join(&safe)
        .join("SKILL.md");
    if let Ok(meta) = std::fs::metadata(&primary) {
        return Some(meta.len());
    }
    // Shallow subproject scan, one level deep.
    let entries = std::fs::read_dir(project_dir).ok()?;
    for entry in entries.filter_map(std::result::Result::ok) {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let dname = entry.file_name().to_string_lossy().into_owned();
        if dname.starts_with('.') || dname == "node_modules" {
            continue;
        }
        let cand = entry
            .path()
            .join(".claude")
            .join("skills")
            .join(&safe)
            .join("SKILL.md");
        if let Ok(meta) = std::fs::metadata(&cand) {
            return Some(meta.len());
        }
    }
    None
}

impl Check for SkillsAudit {
    /// Audit the recommended-skills list of a `PreToolUse(Task)` dispatch.
    ///
    /// Advisory only — returns `Warn` above the threshold, `Allow` otherwise.
    /// Never denies. Emits a `recommended-skills` metric as a side effect.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if !matches!(input.tool_name.as_deref(), Some("Task" | "Agent")) {
            return Ok(Verdict::Allow);
        }
        let project = if ctx.project_dir.is_empty() {
            input.cwd.as_deref().unwrap_or(".")
        } else {
            ctx.project_dir.as_str()
        };
        let tool_input = &input.tool_input;
        let prompt = tool_input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let subagent_type = tool_input
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        let skills = extract_skills(prompt);
        let skill_count = skills.len();

        // Emit the `recommended-skills` metric (fail-silent side effect).
        let (total_bytes, resolved) = skills.iter().fold((0u64, 0usize), |(b, r), name| {
            match resolve_skill_bytes(project, name) {
                Some(size) => (b + size, r + 1),
                None => (b, r),
            }
        });
        let line = MetricLine::new(now_iso8601(), "recommended-skills")
            .tokens_affected((total_bytes / 4) as i64)
            .tokens_saved(0)
            .note("pipeline dispatch")
            .extras(json!({
                "skill_count": skill_count,
                "resolved_count": resolved,
                "skills": skills.iter().take(20).cloned().collect::<Vec<_>>().join(","),
                "subagent_type": subagent_type,
            }));
        let _ = emit_metric(Path::new(project), &line);

        // Advisory: warn above the threshold, never block.
        if skill_count > WARN_THRESHOLD {
            return Ok(Verdict::Warn {
                message: format!(
                    "[recommended-skills] WARN: pipeline dispatch lists \
                     {skill_count}>{WARN_THRESHOLD} skills — consider pruning the list."
                ),
            });
        }
        Ok(Verdict::Allow)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn task_prompt(prompt: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Task".to_string()),
            tool_input: json!({
                "subagent_type": "general-purpose",
                "description": "do work",
                "prompt": prompt,
            }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        (input, ctx)
    }

    // --- extract_skills parity (recommended-skills-audit.js extractSkills) --

    #[test]
    fn extracts_array_literal_shape() {
        let skills = extract_skills("recommended_skills: [alpha, beta, gamma]");
        assert_eq!(skills, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn extracts_array_with_spacing_and_quotes() {
        let skills = extract_skills("recommended skills: [\"one\", 'two', `three`]");
        assert_eq!(skills, vec!["one", "three", "two"]);
    }

    #[test]
    fn extracts_markdown_section_bullets() {
        let prompt = "## Recommended Skills\n- karpathy-guidelines\n- commit-workflow\n\n## Next\n- ignored";
        let skills = extract_skills(prompt);
        assert!(skills.contains(&"karpathy-guidelines".to_string()));
        assert!(skills.contains(&"commit-workflow".to_string()));
        assert!(!skills.contains(&"ignored".to_string()));
    }

    #[test]
    fn dedupes_skill_names() {
        let skills = extract_skills("recommended_skills: [alpha, alpha, beta]");
        assert_eq!(skills, vec!["alpha", "beta"]);
    }

    #[test]
    fn empty_prompt_has_no_skills() {
        assert!(extract_skills("").is_empty());
        assert!(extract_skills("a prompt with no skills section").is_empty());
    }

    // --- audit verdict ----------------------------------------------------

    #[test]
    fn under_threshold_allows() {
        let (input, ctx) = task_prompt("recommended_skills: [a, b, c]");
        assert_eq!(
            SkillsAudit.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn exactly_threshold_allows() {
        // 10 skills == WARN_THRESHOLD → not over → allow.
        let list = (0..10)
            .map(|i| format!("skill-{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let (input, ctx) = task_prompt(&format!("recommended_skills: [{list}]"));
        assert_eq!(
            SkillsAudit.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn over_threshold_warns_never_denies() {
        // 11 skills > WARN_THRESHOLD → Warn, and never a blocking Deny.
        let list = (0..11)
            .map(|i| format!("skill-{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let (input, ctx) = task_prompt(&format!("recommended_skills: [{list}]"));
        match SkillsAudit.evaluate(&input, &ctx).expect("no error") {
            Verdict::Warn { message } => {
                assert!(message.contains("recommended-skills"));
                assert!(message.contains("11>10"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn audit_is_advisory_and_never_blocks() {
        // Even a huge list must not block.
        let list = (0..100)
            .map(|i| format!("skill-{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let (input, ctx) = task_prompt(&format!("recommended_skills: [{list}]"));
        assert!(
            !SkillsAudit
                .evaluate(&input, &ctx)
                .expect("no error")
                .is_blocking()
        );
    }

    #[test]
    fn non_task_tool_allows() {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        assert_eq!(
            SkillsAudit.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn non_pre_tool_use_trigger_allows() {
        let (input, _) = task_prompt("recommended_skills: [a, b, c, d, e, f, g, h, i, j, k, l]");
        let ctx = Ctx {
            project_dir: ".".to_string(),
            trigger: Some(Trigger::PostToolUse),
        };
        assert_eq!(
            SkillsAudit.evaluate(&input, &ctx).expect("no error"),
            Verdict::Allow
        );
    }

    #[test]
    fn resolve_skill_bytes_finds_primary_skill() {
        let dir = tempdir().unwrap();
        let skill_dir = dir
            .path()
            .join(".claude")
            .join("skills")
            .join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "content").unwrap();
        let bytes = resolve_skill_bytes(dir.path().to_str().unwrap(), "my-skill");
        assert_eq!(bytes, Some(7));
        // An unknown skill resolves to None.
        assert_eq!(
            resolve_skill_bytes(dir.path().to_str().unwrap(), "nope"),
            None
        );
    }
}
