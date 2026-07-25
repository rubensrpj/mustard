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
//! statedReason, termsCovered, termsTotal, uncovered, verdict }`. The `uncovered`
//! list IS the actionable payload — the weak/missing domain terms the orchestrator
//! hands to the inline grill (`grill-capture`) so each confirmed definition lands
//! in the glossary; `contextFile` names the resolved target to write them into (the
//! first existing CONTEXT.md, or the first requested path when none resolved yet,
//! so a still-empty glossary still has a destination). Fail-open: a missing model
//! / unreadable glossary degrades to `verdict: "na"` (the SKILL then stays
//! silent); exit 0.
//!
//! The check was designed for a business domain, where a matched term like
//! `payable` has a definition worth capturing. In a harness the domain vocabulary
//! IS technical vocabulary, so the matcher answers with the words the repository
//! says everywhere and the grill would ask low-value questions. `verdict:
//! "declined"` is that outcome said out loud: the terms matched, but the corpus
//! itself reports them as repository-wide vocabulary, so no definition is worth
//! grilling. It is NOT an error and NOT a skip — it carries `statedReason`, the
//! sentence `grill-capture --finalize --reason "<sentence>"` records into the
//! clarification marker, which is exactly the substance the approval gate accepts.
//! Declining therefore becomes a recorded outcome instead of a silent skip.

use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::scan::DigestTerm;
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
    /// Why no grill applies, as a sentence — non-empty ONLY on the `declined`
    /// verdict. It is written to be passed VERBATIM to `grill-capture --finalize
    /// --reason`, which is what turns the decline into a recorded clarification
    /// instead of a skip nobody wrote down.
    stated_reason: String,
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
        stated_reason: String::new(),
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

/// The verdict for "the terms matched, but they are not domain vocabulary".
/// Beside the others, not above them: declining is a legitimate outcome, and
/// naming it is what makes it visible.
const DECLINED: &str = "declined";

/// A published term's corpus RARITY ×1024, recovered from the two numbers the
/// scan model publishes about it: `specificity_x1024` is TF·IDF (`count` ×
/// `idf_x1024`), so dividing it back by `count` leaves the inverse document
/// frequency alone — how CONCENTRATED the term is across the repository,
/// independent of how often it is repeated. A word in nearly every module tends
/// to 0; a word living in a few places scores high. That is the exact statement
/// this verdict needs, and it is the corpus's own arithmetic — no list of words
/// is written down anywhere, so nothing here can rot or encode one person's
/// taste.
fn rarity_x1024(t: &DigestTerm) -> u64 {
    t.specificity_x1024 / (t.count.max(1) as u64)
}

/// The rarity a term must reach to count as domain vocabulary HERE: the median
/// rarity of the repository's own published term index.
///
/// Read off the corpus rather than fixed, because the scale of `idf_x1024`
/// depends on the corpus size — a constant would mean something different in
/// every repository. The median is the repo's middle word: below it a term is
/// more widespread than half the vocabulary the miner publishes.
///
/// `None` for an empty index (nothing to compare against → no decline). A model
/// from an older scan binary carries no `specificity_x1024` at all, so every
/// rarity is 0 and the cut is 0 — and since the comparison is strict, nothing is
/// ever below it. The signal's absence can only silence the decline, never
/// invent one.
fn corpus_rarity_cut(index: &[DigestTerm]) -> Option<u64> {
    if index.is_empty() {
        return None;
    }
    let mut rarities: Vec<u64> = index.iter().map(rarity_x1024).collect();
    rarities.sort_unstable();
    rarities.get(rarities.len() / 2).copied()
}

/// The corpus rarity the published index reports for `group` — the LOWEST
/// across every spelling of the word the index carries (`spec` and `specs` are
/// often both published).
///
/// Lowest, because the group is ONE word and the check already treats it as one
/// (defining either spelling closes it). The word's documents are the UNION of
/// its spellings' documents, so its document frequency is at least the largest
/// of them and its rarity is therefore at most the smallest of them: `spec`
/// living in most modules makes the word repository-wide however narrowly the
/// plural is used. Taking the highest instead would let the rarer spelling
/// speak for a word that is everywhere.
///
/// `None` when the index says nothing about the word.
fn group_rarity_x1024(group: &TermGroup, index: &[DigestTerm]) -> Option<u64> {
    index
        .iter()
        .filter(|t| !inflection_keys(&t.term).is_disjoint(&group.stems))
        .map(rarity_x1024)
        .min()
}

/// The sentence to record when the corpus says none of the terms still open is
/// domain vocabulary — `None` when the grill has something worth asking.
///
/// Three conditions, all read off the corpus:
/// - **every judged word is ubiquitous.** One word above the median rarity is a
///   word concentrated somewhere, which is a question worth asking, so the first
///   one found ends the decline;
/// - **the corpus judged at least half the open set** (the quorum below). A word
///   the index never published is an ABSTENTION: it might be a genuinely
///   concentrated term the rank cap trimmed, and calling it generic would be a
///   guess. Abstentions neither support the decline nor veto it — but a decline
///   resting on a minority of the open set would be the corpus deciding about
///   words it never saw;
/// - **at least one word was judged**, so a wholly unpublished set cannot
///   decline by the quorum alone.
///
/// Why abstentions must not veto: scan's stem tier answers with fragments no
/// human ever typed (`waveli` for `waveList`), which the term index never
/// publishes. Letting one of those block the verdict is what made the decline
/// unreachable on this very repository — a decline that never fires is as
/// useless as one that fires always.
///
/// Pure and deterministic: the named terms keep the digest's order, and the same
/// inputs always produce the same sentence.
fn decline_reason(matched: &[String], uncovered: &[String], index: &[DigestTerm]) -> Option<String> {
    let cut = corpus_rarity_cut(index)?;
    let mut named: Vec<String> = Vec::new();
    let mut open = 0usize;
    for group in group_inflections(matched) {
        if !uncovered.contains(&group.representative) {
            continue;
        }
        open += 1;
        let Some(rarity) = group_rarity_x1024(&group, index) else {
            continue; // abstention: the corpus never published this word
        };
        if rarity >= cut {
            return None;
        }
        named.push(group.representative);
    }
    // The quorum: judged words must be at least half of what the grill would ask
    // about (`2 * judged >= open`, integer and exact — no percentage to round).
    if named.is_empty() || named.len() * 2 < open {
        return None;
    }
    Some(format!(
        "the glossary grill declines: every term the corpus can judge here ({}) is \
         repository-wide vocabulary — the scan model ranks each below its median \
         term rarity, so there is no domain definition worth capturing",
        named.join(", ")
    ))
}

/// Turn a `missing`/`weak` verdict into `declined` when the corpus reports that
/// none of the still-open terms carries domain meaning.
///
/// Only those two verdicts are eligible: `ok` runs no grill to decline, and `na`
/// never reaches here (the digest was unavailable, so there is no corpus to ask).
/// Fail-open twice over — an empty index or a single unjudgeable word leaves the
/// coverage exactly as scored.
fn apply_decline(c: &mut Coverage, matched: &[String], index: &[DigestTerm]) {
    if !matches!(c.verdict, "missing" | "weak") {
        return;
    }
    if let Some(reason) = decline_reason(matched, &c.uncovered, index) {
        c.verdict = DECLINED;
        c.stated_reason = reason;
    }
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

    // The corpus is consulted only when a grill would actually run: on `ok` the
    // decline could not change the answer, so the lean path keeps its single
    // spawn. The published index is the same model, re-projected — an error
    // leaves an empty index, which by construction can only fail open.
    if matches!(coverage.verdict, "missing" | "weak") {
        let index = Scan::locate()
            .digest(&model)
            .map(|d| d.terms)
            .unwrap_or_default();
        apply_decline(&mut coverage, &matched, &index);
    }
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
        // Always present (stable shape), non-empty only on `declined` — the
        // sentence the caller hands to `grill-capture --finalize --reason`.
        "statedReason": c.stated_reason,
    })
}

/// Score `matched` repo-vocabulary terms against a glossary supplied as raw
/// markdown — the same parse → score → render path [`run`] takes, minus the
/// `--context` file resolution and minus the corpus consultation (there is no
/// model behind a raw markdown string, so the answer is the ordinary
/// coverage verdict, never `declined`). `present` is derived exactly as
/// production derives it (any parsed block at all).
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
            // No corpus to ask, so nothing was declined — the shape stays stable
            // and the caller has no reason to record.
            "statedReason": "",
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

    /// A published term index, given as `(term, count, rarity_x1024)` — the
    /// rarity is what the corpus judges on, so the fixture states it directly
    /// and multiplies back into the `specificity_x1024` (TF·IDF) the model
    /// really publishes.
    fn corpus(rows: &[(&str, usize, u64)]) -> Vec<DigestTerm> {
        rows.iter()
            .map(|(term, count, rarity)| DigestTerm {
                term: (*term).to_string(),
                count: *count,
                specificity_x1024: (*count as u64) * rarity,
                samples: Vec::new(),
                purpose: None,
            })
            .collect()
    }

    /// A repository whose published vocabulary spans both ends: `run`/`path` are
    /// said everywhere, `ledger`/`payable` live in a corner. Median rarity =
    /// 3000, the middle of five rows.
    fn sample_corpus() -> Vec<DigestTerm> {
        corpus(&[
            ("run", 200, 1000),
            ("path", 150, 2000),
            ("gate", 100, 3000),
            ("ledger", 40, 4000),
            ("payable", 30, 5000),
        ])
    }

    /// Both directions, because a decline that fires always is as useless as one
    /// that never fires.
    #[test]
    fn grill_declines_when_terms_are_not_domain_vocabulary() {
        let index = sample_corpus();

        // (a) Ubiquitous matched terms → the grill declines, WITH a sentence.
        // Both words live below the corpus median rarity: they are what the
        // repository says everywhere, so a definition would restate the code.
        let matched = vec!["run".to_string(), "path".to_string()];
        let mut c = score(&matched, &[], false);
        assert_eq!(c.verdict, "missing", "without the corpus this asks for a glossary");
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, DECLINED);
        assert!(
            c.stated_reason.contains("run") && c.stated_reason.contains("path"),
            "the reason must NAME the terms it declines: {}",
            c.stated_reason
        );

        // (b) Genuinely concentrated domain terms → the ordinary verdict stands,
        // and nothing is stated (there is nothing to decline).
        let matched = vec!["payable".to_string(), "ledger".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, "missing", "concentrated terms are worth grilling");
        assert!(c.stated_reason.is_empty());

        // (c) ONE concentrated term among ubiquitous ones is enough to keep the
        // question — the decline is about the whole open set.
        let matched = vec!["run".to_string(), "payable".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, "missing");

        // (d) A word the corpus never published ABSTAINS. It is not swept into
        // the decline (it is never named), and it does not veto one either —
        // scan's stem tier answers with fragments like `waveli` that no index
        // publishes, and letting one of those decide made the verdict
        // unreachable on the repository this check was written against.
        // (`gate` sits exactly AT the median and is deliberately absent: the cut
        // is strict, so a median-rarity word is not ubiquitous.)
        let matched = vec!["run".to_string(), "path".to_string(), "waveli".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, DECLINED, "one unpublished fragment must not veto");
        assert!(
            !c.stated_reason.contains("waveli"),
            "an abstention is never NAMED as generic: {}",
            c.stated_reason
        );

        // (e) But the quorum holds the line: when the corpus judged fewer than
        // half the open terms, it is deciding about words it never saw, so the
        // decline is refused. Here one judged word against three open ones.
        let matched = vec![
            "run".to_string(),
            "desdobramento".to_string(),
            "hierarquia".to_string(),
        ];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, "missing", "a minority of judged terms cannot decline");

        // (f) Fail-open: no index at all (scan model unavailable, or a model
        // from a binary that published no specificity) changes nothing.
        let matched = vec!["run".to_string(), "path".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &[]);
        assert_eq!(c.verdict, "missing");
        let flat = corpus(&[("run", 200, 0), ("path", 150, 0)]);
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &flat);
        assert_eq!(c.verdict, "missing", "a signal-less model must not mint a decline");

        // (g) A covered glossary is `ok`, and `ok` is never turned into a
        // decline — there is no grill to decline.
        let blocks = parse_term_blocks("## Run\nStart something.\n## Path\nA location.");
        let mut c = score(&matched, &blocks, true);
        assert_eq!(c.verdict, "ok");
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, "ok");
        assert!(c.stated_reason.is_empty());
    }

    /// The decline judges the word, not the spelling: the index publishes the
    /// singular, the digest matched the plural, and the group still resolves.
    #[test]
    fn decline_matches_the_word_across_its_spellings() {
        let index = sample_corpus();
        let matched = vec!["runs".to_string(), "paths".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, DECLINED, "`runs` is `run`: {}", c.stated_reason);

        // And when BOTH spellings are published, the widespread one speaks for
        // the word: `spec` in most modules makes the word repository-wide even
        // though the narrow plural scores as concentrated. Judging by the plural
        // is what silenced this verdict on the repository it was written for.
        let index = corpus(&[
            ("spec", 670, 1916),
            ("specs", 56, 4676),
            ("gate", 100, 3000),
            ("ledger", 40, 4000),
            ("payable", 30, 5000),
        ]);
        let matched = vec!["spec".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, DECLINED, "the word is where its spellings are: {}", c.stated_reason);
    }

    /// The stated reason is the payload, not a log line: it must survive into
    /// `grill-capture --finalize --reason` and into the JSON the caller reads.
    #[test]
    fn declined_verdict_publishes_its_stated_reason() {
        let index = sample_corpus();
        let matched = vec!["run".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        let payload = to_json(&c);
        assert_eq!(payload["verdict"], serde_json::json!(DECLINED));
        let stated = payload["statedReason"].as_str().unwrap_or_default();
        assert!(stated.contains("run"), "the sentence must name the term: {payload}");
        assert!(
            stated.ends_with("capturing"),
            "a complete sentence, ready to be recorded verbatim: {stated}"
        );
        // Every other verdict publishes the key EMPTY — the shape is stable, so
        // a caller can always read it, and only a decline has something to say.
        let ok = to_json(&score(&[], &[], false));
        assert_eq!(ok["statedReason"], serde_json::json!(""));
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
