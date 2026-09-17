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
        // The conversation channel (`spec-draft --material`). The drafter emits
        // the EN display heading — language-agnostic, exactly like
        // `## Checklist` — so every reader keys off one literal; the PT variant
        // is registered so a hand-authored PT spec still resolves through THIS
        // resolver instead of growing a second parser.
        "definitions" => &["Definitions", "Definições"],
        "evidence" => &["Evidence", "Evidências"],
        // Os riscos do canal de material. O rascunho escreve o título no idioma
        // da spec, então as duas grafias precisam resolver aqui — é por esta
        // tabela que o `--material-only` acha a seção para trocá-la.
        "risks" => &["Risks", "Riscos"],
        // The reality obligations a plan declares per wave — duties to check the
        // world OUTSIDE the repository (an official document, a live endpoint, a
        // stored row) before writing the code they govern. Rendered into each
        // wave's `spec.md` by the wave-scaffold renderer and read back by the
        // dispatch prompt and by `wave-done`; registered HERE so all three resolve
        // the heading through the one resolver instead of matching a literal.
        "reality-obligations" | "realityobligations" => {
            &["Reality Obligations", "Obrigações de Realidade"]
        }
        "dependencies" => &["Dependencies", "Dependências"],
        "entityinfo" => &["Entity Info", "Informações da Entidade"],
        "symptom" => &["Symptom", "Sintoma"],
        _ => return None,
    })
}

/// Every canonical key [`variants`] resolves — the whole vocabulary a spec
/// heading may carry, in the order [`canonical_key`] probes it.
///
/// One list, next to the table it mirrors: a key added above and forgotten here
/// would make [`canonical_key`] read a perfectly canonical heading as foreign.
/// Only the kebab spellings are listed; the camelCase aliases resolve to the
/// same variants, so probing both would just double the work.
const CANONICAL_KEYS: &[&str] = &[
    "context",
    "users",
    "metric",
    "summary",
    "boundaries",
    "files",
    "rootcause",
    "tasks",
    "acceptance-criteria",
    "non-goals",
    "concerns",
    "decisions",
    "definitions",
    "evidence",
    "risks",
    "reality-obligations",
    "dependencies",
    "entityinfo",
    "symptom",
];

/// The canonical key a `## ` heading line resolves to, or `None` when its title
/// is OUTSIDE the vocabulary above — a heading no reader of this repository
/// parses, in either language.
///
/// The inverse of [`is_heading`], which answers "is this line the heading for
/// key K?": here the key is the answer, not the question. A lint that has to
/// tell a canonical section from a bespoke one needs exactly this direction —
/// asking `is_heading` per key from the outside means restating this table at
/// every call site, and a caller that lists only some of the keys reports every
/// key it forgot as foreign.
#[must_use]
pub fn canonical_key(line: &str) -> Option<&'static str> {
    CANONICAL_KEYS.iter().copied().find(|key| is_heading(line, key))
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

/// The line index one-past-the-end of the `## ` section whose heading sits at
/// `heading_idx`: the first `## ` heading boundary strictly after the heading, or
/// `lines.len()` when the section runs to EOF. So the body is
/// `lines[heading_idx + 1 .. section_end(lines, heading_idx)]` and the block
/// *including* the heading is `lines[heading_idx .. section_end(..)]`.
///
/// The single owner of the "where does a `## ` section stop" scan — shared by
/// [`section_blocks`], the render TASK-block cutters ([`super::super`]'s
/// `cut_section_at`), `reference::files_section_paths`, and the `/scan`
/// `## Guards` swap. Two callers deliberately keep their own boundary loop and
/// must NOT be folded in: `close_gates::checklist_unmarked_in` also treats a
/// bare `##` (no text after the hashes) as a boundary, and
/// `render::read_guards_block` tolerates an *indented* `## ` — folding either in
/// would change behaviour, not remove duplication.
#[must_use]
pub fn section_end(lines: &[&str], heading_idx: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .skip(heading_idx + 1)
        .find(|(_, l)| l.starts_with("## "))
        .map_or(lines.len(), |(j, _)| j)
}

/// The full `## <key>` section — from its heading line (inclusive) through the
/// line before the next `## ` heading (or EOF) — or `None` when the section is
/// absent. Heading recognition is i18n-aware via [`is_heading`], so the block
/// preserves whatever heading text (EN or PT) the source used. Used to carry a
/// section verbatim from one document into another (e.g. the parent spec's
/// `## Acceptance Criteria` into a generated `wave-plan.md`).
///
/// Defensive pick among HOMONYMOUS sections: legacy drafts (from binaries
/// older than the single AC heading key) duplicated the AC heading — a placeholder
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
        let end = section_end(&lines, i);
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

    /// The conversation channel's three sections resolve through the SAME
    /// resolver as every other section — in both spellings — so the QA
    /// extractor, the per-wave renderer and the boundary gate read them with
    /// `section_block`, never with a second parser of their own.
    #[test]
    fn conversation_material_sections_resolve_through_the_shared_resolver() {
        for (key, en, pt) in [
            ("definitions", "## Definitions", "## Definições"),
            ("decisions", "## Decisions", "## Decisões não-óbvias"),
            ("evidence", "## Evidence", "## Evidências"),
        ] {
            assert!(is_heading(en, key), "{key}: EN heading must resolve");
            assert!(is_heading(pt, key), "{key}: PT heading must resolve");
        }
        // The block extractor reaches them too, and stops at the next `## `.
        let md = "## Context\n\nprose\n\n## Evidence\n\n- checked\n  Evidence: `src/a.rs:12`\n\n## Files\n\n- `a.rs`\n";
        let block = section_block(md, "evidence").expect("evidence section found");
        assert!(block.contains("src/a.rs:12"), "{block}");
        assert!(!block.contains("Files"), "stops at the next `## `: {block}");
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

    /// `canonical_key` resolves EVERY display heading the table knows, in both
    /// languages, and answers `None` for a title outside the vocabulary.
    ///
    /// The round trip is the point: a key listed in `CANONICAL_KEYS` that the
    /// table does not know would resolve nothing, and a key the table knows but
    /// the list forgot would read as bespoke — which is how a caller that has to
    /// tell canonical from bespoke starts reporting perfectly ordinary sections.
    #[test]
    fn canonical_key_resolves_the_whole_vocabulary() {
        for key in CANONICAL_KEYS {
            let names = variants(key).unwrap_or_else(|| panic!("`{key}` is not in the table"));
            for name in names {
                let line = format!("## {name}");
                assert_eq!(
                    canonical_key(&line),
                    Some(*key),
                    "`{line}` must resolve to `{key}`",
                );
            }
        }
        // O título do defeito de campo: fora do vocabulário, em nenhuma língua.
        assert_eq!(
            canonical_key("## Decisão em aberto — como saber se uma previsão já foi efetivada"),
            None,
        );
        assert_eq!(canonical_key("## Why now"), None);
        // E o que nem é título `##` nunca resolve.
        assert_eq!(canonical_key("### Files"), None);
        assert_eq!(canonical_key("Files"), None);
    }
}
