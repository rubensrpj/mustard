//! `glossary-coverage` — deterministic check of how well a `CONTEXT.md` domain
//! glossary covers the repo-vocabulary terms a feature request touches.
//!
//! This is the Rust half of the `/feature` ANALYZE "grill nudge" (Selo 1): the
//! orchestrator runs it once, cheaply, before DECOMPOSE, and surfaces a single
//! dismissible suggestion to author/extend `CONTEXT.md` (via the `grill-with-docs`
//! skill) only when the glossary is `missing` or `weak`. It NEVER grills inline
//! and NEVER blocks.
//!
//! N (the denominator) is the distinct **word stems** among the digest's
//! **matched terms** — the repo-vocabulary terms the intent actually maps to —
//! NOT the raw intent tokens (`domain_terms` keeps stopwords like "the"), and
//! NOT one entry per inflection: the digest matches `spec` and `specs`
//! independently, but they are one word and one glossary entry, so they are
//! collapsed before scoring (see `group_inflections`). K is how many of those
//! stems have a covering block in `CONTEXT.md`, scored with the EXACT term
//! matcher `context-slice` uses (`parse_term_blocks` + `block_matches`) over
//! every matched spelling of the stem, so the producer and consumer of the
//! glossary cannot drift.
//!
//! Output (stdout, byte-stable pretty JSON): `{ coveragePct, contextFile, present,
//! termsCovered, termsTotal, uncovered, verdict }`. The `uncovered` list IS the
//! actionable payload — the weak/missing domain terms the orchestrator hands to
//! the inline grill (`grill-capture`) so each confirmed definition lands in the
//! glossary; `contextFile` names the resolved target to write them into (the
//! first existing CONTEXT.md, or the first requested path when none resolved yet,
//! so a still-empty glossary still has a destination). Fail-open: a missing model
//! / unreadable glossary degrades to `verdict: "na"` (the SKILL then stays
//! silent); exit 0.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::Scan;
use serde_json::json;

use crate::commands::economy::context_slice::{
    block_matches, parse_term_blocks, resolve_context_files, TermBlock,
};
use crate::commands::feature::domain_terms;

/// Below this covered-term percentage the glossary is `weak`.
const WEAK_COVERAGE_PCT: u64 = 50;
/// At or above this many uncovered matched terms the glossary is `weak`
/// regardless of percentage (an absolute floor catches wide features whose ratio
/// still looks healthy).
const WEAK_UNCOVERED_FLOOR: usize = 3;

/// Pure coverage scoring over already-resolved inputs — the unit-testable core.
struct Coverage {
    present: bool,
    total: usize,
    covered: usize,
    uncovered: Vec<String>,
    verdict: &'static str,
    /// The glossary file the orchestrator's inline grill should write confirmed
    /// terms into — the resolved CONTEXT.md, or the first requested path when
    /// none exists yet (so a `missing` verdict still has a destination). Empty
    /// when no `--context` was given.
    context_file: String,
}

impl Coverage {
    fn pct(&self) -> u64 {
        if self.total == 0 {
            100
        } else {
            (self.covered as u64 * 100) / self.total as u64
        }
    }
}

/// One word stem and every spelling of it the digest returned.
///
/// The digest matches inflections independently, so `spec` and `specs` — one
/// word, one thing to define — arrived as two terms. Both were scored, both
/// counted toward `termsTotal`, and both appeared in `uncovered`: a glossary
/// with nine open terms reported eighteen, and its percentage was computed over
/// a denominator inflated by duplicates. Grouping collapses them back to one.
struct TermGroup {
    /// The first spelling seen — what the user is shown and asked to define.
    representative: String,
    /// Every spelling in this group, including the representative. Coverage
    /// tries them ALL, so when the digest matched both `spec` and `specs` a
    /// glossary defining either one closes the word. Deliberately NOT widened
    /// to `stems`: those include crude truncations (`process` → `proc`), and a
    /// false "covered" hides an undefined term — the worse error for a nudge.
    variants: Vec<String>,
    /// The inflection keys shared by the variants; the grouping key. See
    /// [`inflection_keys`].
    stems: BTreeSet<String>,
}

/// English inflection suffixes stripped when deriving [`inflection_keys`].
/// Ordered longest-first only for readability — every one is tried.
const INFLECTION_SUFFIXES: &[&str] = &["ing", "tion", "sion", "ies", "es", "ed", "s"];

/// Shortest base an inflection strip may leave. Below this the "stem" is noise
/// (`bus` → `bu`) and would collide unrelated words.
const MIN_STEM_LEN: usize = 3;

/// Every form `term` could share with another spelling of the same word: the
/// lowercased surface form plus each inflection-stripped base.
///
/// Deliberately NOT `agent::context_inject::name_stems`, which does the same
/// folding for memory RECALL and `break`s at the first suffix that strips —
/// enough when any single stem firing is a hit, but wrong for an equivalence
/// class: it folds `waves` to `wav` (via `es`) and never to `wave` (via `s`), so
/// `wave` and `waves` would never meet. Membership in one class needs every
/// fold, so all suffixes are tried here.
fn inflection_keys(term: &str) -> BTreeSet<String> {
    let lower = term.to_lowercase();
    let mut keys: BTreeSet<String> = std::iter::once(lower.clone()).collect();
    for suffix in INFLECTION_SUFFIXES {
        if let Some(base) = lower.strip_suffix(suffix) {
            if base.len() >= MIN_STEM_LEN {
                keys.insert(base.to_string());
                // `ies` → `y` reconstruction (`policies` → `policy`).
                if *suffix == "ies" {
                    keys.insert(format!("{base}y"));
                }
            }
        }
    }
    keys
}

/// Collapse inflections of one word stem into a single [`TermGroup`], keeping
/// input order (the output stays deterministic, and the first spelling wins).
///
/// Two terms are the same word when their [`inflection_keys`] intersect. A term
/// too short to strip keeps only its own surface form as a key, so it stays
/// distinct instead of collapsing into everything else.
fn group_inflections(matched: &[String]) -> Vec<TermGroup> {
    let mut groups: Vec<TermGroup> = Vec::new();
    for term in matched {
        let key = inflection_keys(term);
        match groups
            .iter_mut()
            .find(|g| !g.stems.is_disjoint(&key))
        {
            Some(group) => {
                if !group.variants.contains(term) {
                    group.variants.push(term.clone());
                }
                // A longer spelling contributes its stems too, so a later
                // inflection can still join through either form.
                group.stems.extend(key);
            }
            None => groups.push(TermGroup {
                representative: term.clone(),
                variants: vec![term.clone()],
                stems: key,
            }),
        }
    }
    groups
}

/// Score `matched` repo-vocabulary terms against the parsed glossary `blocks`.
/// `present` is whether any glossary file resolved at all (distinguishes the
/// "no `CONTEXT.md` authored" case from "authored but thin").
///
/// Terms are scored per WORD STEM, not per inflection — see [`group_inflections`].
fn score(matched: &[String], blocks: &[TermBlock], present: bool) -> Coverage {
    let groups = group_inflections(matched);
    let mut uncovered: Vec<String> = Vec::new();
    let mut covered = 0usize;
    for group in &groups {
        // Any spelling the digest matched counts for the whole word: defining
        // `Spec` once closes it, where scoring each inflection separately
        // reported the same word simultaneously covered and open.
        let matched_any = group.variants.iter().any(|variant| {
            let needle: BTreeSet<String> = std::iter::once(variant.to_lowercase()).collect();
            blocks.iter().any(|b| block_matches(b, &needle))
        });
        if present && matched_any {
            covered += 1;
        } else {
            uncovered.push(group.representative.clone());
        }
    }
    let total = groups.len();
    let mut c = Coverage {
        present,
        total,
        covered,
        uncovered,
        verdict: "ok",
        context_file: String::new(),
    };
    c.verdict = if total == 0 {
        // No domain terms touched → nothing a glossary could cover; never nudge.
        "ok"
    } else if !present {
        "missing"
    } else if c.pct() < WEAK_COVERAGE_PCT || c.uncovered.len() >= WEAK_UNCOVERED_FLOOR {
        "weak"
    } else {
        "ok"
    };
    c
}

/// Resolve the digest's matched terms + glossary blocks, then score. Fail-open:
/// returns `None` only when the scan digest is unavailable (the caller maps that
/// to `verdict: "na"`).
fn compute(intent: &str, context: &[String], root: &Path) -> Option<Coverage> {
    let terms = domain_terms(intent);
    let model = root.join(".claude").join("grain.model.json");

    // N = the repo-vocabulary terms the intent maps to (matched against the
    // grain model) — NOT the raw intent tokens, which keep stopwords.
    let matched: Vec<String> = match Scan::locate().digest_query(&model, &terms) {
        Ok(q) => q.matched_terms.iter().map(|t| t.term.clone()).collect(),
        Err(_) => return None,
    };

    // Parse the glossary through the SAME resolver `context-slice` uses
    // (CONTEXT-MAP.md expansion + silent skip of missing files).
    let resolved = resolve_context_files(context);
    let blocks: Vec<TermBlock> = resolved
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .flat_map(|text| parse_term_blocks(&text))
        .collect();
    let present = !blocks.is_empty();

    let mut coverage = score(&matched, &blocks, present);
    coverage.context_file = target_context_file(&resolved, context);
    Some(coverage)
}

/// The glossary path the orchestrator's inline grill writes confirmed terms
/// into. Prefer the first file that resolved on disk (an authored CONTEXT.md);
/// when none resolved (the `missing` case, or only a CONTEXT-MAP pointing at
/// absent files), fall back to the first non-empty requested `--context` path so
/// a still-empty glossary still names a concrete destination. Empty when no
/// `--context` was given at all.
fn target_context_file(resolved: &[std::path::PathBuf], requested: &[String]) -> String {
    if let Some(p) = resolved.first() {
        return p.display().to_string();
    }
    requested
        .iter()
        .find(|p| !p.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// Render the coverage verdict as byte-stable JSON (deterministic key order).
fn to_json(c: &Coverage) -> serde_json::Value {
    json!({
        "verdict": c.verdict,
        "present": c.present,
        "termsTotal": c.total,
        "termsCovered": c.covered,
        "coveragePct": c.pct(),
        "uncovered": c.uncovered,
        "contextFile": c.context_file,
    })
}

/// Score `matched` repo-vocabulary terms against a glossary supplied as raw
/// markdown — the same parse → score → render path [`run`] takes, minus the
/// `--context` file resolution. `present` is derived exactly as production
/// derives it (any parsed block at all).
///
/// Exposed so the acceptance tests exercise the PUBLISHED JSON contract
/// (`termsTotal` / `termsCovered` / `coveragePct` / `uncovered` / `verdict`)
/// instead of a private shape that could drift away from what the command
/// actually prints.
///
/// `dead_code` is allowed for the same reason `lib.rs` allows it crate-wide:
/// this is the lib face that `tests/` imports, and the BIN face (`main.rs`,
/// which declares the same module tree without that blanket allow) never calls
/// it. [`compute`] is not routed through here on purpose — it parses each
/// resolved `CONTEXT.md` SEPARATELY, and concatenating them would let a file
/// that opens with prose be absorbed into the previous file's last block body,
/// which `block_matches` searches.
#[allow(dead_code)]
#[must_use]
pub fn score_terms(matched: &[String], glossary: &str) -> serde_json::Value {
    let blocks = parse_term_blocks(glossary);
    let present = !blocks.is_empty();
    to_json(&score(matched, &blocks, present))
}

/// Dispatch `mustard-rt run glossary-coverage`. Always exits 0.
pub fn run(intent: &str, context: &[String], root: &Path) {
    let payload = match compute(intent, context, root) {
        Some(c) => to_json(&c),
        // Digest unavailable → not-applicable; the SKILL nudge stays silent and
        // /feature continues unaffected.
        None => json!({
            "verdict": "na",
            "present": false,
            "termsTotal": 0,
            "termsCovered": 0,
            "coveragePct": 0,
            "uncovered": [],
            "contextFile": "",
        }),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_glossary_yields_missing_verdict() {
        let matched = vec!["payable".to_string(), "tenant".to_string()];
        let c = score(&matched, &[], false);
        assert_eq!(c.verdict, "missing");
        assert_eq!(c.covered, 0);
        assert_eq!(c.uncovered.len(), 2);
        assert_eq!(c.pct(), 0);
    }

    #[test]
    fn full_coverage_is_ok() {
        let blocks = parse_term_blocks("## Payable\nA bill owed.\n## Tenant\nAn org.");
        let matched = vec!["payable".to_string(), "tenant".to_string()];
        let c = score(&matched, &blocks, true);
        assert_eq!(c.verdict, "ok");
        assert_eq!(c.covered, 2);
        assert!(c.uncovered.is_empty());
        assert_eq!(c.pct(), 100);
    }

    #[test]
    fn thin_glossary_below_floor_is_weak() {
        // 1 of 4 covered → 25% < 50% AND 3 uncovered >= floor.
        let blocks = parse_term_blocks("## Payable\nA bill owed.");
        let matched = vec![
            "payable".to_string(),
            "tenant".to_string(),
            "ledger".to_string(),
            "invoice".to_string(),
        ];
        let c = score(&matched, &blocks, true);
        assert_eq!(c.verdict, "weak");
        assert_eq!(c.covered, 1);
        assert_eq!(c.uncovered, vec!["tenant", "ledger", "invoice"]);
    }

    #[test]
    fn uncovered_floor_trips_weak_even_above_percentage() {
        // 5 of 8 covered = 62% (>= 50%), but 3 uncovered hits the absolute floor.
        let blocks = parse_term_blocks(
            "## A\nx\n## B\nx\n## C\nx\n## D\nx\n## E\nx",
        );
        let matched = vec![
            "a".to_string(), "b".to_string(), "c".to_string(), "d".to_string(),
            "e".to_string(), "f".to_string(), "g".to_string(), "h".to_string(),
        ];
        let c = score(&matched, &blocks, true);
        assert_eq!(c.pct(), 62);
        assert_eq!(c.verdict, "weak");
    }

    #[test]
    fn inflections_of_one_word_group_together() {
        let groups = group_inflections(&[
            "spec".to_string(),
            "specs".to_string(),
            "tenant".to_string(),
        ]);
        assert_eq!(groups.len(), 2, "spec/specs are one word, tenant is another");
        assert_eq!(groups[0].representative, "spec", "first spelling wins");
        assert_eq!(groups[0].variants, vec!["spec", "specs"]);
        // The later inflection can arrive first and still absorb the shorter one.
        let reversed = group_inflections(&["specs".to_string(), "spec".to_string()]);
        assert_eq!(reversed.len(), 1);
        assert_eq!(reversed[0].representative, "specs");
    }

    #[test]
    fn terms_too_short_to_stem_stay_distinct() {
        // Nothing can be stripped below `MIN_STEM_LEN`, so each keeps only its
        // own surface form and they stay apart instead of sharing one bucket.
        let groups = group_inflections(&["a".to_string(), "b".to_string(), "ab".to_string()]);
        assert_eq!(groups.len(), 3);
    }

    #[test]
    fn every_suffix_is_tried_not_only_the_first_that_strips() {
        // The recall matcher folds `waves` through `es` → `wav` and stops, so it
        // never reaches `wave`. An equivalence class needs both, or the singular
        // and the plural never meet — the exact case this whole fix exists for.
        let keys = inflection_keys("waves");
        assert!(keys.contains("wave"), "the `s` fold must still be reached: {keys:?}");
        assert!(keys.contains("waves"), "the surface form is always a key");
        assert_eq!(group_inflections(&["wave".to_string(), "waves".to_string()]).len(), 1);
        // `ies` → `y` reconstruction still resolves to the dictionary form.
        assert!(inflection_keys("policies").contains("policy"));
        assert_eq!(
            group_inflections(&["policy".to_string(), "policies".to_string()]).len(),
            1
        );
    }

    #[test]
    fn one_defined_spelling_covers_the_whole_group() {
        // The digest matched both spellings; the glossary defines one. Scoring
        // each inflection separately reported the word half-covered — one
        // covered term AND one open term for a single entry that exists.
        let blocks = parse_term_blocks("## Spec\nA unit of work.");
        let c = score(&["spec".to_string(), "specs".to_string()], &blocks, true);
        assert_eq!(c.total, 1, "one word, one slot");
        assert_eq!(c.covered, 1, "any spelling in the group satisfies it");
        assert!(c.uncovered.is_empty(), "nothing is left open: {:?}", c.uncovered);
        assert_eq!(c.pct(), 100);
    }

    #[test]
    fn coverage_never_guesses_past_the_spellings_it_saw() {
        // Deliberately NOT widened to the morphological stems: they include
        // crude truncations (`process` → `proc`), and a false "covered" hides
        // an undefined term, which is the worse of the two errors for a nudge.
        let blocks = parse_term_blocks("## Spec\nA unit of work.");
        let c = score(&["specs".to_string()], &blocks, true);
        assert_eq!(c.covered, 0);
        assert_eq!(c.uncovered, vec!["specs"]);
    }

    #[test]
    fn no_domain_terms_is_ok_not_missing() {
        let c = score(&[], &[], false);
        assert_eq!(c.verdict, "ok");
        assert_eq!(c.pct(), 100);
    }

    #[test]
    fn target_context_file_prefers_resolved_then_falls_back_to_requested() {
        use std::path::PathBuf;
        // A resolved (on-disk) file wins.
        let resolved = vec![PathBuf::from("/repo/CONTEXT.md")];
        let requested = vec!["./CONTEXT.md".to_string()];
        assert_eq!(
            target_context_file(&resolved, &requested),
            "/repo/CONTEXT.md"
        );
        // Nothing resolved (the `missing` case) → first non-empty requested path.
        let requested = vec![String::new(), "docs/CONTEXT.md".to_string()];
        assert_eq!(
            target_context_file(&[], &requested),
            "docs/CONTEXT.md"
        );
        // No --context at all → empty (no destination to offer).
        assert!(target_context_file(&[], &[]).is_empty());
    }
}
