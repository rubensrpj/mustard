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
    sorted.sort_by(|a, b| b.len().cmp(&a.len()));
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
}
