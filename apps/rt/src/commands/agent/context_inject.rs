//! Shared deterministic context-injection helpers used by both the
//! orchestrator-side renderer ([`crate::commands::agent::agent_prompt_render`])
//! and the ad-hoc dispatch hook
//! ([`crate::hooks::task::subagent_inject`]).
//!
//! Before this module the two call sites each shipped their *own* copy of:
//!
//! - the spec-`memory/*.md` relevance match (weak token-equality: a prompt
//!   token had to equal a memory-name token verbatim, so "routing" never found
//!   `tabs-routing.md` and "tab" never found `tabs-routing.md` either), and
//! - the `## RECOMMENDED SKILLS` / `## SPEC MEMORY` block formatting, and
//! - the regression-vocabulary inject block.
//!
//! This module is the single home for that logic. The memory match is upgraded
//! from token-equality to an Aho-Corasick scan over the memory-name **stems**
//! (reusing [`mustard_core::domain::vocabulary::VocabularyMatcher`], the same
//! engine the entity floor and the regression gate run on — no second
//! automaton). The stem set is morphology-tolerant: a name like
//! `tabs-routing.md` contributes the stems `tab`, `tabs`, `route`, `routing`,
//! so a prompt mentioning either "tab(s)" or "rout(e|ing)" surfaces the file.
//!
//! Everything is deterministic, fail-open, and locale-agnostic at the engine
//! level: the only locale-aware piece is the vocabulary block's heading/labels,
//! which flow through `i18n::translate` exactly as the legacy copies did.

use mustard_core::domain::vocabulary::{Layer, VocabLayer, VocabularyMatcher};
use mustard_core::io::fs;
use mustard_core::platform::i18n::{self, Locale};
use std::fmt::Write as _;
use std::path::Path;

use crate::commands::review::gate_regression_check;

/// A spec-memory principle file that matched the dispatch intent: its name
/// stem (without the `.md`) and the one-line body summary (empty when the
/// caller asked for a name-only listing).
pub struct MemoryMatch {
    /// File name without the `.md` extension — used both as the wikilink slug
    /// and as the display name.
    pub name: String,
    /// First meaningful body line, capped — empty when not requested.
    pub summary: String,
}

/// Match the `memory/*.md` principle files under `memory_dir` against the
/// dispatch `intent` (role + task/prompt text), capped at `max` results.
///
/// The match is an Aho-Corasick scan: every memory file name is decomposed
/// into morphological stems (see [`name_stems`]) which become the matcher's
/// term list; the intent is the haystack. A file is kept the moment any of its
/// stems fires in the intent. This is strictly stronger than the previous
/// token-equality (which required an exact whole-token match between the intent
/// and the memory name): the stem set folds plural/`-ing`/`-ed`/`-tion`
/// variants together, so "routing" finds `tabs-routing.md` and "tab" finds
/// `tabs-routing.md`.
///
/// `with_summary` controls whether each match carries its first body line (the
/// renderer wants it for the inline `— summary`; the hook lists names only).
///
/// Fail-open: a missing/unreadable directory, or an intent that matches
/// nothing, yields an empty `Vec`.
#[must_use]
pub fn match_spec_memory(
    memory_dir: &Path,
    intent: &str,
    max: usize,
    with_summary: bool,
) -> Vec<MemoryMatch> {
    let Ok(entries) = fs::read_dir(memory_dir) else {
        return Vec::new();
    };

    // Collect candidate (name, stems) once; build the automaton over the union
    // of every file's stems, tagged per-file via the term string.
    struct Candidate {
        name: String,
        path: std::path::PathBuf,
        stems: Vec<String>,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut all_terms: Vec<String> = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        if !entry.file_name.ends_with(".md") || entry.file_name.starts_with('_') {
            continue;
        }
        let name = entry.file_name.trim_end_matches(".md").to_string();
        let stems = name_stems(&name);
        if stems.is_empty() {
            continue;
        }
        all_terms.extend(stems.iter().cloned());
        candidates.push(Candidate {
            name,
            path: entry.path,
            stems,
        });
    }
    if candidates.is_empty() {
        return Vec::new();
    }

    // The haystack scan is case-insensitive: stems are already lowercased, and
    // the intent is lowercased here so the case-sensitive automaton matches.
    let haystack = intent.to_ascii_lowercase();
    let Ok(matcher) = VocabularyMatcher::from_layers(vec![VocabLayer {
        kind: Layer::Keyword,
        terms: all_terms,
    }]) else {
        return Vec::new();
    };
    let hits: std::collections::HashSet<String> = matcher
        .scan(&haystack)
        .into_iter()
        .map(|h| h.term)
        .collect();
    if hits.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<MemoryMatch> = Vec::new();
    for cand in candidates {
        if !cand.stems.iter().any(|s| hits.contains(s)) {
            continue;
        }
        let summary = if with_summary {
            fs::read_to_string(&cand.path)
                .ok()
                .map(|t| extract_memory_summary(&t))
                .unwrap_or_default()
        } else {
            String::new()
        };
        out.push(MemoryMatch {
            name: cand.name,
            summary,
        });
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Decompose a memory file name into morphology-tolerant lowercase stems.
///
/// The name is split on non-alphanumeric boundaries (`tabs-routing` →
/// `["tabs", "routing"]`); each token ≥3 chars is kept, and a crude stem is
/// added alongside the full token so plural / gerund / participle / nominal
/// variants collapse:
///
/// - `tabs`    → `tabs`, `tab`
/// - `routing` → `routing`, `rout`, plus the surface `route`-family stem
/// - `actions` → `actions`, `action`
///
/// Stems shorter than 3 chars are dropped (they match too broadly). The result
/// is deduplicated.
#[must_use]
pub fn name_stems(name: &str) -> Vec<String> {
    let mut stems: Vec<String> = Vec::new();
    let push = |s: String, stems: &mut Vec<String>| {
        if s.len() >= 3 && !stems.contains(&s) {
            stems.push(s);
        }
    };
    for token in name
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if token.len() < 3 {
            continue;
        }
        let token = token.to_string();
        push(token.clone(), &mut stems);
        // Crude English suffix folding — additive, never replaces the surface
        // form. Each branch keeps the longest sensible stem so the term still
        // anchors on a meaningful boundary.
        for suffix in ["ing", "tion", "sion", "ies", "es", "ed", "s"] {
            if let Some(base) = token.strip_suffix(suffix) {
                if base.len() >= 3 {
                    // `ies` → `y` reconstruction (`policies` → `policy`).
                    if suffix == "ies" {
                        push(format!("{base}y"), &mut stems);
                    }
                    push(base.to_string(), &mut stems);
                    break;
                }
            }
        }
    }
    stems
}

/// First non-empty body line (skipping frontmatter + headings), capped at 120
/// chars — the inline summary for a memory entry. Shared so both call sites
/// render identical summaries.
#[must_use]
pub fn extract_memory_summary(text: &str) -> String {
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
        return trimmed.chars().take(120).collect();
    }
    String::new()
}

/// Render a `## SPEC MEMORY` block from matched principle files.
///
/// `with_summary` mirrors [`match_spec_memory`]: when `true`, each line is
/// `- [[name]] — summary`; when `false`, `- [[name]]`. Empty when `matches` is
/// empty so callers can skip appending.
#[must_use]
pub fn render_spec_memory_block(matches: &[MemoryMatch]) -> String {
    if matches.is_empty() {
        return String::new();
    }
    let mut out = String::from("## SPEC MEMORY\n");
    for m in matches {
        if m.summary.is_empty() {
            let _ = writeln!(out, "- [[{}]]", m.name);
        } else {
            let _ = writeln!(out, "- [[{}]] — {}", m.name, m.summary);
        }
    }
    out
}

/// Resolve the (semantic, pattern) layer term lists for the project's
/// regression vocabulary, with the gate's in-memory defaults as the fallback.
///
/// Single owner of the `regression.toml` walk that `agent_prompt_render` and
/// `subagent_inject` previously duplicated verbatim.
#[must_use]
pub fn vocab_layer_terms(project: &Path) -> (Vec<String>, Vec<String>) {
    use mustard_core::domain::vocabulary::{Layer as VLayer, VocabularyDoc};
    let toml_path = project.join(".claude").join("vocab").join("regression.toml");
    let (mut semantic, mut pattern) = match VocabularyDoc::load_from_file(&toml_path) {
        Ok(doc) => (
            doc.layer_terms(VLayer::Semantic)
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<String>>(),
            doc.layer_terms(VLayer::Pattern)
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

/// Render the regression-vocabulary inject block (Semantic + Pattern layers).
///
/// Reuses [`gate_regression_check::build_vocab_matcher`] for the present/absent
/// decision so the inject path agrees with the gate's Moment-1 scan, then lists
/// the layer terms with i18n headings/labels. Empty when the project resolves
/// no vocabulary (fail-open). Shared by both call sites.
#[must_use]
pub fn vocabulary_inject_block(project: &Path, locale: Locale) -> String {
    if gate_regression_check::build_vocab_matcher(project).is_none() {
        return String::new();
    }
    let (semantic, pattern) = vocab_layer_terms(project);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn name_stems_folds_plural_and_gerund() {
        let stems = name_stems("tabs-routing");
        assert!(stems.contains(&"tabs".to_string()));
        assert!(stems.contains(&"tab".to_string()));
        assert!(stems.contains(&"routing".to_string()));
        assert!(stems.contains(&"rout".to_string()));
    }

    #[test]
    fn match_finds_file_by_morphological_variant() {
        let dir = tempdir().unwrap();
        let mem = dir.path();
        std::fs::write(mem.join("tabs-routing.md"), "Body line about routing.\n").unwrap();
        std::fs::write(mem.join("unrelated.md"), "nothing here\n").unwrap();
        // Prompt mentions "routing" — the gerund must surface `tabs-routing.md`
        // even though the file name carries no whole-token "routing" boundary
        // match against a verbatim equality test.
        let hits = match_spec_memory(mem, "we must fix the routing layer", 3, true);
        assert_eq!(hits.len(), 1, "got {:?}", hits.iter().map(|m| &m.name).collect::<Vec<_>>());
        assert_eq!(hits[0].name, "tabs-routing");
        assert!(hits[0].summary.contains("routing"));
    }

    #[test]
    fn match_finds_file_by_singular_when_name_is_plural() {
        let dir = tempdir().unwrap();
        let mem = dir.path();
        std::fs::write(mem.join("tabs-routing.md"), "x\n").unwrap();
        // Prompt says "tab" (singular); name is "tabs" (plural).
        let hits = match_spec_memory(mem, "add a tab to the view", 3, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "tabs-routing");
        assert!(hits[0].summary.is_empty(), "name-only mode keeps summary empty");
    }

    #[test]
    fn match_skips_underscore_prefixed_and_empty_dir() {
        let dir = tempdir().unwrap();
        let mem = dir.path();
        std::fs::write(mem.join("_index.md"), "skip me\n").unwrap();
        assert!(match_spec_memory(mem, "index", 3, false).is_empty());
        // Missing dir is fail-open.
        assert!(match_spec_memory(&mem.join("nope"), "anything", 3, false).is_empty());
    }

    #[test]
    fn render_block_switches_on_summary() {
        let with = vec![MemoryMatch {
            name: "tabs-routing".into(),
            summary: "do the thing".into(),
        }];
        let rendered = render_spec_memory_block(&with);
        assert!(rendered.contains("- [[tabs-routing]] — do the thing"));
        let without = vec![MemoryMatch {
            name: "tabs-routing".into(),
            summary: String::new(),
        }];
        assert!(render_spec_memory_block(&without).contains("- [[tabs-routing]]\n"));
        assert!(render_spec_memory_block(&[]).is_empty());
    }

    #[test]
    fn vocab_layer_terms_falls_back_to_defaults() {
        let dir = tempdir().unwrap();
        let (sem, pat) = vocab_layer_terms(dir.path());
        assert!(sem.iter().any(|s| s == "fail-open"));
        assert!(pat.iter().any(|s| s == "None"));
    }
}
