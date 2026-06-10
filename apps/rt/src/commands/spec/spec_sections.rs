//! Spec markdown section heading resolution — a port of
//! `scripts/_lib/spec-sections.js`.
//!
//! Mustard specs may be written in English (`## Files`) or Portuguese
//! (`## Arquivos`). This module centralizes the canonical key ↔ language
//! variant mapping so every `run`-face parser resolves headings the same way.
//!
//! Port note: the JS version built a `RegExp`. `mustard-rt` carries no regex
//! dependency, so heading matching is done with plain string scanning. The JS
//! pattern was `^##\s+(?:variant)\b.*$`, case-insensitive, multiline — a `\b`
//! word boundary after the name and an arbitrary suffix. [`is_heading`]
//! reproduces that contract exactly.

/// Canonical section key → ordered list of accepted display heading names.
/// Index 0 is the canonical EN display name; the last entry is the canonical
/// PT display name.
///
/// The keys here are the language-agnostic canonical identifiers used by
/// [`mustard_core::domain::spec::contract::PRD_SECTIONS`] /
/// [`PLAN_SECTIONS`](mustard_core::domain::spec::contract::PLAN_SECTIONS) and
/// throughout the rt parsers. They are matched case-insensitively, and both
/// the kebab (`acceptance-criteria`, `non-goals`) and the legacy camelCase
/// (`acceptanceCriteria`, `nonGoals`) spellings resolve to the same variants
/// so older callers keep working. A spec on disk carries the *display*
/// heading (per the author's `language`); this table bridges key → display.
fn variants(key: &str) -> Option<&'static [&'static str]> {
    Some(match key.trim().to_ascii_lowercase().as_str() {
        "context" => &["Context", "Contexto"],
        "users" => &["Users/Stakeholders", "Usuários/Stakeholders", "Users", "Usuários"],
        "metric" => &["Success Metric", "Métrica de sucesso", "Metric", "Métrica"],
        "summary" => &["Summary", "Resumo"],
        "boundaries" => &["Boundaries", "Limites"],
        "files" => &["Files", "Arquivos"],
        "rootcause" => &["Root cause", "Causa raiz"],
        "tasks" => &["Tasks", "Checklist", "Tarefas"],
        "acceptance-criteria" | "acceptancecriteria" => {
            &["Acceptance Criteria", "Critérios de Aceitação"]
        }
        "non-goals" | "nongoals" => &["Non-Goals", "Não-Objetivos"],
        "concerns" => &["Concerns", "Preocupações"],
        "decisions" => &["Decisions", "Decisões não-óbvias"],
        "dependencies" => &["Dependencies", "Dependências"],
        "entityinfo" => &["Entity Info", "Informações da Entidade"],
        "symptom" => &["Symptom", "Sintoma"],
        _ => return None,
    })
}

/// Whether a character ends a `\b` word boundary — i.e. is *not* a word char.
/// JavaScript `\b` treats `[A-Za-z0-9_]` as word characters.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Test whether a single line is a `## ` heading for the given canonical key.
///
/// Mirrors the JS `headingRegex(key).test(line)` contract: the line must start
/// with `## ` (after optional surrounding whitespace is *not* trimmed — the JS
/// regex anchors `^##`), the heading name matches case-insensitively, and the
/// name is followed by a word boundary (end of name not adjacent to another
/// word char). An unknown key never matches (fail-open).
#[must_use]
pub fn is_heading(line: &str, key: &str) -> bool {
    let Some(names) = variants(key) else {
        return false;
    };
    // `^##\s+` — `## ` then one or more whitespace chars.
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    let after_ws = rest.trim_start_matches([' ', '\t']);
    if after_ws.len() == rest.len() {
        // `\s+` requires at least one whitespace char after `##`.
        return false;
    }
    let lower = after_ws.to_lowercase();
    // Longest variants first so the longer PT name wins over a shorter prefix.
    let mut sorted: Vec<&str> = names.to_vec();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.len()));
    for name in sorted {
        let name_lower = name.to_lowercase();
        if let Some(tail) = lower.strip_prefix(&name_lower) {
            // `\b` after the name: next char (if any) must not be a word char,
            // and the last char of the name must be a word char (it always is
            // for these names).
            match tail.chars().next() {
                None => return true,
                Some(c) if !is_word_char(c) => return true,
                _ => {}
            }
        }
    }
    false
}

/// The full `## <key>` section — from its heading line (inclusive) through the
/// line before the next `## ` heading (or EOF) — or `None` when the section is
/// absent. Heading recognition is i18n-aware via [`is_heading`], so the block
/// preserves whatever heading text (EN or PT) the source used. Used to carry a
/// section verbatim from one document into another (e.g. the parent spec's
/// `## Acceptance Criteria` into a generated `wave-plan.md`).
///
/// Defensive pick among HOMONYMOUS sections: legacy drafts (binaries before
/// TF 2026-06-10-ac-heading-unico) duplicated the AC heading — a placeholder
/// body first ("Ver abaixo."), the real list second — so "first heading wins"
/// returned the placeholder to every reader (qa-run, analyze-validation,
/// wave-scaffold's AC carry). Among duplicates, the first block carrying a
/// markdown list item (`- `) wins; with no such block, the first one (the
/// historical behaviour, and the only case for well-formed specs).
#[must_use]
pub fn section_block(markdown: &str, key: &str) -> Option<String> {
    let blocks = section_blocks(markdown, key);
    if let Some(listy) = blocks.iter().find(|b| has_list_item(b)) {
        return Some(listy.clone());
    }
    blocks.into_iter().next()
}

/// Every `## <key>` section block in document order — the building block of
/// [`section_block`]'s defensive pick. Each block spans its heading line
/// (inclusive) through the line before the next `## ` heading.
fn section_blocks(markdown: &str, key: &str) -> Vec<String> {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !is_heading(lines[i], key) {
            i += 1;
            continue;
        }
        let mut end = lines.len();
        for (j, l) in lines.iter().enumerate().skip(i + 1) {
            if l.starts_with("## ") {
                end = j;
                break;
            }
        }
        out.push(lines[i..end].join("\n"));
        i = end;
    }
    out
}

/// Whether a section block's BODY (heading line excluded) carries at least one
/// markdown list item — the "has parseable content" signal of the defensive
/// pick (every parsed section shape in mustard — AC items, file bullets,
/// checklist boxes — is a `- ` list).
fn has_list_item(block: &str) -> bool {
    block
        .lines()
        .skip(1)
        .any(|l| l.trim_start().starts_with("- "))
}

/// Extracts the parent spec slug from a `### Parent: <slug>` header line.
///
/// Case-insensitive on the `Parent` key; trims surrounding whitespace from
/// the slug. Returns the first match's slug, or `None` when the marker is
/// absent. The line must start with `###` followed by at least one
/// whitespace character — leading whitespace on the line itself is also
/// permitted (the scan trims each line before testing the prefix).
///
/// String-only implementation: the crate carries no regex dependency and
/// this function is not worth adding one.
#[allow(dead_code)] // kept for API consistency with sibling spec-section helpers
#[must_use]
pub fn extract_parent(markdown: &str) -> Option<String> {
    const KEY: &str = "parent";
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("###") else {
            continue;
        };
        // `\s+` — require at least one whitespace char after `###`.
        let after_hashes = rest.trim_start_matches([' ', '\t']);
        if after_hashes.len() == rest.len() {
            continue;
        }
        // Case-insensitive match on the literal key `Parent` (ASCII only).
        if after_hashes.len() < KEY.len() {
            continue;
        }
        let (key_slice, mut tail) = after_hashes.split_at(KEY.len());
        if !key_slice.eq_ignore_ascii_case(KEY) {
            continue;
        }
        // Optional whitespace between the key and the colon.
        tail = tail.trim_start_matches([' ', '\t']);
        let Some(after_colon) = tail.strip_prefix(':') else {
            continue;
        };
        let slug = after_colon.trim();
        if slug.is_empty() {
            continue;
        }
        return Some(slug.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_english_and_portuguese_headings() {
        assert!(is_heading("## Files", "files"));
        assert!(is_heading("## Arquivos", "files"));
        assert!(is_heading("## Files (this pipeline)", "files"));
        assert!(is_heading("##  Files", "files"));
        assert!(!is_heading("## Summary", "files"));
        assert!(!is_heading("## Filesystem", "files"));
        assert!(!is_heading("###Files", "files"));
    }

    #[test]
    fn requires_whitespace_after_hashes() {
        assert!(!is_heading("##Files", "files"));
    }

    #[test]
    fn unknown_key_never_matches() {
        assert!(!is_heading("## Files", "nonsense"));
    }

    /// Roundtrip (legacy corpus): a spec drafted by a pre-fix binary carries
    /// the AC heading TWICE — placeholder body first, the real list second.
    /// The defensive pick must return the block with parseable items so specs
    /// already duplicated on disk keep validating.
    #[test]
    fn roundtrip_section_block_prefers_homonymous_section_with_list_items() {
        let md = "# Spec\n\n## Critérios de Aceitação\n\nVer abaixo.\n\n\
                  ## Critérios de Aceitação\n\n- **AC-1** — builds.\n  Command: `cargo build`\n\n\
                  ## Arquivos\n\n- `a.rs`\n";
        let block = section_block(md, "acceptanceCriteria").expect("AC section found");
        assert!(block.contains("**AC-1**"), "real list wins over placeholder: {block}");
        assert!(!block.contains("Ver abaixo"), "placeholder block skipped: {block}");
        assert!(!block.contains("Arquivos"), "stops at the next `## `: {block}");
    }

    /// Single-section documents keep the historical behaviour exactly: the
    /// first (only) block is returned even when it carries no list item.
    #[test]
    fn section_block_single_section_unchanged_even_without_list() {
        let md = "# Spec\n\n## Context\n\nprose only\n\n## Files\n\n- `a.rs`\n";
        let block = section_block(md, "context").expect("context found");
        assert!(block.contains("prose only"));
        // Duplicates with NO listy candidate also fall back to the first.
        let dup = "## Context\n\nfirst\n\n## Context\n\nsecond\n";
        let block = section_block(dup, "context").expect("context found");
        assert!(block.contains("first"));
        assert!(!block.contains("second"));
    }

    #[test]
    fn extract_parent_basic() {
        assert_eq!(
            extract_parent("### Parent: feature-x\n"),
            Some("feature-x".to_string())
        );
    }

    #[test]
    fn extract_parent_absent() {
        assert_eq!(extract_parent("# Title\n\nSome body\n"), None);
        assert_eq!(extract_parent(""), None);
    }

    #[test]
    fn extract_parent_whitespace() {
        assert_eq!(
            extract_parent("###    Parent:   feature-x   \n"),
            Some("feature-x".to_string())
        );
    }

    #[test]
    fn extract_parent_lowercase_key() {
        assert_eq!(
            extract_parent("### parent: feature-x\n"),
            Some("feature-x".to_string())
        );
    }

    #[test]
    fn extract_parent_after_other_h3() {
        assert_eq!(
            extract_parent("### Status: draft\n### Parent: parent-y\n"),
            Some("parent-y".to_string())
        );
    }

    #[test]
    fn extract_parent_ignores_other_h3_without_match() {
        // `### Parents:` (note trailing `s`) is not the marker.
        assert_eq!(extract_parent("### Parents: a, b\n"), None);
        // `### Parent` with no colon is not the marker either.
        assert_eq!(extract_parent("### Parent feature-x\n"), None);
    }

    #[test]
    fn extract_parent_empty_slug_is_none() {
        assert_eq!(extract_parent("### Parent:   \n"), None);
    }
}
