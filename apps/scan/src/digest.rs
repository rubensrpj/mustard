//! Deterministic capability DIGEST — a small, AI-sized projection of the model.
//!
//! The full `grain.model.json` is large (every module + declaration + the whole
//! graph). A decomposition/elicitation step (the `feature` flow) must NOT read
//! it — that would blow the low-consumption budget. The digest is the bounded
//! "capability catalog" it queries instead: the recurring slices, roles, shared
//! contracts, registration hubs, the high-fan-in (often *injected*) contracts,
//! the projects, and a domain-term index so a request like "contas a receber"
//! can be looked up by term without reading any source.
//!
//! It is a pure projection of the (deterministic) model, so the digest is
//! deterministic too. Nothing here is language- or framework-specific.

use crate::model::ProjectModel;
use mustard_core::domain::vocabulary::stacks::StackDetection;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Caps keep the digest bounded regardless of repo size. `MAX_TERMS` bounds the
/// PUBLISHED full digest only — `query` searches the uncapped term index, so a
/// rare discriminative term that falls off this tail stays findable per lookup.
const MAX_ROLES: usize = 30;
const MAX_TOUCHPOINTS: usize = 20;
const MAX_FAN_IN: usize = 15;
const MAX_TERMS: usize = 120;
const MAX_TERM_SAMPLES: usize = 3;
/// Tighter caps for a per-query response so each lookup stays a few KB.
const Q_MAX_TERMS: usize = 25;
const Q_MAX_SLICES: usize = 12;
const Q_MAX_HUBS: usize = 8;
const Q_MAX_TOUCHPOINTS: usize = 10;
/// Anchor-file cap for a per-query response (`files` + its `files_detail`).
const Q_MAX_FILES: usize = 12;

#[derive(Serialize)]
pub(crate) struct CapabilityDigest {
    pub root: String,
    pub languages: Vec<LangD>,
    pub frameworks: Vec<String>,
    /// Stacks the model carries (evidence-converged, already deterministically
    /// ordered by the engine) — copied verbatim, never re-inferred here.
    pub detected_stacks: Vec<StackDetection>,
    pub projects: Vec<ProjD>,
    /// Top role affixes by frequency; `roles_omitted` is the truncated tail.
    pub roles: Vec<RoleD>,
    pub roles_omitted: usize,
    /// Recurring vertical slices — the build patterns available to compose.
    pub slices: Vec<SliceD>,
    /// Base types many entities inherit/implement (mined supertypes).
    pub shared_contracts: Vec<ContractD>,
    pub graph: GraphD,
    /// Domain-term index: token -> frequency + sample files (BM25-ranked,
    /// ranking.toml). The search surface for mapping a free-text request onto
    /// where it lives in the repo. Stopword-filtered (stopwords.toml) and
    /// capped at MAX_TERMS in this published view — `query` searches the
    /// uncapped index.
    pub terms: Vec<TermD>,
}

#[derive(Serialize)]
pub(crate) struct LangD {
    pub language: String,
    pub files: usize,
    pub loc: usize,
}

#[derive(Serialize)]
pub(crate) struct ProjD {
    pub name: String,
    pub dir: String,
    pub kind: String,
    pub code_files: usize,
}

#[derive(Serialize)]
pub struct RoleD {
    pub affix: String,
    pub kind: String,
    pub count: usize,
    pub common_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implements: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SliceD {
    /// Core role affixes joined with '+', e.g. "Handler+Validator".
    pub label: String,
    pub recurrence: usize,
    pub confidence: f32,
    pub entities: Vec<String>,
    pub optional_roles: Vec<String>,
    /// Real file paths that EXEMPLIFY this slice — the "street": the actual
    /// reference-implementation files to mirror, drawn from the convention's
    /// exemplars (most complex first), deduped, capped. Lets a consumer go
    /// straight to the files to copy instead of only the pattern name.
    pub exemplar_files: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ContractD {
    pub name: String,
    pub implementors: usize,
}

#[derive(Serialize)]
pub(crate) struct GraphD {
    pub nodes: usize,
    pub edges: usize,
    pub cyclic: bool,
    pub layers: Vec<LayerD>,
    pub touchpoints: Vec<TouchD>,
    /// Highest fan-in modules — where cross-cutting *injected* contracts live
    /// (e.g. a current-tenant accessor). These never show up as `shared_contracts`
    /// because they are dependencies, not supertypes, so they are surfaced here.
    pub top_fan_in: Vec<HubD>,
}

#[derive(Serialize)]
pub(crate) struct LayerD {
    pub name: String,
    pub modules: usize,
}

#[derive(Serialize)]
pub(crate) struct TouchD {
    pub module: String,
    pub fan_out: usize,
    pub breadth: usize,
}

#[derive(Serialize)]
pub(crate) struct HubD {
    pub module: String,
    pub degree: usize,
}

#[derive(Serialize)]
pub(crate) struct TermD {
    pub term: String,
    pub count: usize,
    /// Domain specificity ×1024: the term's TF·IDF over the corpus
    /// (`ranking::domain_specificity_x1024`). A discriminative-power signal that
    /// PEAKS IN THE MIDDLE of the frequency range — it demotes the ubiquitous
    /// term (seen in nearly every module) AND the hapax (one stray occurrence),
    /// so a mid-frequency domain word ranks above both. ADDITIVE: published
    /// alongside `count`; the term ORDER (and the `MAX_TERMS` cut) still follow
    /// the kind-weighted rank, never this field.
    pub specificity_x1024: u64,
    pub samples: Vec<String>,
}

/// A focused slice of the digest matching some domain terms — the cheap
/// per-interaction lookup a `feature` step does (a few KB, not the whole
/// catalog). The truth about what matched is the [`MatchReport`]: per request
/// term, which ladder tier carried it (or `none`), plus the aggregate
/// `matched k/n` and a reason. The legacy `miss` flag stays for cheap read
/// compatibility, but a `miss=false` answer can still be `weak` — consumers
/// must read the report, never just the flag.
#[derive(Serialize)]
pub(crate) struct QueryResult {
    pub query: Vec<String>,
    /// Stacks the model carries (same shape as the full digest) — copied
    /// verbatim from the model, so a per-query consumer never has to fetch the
    /// full catalog (or re-infer) to know what the repo runs on.
    pub detected_stacks: Vec<StackDetection>,
    /// Matching terms, rarest first (count asc): rarity ≈ discriminative power,
    /// so the per-query cap trims the frequent matches, never the rare ones.
    pub matched_terms: Vec<TermD>,
    /// Terms that matched but were trimmed by the per-query cap (no silent
    /// loss) — given the rarity ranking, these are the most frequent matches.
    pub terms_omitted: usize,
    pub slices: Vec<SliceD>,
    /// Slices that matched but were trimmed by the per-query cap — mirrors
    /// `terms_omitted` (additive; no silent loss).
    pub slices_omitted: usize,
    pub contracts: Vec<ContractD>,
    /// High fan-in modules whose path carries a query term — surfaces *injected*
    /// cross-cutting contracts (e.g. a current-tenant accessor) for `--invariant`.
    pub hubs: Vec<HubD>,
    pub touchpoints: Vec<TouchD>,
    /// Real files to read next, RANKED by BM25F (fielded retrieval): modules
    /// that DECLARE the matched terms or IMPLEMENT a matched contract, scored
    /// over three fields — the module's declarations, its path/filename, and its
    /// situating contracts (the supertypes it implements). A query that names a
    /// path segment lifts the files under that path (path is a boosted field);
    /// a query that names a contract lifts the modules that implement it (the
    /// situating field, weaker-boosted) even when neither their names nor paths
    /// carry the term; while BM25's length-normalization stops a sprawling
    /// god/seed file that only mentions many common terms from dominating. A hub
    /// anchors only when the vocabulary lives in its declarations or situating
    /// contracts — a path hit ALONE keeps it in `hubs`, never here (boost, not
    /// admission). Test/fixture and machine-written modules are excluded
    /// (evidence, not anchors). The handful the feature reads for ground truth
    /// instead of the repo.
    pub files: Vec<String>,
    /// Audit trail for `files`, additive and same order: per anchor, the
    /// fixed-point BM25F score it ranked with and the matched terms that carry
    /// it (by declaration or path).
    pub files_detail: Vec<FileDetail>,
    /// Legacy flag: every view above came back empty. Kept additively for old
    /// readers; the `report` is the truth (a non-miss can still be `weak`).
    pub miss: bool,
    /// Honest per-term match report — what each request term matched, at
    /// which ladder tier, in which language, and where.
    pub report: MatchReport,
    /// Legacy duplicate of `report.reason == "generated_only"`, kept for old
    /// readers (additive compat).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Concern split: when the query's concepts form ≥2 disconnected groups
    /// (no shared module, no import bridge), each group is a separate
    /// CONCERN with its OWN ranked anchors restricted to its concepts —
    /// so a multi-concern request surfaces the right files per concern
    /// instead of one blended list dominated by the larger concern. Empty
    /// for a single-concern query (the flat `files`/`files_detail` already
    /// are that one concern's view — no regression). Additive: an old reader
    /// keeps using `files`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub concerns: Vec<ConcernD>,
}

/// One concern of a multi-concern query — a connected group of the query's
/// concepts (co-occurrence components) with its own ranked anchors. A
/// sub-`QueryResult` view: same `files`/`files_detail` shape, scoped to this
/// concern's concepts only. The `label` joins the concern's concept tokens
/// (asc) with '+', so a reader names it without re-deriving the grouping.
#[derive(Serialize)]
pub(crate) struct ConcernD {
    /// The concern's concept tokens (sorted asc), joined with '+'.
    pub label: String,
    /// The query concepts in this concern (sorted asc — the grouping is set
    /// arithmetic, order is presentation only).
    pub concepts: Vec<String>,
    /// Files to read for THIS concern, ranked by BM25F over its concepts only
    /// (same retrieval as the flat `files`, restricted to the concern).
    pub files: Vec<String>,
    /// Audit trail for `files`, same order — per anchor the BM25F score and the
    /// concern concepts that carry it.
    pub files_detail: Vec<FileDetail>,
    /// This concern's strength on the report ladder, derived from its concepts'
    /// best tiers: `strong` (a concept reached exact/fold), `weak` (matched
    /// only on derived tiers), `none` (no anchor surfaced). A per-concern echo
    /// of the aggregate `report.reason`, so a reader trusts each concern on its
    /// own evidence.
    pub reason: String,
}

/// The aggregate match report: `matched` of `total` request terms found a
/// rung on the ladder, and `reason` summarizes the answer's strength:
/// `none` (nothing matched, no structural hit), `generated_only` (matches
/// exist but only in machine-written modules), `weak` (under half the terms
/// matched, or nothing matched at the exact/fold tiers — re-query in the
/// code's own vocabulary or explore before trusting), `strong` (solid
/// precedent). Pure serde data, mirrored by the consumer contract in
/// mustard-core (`domain::scan::DigestQuery`).
#[derive(Serialize)]
pub(crate) struct MatchReport {
    pub matched: usize,
    pub total: usize,
    pub reason: String,
    /// `true` when `reason == "weak"` ONLY because no term reached exact/fold,
    /// yet the trigram RESCUE rung carried a non-thin query (`matched*2 >=
    /// total`) — shared-root / morphology bridged by form. The consumer keeps
    /// the planning fields (with a caveat) instead of forcing a re-query the
    /// fuzzy rung already answered; speculative `stem`-only weakness stays
    /// `false`.
    pub bridged: bool,
    pub terms: Vec<TermReport>,
}

/// One request term's outcome: the ladder tier that carried it (`exact` |
/// `fold` | `stem` | `trigram` | `none`), the natural-language evidence
/// (stemmer language for `stem`, the literal `trigram` for the fuzzy rescue,
/// empty otherwise) and the top sample files where the matched vocabulary lives.
#[derive(Serialize)]
pub struct TermReport {
    pub term: String,
    pub tier: String,
    pub lang: String,
    pub files: Vec<String>,
}

/// One anchor's audit row (parallel to `files`): the fixed-point BM25F score
/// it ranked with and the matched index terms that carry it (by declaration or
/// path), in matched-term (tier, then rarity) order — why THIS file, verifiable
/// without rerunning the query.
#[derive(Serialize)]
pub struct FileDetail {
    pub file: String,
    pub score_x1024: u64,
    pub terms: Vec<String>,
}

/// Look up the digest by domain term(s) — OR across terms. Returns only the
/// matching slice (a few KB, capped) so the caller spends little per
/// interaction. Query terms shorter than 3 chars are ignored (mirrors the
/// mined-token floor). Matching runs on the ENGLISH intra-language tier ladder
/// in `matching` (exact > fold > stem, with an opt-in trigram rescue), and the
/// answer carries a per-term [`MatchReport`]. Deterministic.
pub fn query(model: &ProjectModel, terms: &[String]) -> QueryResult {
    let c = corpus(model);
    let dig = catalog(model, &c);
    let stop = stopwords();
    let ladder = crate::matching::Ladder::new();
    // Query tokens: trimmed, lowercased, length-floored AND stopword-filtered —
    // a glue token like "and" must never act as a discriminator, neither
    // against the term index nor against paths/labels via `hit`. Natural-
    // language glue in the active languages (vendored stoplists) is dropped
    // by the same contract.
    let mut ql: Vec<String> = terms
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s.len() >= 3 && !stop.contains(s) && !ladder.query_stopword(s))
        .collect();
    // Order-preserving dedup of the lowercased query tokens. The orchestration
    // layer now passes the cross-lingual translation INSIDE the intent
    // (`--intent "<PT words> <english translation>"`), so the same token often
    // arrives twice; collapsing it to one keeps `report.terms` / `matched_terms`
    // DISTINCT (a term is reported once) and saves redundant ladder work. A
    // BTreeSet seen-guard (not `.dedup()`, which only folds ADJACENT dups)
    // removes any repeat regardless of position; first occurrence wins, so the
    // surviving order stays deterministic (stable tie-break, per the miner's
    // contract).
    let mut seen = BTreeSet::new();
    ql.retain(|t| seen.insert(t.clone()));
    let qsigs: Vec<crate::matching::Sig> = ql.iter().map(|q| ladder.sig(q)).collect();
    // A name/path "hits" when any of its tokens matches any query token on
    // any rung of the ladder.
    let hit = |hay: &str| {
        let toks = tokenize(hay);
        toks.iter().any(|tk| {
            let ks = ladder.sig(tk);
            qsigs.iter().any(|qs| ladder.tier(&ks, qs, false).is_some())
        })
    };

    // One index term's hit for one query token — the raw material of the
    // per-term match report.
    struct QHit {
        tier: u8,
        lang: String,
        count: usize,
        term: String,
        samples: Vec<String>,
    }
    // Sweep the UNCAPPED term index once: per index term, the best (lowest)
    // tier across the query tokens; per query token, every hit. BTree-ordered
    // terms + fixed query order keep every outcome deterministic.
    let mut matched: Vec<(u8, TermD)> = Vec::new(); // (tier, term) for ranking
    let mut qhits: Vec<Vec<QHit>> = (0..ql.len()).map(|_| Vec::new()).collect();
    // The sweep enables the T5 fuzzy RESCUE rung; whether its matches are KEPT is
    // decided just below, gated on the strict (tiers 1-4) outcome.
    for t in dig.terms.into_iter() {
        let ks = ladder.sig(&t.term);
        let mut best: Option<u8> = None;
        for (qi, qs) in qsigs.iter().enumerate() {
            if let Some(h) = ladder.tier(&ks, qs, true) {
                qhits[qi].push(QHit { tier: h.tier, lang: h.lang, count: t.count, term: t.term.clone(), samples: t.samples.clone() });
                best = Some(best.map_or(h.tier, |b: u8| b.min(h.tier)));
            }
        }
        if let Some(tier) = best {
            matched.push((tier, t));
        }
    }
    // Trigram RESCUE gating: keep the fuzzy T5 hits ONLY when the STRICT ladder
    // (tiers 1-4) leaves the query weak/none — otherwise a strong query would
    // inherit T5's false cognates (it matches FORM, not sense). Computed from the
    // non-fuzzy hits already in `qhits`; if strict is solid, drop every T5 hit so
    // anchors and the report stay strict. The cost of fuzzy lands only on queries
    // that were already failing.
    let n_q = ql.len();
    let strict_k = (0..n_q).filter(|&qi| qhits[qi].iter().any(|h| h.tier <= 4)).count();
    let strict_solid = qhits.iter().flatten().any(|h| h.tier == 1 || h.tier == 2);
    let strict_strong = strict_k > 0 && strict_k * 2 >= n_q && strict_solid;
    if strict_strong {
        for qh in &mut qhits {
            qh.retain(|h| h.tier <= 4);
        }
        matched.retain(|(tier, _)| *tier <= 4);
    }
    // SITUATING sweep — the SAME ladder over the SEPARATE situating index (the
    // contracts each module implements), producing, per query token, the matched
    // situating terms. Kept OUT of `dig.terms` / `matched` (the published
    // declaration catalog stays declaration-only); it only feeds the anchor
    // ranking's situating field below. Deliberately STRICT-ONLY (`allow_fuzzy =
    // false`, tiers 1-3): a module IMPLEMENTING a contract is already contextual
    // evidence, so the fuzzy T5 rescue — reserved for salvaging an otherwise weak
    // DECLARATION query — never widens the situating field into false-cognate
    // contracts. Deterministic: BTree-ordered keys, fixed query order.
    let mut situating_qhits: Vec<Vec<&str>> = (0..ql.len()).map(|_| Vec::new()).collect();
    for term in c.situating.keys() {
        let ks = ladder.sig(term);
        for (qi, qs) in qsigs.iter().enumerate() {
            if ladder.tier(&ks, qs, false).is_some() {
                situating_qhits[qi].push(term.as_str());
            }
        }
    }
    // Matched terms ranked by TIER then RARITY (count asc, stable tie-break on
    // the term): a real vocabulary hit outranks a derived one, and among equals
    // the rare term is the discriminative one, so under the per-query cap the
    // frequent low-tier matches are the ones trimmed.
    matched.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.count.cmp(&b.1.count)).then(a.1.term.cmp(&b.1.term)));
    let terms_omitted = matched.len().saturating_sub(Q_MAX_TERMS);
    matched.truncate(Q_MAX_TERMS);
    // Anchor ranking — BM25F (FIELDED retrieval), summed over the QUERY
    // CONCEPTS. A candidate is a module that DECLARES at least one matched
    // concept OR IMPLEMENTS a queried contract (its situating field), anchor-
    // eligible and non-test; a path hit ALONE never admits one — that keeps a hub
    // named after a domain it does not implement in `hubs`, never here (its
    // grouped evidence still rides in `report.terms[].files`). Each candidate
    // scores the Σ over the query concepts of
    // `idf(concept) * BM25(path_boost*in-path + situating_boost*in-situating +
    // in-declarations, doc_len)`: THREE fields — the module's DECLARATIONS, its
    // PATH/filename, and its SITUATING contracts (the supertypes it implements).
    //
    // Three things make this robust where the flat Σ-idf field bug was not:
    //   * Fielding the PATH: a query that NAMES a path segment lifts the files
    //     under that path, and BM25's length-normalization stops a sprawling
    //     god/seed file that merely mentions many common terms from dominating.
    //   * Fielding the SITUATING contracts: a query that NAMES a contract lifts
    //     the modules that IMPLEMENT it even when neither their declaration names
    //     nor their path carry the term — a deterministic Contextual BM25. A
    //     contract on nearly every module is a common base type (high document
    //     frequency → low idf), so it is down-weighted, not amplified.
    //   * Summing over CONCEPTS, not index variants: a concept the query asked
    //     for folds ALL the index terms it matched (singular/plural, and the
    //     cross-language lexicon bridges) into ONE tf with ONE idf — so a file
    //     that spells one concept many ways no longer out-co-occurs a focused
    //     file that matches two RARE concepts.
    // idf is the parameter-free corpus rarity over the concept's true DOCUMENT
    // frequency ACROSS the fields (`core::domain::ranking::idf_x1024`, NOT the
    // occurrence count, which clamps to 0 once it exceeds the doc count and
    // erases a valid anchor); the lone ranking.toml knob is `path_boost`, and the
    // situating boost is a fixed constant (`rank::SITUATING_BOOST`, no gratuitous
    // knob). Fixed-point, byte-stable.
    //
    // A declaration in a test/fixture file is honest EVIDENCE (it stays in
    // `report.terms[].files`) but never an ANCHOR — you read and edit the
    // production file, not its test (a strong-by-coverage query whose rare terms
    // only collide inside test/fixture declarations must not seat those as the
    // files to touch). Skipped via the canonical agnostic
    // detector (`domain::ast::is_test_path` — dir-segment AND filename
    // convention, polyglot), the same primitive the AST layer uses.
    let n_docs = c.doc_len.len();
    let no_tokens: BTreeSet<String> = BTreeSet::new();
    // One requested concept (a distinct query token, NOT each index variant):
    // its anchor-eligible declaring modules with the folded concept tf, its
    // corpus document frequency → idf, best tier and the path-field sig.
    struct QConcept {
        token: String,
        idf: u64,
        best_tier: u8,
        sig: crate::matching::Sig,
        tf: BTreeMap<String, usize>,
        /// Anchor-eligible modules that IMPLEMENT this concept (a matched
        /// supertype), with the summed situating count — the BM25F situating
        /// field. Separate from `tf` because a situating match earns a FIXED
        /// boost (`rank::SITUATING_BOOST`), not the raw declaration tf; a
        /// situating-only concept has an empty `tf` and a non-empty `situating_tf`.
        situating_tf: BTreeMap<String, usize>,
        /// Every module declaring OR implementing this concept (the
        /// document-frequency set ACROSS both fields, NOT just the anchor-eligible
        /// `tf`/`situating_tf` keys) — feeds the idf (so a common base type is
        /// down-weighted by corpus rarity) and the surface the co-occurrence
        /// graph joins concepts over.
        modules: BTreeSet<String>,
    }
    let concepts: Vec<QConcept> = qhits
        .iter()
        .enumerate()
        .filter_map(|(qi, hits)| {
            let mut tf: BTreeMap<String, usize> = BTreeMap::new();
            let mut situating_tf: BTreeMap<String, usize> = BTreeMap::new();
            let mut df: BTreeSet<String> = BTreeSet::new();
            let mut best_tier = u8::MAX;
            for h in hits {
                best_tier = best_tier.min(h.tier);
                let Some(per_mod) = c.postings.get(&h.term) else { continue };
                for (p, (n, _)) in per_mod {
                    df.insert((*p).to_string());
                    if !mustard_core::domain::ast::is_test_path(p) && crate::classify::anchor_eligible(c.class_of.get(p).copied().unwrap_or("")) {
                        *tf.entry((*p).to_string()).or_insert(0) += *n;
                    }
                }
            }
            // SITUATING field: fold the concept's matched supertype postings. A
            // module that IMPLEMENTS the concept joins the document-frequency set
            // `df` (so the idf reflects the contract's TRUE corpus rarity ACROSS
            // both fields — a ubiquitous base type is down-weighted, a rare
            // contract keeps a high idf) and, when anchor-eligible, its count
            // lands in `situating_tf` for the boosted BM25F contribution. Same
            // test/generated exclusion as `tf`.
            for &term in &situating_qhits[qi] {
                let Some(per_mod) = c.situating.get(term) else { continue };
                for (p, n) in per_mod {
                    df.insert((*p).to_string());
                    if !mustard_core::domain::ast::is_test_path(p) && crate::classify::anchor_eligible(c.class_of.get(p).copied().unwrap_or("")) {
                        *situating_tf.entry((*p).to_string()).or_insert(0) += *n;
                    }
                }
            }
            // Admitted when the concept appears at all — in a declaration OR as a
            // situating contract (a situating-only concept has an empty `tf`, but
            // its implementing modules populate `df`, so this stays non-empty).
            if df.is_empty() {
                return None;
            }
            Some(QConcept {
                token: ql[qi].clone(),
                idf: mustard_core::domain::ranking::idf_x1024(df.len(), n_docs),
                best_tier,
                sig: ladder.sig(&ql[qi]),
                tf,
                situating_tf,
                modules: df,
            })
        })
        .collect();
    // Rank the anchor files over a SUBSET of the query concepts (the full set
    // for the flat view, one co-occurrence component for a concern). The body
    // is the same BM25F retrieval for both, so the flat list and a concern's
    // list can never disagree on how a file is scored:
    //   * candidate = an anchor-eligible module declaring any concept in the
    //     subset OR implementing one as a situating contract (a path hit ALONE
    //     never admits — boost only),
    //   * BM25F summed over the subset's concepts (idf·BM25 of the boosted
    //     path/situating/decl tf), score desc / best-tier / path-asc, byte-stable,
    //   * per-stratum (project) diversity via `stratified_order`, capped.
    // Returns the ranked `(files, files_detail)` for the subset.
    let rank_over = |subset: &[&QConcept]| -> (Vec<String>, Vec<FileDetail>) {
        let mut cand: BTreeSet<String> = BTreeSet::new();
        for qc in subset {
            cand.extend(qc.tf.keys().cloned());
            // A module that only IMPLEMENTS a queried contract (situating field)
            // is a candidate too — even with no declaration of the term and no
            // path hit.
            cand.extend(qc.situating_tf.keys().cloned());
        }
        let mut ranked: Vec<(String, u64, u8, Vec<String>)> = cand
            .into_iter()
            .map(|m| {
                let dl = c.doc_len.get(m.as_str()).copied().unwrap_or(0);
                let psigs: Vec<crate::matching::Sig> = c.path_tokens.get(m.as_str()).unwrap_or(&no_tokens).iter().map(|pt| ladder.sig(pt)).collect();
                let (mut score, mut best_tier, mut terms) = (0u64, u8::MAX, Vec::new());
                for qc in subset {
                    let tf_decl = qc.tf.get(&m).copied().unwrap_or(0);
                    let in_path = psigs.iter().any(|ps| ladder.tier(ps, &qc.sig, false).is_some());
                    let in_situating = qc.situating_tf.get(&m).copied().unwrap_or(0) > 0;
                    if tf_decl == 0 && !in_path && !in_situating {
                        continue;
                    }
                    score = score.saturating_add(crate::rank::bm25f_contribution_x1024(qc.idf, tf_decl, in_path, in_situating, dl, c.avgdl_x1024));
                    best_tier = best_tier.min(qc.best_tier);
                    terms.push(qc.token.clone());
                }
                (m, score, best_tier, terms)
            })
            .collect();
        // Rank: score desc, then best (lowest) tier, then path asc — byte-stable.
        // A candidate scored only by all-zero-idf concepts (a concept in every
        // indexed file) ranks last but is kept — the declaration is honest
        // evidence; the per-query cap drops it when enough discriminative
        // anchors exist.
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));
        // Per-stratum (project) diversity: guarantee each project that has a
        // candidate its BEST anchor an early slot before the global ranking
        // fills the rest — the same guarantee `rank::select_samples` gives
        // per-term samples, applied here to the anchor list so one project
        // cannot monopolize the top-N. Agnostic: stratum = `projects[].dir`.
        // A single project (or none) carrying candidates degenerates to pure
        // relevance.
        let strata: Vec<&str> = ranked.iter().map(|(p, _, _, _)| c.stratum.get(p.as_str()).copied().unwrap_or("")).collect();
        let ranked: Vec<(String, u64, u8, Vec<String>)> =
            crate::rank::stratified_order(&strata, Q_MAX_FILES).into_iter().map(|i| ranked[i].clone()).collect();
        let files: Vec<String> = ranked.iter().map(|(p, _, _, _)| p.clone()).collect();
        let files_detail: Vec<FileDetail> =
            ranked.iter().map(|(f, score, _, terms)| FileDetail { file: f.clone(), score_x1024: *score, terms: terms.clone() }).collect();
        (files, files_detail)
    };

    // Flat anchor view: every concept, exactly as before (zero regression — a
    // single-concern query never enters the concern split below).
    let all_concepts: Vec<&QConcept> = concepts.iter().collect();
    let (files, files_detail) = rank_over(&all_concepts);

    // Concern split: group the concepts by co-occurrence, then rank each
    // connected component's anchors over its OWN concepts. An edge joins two
    // concepts iff their module sets in `postings` INTERSECT (shared evidence),
    // optionally BRIDGED when a module of one imports a module of the other
    // (`Corpus::imports`) — so two concepts wired only through a dependency
    // still group together. A single component (one concern) yields no split:
    // the flat `files` above already IS that concern, so `concerns` stays empty
    // and a strong single-concern query is byte-identical to today. ≥2
    // components surface per-concern lists so a blended request (e.g. "tenant"
    // + "invoice export") does not let the larger concern bury the smaller
    // one's files. `is_empty`-skipped in serde so old readers are unaffected.
    let concerns: Vec<ConcernD> = {
        let n = concepts.len();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for i in 0..n {
            for j in (i + 1)..n {
                // Shared declaring module → direct co-occurrence edge.
                let shares_module = concepts[i].modules.intersection(&concepts[j].modules).next().is_some();
                // Import bridge: any module of one concept imports any module of
                // the other (directed in the model, undirected for grouping).
                let bridged = !shares_module
                    && (imports_into(&c, &concepts[i].modules, &concepts[j].modules)
                        || imports_into(&c, &concepts[j].modules, &concepts[i].modules));
                if shares_module || bridged {
                    edges.push((i, j));
                }
            }
        }
        let components = crate::rank::connected_components(n, &edges);
        // No split when the query is a single concern (≤1 concept, or every
        // concept co-occurs into one component) — the flat view suffices.
        if components.len() < 2 {
            Vec::new()
        } else {
            components
                .into_iter()
                .map(|comp| {
                    let subset: Vec<&QConcept> = comp.iter().map(|&i| &concepts[i]).collect();
                    let mut tokens: Vec<String> = subset.iter().map(|qc| qc.token.clone()).collect();
                    tokens.sort();
                    let (files, files_detail) = rank_over(&subset);
                    // A concern is `strong` if a concept reached exact/fold,
                    // `none` if it surfaced no anchor, else `weak` (derived
                    // tiers only) — a per-concern echo of the aggregate reason.
                    let best_tier = subset.iter().map(|qc| qc.best_tier).min().unwrap_or(u8::MAX);
                    let reason = if files.is_empty() {
                        "none"
                    } else if best_tier <= 2 {
                        "strong"
                    } else {
                        "weak"
                    };
                    ConcernD { label: tokens.join("+"), concepts: tokens, files, files_detail, reason: reason.to_string() }
                })
                .collect()
        }
    };

    let matched_terms: Vec<TermD> = matched.into_iter().map(|(_, t)| t).collect();

    let mut slices: Vec<SliceD> = dig.slices.into_iter().filter(|s| hit(&s.label) || s.entities.iter().any(|e| hit(e))).collect();
    let slices_omitted = slices.len().saturating_sub(Q_MAX_SLICES);
    slices.truncate(Q_MAX_SLICES);
    let contracts: Vec<ContractD> = dig.shared_contracts.into_iter().filter(|c| hit(&c.name)).collect();
    let mut hubs: Vec<HubD> = dig.graph.top_fan_in.into_iter().filter(|h| hit(&h.module)).collect();
    hubs.truncate(Q_MAX_HUBS);
    let mut touchpoints: Vec<TouchD> = dig.graph.touchpoints.into_iter().filter(|t| hit(&t.module)).collect();
    touchpoints.truncate(Q_MAX_TOUCHPOINTS);

    let miss = matched_terms.is_empty() && slices.is_empty() && contracts.is_empty() && hubs.is_empty() && touchpoints.is_empty();
    // A non-miss answer with NO anchorable surface: every matched term lives
    // only in machine-written modules (their samples were filtered down to
    // nothing) and no slice/contract/hub/touchpoint matched. Say WHY instead
    // of handing back an empty `files` the caller would misread as "no
    // precedent".
    let structural = !(slices.is_empty() && contracts.is_empty() && hubs.is_empty() && touchpoints.is_empty());
    let generated_only = !matched_terms.is_empty() && matched_terms.iter().all(|t| t.samples.is_empty()) && !structural;
    let reason = generated_only.then(|| "generated_only".to_string());

    // Per-request-term report. Each term's hits sort by tier asc, count asc,
    // term asc (the matched_terms discipline); the best hit names the tier +
    // language evidence, and the files are the best-tier hits' samples —
    // rarest vocabulary first, order-preserving dedup, a handful only.
    let report_terms: Vec<TermReport> = ql
        .iter()
        .enumerate()
        .map(|(qi, q)| {
            let hits = &mut qhits[qi];
            hits.sort_by(|a, b| a.tier.cmp(&b.tier).then(a.count.cmp(&b.count)).then(a.term.cmp(&b.term)));
            let Some(first) = hits.first() else {
                return TermReport { term: q.clone(), tier: crate::matching::tier_name(0).into(), lang: String::new(), files: Vec::new() };
            };
            let (best, lang) = (first.tier, first.lang.clone());
            let mut tfiles: Vec<String> = Vec::new();
            for sample in hits.iter().filter(|h| h.tier == best).flat_map(|h| h.samples.iter()) {
                // A test is never an actionable target — exclude it from the
                // per-term evidence the same way anchors/hubs already do
                // (`is_test_path`: polyglot dir-segment + filename convention,
                // names no language). Closes the test leak the anchor path
                // already blocked.
                if !mustard_core::domain::ast::is_test_path(sample) && !tfiles.contains(sample) {
                    tfiles.push(sample.clone());
                }
            }
            tfiles.truncate(MAX_TERM_SAMPLES);
            TermReport { term: q.clone(), tier: crate::matching::tier_name(best).into(), lang, files: tfiles }
        })
        .collect();
    let k = report_terms.iter().filter(|t| t.tier != "none").count();
    let n = ql.len();
    // `weak` = thin evidence: under half the request vocabulary found a rung,
    // or nothing landed on the exact/fold tiers (everything is stem/trigram-
    // derived) — the caller should re-query in the code's own vocabulary (the
    // matched terms/files show it) or explore before trusting the answer.
    let has_solid = report_terms.iter().any(|t| t.tier == "exact" || t.tier == "fold");
    // The T5 fuzzy RESCUE only survives the gating above on an otherwise
    // weak/none query — so its presence here means the strict ladder was thin
    // and trigram similarity (shared-root / morphology) carried the request.
    // Flagged as `bridged`: real evidence, form-not-literal, so the planning
    // fields ride along instead of forcing a re-query the fuzzy rung already
    // answered.
    let has_trigram_rescue = report_terms.iter().any(|t| t.tier == "trigram");
    let reason_word = if n == 0 || (k == 0 && !structural) {
        "none"
    } else if generated_only {
        "generated_only"
    } else if k * 2 < n || !has_solid {
        "weak"
    } else {
        "strong"
    };
    // A `weak` answer whose ONLY missing strength is the absence of an
    // exact/fold hit — not thinness (`k*2 >= n`, at least half the request
    // vocabulary found a rung) — that the trigram RESCUE carried (shared-root /
    // morphology by form). Real evidence, just not literal, so the consumer
    // keeps the planning fields (with a caveat) instead of forcing a re-query
    // the fuzzy rung already answered.
    let bridged = reason_word == "weak" && k * 2 >= n && has_trigram_rescue;
    let report = MatchReport { matched: k, total: n, reason: reason_word.into(), bridged, terms: report_terms };

    QueryResult {
        query: ql,
        detected_stacks: dig.detected_stacks,
        matched_terms,
        terms_omitted,
        slices,
        slices_omitted,
        contracts,
        hubs,
        touchpoints,
        files,
        files_detail,
        miss,
        report,
        reason,
        concerns,
    }
}

/// `true` when ANY module in `from` imports ANY module in `to` — the directed
/// import-bridge test for the concern co-occurrence graph (undirected at the
/// call site, which tries both directions). Imports are verbatim strings
/// (`Corpus::imports`), so a bridge holds when an import substring-matches a
/// target module's path — the same loose join the graph layer uses, kept here
/// to avoid a second resolution pass. Pure set arithmetic, no name knowledge.
fn imports_into(c: &Corpus, from: &BTreeSet<String>, to: &BTreeSet<String>) -> bool {
    from.iter().any(|m| {
        let Some(imps) = c.imports.get(m.as_str()) else { return false };
        imps.iter().any(|imp| to.iter().any(|t| t == imp || imp.contains(t.as_str()) || t.contains(imp)))
    })
}

/// Project the full model down to the bounded capability digest.
pub fn build(model: &ProjectModel) -> CapabilityDigest {
    let c = corpus(model);
    let mut dig = catalog(model, &c);
    // The published catalog stays bounded: cap the (count-desc-sorted) term
    // index here ONLY. `query` searches the uncapped index from `catalog`, so
    // capping the answer (not the index) is what keeps rare domain terms
    // findable without unbounding this view.
    dig.terms.truncate(MAX_TERMS);
    dig
}

/// The digest with the UNCAPPED term index — shared by [`build`] (which caps
/// the terms for the published catalog) and [`query`] (which must search every
/// term). Projects the model plus the prebuilt [`Corpus`]; rebuilt per call,
/// nothing here persists in the model.
fn catalog(model: &ProjectModel, c: &Corpus) -> CapabilityDigest {
    let languages = model.languages.iter().map(|l| LangD { language: l.language.clone(), files: l.files, loc: l.loc }).collect();
    let frameworks = model.frameworks.clone();
    let detected_stacks = model.detected_stacks.clone();
    let projects = model.projects.iter().map(|p| ProjD { name: p.name.clone(), dir: p.dir.clone(), kind: p.kind.clone(), code_files: p.code_files }).collect();

    // Roles: top by count (stable tie-break by affix), tail counted not dropped silently.
    let mut roles_sorted: Vec<&crate::model::RoleStat> = model.roles.iter().collect();
    roles_sorted.sort_by(|a, b| b.count.cmp(&a.count).then(a.affix.cmp(&b.affix)));
    let roles_omitted = roles_sorted.len().saturating_sub(MAX_ROLES);
    let roles = roles_sorted
        .iter()
        .take(MAX_ROLES)
        .map(|r| RoleD { affix: r.affix.clone(), kind: r.kind.clone(), count: r.count, common_dir: r.common_dir.clone(), implements: r.implements.clone() })
        .collect();

    // Slices: the multi-role conventions, trimmed (drop the verbose steps/examples).
    let mut slices: Vec<SliceD> = model
        .conventions
        .iter()
        .filter(|c| c.is_slice)
        .map(|c| SliceD {
            label: c.roles.iter().map(|s| s.as_str()).filter(|r| *r != "(core)").collect::<Vec<_>>().join("+"),
            recurrence: c.recurrence,
            confidence: c.confidence,
            entities: c.entities.iter().take(5).cloned().collect(),
            optional_roles: c.optional_roles.clone(),
            // The "street": the real files that exemplify this slice. Exemplars
            // are stored simple→complex (mine.rs push order), so iterate in
            // REVERSE to put the most complete reference first; DROP test/fixture
            // files (you mirror the production file, not its test builder — the
            // same `is_test_path` exclusion the anchors use); union across
            // exemplars, DEDUP preserving order, cap at 4 paths.
            exemplar_files: {
                let mut seen = std::collections::HashSet::new();
                c.exemplars
                    .iter()
                    .rev()
                    .flat_map(|e| e.files.iter())
                    .filter(|&f| !mustard_core::domain::ast::is_test_path(f))
                    .filter(|f| seen.insert((*f).clone()))
                    .take(4)
                    .cloned()
                    .collect()
            },
        })
        .collect();
    slices.sort_by(|a, b| b.recurrence.cmp(&a.recurrence).then(a.label.cmp(&b.label)));

    let shared_contracts = model.shared_contracts.iter().map(|s| ContractD { name: s.name.clone(), implementors: s.implementors }).collect();

    // Machine-written modules (generated/vendored/…) are never the file a
    // caller should read or edit: drop them from hubs and touchpoints — and
    // therefore from the anchor candidates `query` derives from these. Policy
    // is owned by `classify` (module-qualified call, no local wrapper).
    let eligible = |path: &str| crate::classify::anchor_eligible(c.class_of.get(path).copied().unwrap_or(""));

    let mut top_fan_in: Vec<HubD> =
        model.graph.top_fan_in.iter().filter(|n| eligible(&n.module)).map(|n| HubD { module: n.module.clone(), degree: n.degree }).collect();
    top_fan_in.sort_by(|a, b| b.degree.cmp(&a.degree).then(a.module.cmp(&b.module)));
    top_fan_in.truncate(MAX_FAN_IN);

    let mut touchpoints: Vec<TouchD> = model
        .graph
        .touchpoints
        .iter()
        .filter(|t| eligible(&t.module))
        .map(|t| TouchD { module: t.module.clone(), fan_out: t.fan_out, breadth: t.breadth })
        .collect();
    touchpoints.sort_by(|a, b| b.fan_out.cmp(&a.fan_out).then(a.module.cmp(&b.module)));
    touchpoints.truncate(MAX_TOUCHPOINTS);

    let graph = GraphD {
        nodes: model.graph.nodes,
        edges: model.graph.edges,
        cyclic: model.graph.cyclic,
        layers: model.graph.layers.iter().map(|l| LayerD { name: l.name.clone(), modules: l.modules }).collect(),
        touchpoints,
        top_fan_in,
    };

    let terms = build_terms(c);

    CapabilityDigest { root: model.root.clone(), languages, frameworks, detected_stacks, projects, roles, roles_omitted, slices, shared_contracts, graph, terms }
}

/// English glue words that occur inside identifiers without carrying domain
/// meaning. DATA, not logic: the list lives in `stopwords.toml` next to
/// `languages.toml` (embedded at compile time, justified in its header) —
/// tuning the vocabulary is a data change, never a code change. Parsed once
/// per process; a malformed embedded file is a programmer error caught by any
/// test run, same contract as build.rs over languages.toml.
pub(crate) fn stopwords() -> &'static BTreeSet<String> {
    static SET: OnceLock<BTreeSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        let raw: toml::Value = toml::from_str(include_str!("../stopwords.toml")).expect("stopwords.toml is not valid TOML");
        raw.get("stopwords")
            .and_then(|v| v.as_array())
            .expect("stopwords.toml must contain a `stopwords` array")
            .iter()
            .map(|w| w.as_str().expect("each stopword must be a string").to_lowercase())
            .collect()
    })
}

/// The shared ranking corpus: per-term postings (occurrences by module),
/// per-module document length (declaration count), fan-in and the corpus
/// average length — built in ONE pass over the model and consumed by both the
/// published term view ([`build_terms`]) and the per-query anchor scoring
/// ([`query`]), so the two can never disagree. The scoring arithmetic itself
/// lives in [`crate::rank`]. A SECOND term index — `situating` — maps the
/// contracts each module implements (its declaration supertypes); it feeds the
/// anchor ranking's situating field ONLY and is NEVER published in the
/// declaration catalog. BTreeMaps throughout: deterministic iteration.
struct Corpus<'a> {
    /// term -> module path -> (occurrences, Σ kind-class weights ×1024) in
    /// that module's declarations. The raw count feeds BM25; the weighted sum
    /// feeds the published-catalog rank (rank::kind_weight_x1024 — values and
    /// the type-kind list are DATA in ranking.toml).
    postings: BTreeMap<String, BTreeMap<&'a str, (usize, u64)>>,
    /// SITUATING field — situating token -> module path -> occurrence count in
    /// that module's SUPERTYPES (the contracts each declaration implements),
    /// tokenized by the SAME rule as `postings` (`index_ident`). A SEPARATE
    /// index whose searchable terms are `situating.keys()`: they lift the
    /// modules that IMPLEMENT a queried contract in the anchor ranking (BM25F's
    /// situating field), but are NEVER added to the published declaration
    /// catalog (`build_terms`/`dig.terms`). No kind-class weight (the catalog
    /// never sees these), so a plain count is enough.
    situating: BTreeMap<String, BTreeMap<&'a str, usize>>,
    /// Module path -> machine-written class (hand-written modules absent).
    class_of: BTreeMap<&'a str, &'a str>,
    /// Module path -> declaration count — the BM25 document length.
    doc_len: BTreeMap<&'a str, usize>,
    /// Module path -> the project stratum it lives under: the longest
    /// `projects[].dir` prefixing the path (same rule as the spec compiler's
    /// project attribution), "" when no project claims it.
    stratum: BTreeMap<&'a str, &'a str>,
    /// Module path -> lowercased path subtokens — the MMR Jaccard surface.
    path_tokens: BTreeMap<&'a str, BTreeSet<String>>,
    /// Module path -> verbatim import strings — the MMR neighborhood surface.
    imports: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// Average document length ×1024 over the indexed modules.
    avgdl_x1024: u64,
}

/// Index one identifier `name` into `bump`: the whole-identifier lowercased-alnum
/// key (when it differs from the sole token) plus each case/separator token, both
/// under the shared stopword filter and the tokenizer's ≥3-char alphabetic floor.
/// The SINGLE tokenization rule shared by the declaration field (`d.name` →
/// `postings`) and the situating field (`d.supertypes` → `situating`), so the two
/// can never drift. Deterministic: `bump` fires ident-first, then the tokens in
/// `tokenize()` order.
fn index_ident(name: &str, stop: &BTreeSet<String>, mut bump: impl FnMut(String)) {
    let toks = tokenize(name);
    // ONE extra entry per identifier: the whole identifier, lowercased and
    // stripped of separators ("SplitAsync" -> "splitasync", "parent_id" ->
    // "parentid"). Tier-1 of the match ladder accepts it as an exact key, so a
    // same-case or concatenated request term lands without any prefix guessing.
    // Skipped when it equals the single token (no double count) and under the
    // same glue rules.
    let ident: String = name.chars().filter(|ch| ch.is_alphanumeric()).collect::<String>().to_lowercase();
    if ident.len() >= 3
        && ident.chars().any(|ch| ch.is_alphabetic())
        && !(toks.len() == 1 && toks[0] == ident)
        && !stop.contains(&ident)
    {
        bump(ident);
    }
    for tok in toks {
        if stop.contains(&tok) {
            continue;
        }
        bump(tok);
    }
}

/// Build the corpus from declaration names (the repo's own vocabulary) plus the
/// SITUATING field (each declaration's supertypes — the contracts it implements)
/// in a SEPARATE index. Stopwords are never indexed; the two fields share ONE
/// tokenization rule ([`index_ident`]).
fn corpus(model: &ProjectModel) -> Corpus<'_> {
    let stop = stopwords();
    let class_of: BTreeMap<&str, &str> = model
        .modules
        .iter()
        .filter(|m| !m.file_class.is_empty())
        .map(|m| (m.path.as_str(), m.file_class.as_str()))
        .collect();
    let mut postings: BTreeMap<String, BTreeMap<&str, (usize, u64)>> = BTreeMap::new();
    let mut situating: BTreeMap<String, BTreeMap<&str, usize>> = BTreeMap::new();
    let mut doc_len: BTreeMap<&str, usize> = BTreeMap::new();
    let mut stratum: BTreeMap<&str, &str> = BTreeMap::new();
    let mut path_tokens: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut imports: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    // Longest-prefix project attribution (the spec compiler's `project_of`
    // rule): `dir` itself or anything under `dir/`.
    let under = |path: &str, dir: &str| path == dir || (path.len() > dir.len() && path.starts_with(dir) && path.as_bytes()[dir.len()] == b'/');
    let (mut len_sum, mut docs) = (0usize, 0usize);
    for m in &model.modules {
        // Lockfiles and minified output never enter the index; generated and
        // vendored modules stay (demoted in the published count) so a query
        // still lands.
        if !crate::classify::index_eligible(&m.file_class) {
            continue;
        }
        doc_len.insert(m.path.as_str(), m.declarations.len());
        len_sum += m.declarations.len();
        docs += 1;
        let strat = model
            .projects
            .iter()
            .filter(|p| !p.dir.is_empty() && under(&m.path, &p.dir))
            .max_by_key(|p| p.dir.len())
            .map_or("", |p| p.dir.as_str());
        stratum.insert(m.path.as_str(), strat);
        path_tokens.insert(m.path.as_str(), tokenize(&m.path).into_iter().collect());
        imports.insert(m.path.as_str(), m.imports.iter().map(|s| s.as_str()).collect());
        for d in &m.declarations {
            // Each occurrence carries its declaration's kind-class weight for
            // the published rank, alongside the raw count BM25 consumes.
            let kw = crate::rank::kind_weight_x1024(&d.kind);
            index_ident(&d.name, stop, |term| {
                let e = postings.entry(term).or_default().entry(m.path.as_str()).or_insert((0, 0));
                e.0 += 1;
                e.1 += kw;
            });
            // SITUATING field: index the contracts this declaration IMPLEMENTS
            // (its supertypes) under the SAME tokenization rule, into a SEPARATE
            // map. A query that names a contract then lifts the modules that
            // implement it (BM25F situating field) even when the contract name is
            // absent from the declaration names and the path — WITHOUT the
            // supertype vocabulary ever entering the published declaration
            // catalog. Plain count (no kind weight): the catalog never sees it.
            for sup in &d.supertypes {
                index_ident(sup, stop, |term| {
                    *situating.entry(term).or_default().entry(m.path.as_str()).or_insert(0) += 1;
                });
            }
        }
    }
    Corpus {
        postings,
        situating,
        class_of,
        doc_len,
        stratum,
        path_tokens,
        imports,
        avgdl_x1024: crate::rank::avgdl_x1024(len_sum, docs),
    }
}

/// Project the corpus into the domain-term index. The index is UNCAPPED here
/// — see [`build`] vs [`query`] for who caps what. Samples come ONLY from
/// hand-written modules, scored by BM25 (rank::bm25_x1024, fixed-point
/// integer) and picked by rank::select_samples: each matched project stratum
/// keeps ≥1 slot when ≥2 strata match, the rest go to greedy MMR diversity —
/// every tie breaks on path asc, byte-stable output. The published ORDER (and
/// therefore who survives the MAX_TERMS cap) follows the kind-class-weighted
/// rank — type-name vocabulary outranks a member flood — while `count` stays
/// the demoted occurrence count.
fn build_terms(c: &Corpus) -> Vec<TermD> {
    let class = |p: &str| c.class_of.get(p).copied().unwrap_or("");
    let (no_tokens, no_imports) = (BTreeSet::new(), BTreeSet::new());
    // Corpus size for the specificity IDF: the total number of indexed modules,
    // the same denominator the anchor ranking uses (`query`'s `n_docs`).
    let n_docs = c.doc_len.len();


    let mut terms: Vec<(u64, TermD)> = c
        .postings
        .iter()
        .map(|(term, per_module)| {
            // Machine-written occurrences are demoted by the catalog
            // multiplier (classify::index_weight) — present, never dominant.
            let count = per_module.iter().map(|(p, (n, _))| crate::classify::index_weight(*n, class(p))).sum();
            // Domain specificity (TF·IDF ×1024): the term's RAW occurrence
            // total times its corpus rarity. `df` is the document frequency
            // (modules the term appears in); the raw `count` (NOT the catalog-
            // demoted one) is the true TF. Additive signal — never reorders the
            // index (the sort below stays on `rank_key`).
            let raw_count: usize = per_module.iter().map(|(_, (n, _))| *n).sum();
            let df = per_module.len();
            let specificity_x1024 = mustard_core::domain::ranking::domain_specificity_x1024(raw_count, df, n_docs);
            // The catalog rank key: per module, the kind-weighted occurrence
            // sum (rank::weighted_count) under the SAME machine-class
            // demotion as `count`. Not serialized — ranking only.
            let rank_key: u64 = per_module
                .iter()
                .map(|(p, (_, w))| crate::classify::index_weight(crate::rank::weighted_count(*w), class(p)) as u64)
                .sum();
            let cands: Vec<crate::rank::SampleCand> = per_module
                .iter()
                .filter(|(p, _)| crate::classify::anchor_eligible(class(p)))
                .map(|(p, (n, _))| crate::rank::SampleCand {
                    path: p,
                    score_x1024: crate::rank::bm25_x1024(*n, c.doc_len.get(p).copied().unwrap_or(0), c.avgdl_x1024),
                    stratum: c.stratum.get(p).copied().unwrap_or(""),
                    subtokens: c.path_tokens.get(p).unwrap_or(&no_tokens),
                    neighbors: c.imports.get(p).unwrap_or(&no_imports),
                })
                .collect();
            let samples = crate::rank::select_samples(&cands, MAX_TERM_SAMPLES);
            (rank_key, TermD { term: term.clone(), count, specificity_x1024, samples })
        })
        .collect();
    terms.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.count.cmp(&a.1.count)).then(a.1.term.cmp(&b.1.term)));
    terms.into_iter().map(|(_, t)| t).collect()
}

/// Split an identifier into lowercased domain tokens on case boundaries and
/// non-alphanumerics, handling acronyms. Splits on lower/digit -> Upper AND on
/// Upper -> Upper-followed-by-lower, so "ICurrentTenant" -> ["current","tenant"]
/// and "HTTPServer" -> ["http","server"]. Drops glue tokens (<3 chars) so
/// "ListTransfersByTenantId" yields ["list","transfers","tenant"]. Shared with
/// the spec compiler. No language/framework knowledge.
pub(crate) fn tokenize(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for i in 0..chars.len() {
        let ch = chars[i];
        if !ch.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if !cur.is_empty() {
            let prev = chars[i - 1];
            let next = chars.get(i + 1).copied();
            let boundary =
                // camelCase / digit -> Upper:  "fooBar" -> foo|Bar
                (ch.is_uppercase() && (prev.is_lowercase() || prev.is_ascii_digit()))
                // acronym -> word:  "HTTPServer" -> HTTP|Server
                || (ch.is_uppercase() && prev.is_uppercase() && next.is_some_and(|n| n.is_lowercase()));
            if boundary {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|s| s.to_lowercase()).filter(|s| s.len() >= 3 && s.chars().any(|c| c.is_alphabetic())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Decl, Module, ProjectModel};

    /// One class declaration implementing the given contracts (supertypes).
    fn decl(name: &str, supertypes: &[&str]) -> Decl {
        Decl { kind: "class".to_string(), name: name.to_string(), line: 1, supertypes: supertypes.iter().map(|s| s.to_string()).collect() }
    }

    /// One hand-written module (empty `file_class` → indexed and anchor-eligible).
    fn module(path: &str, decls: Vec<Decl>) -> Module {
        Module { path: path.to_string(), declarations: decls, ..Default::default() }
    }

    fn model(modules: Vec<Module>) -> ProjectModel {
        ProjectModel { root: "/repo".to_string(), modules, ..Default::default() }
    }

    #[test]
    fn situating_field_lifts_a_module_implementing_a_rare_contract() {
        // "PaymentGateway" is a SUPERTYPE on exactly one module whose declaration
        // name ("Handler") and path ("apps/svc/handler.x") are both generic — the
        // contract name is absent from the declaration names AND the path. A query
        // naming the contract must still rank that module as an anchor: the
        // situating field (the contracts a module implements) carries it — a
        // deterministic Contextual BM25.
        let mut mods = vec![module("apps/svc/handler.x", vec![decl("Handler", &["PaymentGateway"])])];
        // Filler modules keep "paymentgateway" rare (idf > 0) and give the corpus
        // a realistic size; none declare or implement the contract.
        for i in 0..8 {
            mods.push(module(&format!("apps/other/mod{i:02}.x"), vec![decl(&format!("Widget{i:02}"), &[])]));
        }
        let m = model(mods);
        let q = query(&m, &["PaymentGateway".to_string()]);

        assert_eq!(
            q.files.first().map(String::as_str),
            Some("apps/svc/handler.x"),
            "the module implementing the rare contract anchors via the situating field: {:?}",
            q.files
        );
        // A real BM25F score (high idf over df=1 across fields, boosted tf > 0).
        let detail = |q: &QueryResult| q.files_detail.iter().map(|d| (d.file.clone(), d.score_x1024)).collect::<Vec<_>>();
        assert!(q.files_detail[0].score_x1024 > 0, "the situating anchor carries a real BM25F score: {:?}", detail(&q));

        // The contract vocabulary must NOT leak into the PUBLISHED declaration
        // catalog — situating is a separate, unpublished index.
        let dig = build(&m);
        assert!(
            !dig.terms.iter().any(|t| ["paymentgateway", "payment", "gateway"].contains(&t.term.as_str())),
            "situating terms never enter the published `terms` catalog: {:?}",
            dig.terms.iter().map(|t| t.term.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_common_situating_contract_does_not_outrank_a_focused_declaration() {
        // "BaseService" is an omnipresent supertype (on ten modules), so its
        // corpus rarity is LOW and the idf down-weights it. A focused module that
        // DECLARES a rare term must outrank the flood of modules that merely
        // IMPLEMENT the common contract, even though each earns the situating
        // boost. The focused file's path does NOT name the term, so this isolates
        // the declaration idf vs. the situating idf (no path-field help).
        let mut mods = vec![module("billing/core.x", vec![decl("Invoice", &[])])];
        for i in 0..10 {
            // Generic declaration names → "service" lives ONLY in the situating
            // field (the supertype), never in a declaration name or a path.
            mods.push(module(&format!("svc/unit{i:02}.x"), vec![decl(&format!("Unit{i:02}"), &["BaseService"])]));
        }
        let m = model(mods);
        // The query names BOTH the rare declaration term and the common contract.
        let q = query(&m, &["invoice".to_string(), "service".to_string()]);
        let detail = q.files_detail.iter().map(|d| (d.file.clone(), d.score_x1024)).collect::<Vec<_>>();

        assert_eq!(
            q.files.first().map(String::as_str),
            Some("billing/core.x"),
            "the focused rare declaration leads; the common contract's implementors do not dominate: {detail:?}",
        );
        // The common contract's implementors DO still surface (situating admits
        // them) — just ranked strictly below the focused declaration match.
        assert!(
            q.files.iter().any(|f| f.starts_with("svc/unit")),
            "modules implementing the common contract still surface as situating anchors: {:?}",
            q.files
        );
        let lead = q.files_detail[0].score_x1024;
        let common = q.files_detail.iter().find(|d| d.file.starts_with("svc/unit")).map(|d| d.score_x1024).unwrap_or(0);
        assert!(lead > common, "the rare declaration outscores the common contract's implementors: {lead} vs {common}");
    }

    #[test]
    fn situating_driven_ranking_is_deterministic() {
        // A fixture mixing a rare contract, a common contract and a declaration
        // match over several modules, so the BTree ordering of the situating
        // index is exercised. Two independent queries must serialize
        // byte-identically — every situating tie-break is stable
        // (BTreeMap/BTreeSet throughout, no HashMap iteration).
        let mut mods = vec![
            module("pay/gateway_impl.x", vec![decl("StripeAdapter", &["PaymentGateway", "BaseService"])]),
            module("pay/gateway_alt.x", vec![decl("PaypalAdapter", &["PaymentGateway", "BaseService"])]),
            module("billing/invoice.x", vec![decl("Invoice", &["BaseService"])]),
        ];
        for i in 0..5 {
            mods.push(module(&format!("misc/unit{i:02}.x"), vec![decl(&format!("Unit{i:02}"), &["BaseService"])]));
        }
        let m = model(mods);
        let terms = ["paymentgateway".to_string(), "invoice".to_string()];
        let a = serde_json::to_string(&query(&m, &terms)).expect("serialize query result");
        let b = serde_json::to_string(&query(&m, &terms)).expect("serialize query result");
        assert_eq!(a, b, "identical situating-driven answers across runs");
        // Sanity: both concepts anchored their evidence — the rare contract's
        // implementors AND the declaration match.
        let q = query(&m, &terms);
        assert!(q.files.contains(&"pay/gateway_impl.x".to_string()), "rare contract implementor anchors: {:?}", q.files);
        assert!(q.files.contains(&"billing/invoice.x".to_string()), "declaration match anchors: {:?}", q.files);
    }
}
