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
//! statedReason, termsCovered, termsTotal, uncovered, verdict }` — the same keys
//! on every verdict, so a caller can always read them. The `uncovered` list IS
//! the actionable payload — the domain terms of a THIN glossary (`weak`) that the
//! orchestrator hands to the inline grill (`grill-capture`) so each confirmed
//! definition lands in the glossary; `contextFile` names the resolved target to
//! write them into (the first existing CONTEXT.md, or the first requested path
//! when none resolved yet, so a still-empty glossary still has a destination).
//! Fail-open: a missing model / unreadable glossary degrades to `verdict: "na"`
//! (the SKILL then stays silent); exit 0.
//!
//! ABSENCE and THINNESS are different questions and get different answers. An
//! absent glossary (`missing`) asks for a FIRST glossary — one decision about the
//! project, not a queue of words to interrogate — so it publishes NO term list at
//! all. A thin one (`weak`) asks for MORE entries, and there the list is exactly
//! what the grill needs. Publishing the whole matched vocabulary under `missing`
//! answered the thin question with the absent verdict.
//!
//! And a term is only offered when the corpus published it. Scan's stem tier
//! answers with fragments no human ever typed (`waveli` for `waveList`,
//! `split201`), and the term index — the repository's own record of what it says
//! — carries nothing about them. A word the corpus never published is not
//! vocabulary anyone can define, so it is dropped from the offer rather than
//! filtered by a hand-written stopword list (see [`keep_only_published`]).
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
    /// Every word the glossary leaves open — the WORKING set, not the published
    /// one. What the report offers is derived from it in [`to_json`]: a thin
    /// glossary publishes these terms, an absent one publishes none (it is asked
    /// for a first glossary, not for a list of words), and the corpus filter in
    /// [`keep_only_published`] has already removed anything the term index never
    /// published. The decline reads THIS set, so absence keeps its judgement.
    open: Vec<String>,
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
    /// Terms a FIRST glossary would be worth opening with — non-empty ONLY on
    /// the `missing` verdict, and only when the corpus judges some word of this
    /// request concentrated. See [`seed_terms`] for why this is a different
    /// question from `uncovered` and must not be folded into it.
    seed: Vec<String>,
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
    let mut open: Vec<String> = Vec::new();
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
            open.push(group.representative.clone());
        }
    }
    let total = groups.len();
    let mut c = Coverage {
        present,
        total,
        covered,
        open,
        verdict: "ok",
        context_file: String::new(),
        stated_reason: String::new(),
        seed: Vec::new(),
    };
    c.verdict = if total == 0 {
        // No domain terms touched → nothing a glossary could cover; never nudge.
        "ok"
    } else if !present {
        // ABSENT: there is no glossary to extend, so the answer is "author one".
        // The open words are still tracked (the decline judges them), but the
        // report offers none of them — see `to_json`.
        "missing"
    } else if c.pct() < WEAK_COVERAGE_PCT || c.open.len() >= WEAK_UNCOVERED_FLOOR {
        // THIN: a glossary exists and is short of entries — here the open words
        // ARE the answer.
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

/// The rarity a term must reach to count as domain vocabulary HERE: the UPPER
/// QUARTILE of the repository's own published term index.
///
/// Read off the corpus rather than fixed, because the scale of `idf_x1024`
/// depends on the corpus size — a constant would mean something different in
/// every repository.
///
/// It was the median first, and review proved that unreachable on the corpus it
/// was written against: with the cut at the middle word, HALF the published
/// vocabulary sits at or above it, and a single such word vetoes the decline
/// (the veto is correct — one genuine domain term means the grill still has a
/// question worth asking). Measured on this repository: 120 published terms,
/// median rarity 4132, and everyday words like `agent` landed exactly ON the
/// median, so four real intents all failed to decline. A verdict that no input
/// can reach is decoration, which is the very defect the spec carrying this
/// change exists to end.
///
/// The upper quartile says something a reader can check: a word counts as domain
/// vocabulary only when it is more concentrated than three quarters of what this
/// repository publishes. Everything below is the shared background language.
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
    // Upper quartile by index, integer and exact — no percentage to round.
    rarities.get(rarities.len() * 3 / 4).copied()
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
/// The quorum survives as the backstop for a caller that hands over a RAW open
/// set. In production it can no longer bite: [`keep_only_published`] runs first
/// and takes the unpublished words out of `open`, so every remaining word is one
/// the index can judge and `judged == open`. That was the whole deadlock — the
/// fragments dirtying the offer were also the ones holding the quorum below half
/// and blocking the decline that would have silenced the offer.
///
/// Pure and deterministic: the named terms keep the digest's order, and the same
/// inputs always produce the same sentence.
fn decline_reason(matched: &[String], open_terms: &[String], index: &[DigestTerm]) -> Option<String> {
    let cut = corpus_rarity_cut(index)?;
    let mut named: Vec<String> = Vec::new();
    let mut open = 0usize;
    for group in group_inflections(matched) {
        if !open_terms.contains(&group.representative) {
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
         repository-wide vocabulary — the scan model ranks each below the upper \
         quartile of this repository's term rarity, so there is no domain \
         definition worth capturing",
        named.join(", ")
    ))
}

/// Drop from the open set every word the published term index says nothing
/// about — the words the grill must never offer.
///
/// The matched terms arrive from the scan model, whose stem tier answers with
/// fragments no human ever typed (`waveli` for `waveList`, `split201`, an
/// interface-prefixed identifier, the package's own name). Asking a user to
/// define one of those is asking about a word the repository never published,
/// and the index is what says so: [`group_rarity_x1024`] already answers "did
/// the corpus publish this word" with `None`. Read off the corpus, never a
/// hand-written stopword list — a curated list would rot and encode one person's
/// taste, and this project forbids one.
///
/// Fail-open on an empty index: with nothing published, EVERY word looks
/// unpublished and the offer would be emptied on a missing model rather than on
/// evidence. No index → no filtering.
///
/// Side effect on the verdict, and only this one: a THIN glossary whose whole
/// open set was fragments has nothing a grill could ask about, so it stops
/// nudging (`ok`). ABSENCE is left alone — "author a first glossary" is a
/// decision about the project and never depended on the term list.
fn keep_only_published(c: &mut Coverage, matched: &[String], index: &[DigestTerm]) {
    if index.is_empty() {
        return;
    }
    let published: BTreeSet<String> = group_inflections(matched)
        .into_iter()
        .filter(|g| group_rarity_x1024(g, index).is_some())
        .map(|g| g.representative)
        .collect();
    c.open.retain(|term| published.contains(term));
    if c.verdict == "weak" && c.open.is_empty() {
        c.verdict = "ok";
    }
}

/// The terms a FIRST glossary would be worth opening with — the answer to the
/// question an absent glossary actually raises.
///
/// `uncovered` answers "which authored entries are thin", which has no meaning
/// with no file, so it is deliberately empty on `missing`. This answers the
/// other one: of the words this request touches, which does the repository's own
/// index report as CONCENTRATED — said in few places rather than everywhere?
///
/// The same arithmetic [`decline_reason`] uses, read from the other end. There
/// it proves a word is repository-wide vocabulary and therefore not worth
/// defining; here the words at or above [`corpus_rarity_cut`] are exactly the
/// ones that are. Nothing is written down anywhere, so nothing can rot into one
/// person's taste.
///
/// Offers NOTHING rather than noise, in both directions that matter: a word the
/// index never published ([`group_rarity_x1024`] answers `None`) is left out —
/// which keeps scan's stem fragments (`waveli` for `waveList`) out for free —
/// and a request touching only ubiquitous vocabulary yields an empty list, the
/// same reading `declined` publishes. A padded list is the theatre that teaches
/// an operator to skip the step.
///
/// Deterministic: the digest's order is kept, and the same inputs always produce
/// the same list. This NEVER decides what a term means — it names words to ask a
/// human about.
fn seed_terms(matched: &[String], index: &[DigestTerm]) -> Vec<String> {
    let Some(cut) = corpus_rarity_cut(index) else {
        return Vec::new();
    };
    group_inflections(matched)
        .into_iter()
        .filter(|group| group_rarity_x1024(group, index).is_some_and(|r| r >= cut))
        .map(|group| group.representative)
        .collect()
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
    // The seed answers the question an ABSENT glossary raises, so it is offered
    // on `missing` only — never on `weak`, where the file exists and `uncovered`
    // is already the actionable list. Computed before the decline below because
    // a declined project has nothing worth defining by definition, and the
    // decline clears it.
    if c.verdict == "missing" {
        c.seed = seed_terms(matched, index);
    }
    if let Some(reason) = decline_reason(matched, &c.open, index) {
        c.verdict = DECLINED;
        c.stated_reason = reason;
        // A declined project has, by the corpus's own reading, nothing worth
        // defining — so it must not also be handed a list to define. The two
        // verdicts would contradict each other in the same document.
        c.seed.clear();
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
        // Order matters: the fragments leave the open set BEFORE the decline
        // counts it, so the quorum weighs the words the corpus can judge.
        keep_only_published(&mut coverage, &matched, &index);
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
    // Terms are OFFERED only when there is a glossary to extend. With none
    // authored the answer is "write a first glossary", and a list of words to
    // interrogate answers the other question — so the key stays (callers read
    // the shape positionally) and arrives empty.
    let uncovered: &[String] = if c.present { &c.open } else { &[] };
    json!({
        "verdict": c.verdict,
        "present": c.present,
        "termsTotal": c.total,
        "termsCovered": c.covered,
        "coveragePct": c.pct(),
        "uncovered": uncovered,
        "contextFile": c.context_file,
        // Always present (stable shape), non-empty only on `declined` — the
        // sentence the caller hands to `grill-capture --finalize --reason`.
        "statedReason": c.stated_reason,
        // Always present, non-empty only on `missing` — the terms a FIRST
        // glossary is worth opening with. Beside `uncovered`, never inside it:
        // the two answer different questions, and folding them would re-teach
        // the caller to read an absent glossary as thin coverage.
        "seed": c.seed,
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
    score_terms_with_corpus(matched, glossary, &[])
}

/// [`score_terms`] with the repository's published term index supplied — the
/// whole production sequence over inputs a test can state, so the corpus-driven
/// steps (dropping the words the index never published, then the decline that
/// judges what is left) are exercised through the SAME JSON contract the command
/// prints instead of through private shapes.
///
/// An empty `index` is exactly the "no corpus" case [`score_terms`] wants: the
/// filter and the decline both fail open, leaving the ordinary coverage verdict.
#[allow(dead_code)]
#[must_use]
pub fn score_terms_with_corpus(
    matched: &[String],
    glossary: &str,
    index: &[DigestTerm],
) -> serde_json::Value {
    let blocks = parse_term_blocks(glossary);
    let present = !blocks.is_empty();
    let mut c = score(matched, &blocks, present);
    if matches!(c.verdict, "missing" | "weak") {
        keep_only_published(&mut c, matched, index);
        apply_decline(&mut c, matched, index);
    }
    to_json(&c)
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
            // No corpus to ask, so nothing was declined and nothing can be
            // seeded — the shape stays stable and the caller has no reason to
            // record or to offer.
            "statedReason": "",
            "seed": [],
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
        assert_eq!(c.open.len(), 2, "the words are still tracked internally");
        assert_eq!(c.pct(), 0);
        // ...but none is OFFERED: absence asks for a first glossary, not for a
        // queue of words to interrogate.
        assert_eq!(to_json(&c)["uncovered"], serde_json::json!([]));
    }

    /// Absence and thinness ask for opposite actions, so they must not publish
    /// the same payload. Same words, same open set — only the glossary differs.
    #[test]
    fn an_absent_glossary_is_not_a_coverage_failure() {
        let matched = vec![
            "payable".to_string(),
            "tenant".to_string(),
            "ledger".to_string(),
        ];
        let absent = to_json(&score(&matched, &[], false));
        assert_eq!(absent["verdict"], serde_json::json!("missing"));
        assert_eq!(absent["present"], serde_json::json!(false));
        assert_eq!(absent["uncovered"], serde_json::json!([]));
        // The shape is stable — every key is still there to read.
        assert_eq!(absent["termsTotal"], serde_json::json!(3));
        assert_eq!(absent["termsCovered"], serde_json::json!(0));
        assert_eq!(absent["coveragePct"], serde_json::json!(0));

        let blocks = parse_term_blocks("## Payable\nA bill owed.");
        let thin = to_json(&score(&matched, &blocks, true));
        assert_eq!(thin["verdict"], serde_json::json!("weak"));
        assert_eq!(
            thin["uncovered"],
            serde_json::json!(["tenant", "ledger"]),
            "an authored glossary is asked for MORE entries, by name: {thin}"
        );
    }

    #[test]
    fn full_coverage_is_ok() {
        let blocks = parse_term_blocks("## Payable\nA bill owed.\n## Tenant\nAn org.");
        let matched = vec!["payable".to_string(), "tenant".to_string()];
        let c = score(&matched, &blocks, true);
        assert_eq!(c.verdict, "ok");
        assert_eq!(c.covered, 2);
        assert!(c.open.is_empty());
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
        assert_eq!(c.open, vec!["tenant", "ledger", "invoice"]);
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
        assert!(c.open.is_empty(), "nothing is left open: {:?}", c.open);
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
        assert_eq!(c.open, vec!["specs"]);
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
        // (`gate` is deliberately absent from this intent: the cut is the upper
        // quartile, so a mid-rarity word counts as ubiquitous and would not
        // change the outcome either way.)
        let matched = vec!["run".to_string(), "path".to_string(), "waveli".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, DECLINED, "one unpublished fragment must not veto");
        assert!(
            !c.stated_reason.contains("waveli"),
            "an abstention is never NAMED as generic: {}",
            c.stated_reason
        );

        // (e) The quorum holds the line for a RAW open set: given one judged word
        // against three open ones, the corpus would be deciding about words it
        // never saw, so the decline is refused.
        let matched = vec![
            "run".to_string(),
            "desdobramento".to_string(),
            "hierarquia".to_string(),
        ];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, "missing", "a minority of judged terms cannot decline");

        // ...and that is exactly the deadlock the corpus filter breaks. Run the
        // production order — the unpublished words leave the open set first —
        // and the same input reaches the decline, because what remains is one
        // repository-wide word and nothing the corpus never saw.
        let mut c = score(&matched, &[], false);
        keep_only_published(&mut c, &matched, &index);
        assert_eq!(c.open, vec!["run"], "only published words stay open: {:?}", c.open);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(
            c.verdict, DECLINED,
            "the fragments were both the noise and the blocker: {}",
            c.stated_reason
        );

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

    /// A word the published index says nothing about is not vocabulary anyone
    /// can define — the fragments scan's stem tier answers with (`split201`,
    /// `completedat`) never reach the user, and what survives is the real
    /// vocabulary, unchanged.
    #[test]
    fn unpublished_fragments_are_not_grill_material() {
        let index = sample_corpus();
        // A glossary exists (so terms ARE offered) but defines none of these.
        let blocks = parse_term_blocks("## Seed\nA starting record.");
        let matched = vec![
            "payable".to_string(),
            "split201".to_string(),
            "ledger".to_string(),
            "completedat".to_string(),
        ];
        let mut c = score(&matched, &blocks, true);
        assert_eq!(c.verdict, "weak");
        keep_only_published(&mut c, &matched, &index);
        assert_eq!(
            to_json(&c)["uncovered"],
            serde_json::json!(["payable", "ledger"]),
            "only words the corpus published are worth asking about"
        );
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, "weak", "real domain terms still deserve the grill");

        // Fail-open: no index at all means nothing was judged unpublished, so
        // the offer is left exactly as scored (a missing model must not empty it).
        let mut c = score(&matched, &blocks, true);
        keep_only_published(&mut c, &matched, &[]);
        assert_eq!(c.open.len(), 4);

        // A thin glossary whose whole open set was fragments has nothing a grill
        // could ask — it stops nudging instead of nudging with an empty list.
        let fragments = vec![
            "split201".to_string(),
            "completedat".to_string(),
            "waveli".to_string(),
        ];
        let mut c = score(&fragments, &blocks, true);
        assert_eq!(c.verdict, "weak");
        keep_only_published(&mut c, &fragments, &index);
        assert_eq!(c.verdict, "ok", "no words left to ask about");
        assert_eq!(to_json(&c)["uncovered"], serde_json::json!([]));
    }

    /// AC-1 — a project with NO glossary is handed a starting list.
    ///
    /// The gap this closes: the report correctly said the glossary was missing
    /// and correctly handed back no `uncovered` (there are no authored entries
    /// to be thin), which left the operator with a correct message and a blank
    /// page — so the only route out was the stated-reason escape, taken every
    /// time.
    #[test]
    fn a_project_with_no_glossary_is_handed_a_seed() {
        let index = sample_corpus();
        // `payable` and `ledger` sit at/above the upper quartile; `run` is said
        // everywhere. One concentrated word also keeps the decline from firing.
        let matched = vec!["payable".to_string(), "run".to_string(), "ledger".to_string()];
        let mut c = score(&matched, &[], false);
        assert_eq!(c.verdict, "missing");
        apply_decline(&mut c, &matched, &index);

        assert!(c.seed.contains(&"payable".to_string()), "seed: {:?}", c.seed);
        assert!(c.seed.contains(&"ledger".to_string()), "seed: {:?}", c.seed);
        assert!(
            !c.seed.contains(&"run".to_string()),
            "a word said everywhere is not worth defining: {:?}",
            c.seed
        );
        // The two questions stay apart — the previous unit's separation holds.
        assert!(
            to_json(&c)["uncovered"].as_array().is_some_and(|a| a.is_empty()),
            "an absent glossary still publishes no open-entry list"
        );
        assert_eq!(to_json(&c)["seed"], serde_json::json!(["payable", "ledger"]));
    }

    /// AC-2 — nothing rather than noise, in both directions that matter.
    #[test]
    fn a_seed_is_empty_rather_than_padded_with_noise() {
        let index = sample_corpus();

        // (a) only ubiquitous vocabulary → the corpus declines, and a declined
        // project must not ALSO be handed a list to define: the two verdicts
        // would contradict each other in one document.
        let matched = vec!["run".to_string(), "path".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.verdict, DECLINED);
        assert!(c.seed.is_empty(), "a declined project gets no seed: {:?}", c.seed);

        // (b) words the index never published are left out — this is what keeps
        // scan's stem fragments (`waveli` for `waveList`) from ever being
        // offered as vocabulary.
        let matched = vec!["payable".to_string(), "waveli".to_string()];
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &index);
        assert_eq!(c.seed, vec!["payable".to_string()], "unpublished words stay out");

        // (c) no corpus at all → nothing to judge, so nothing offered.
        let mut c = score(&matched, &[], false);
        apply_decline(&mut c, &matched, &[]);
        assert!(c.seed.is_empty(), "an empty index cannot mint a seed");
    }

    /// AC-3 — an authored glossary is never offered a seed, however thin it is.
    ///
    /// The seed answers "what would a FIRST glossary open with". A project that
    /// already keeps one is asking the other question, and `uncovered` is
    /// already its actionable answer — offering both would collapse the
    /// separation this rests on.
    #[test]
    fn an_authored_glossary_is_never_offered_a_seed() {
        let index = sample_corpus();
        // A thin-but-present glossary: `weak`, with real open terms.
        let blocks = parse_term_blocks("## Run\nStart something.");
        let matched = vec!["run".to_string(), "payable".to_string(), "ledger".to_string()];
        let mut c = score(&matched, &blocks, true);
        assert_eq!(c.verdict, "weak", "the fixture must really be thin, not absent");
        apply_decline(&mut c, &matched, &index);

        assert!(c.seed.is_empty(), "an authored glossary gets no seed: {:?}", c.seed);
        // And the thin-coverage answer is unchanged: the open terms are still
        // published, which is what that project acts on.
        let payload = to_json(&c);
        assert!(
            payload["uncovered"].as_array().is_some_and(|a| !a.is_empty()),
            "a thin glossary still publishes its open terms: {payload}"
        );
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
