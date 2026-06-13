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
pub struct CapabilityDigest {
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
pub struct LangD {
    pub language: String,
    pub files: usize,
    pub loc: usize,
}

#[derive(Serialize)]
pub struct ProjD {
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
pub struct SliceD {
    /// Core role affixes joined with '+', e.g. "Handler+Validator".
    pub label: String,
    pub recurrence: usize,
    pub confidence: f32,
    pub entities: Vec<String>,
    pub optional_roles: Vec<String>,
}

#[derive(Serialize)]
pub struct ContractD {
    pub name: String,
    pub implementors: usize,
}

#[derive(Serialize)]
pub struct GraphD {
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
pub struct LayerD {
    pub name: String,
    pub modules: usize,
}

#[derive(Serialize)]
pub struct TouchD {
    pub module: String,
    pub fan_out: usize,
    pub breadth: usize,
}

#[derive(Serialize)]
pub struct HubD {
    pub module: String,
    pub degree: usize,
}

#[derive(Serialize)]
pub struct TermD {
    pub term: String,
    pub count: usize,
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
pub struct QueryResult {
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
    /// Real files to read next (anchor candidates), MATCH-FIRST and
    /// COVERAGE-FIRST: modules whose DECLARATIONS carry the matched terms. A
    /// coverage pass seats each matched term's best file first (rarest, most
    /// discriminative terms lead), then the remaining slots fill by the
    /// IDF-weighted aggregate over the matched terms plus a small fan-in
    /// tiebreak — so a frequent-term neighbour can never crowd a rare
    /// domain's top file out. A hub anchors only when the vocabulary lives
    /// in its declarations — a path hit alone keeps it in `hubs` but never
    /// here. Structural stop-files (fan-in above the ranking.toml percent of
    /// all modules) are excluded; path-matched touchpoints are appended as a
    /// low-priority tail. The handful the feature reads for ground truth
    /// instead of the repo.
    pub files: Vec<String>,
    /// Audit trail for `files`, additive and same order: per anchor, the
    /// fixed-point selection score and the matched terms that carry it. A
    /// touchpoint-tail anchor (path hit only) honestly shows score 0 and no
    /// terms.
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
pub struct MatchReport {
    pub matched: usize,
    pub total: usize,
    pub reason: String,
    pub terms: Vec<TermReport>,
}

/// One request term's outcome: the ladder tier that carried it (`exact` |
/// `fold` | `stem` | `lexicon` | `none`), the natural-language evidence
/// (stemmer language for `stem`, pair label for `lexicon`, empty otherwise)
/// and the top sample files where the matched vocabulary lives.
#[derive(Serialize)]
pub struct TermReport {
    pub term: String,
    pub tier: String,
    pub lang: String,
    pub files: Vec<String>,
}

/// One anchor's audit row (parallel to `files`): the aggregate fixed-point
/// score it ranked with and the matched index terms whose declarations carry
/// it, in matched-term (tier, then rarity) order — why THIS file, verifiable
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
/// mined-token floor). `request_lang` is the DECLARED language of the request
/// (root config / CLI — never detected); matching runs on the tier ladder in
/// `matching` (exact > fold > same-language stem > lexicon), and the answer
/// carries a per-term [`MatchReport`]. Deterministic.
pub fn query(model: &ProjectModel, terms: &[String], request_lang: &str) -> QueryResult {
    let c = corpus(model);
    let dig = catalog(model, &c);
    let stop = stopwords();
    // The ladder's project lexicon overlay resolves against the SCANNED
    // project's root from the loaded model — never the cwd, since the tool
    // can run from anywhere.
    let ladder = crate::matching::Ladder::new(request_lang, Some(std::path::Path::new(&model.root)));
    // Query tokens: trimmed, lowercased, length-floored AND stopword-filtered —
    // a glue token like "and" must never act as a discriminator, neither
    // against the term index nor against paths/labels via `hit`. Natural-
    // language glue in the active languages (vendored stoplists) is dropped
    // by the same contract.
    let ql: Vec<String> = terms
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s.len() >= 3 && !stop.contains(s) && !ladder.query_stopword(s))
        .collect();
    let qsigs: Vec<crate::matching::Sig> = ql.iter().map(|q| ladder.sig(q)).collect();
    // A name/path "hits" when any of its tokens matches any query token on
    // any rung of the ladder.
    let hit = |hay: &str| {
        let toks = tokenize(hay);
        toks.iter().any(|tk| {
            let ks = ladder.sig(tk);
            qsigs.iter().any(|qs| ladder.tier(&ks, qs).is_some())
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
    for t in dig.terms.into_iter() {
        let ks = ladder.sig(&t.term);
        let mut best: Option<u8> = None;
        for (qi, qs) in qsigs.iter().enumerate() {
            if let Some(h) = ladder.tier(&ks, qs) {
                qhits[qi].push(QHit { tier: h.tier, lang: h.lang, count: t.count, term: t.term.clone(), samples: t.samples.clone() });
                best = Some(best.map_or(h.tier, |b: u8| b.min(h.tier)));
            }
        }
        if let Some(tier) = best {
            matched.push((tier, t));
        }
    }
    // Matched terms ranked by TIER then RARITY (count asc, stable tie-break on
    // the term): a real vocabulary hit outranks a derived one, and among equals
    // the rare term is the discriminative one, so under the per-query cap the
    // frequent low-tier matches are the ones trimmed.
    matched.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.count.cmp(&b.1.count)).then(a.1.term.cmp(&b.1.term)));
    let terms_omitted = matched.len().saturating_sub(Q_MAX_TERMS);
    matched.truncate(Q_MAX_TERMS);
    let tiers: Vec<u8> = matched.iter().map(|(tier, _)| *tier).collect();
    let matched_terms: Vec<TermD> = matched.into_iter().map(|(_, t)| t).collect();

    let mut slices: Vec<SliceD> = dig.slices.into_iter().filter(|s| hit(&s.label) || s.entities.iter().any(|e| hit(e))).collect();
    let slices_omitted = slices.len().saturating_sub(Q_MAX_SLICES);
    slices.truncate(Q_MAX_SLICES);
    let contracts: Vec<ContractD> = dig.shared_contracts.into_iter().filter(|c| hit(&c.name)).collect();
    let mut hubs: Vec<HubD> = dig.graph.top_fan_in.into_iter().filter(|h| hit(&h.module)).collect();
    hubs.truncate(Q_MAX_HUBS);
    let mut touchpoints: Vec<TouchD> = dig.graph.touchpoints.into_iter().filter(|t| hit(&t.module)).collect();
    touchpoints.truncate(Q_MAX_TOUCHPOINTS);

    // Anchor candidates, MATCH-FIRST: a module enters on a term match in its
    // DECLARATIONS, never on its path alone — a hub that only path-hits stays
    // listed in `hubs` but does not anchor. Selection is COVERAGE-FIRST, two
    // passes over per-(term, module) scores computed once below:
    //
    //   (a) COVERAGE — walk `matched_terms` in their existing order (tier
    //       asc, then rarity asc) and seat each term's best module: argmax
    //       of tier-weight × BM25 (the ~10×-per-rung tier weight keeps an
    //       exact vocabulary hit above any derived one) with the small
    //       fan-in boost as tiebreak, path asc on full ties. A term whose
    //       best module is already seated is covered — no second-best taken.
    //       This is what a frequent-term neighbour can never crowd out:
    //       every matched term keeps its top file among the anchors, rarest
    //       (most discriminative) terms leading.
    //   (b) FILL — remaining slots go to the leftover candidates by the
    //       aggregate Σ tier-weight × IDF × BM25 over the matched terms
    //       carrying the module (rank::idf_x1024, document-frequency
    //       derived: a term half the repo carries cannot drown a three-file
    //       term by double-dipping in every declaration; co-occurrence still
    //       pays — a file where several queried concepts meet accumulates
    //       every term's weighted contribution) + α·log2(1+fan_in), a small
    //       additive tiebreak that never outranks a term match. Fill ties
    //       break on the first (best-tier, rarest) matched term that carries
    //       the module, then on the path.
    //
    // Structural stop-files (rank::anchor_stopfile — fan-in above the
    // configured percent of the repo's module count) leave anchor
    // eligibility entirely. Fully deterministic.
    let class = |p: &str| c.class_of.get(p).copied().unwrap_or("");
    let anchorable = |p: &str| {
        crate::classify::anchor_eligible(class(p))
            && !crate::rank::anchor_stopfile(c.fan_in.get(p).copied().unwrap_or(0), c.total_modules)
    };
    let boost = |p: &str| crate::rank::fanin_boost_x1024(c.fan_in.get(p).copied().unwrap_or(0));
    // Candidate -> (IDF-weighted aggregate, first carrying term, every
    // carrying term — the audit trail `files_detail` publishes).
    let mut cand: BTreeMap<&str, (u64, usize, Vec<usize>)> = BTreeMap::new();
    // Per matched term, its best anchorable module — the coverage argmax.
    let mut top_of: Vec<Option<(u64, &str)>> = vec![None; matched_terms.len()];
    let docs = c.doc_len.len();
    for (i, t) in matched_terms.iter().enumerate() {
        let Some(per_module) = c.postings.get(&t.term) else { continue };
        let idf = crate::rank::idf_x1024(per_module.len(), docs);
        for (path, (tf, _)) in per_module {
            // Same anchor discipline as the term samples: hand-written
            // modules only, and never a structural stop-file.
            if !anchorable(path) {
                continue;
            }
            let bm25 = crate::rank::bm25_x1024(*tf, c.doc_len.get(path).copied().unwrap_or(0), c.avgdl_x1024);
            let w = crate::matching::weight(tiers[i]);
            let cover = w * bm25 + boost(path);
            if top_of[i].is_none_or(|(bs, bp)| cover > bs || (cover == bs && *path < bp)) {
                top_of[i] = Some((cover, *path));
            }
            let e = cand.entry(path).or_insert((0, i, Vec::new()));
            e.0 += w * crate::rank::idf_term_score_x1024(idf, bm25);
            e.2.push(i);
        }
    }
    // PATH CO-OCCURRENCE — fill evidence for capability folders that name
    // themselves. A composing view (the screen that wires the pieces) often
    // declares little vocabulary, but its folder does — and the directory
    // donates its subtokens to every file inside, so the whole cluster
    // surfaces together. When at least `path_co_min_terms` DISTINCT matched
    // terms appear as exact lowercased path subtokens of an anchorable
    // module, each contributes `path_co_bm25` BM25 units × its IDF to the
    // FILL aggregate — never a coverage seat. A single path token stays
    // worthless: that is the anti-noise rule that keeps mere path hits in
    // `hubs`, not here — a folder whose path carries several query terms
    // donates them to every file inside, while a file whose path matches just
    // one query term stays single-token and gains nothing.
    let path_co_bm25 = crate::rank::path_co_bm25_x1024();
    if path_co_bm25 > 0 && !matched_terms.is_empty() {
        let min_co = crate::rank::path_co_min_terms();
        let lowered: Vec<String> = matched_terms.iter().map(|t| t.term.to_lowercase()).collect();
        for (path, toks) in &c.path_tokens {
            if !anchorable(path) {
                continue;
            }
            let hit_idx: Vec<usize> =
                (0..matched_terms.len()).filter(|&i| toks.contains(&lowered[i])).collect();
            if hit_idx.len() < min_co {
                continue;
            }
            let mut add = 0u64;
            for &i in &hit_idx {
                let df = c.postings.get(&matched_terms[i].term).map_or(1, BTreeMap::len);
                let idf = crate::rank::idf_x1024(df, docs);
                add += crate::matching::weight(tiers[i])
                    * crate::rank::idf_term_score_x1024(idf, path_co_bm25);
            }
            let e = cand.entry(path).or_insert((0, hit_idx[0], Vec::new()));
            e.0 += add;
            for &i in &hit_idx {
                if !e.2.contains(&i) {
                    e.2.push(i);
                }
            }
        }
    }
    // Pass (a): the coverage walk, capped — minus the fill reserve on a wide
    // query. With more matched terms than slots, pure coverage degenerates to
    // one-file-per-rare-term and the capability cluster (where the queried
    // concepts CO-OCCUR) is crowded out entirely (field case: 12 single-term
    // seats, target folder 0/12). The reserve binds only when a multi-term
    // candidate exists to claim it; otherwise coverage keeps every slot.
    let has_multi = cand.values().any(|(_, _, terms)| terms.len() >= 2);
    let coverage_cap = if has_multi {
        Q_MAX_FILES.saturating_sub(crate::rank::fill_reserve_slots()).max(1)
    } else {
        Q_MAX_FILES
    };
    let mut files: Vec<String> = Vec::new();
    let mut seated: BTreeSet<&str> = BTreeSet::new();
    for (_, p) in top_of.iter().flatten() {
        if files.len() == coverage_cap {
            break;
        }
        if seated.insert(p) {
            files.push(p.to_string());
        }
    }
    // Pass (b): leftover candidates by aggregate desc.
    let mut ranked: Vec<(&str, u64, usize)> =
        cand.iter().filter(|(p, _)| !seated.contains(*p)).map(|(p, (s, i, _))| (*p, s + boost(p), *i)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(b.0)));

    // Path-matched touchpoints stay a low-priority tail (registration points
    // are edited, not read for vocabulary), behind every term-matched
    // candidate. Order-preserving dedup; capped to ~a dozen.
    let src = ranked
        .iter()
        .map(|(p, _, _)| (*p).to_string())
        .chain(touchpoints.iter().filter(|t| anchorable(&t.module)).map(|t| t.module.clone()));
    for m in src {
        if files.len() == Q_MAX_FILES {
            break;
        }
        if !files.contains(&m) {
            files.push(m);
        }
    }

    // The audit trail: one row per anchor, same order as `files`, with the
    // (fill-comparable) aggregate score and the matched terms carrying it.
    let files_detail: Vec<FileDetail> = files
        .iter()
        .map(|f| match cand.get(f.as_str()) {
            Some((s, _, idxs)) => FileDetail {
                file: f.clone(),
                score_x1024: s + boost(f),
                terms: idxs.iter().map(|&i| matched_terms[i].term.clone()).collect(),
            },
            None => FileDetail { file: f.clone(), score_x1024: 0, terms: Vec::new() },
        })
        .collect();

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
                if !tfiles.contains(sample) {
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
    // or nothing landed on the exact/fold tiers (everything is stem/lexicon-
    // derived) — the caller should re-query in the code's own vocabulary (the
    // matched terms/files show it) or explore before trusting the answer.
    let has_solid = report_terms.iter().any(|t| t.tier == "exact" || t.tier == "fold");
    let reason_word = if n == 0 || (k == 0 && !structural) {
        "none"
    } else if generated_only {
        "generated_only"
    } else if k * 2 < n || !has_solid {
        "weak"
    } else {
        "strong"
    };
    let report = MatchReport { matched: k, total: n, reason: reason_word.into(), terms: report_terms };

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
    }
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
fn stopwords() -> &'static BTreeSet<String> {
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
/// lives in [`crate::rank`]. BTreeMaps throughout: deterministic iteration.
struct Corpus<'a> {
    /// term -> module path -> (occurrences, Σ kind-class weights ×1024) in
    /// that module's declarations. The raw count feeds BM25; the weighted sum
    /// feeds the published-catalog rank (rank::kind_weight_x1024 — values and
    /// the type-kind list are DATA in ranking.toml).
    postings: BTreeMap<String, BTreeMap<&'a str, (usize, u64)>>,
    /// Module path -> machine-written class (hand-written modules absent).
    class_of: BTreeMap<&'a str, &'a str>,
    /// Module path -> declaration count — the BM25 document length.
    doc_len: BTreeMap<&'a str, usize>,
    /// Module path -> import-graph fan-in, persisted on the model by scan.
    fan_in: BTreeMap<&'a str, usize>,
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
    /// Total modules in the model — the base of the stop-file percent.
    total_modules: usize,
}

/// Build the corpus from declaration names (the repo's own vocabulary).
/// Stopwords are never indexed.
fn corpus(model: &ProjectModel) -> Corpus<'_> {
    let stop = stopwords();
    let class_of: BTreeMap<&str, &str> = model
        .modules
        .iter()
        .filter(|m| !m.file_class.is_empty())
        .map(|m| (m.path.as_str(), m.file_class.as_str()))
        .collect();
    let mut postings: BTreeMap<String, BTreeMap<&str, (usize, u64)>> = BTreeMap::new();
    let mut doc_len: BTreeMap<&str, usize> = BTreeMap::new();
    let mut fan_in: BTreeMap<&str, usize> = BTreeMap::new();
    let mut stratum: BTreeMap<&str, &str> = BTreeMap::new();
    let mut path_tokens: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut imports: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    // Longest-prefix project attribution (the spec compiler's `project_of`
    // rule): `dir` itself or anything under `dir/`.
    let under = |path: &str, dir: &str| path == dir || (path.len() > dir.len() && path.starts_with(dir) && path.as_bytes()[dir.len()] == b'/');
    let (mut len_sum, mut docs) = (0usize, 0usize);
    for m in &model.modules {
        fan_in.insert(m.path.as_str(), m.fan_in);
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
            let toks = tokenize(&d.name);
            // Each occurrence carries its declaration's kind-class weight for
            // the published rank, alongside the raw count BM25 consumes.
            let kw = crate::rank::kind_weight_x1024(&d.kind);
            let mut bump = |term: String| {
                let e = postings.entry(term).or_default().entry(m.path.as_str()).or_insert((0, 0));
                e.0 += 1;
                e.1 += kw;
            };
            // ONE extra entry per declaration: the whole identifier, lowercased
            // and stripped of separators ("SplitAsync" -> "splitasync",
            // "parent_id" -> "parentid"). Tier-1 of the match ladder accepts
            // it as an exact key, so a same-case or concatenated request term
            // lands without any prefix guessing. Skipped when it equals the
            // single token (no double count) and under the same glue rules.
            let ident: String = d.name.chars().filter(|ch| ch.is_alphanumeric()).collect::<String>().to_lowercase();
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
    }
    Corpus {
        postings,
        class_of,
        doc_len,
        fan_in,
        stratum,
        path_tokens,
        imports,
        avgdl_x1024: crate::rank::avgdl_x1024(len_sum, docs),
        total_modules: model.modules.len(),
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
    let mut terms: Vec<(u64, TermD)> = c
        .postings
        .iter()
        .map(|(term, per_module)| {
            // Machine-written occurrences are demoted by the catalog
            // multiplier (classify::index_weight) — present, never dominant.
            let count = per_module.iter().map(|(p, (n, _))| crate::classify::index_weight(*n, class(p))).sum();
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
            (rank_key, TermD { term: term.clone(), count, samples })
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
