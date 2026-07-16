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
///
/// ## Scoring (recall stage of the relevance gate)
///
/// A bare "any stem fires → keep" decision is high-recall but order-blind: the
/// previous cap took the first `max` candidates in *directory order*, not the
/// most relevant ones, and a single coincidental stem ranked the same as a
/// strong multi-term overlap. This is the hard-negative trap.
///
/// Each candidate is now scored by the **weighted sum of its distinct stems
/// that fire in the intent** — name stems at full weight, the file's
/// frontmatter `description:` stems (the field reserved "to decide relevance
/// during recall") at half weight, each stem weighted by specificity (length,
/// a cheap IDF proxy via [`stem_weight`]). Candidates are then ranked
/// best-first (score desc, name asc for a deterministic, byte-stable tiebreak)
/// and the top `max` are returned. Recall is preserved (any hit is still a
/// candidate); only the *order* and the *cut* become relevance-driven. The
/// semantic precision pass (the Haiku judge) runs one layer up, over this
/// ranked shortlist — never in this LLM-free, deterministic core.
#[must_use]
pub(crate) fn match_spec_memory(
    memory_dir: &Path,
    intent: &str,
    max: usize,
    with_summary: bool,
) -> Vec<MemoryMatch> {
    let Ok(entries) = fs::read_dir(memory_dir) else {
        return Vec::new();
    };

    // Collect candidate stems once; build the automaton over the union of every
    // file's stems (name + description), tagged per-file via the term string.
    struct Candidate {
        name: String,
        name_stems: Vec<String>,
        desc_stems: Vec<String>,
        summary: String,
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
        let name_stems = name_stems(&name);
        if name_stems.is_empty() {
            continue;
        }
        // One read per candidate, reused for both the description signal and the
        // optional inline summary. Fail-open: an unreadable file degrades to
        // name-only scoring (empty description stems).
        let body = fs::read_to_string(&entry.path).ok();
        let desc_stems = body
            .as_deref()
            .map(description_stems)
            .unwrap_or_default();
        let summary = if with_summary {
            body.as_deref().map(extract_memory_summary).unwrap_or_default()
        } else {
            String::new()
        };
        all_terms.extend(name_stems.iter().cloned());
        all_terms.extend(desc_stems.iter().cloned());
        candidates.push(Candidate {
            name,
            name_stems,
            desc_stems,
            summary,
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

    // Score every candidate by its weighted distinct-stem overlap with the
    // intent. A name stem also present in the description counts once, at the
    // higher (name) weight.
    let mut scored: Vec<(f64, MemoryMatch)> = Vec::new();
    for cand in candidates {
        let mut score = 0.0_f64;
        for stem in &cand.name_stems {
            if hits.contains(stem) {
                score += stem_weight(stem);
            }
        }
        for stem in &cand.desc_stems {
            if cand.name_stems.contains(stem) {
                continue; // already counted at name weight
            }
            if hits.contains(stem) {
                score += stem_weight(stem) * DESC_STEM_WEIGHT;
            }
        }
        if score <= 0.0 {
            continue;
        }
        scored.push((
            score,
            MemoryMatch {
                name: cand.name,
                summary: cand.summary,
            },
        ));
    }

    // Best-first: highest score wins; ties break on name for a deterministic,
    // byte-stable ordering (directory iteration order is not stable).
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.name.cmp(&b.1.name))
    });
    // Relevance threshold — the anti-bloat bound (NO size/count cap). Keep only
    // candidates scoring within `MEMORY_RELEVANCE_FLOOR_FRACTION` of the best:
    // this drops the long tail of weak/coincidental matches that would otherwise
    // flood the prompt, while a lone genuine match (its own top) always clears
    // its own bar. Relevance bounds the set — precision, not a number, decides.
    if let Some(&(top, _)) = scored.first() {
        let floor = top * MEMORY_RELEVANCE_FLOOR_FRACTION;
        scored.retain(|(score, _)| *score >= floor);
    }
    scored.truncate(max);
    scored.into_iter().map(|(_, m)| m).collect()
}

/// Relevance floor as a fraction of the best score: a candidate survives only
/// when it scores at least this share of the top match. Conservative on purpose
/// — a lone match always clears its own bar (no false negative), but a strong
/// match collapses the weak tail behind it (anti-bloat without a size cap).
const MEMORY_RELEVANCE_FLOOR_FRACTION: f64 = 0.34;

/// Per-stem specificity weight — longer stems are rarer and more discriminating,
/// so they contribute more to the relevance score. A cheap, dependency-free IDF
/// proxy: enough to rank memory-name overlaps best-first without a corpus pass.
fn stem_weight(stem: &str) -> f64 {
    match stem.chars().count() {
        0..=4 => 1.0,
        5..=6 => 1.5,
        _ => 2.0,
    }
}

/// Description stems weigh half a name stem: the `description:` line is a
/// supporting relevance signal, the file name the primary one.
const DESC_STEM_WEIGHT: f64 = 0.5;

/// Mine content stems from a memory file's frontmatter `description:` line — the
/// field the memory format reserves "to decide relevance during recall".
/// Lowercased tokens ≥4 chars (descriptions carry more common words than the
/// terse file name, so the floor is one char higher than [`name_stems`]).
/// Fail-open: no frontmatter / no description line yields an empty vec.
#[must_use]
fn description_stems(text: &str) -> Vec<String> {
    let mut in_fm = false;
    for line in text.lines() {
        let t = line.trim();
        if t == "---" {
            if in_fm {
                break; // closing fence — description must live inside the block
            }
            in_fm = true;
            continue;
        }
        if in_fm {
            if let Some(rest) = t.strip_prefix("description:") {
                let mut out: Vec<String> = Vec::new();
                for tok in rest
                    .to_ascii_lowercase()
                    .split(|c: char| !c.is_ascii_alphanumeric())
                {
                    if tok.len() >= 4 && !out.iter().any(|s| s == tok) {
                        out.push(tok.to_string());
                    }
                }
                return out;
            }
        }
    }
    Vec::new()
}

/// Read the relevance gate's approved-name list from `<spec_dir>/.memory-approved`.
/// One name per line; `#`-comment and blank lines are skipped. A missing file
/// yields an empty vec — the ungated fallback, where the renderer runs the
/// deterministic recall matcher instead.
///
/// This file is the per-spec handoff the orchestration-layer precision judge
/// (deterministic shortlist → Haiku yes/no) writes and the renderer reads. A
/// file, not an env var, because `wave-advance` renders a whole dispatch round
/// in one process: every item resolves the same deterministic path, so the
/// approved set survives the batch render that a per-process env var could not.
/// Tri-state, because "no file" and "empty file" mean opposite things:
/// - `None` — no (or unreadable) file: the gate did not run, so the renderer
///   falls back to the deterministic recall matcher.
/// - `Some(vec)` — the gate ran and approved exactly these (possibly **zero**)
///   names. `Some(empty)` is "none relevant" → inject NO memory, NOT a fallback.
#[must_use]
pub(crate) fn read_approved_memory_names(spec_dir: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(spec_dir.join(".memory-approved")).ok()?;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let name = line.trim();
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        out.push(name.to_string());
    }
    Some(out)
}

/// Select spec-memory files by an explicit, already-ranked name list — the
/// **approved set** the relevance gate returns. This bypasses the recall matcher
/// entirely: the gate (deterministic shortlist + Haiku precision judge, one
/// layer up) has already decided membership, so here we only resolve each
/// approved name to its file and render it. Membership is by *relevance*, never
/// by a count cap — the list is exactly the relevant set, however many that is.
///
/// Order follows `names` (the gate ranked them). A trailing `.md`, surrounding
/// whitespace, and an `_`-prefix are tolerated/skipped; an unknown or unreadable
/// name is silently dropped (fail-open — a stale approved name never blocks).
#[must_use]
pub(crate) fn select_spec_memory_by_names(
    memory_dir: &Path,
    names: &[String],
    with_summary: bool,
) -> Vec<MemoryMatch> {
    let mut out: Vec<MemoryMatch> = Vec::new();
    for raw in names {
        let name = raw.trim().trim_end_matches(".md").trim();
        if name.is_empty() || name.starts_with('_') {
            continue;
        }
        // Read also validates existence — a missing/unreadable approved name is
        // dropped rather than rendered as a dangling wikilink.
        let Ok(body) = fs::read_to_string(memory_dir.join(format!("{name}.md"))) else {
            continue;
        };
        let summary = if with_summary {
            extract_memory_summary(&body)
        } else {
            String::new()
        };
        out.push(MemoryMatch {
            name: name.to_string(),
            summary,
        });
    }
    out
}

/// Resolve the spec-memory block for a dispatch THROUGH the relevance gate — the
/// single home both injection paths (the renderer and the `subagent_inject` hook)
/// call, so the tri-state branching lives in exactly ONE place instead of being
/// copy-pasted at each call site:
///
/// - Gate ran (`<spec_dir>/.memory-approved` present) → inject EXACTLY the
///   approved set; `Some(empty)` = "none relevant" → no memory.
/// - Ungated → the deterministic recall matcher, relevance-ranked and uncapped.
///
/// `spec_dir` is the spec root (`memory/` is resolved under it); a spec-less
/// caller passes the project root, where neither `.memory-approved` nor a
/// `memory/` dir exists, so the block fail-opens to empty. Relevance is the only
/// filter — never a count, never a size. `with_summary` mirrors the matcher: the
/// renderer wants inline summaries, the hook lists names only.
#[must_use]
pub fn resolve_spec_memory(spec_dir: &Path, intent: &str, with_summary: bool) -> Vec<MemoryMatch> {
    let memory_dir = spec_dir.join("memory");
    match read_approved_memory_names(spec_dir) {
        Some(approved) => select_spec_memory_by_names(&memory_dir, &approved, with_summary),
        None => match_spec_memory(&memory_dir, intent, usize::MAX, with_summary),
    }
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
pub(crate) fn name_stems(name: &str) -> Vec<String> {
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
pub(crate) fn extract_memory_summary(text: &str) -> String {
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
pub(crate) fn vocab_layer_terms(project: &Path) -> (Vec<String>, Vec<String>) {
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
    fn ranks_best_first_and_caps_by_relevance_not_dir_order() {
        let dir = tempdir().unwrap();
        let mem = dir.path();
        // Strong: name overlaps three intent terms.
        std::fs::write(mem.join("tabs-routing-actions.md"), "x\n").unwrap();
        // Medium: name overlaps one specific term.
        std::fs::write(mem.join("routing-guard.md"), "x\n").unwrap();
        // Weak: name overlaps one short term.
        std::fs::write(mem.join("tabs-only.md"), "x\n").unwrap();
        // None: no overlap — must never surface.
        std::fs::write(mem.join("unrelated-thing.md"), "x\n").unwrap();

        let hits = match_spec_memory(mem, "fix the routing actions in tabs", 2, false);
        let names: Vec<&str> = hits.iter().map(|m| m.name.as_str()).collect();
        // Cap honoured, and the two kept are the most relevant — in score order,
        // not directory order.
        assert_eq!(names, vec!["tabs-routing-actions", "routing-guard"], "got {names:?}");
        assert!(!names.contains(&"unrelated-thing"));
        assert!(!names.contains(&"tabs-only"), "weakest match dropped by the cap");
    }

    #[test]
    fn description_stems_lift_a_name_that_would_not_match_alone() {
        let dir = tempdir().unwrap();
        let mem = dir.path();
        // Name carries no overlap with the intent, but the frontmatter
        // description does — the recall must surface it via the description
        // signal the memory format reserves for relevance.
        std::fs::write(
            mem.join("opaque-slug.md"),
            "---\nname: opaque-slug\ndescription: handles GraphQL pagination cursors\n---\nbody\n",
        )
        .unwrap();
        let hits = match_spec_memory(mem, "the pagination cursor is wrong", 3, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "opaque-slug");
    }

    #[test]
    fn read_approved_names_distinguishes_absent_present_and_empty() {
        let dir = tempdir().unwrap();
        let sd = dir.path();
        // Absent → None (ungated; the renderer runs the recall matcher).
        assert_eq!(read_approved_memory_names(sd), None);
        // Present → Some(parsed); comment + blank lines skipped.
        std::fs::write(sd.join(".memory-approved"), "# gate output\nalpha\n\n  beta  \n").unwrap();
        assert_eq!(
            read_approved_memory_names(sd),
            Some(vec!["alpha".to_string(), "beta".to_string()])
        );
        // Present but no names → Some(empty) = "gate approved nothing" — inject
        // NO memory; this must NOT collapse back to the absent/fallback case.
        std::fs::write(sd.join(".memory-approved"), "# all excluded\n").unwrap();
        assert_eq!(read_approved_memory_names(sd), Some(Vec::new()));
    }

    #[test]
    fn resolve_spec_memory_gates_then_falls_back() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        let mem = spec_dir.join("memory");
        std::fs::create_dir(&mem).unwrap();
        std::fs::write(mem.join("routing.md"), "x\n").unwrap();
        std::fs::write(mem.join("paidat.md"), "x\n").unwrap();

        let names = |v: &[MemoryMatch]| v.iter().map(|m| m.name.clone()).collect::<Vec<_>>();

        // Ungated (no `.memory-approved`) → deterministic recall matcher.
        assert_eq!(
            names(&resolve_spec_memory(spec_dir, "fix the routing layer", false)),
            vec!["routing".to_string()]
        );
        // Gated → the approved set wins over recall, whatever the intent says.
        std::fs::write(spec_dir.join(".memory-approved"), "paidat\n").unwrap();
        assert_eq!(
            names(&resolve_spec_memory(spec_dir, "fix the routing layer", false)),
            vec!["paidat".to_string()]
        );
        // Gated but empty = "none relevant" → no memory (NOT a recall fallback).
        std::fs::write(spec_dir.join(".memory-approved"), "# none\n").unwrap();
        assert!(resolve_spec_memory(spec_dir, "fix the routing layer", false).is_empty());
    }

    #[test]
    fn select_by_names_keeps_order_trims_md_and_drops_unknown() {
        let dir = tempdir().unwrap();
        let mem = dir.path();
        std::fs::write(mem.join("alpha.md"), "---\ndescription: a\n---\nAlpha body line\n").unwrap();
        std::fs::write(mem.join("beta.md"), "Beta body line\n").unwrap();
        let names = vec!["beta".to_string(), "ghost".to_string(), "alpha.md".to_string()];
        let out = select_spec_memory_by_names(mem, &names, true);
        let got: Vec<&str> = out.iter().map(|m| m.name.as_str()).collect();
        // Approved order preserved; `.md` trimmed; unknown "ghost" dropped.
        assert_eq!(got, vec!["beta", "alpha"]);
        assert!(out[0].summary.contains("Beta"));
        // Name-only mode leaves summaries empty.
        let bare = select_spec_memory_by_names(mem, &["alpha".to_string()], false);
        assert_eq!(bare.len(), 1);
        assert!(bare[0].summary.is_empty());
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
