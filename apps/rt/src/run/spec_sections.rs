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

/// Canonical section key → ordered list of accepted heading names. Index 0 is
/// the canonical EN name; the last entry is the canonical PT name.
fn variants(key: &str) -> Option<&'static [&'static str]> {
    Some(match key {
        "context" => &["Context", "Contexto"],
        "summary" => &["Summary", "Resumo"],
        "boundaries" => &["Boundaries", "Limites"],
        "files" => &["Files", "Arquivos"],
        "rootCause" => &["Root cause", "Causa raiz"],
        "tasks" => &["Tasks", "Checklist", "Tarefas"],
        "acceptanceCriteria" => &["Acceptance Criteria", "Critérios de Aceitação"],
        "nonGoals" => &["Non-Goals", "Não-Objetivos"],
        "concerns" => &["Concerns", "Preocupações"],
        "decisions" => &["Decisions", "Decisões não-óbvias"],
        "dependencies" => &["Dependencies", "Dependências"],
        "entityInfo" => &["Entity Info", "Informações da Entidade"],
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
