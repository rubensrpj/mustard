//! pagerank — personalized (topic-sensitive) PageRank ranker over the model's
//! dependency graph, SEEDED by the distinctive-vocabulary dictionary.
//!
//! ## What this is for
//!
//! The `dictionary` sidecar already bridges a raw Portuguese request onto the
//! repo's own domain terms (13/13 prompts match ≥1 distinctive PT term, no LLM).
//! What it CANNOT do is localize: a term's `anchors` are the files where it is
//! most FREQUENT — comment-dense hubs — not the file a task edits, and summing
//! specificity lets a broad high-count term (`conta`, `contrato`) drown the rare
//! discriminative one (`aging`, `desdobramento`, `extrato`) that actually points
//! at the target.
//!
//! This ranker fixes both:
//!   1. It weights each matched term's seeds by RARITY (idf), so a rare term
//!      concentrates its mass on few files while a ubiquitous one spreads thin.
//!   2. It runs personalized PageRank over the resolved import graph, so a file
//!      that is import-central within the seed neighborhood is lifted even when
//!      it is not itself a top anchor — and machine-written code is demoted
//!      BEFORE ranking (in the personalization AND the final score).
//!
//! ## Pipeline (query PT → top-N files)
//!
//! 1. MATCH — tokenize the query, drop PT+EN glue, match each token against the
//!    dictionary `term`s by accent-fold equality OR folded prefix (min side ≥4),
//!    the same rungs `benchmarks/sialia/dict-lookup.ps1` validated, built on the
//!    ladder's `fold`. Collect the matched terms (+ their idf).
//! 2. SEED — resolve each matched term to model files: the modules whose
//!    declaration/path tokens contain the term (rich, model-derived — the case
//!    for an English term), UNION the term's dictionary `anchors` (the only
//!    seeds a PT comment-term has). Personalization mass per term = its idf,
//!    split across its seeds. Machine-written / test files never seed.
//! 3. GRAPH — the directed module→imported-module edges from
//!    [`crate::graph::resolve_edges`] (the SAME graph the model publishes).
//! 4. RANK — power-iterate personalized PageRank in fixed-point INTEGER
//!    arithmetic (no float ever enters a comparison → byte-stable across runs
//!    and platforms), fixed iterations, then order the eligible files by score.
//!
//! ## Determinism / robustness
//!
//! `BTreeMap`/sorted vectors throughout, u128 fixed-point (total mass
//! `PR_SCALE`), a total-order output sort, and NO `unwrap`/`expect` outside
//! tests. Fail-open: no matched term, no seed, or an empty model yields an empty
//! ranked list, never a panic. The graph is reconstructed from the persisted
//! `imports` with no `go_module` (absent from a saved model) — a Go-import-only
//! edge would not resolve, which never applies to a C#/TS corpus.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::dictionary::Dictionary;
use crate::matching::fold;
use crate::model::ProjectModel;

/// Fixed-point scale for damping, weights and idf (mirrors the crate house
/// rule): 10 fractional bits.
const SCALE: u128 = 1024;

/// Total PageRank mass, distributed across the nodes. Large enough (2^32) that a
/// per-node share stays well above the integer-truncation floor even on a
/// multi-thousand-node monorepo, so ranking never collapses to zeros — the
/// reason a bare ×1024 mass would not do.
const PR_SCALE: u128 = 1 << 32;

/// Resolution unit for accumulating fractional seed weights before the single
/// normalization to `PR_SCALE`; keeps a split-thin contribution non-zero.
const SEED_UNIT: u128 = 1 << 20;

/// Edge orientation for the random walk. The import graph is directed
/// (importer→imported); which orientation localizes best is empirical, so it is
/// a knob, not a constant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// importer → imported: mass flows toward dependencies (fan-in hubs).
    Forward,
    /// imported → importer: mass flows toward consumers (leaf feature files).
    Reverse,
    /// both orientations: domain-locality without a direction commitment.
    Undirected,
}

impl Direction {
    /// Parse the CLI spelling; unknown values fall open to `Undirected`.
    pub fn parse(s: &str) -> Direction {
        match s.trim().to_ascii_lowercase().as_str() {
            "forward" | "fwd" => Direction::Forward,
            "reverse" | "rev" => Direction::Reverse,
            _ => Direction::Undirected,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Direction::Forward => "forward",
            Direction::Reverse => "reverse",
            Direction::Undirected => "undirected",
        }
    }
}

/// How a matched term's personalization mass is set before the split across its
/// seeds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeedWeight {
    /// Corpus rarity (`specificity_x1024 / count`): the discriminative signal —
    /// a rare term outweighs a ubiquitous one.
    Idf,
    /// TF·IDF as stored (`specificity_x1024`): rewards high-count terms too.
    Specificity,
    /// Geometric mean of the two, `√(specificity · idf)` — the DEFAULT. Pure idf
    /// localizes a rare-term query (a broad term whose top anchor IS the target
    /// gets under-weighted); pure specificity nails a target that is its own top
    /// anchor (but lets a broad term flood). The geometric mean compresses the
    /// gap between a broad and a rare term, so a single weighting serves both
    /// query shapes — the measured resolution of that tension.
    Balanced,
    /// Every matched term weighs the same.
    Uniform,
}

impl SeedWeight {
    pub fn parse(s: &str) -> SeedWeight {
        match s.trim().to_ascii_lowercase().as_str() {
            "idf" => SeedWeight::Idf,
            "balanced" | "geo" => SeedWeight::Balanced,
            "uniform" | "flat" => SeedWeight::Uniform,
            // Specificity is the measured default (see the module note); an
            // unknown value falls open to it.
            _ => SeedWeight::Specificity,
        }
    }
    fn label(self) -> &'static str {
        match self {
            SeedWeight::Idf => "idf",
            SeedWeight::Specificity => "specificity",
            SeedWeight::Balanced => "balanced",
            SeedWeight::Uniform => "uniform",
        }
    }
}

/// Integer square root of a u128 (Newton's method) — float-free so the balanced
/// seed weight stays byte-stable.
fn isqrt(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Ranker configuration — every empirical knob the harness sweeps without a
/// rebuild.
pub struct RankConfig {
    pub direction: Direction,
    /// Damping ×1024 (classic PageRank ≈ 0.85 → 870). Lower keeps mass nearer
    /// the seeds (a stronger topic bias, less global-hub bleed).
    pub damping_x1024: u64,
    /// Fixed power-iteration count (byte-stability needs a FIXED bound, never a
    /// float convergence test).
    pub iterations: usize,
    /// How many ranked files to emit.
    pub top: usize,
    pub seed_weight: SeedWeight,
    /// When false, skip the random walk and rank on the personalization vector
    /// alone — the ablation that isolates the graph's contribution from the
    /// idf-weighted seeding.
    pub propagate: bool,
    /// Hub penalty ×1024 against a file's ANCHOR PROMISCUITY — how many distinct
    /// dictionary terms list it as an anchor. A comment-dense cross-cutting file
    /// (an error-code enum, an app config, an auth service) is a top-occurrence
    /// anchor for terms across every domain, so it pollutes the top of a query
    /// it has no real stake in; a domain file anchors few, related terms. The
    /// final score is divided by `1 + penalty·promiscuity` — a model-derived
    /// demotion (the count is read straight off the dictionary), never a
    /// hand-curated stop-list. `0` = off.
    pub hub_penalty_x1024: u64,
    /// Fan-in penalty ×1024 against a file's global import fan-in (from the
    /// model). Forward propagation piles mass onto the deep shared sinks every
    /// module depends on (an exceptions catalog, an enums bucket, a
    /// unit-of-work) — the highest-fan-in nodes — burying the mid-graph domain
    /// service the query actually wants. Dividing the final score by
    /// `1 + penalty·fan_in` is the standard PageRank-with-in-degree correction:
    /// a leaf/service (fan-in 0–2) is barely touched, a global sink (fan-in in
    /// the tens–hundreds) is pushed down. Model-derived; `0` = off.
    pub fanin_penalty_x1024: u64,
    /// UNGATE the seeding (Wave-2b fix): when true (default), each glue-filtered
    /// query token ALSO seeds — directly and WITHOUT the dictionary-membership
    /// gate — the eligible modules whose declaration/path identifiers contain it
    /// (prefix-or-equal on the folded token, min side ≥4). This is the English-
    /// gloss baseline's move: a domain word carried in by query expansion
    /// (`channel`, `bank`, `configuration`, `receivable`) reaches the file whose
    /// identifier declares it even when that word is NOT distinctive-dictionary
    /// vocabulary. The stopword/glue filter on the raw query tokens still applies,
    /// so only domain words seed — glue is never reintroduced.
    pub direct_seed: bool,
    /// Multiplier ×1024 on the ABSOLUTE direct identifier-match score (summed
    /// inverse-df of the matched query tokens, BM25 length-normalized). The floor
    /// `direct_base_x1024/1024 · match_score` is added AFTER and EXEMPT FROM the
    /// fan-in penalty, so a low-centrality target the query literally names (an EF
    /// configuration class, an enum) ranks on its match, not on graph propagation.
    /// Absolute (not per-query normalized): a weak single-broad-word match floors
    /// low and leaves the dict route intact; a multi-rare-word match floors high.
    /// Calibrated so a strong match competes with the top propagated mass; `0` =
    /// no floor (rank on the walk alone).
    pub direct_base_x1024: u64,
}

impl Default for RankConfig {
    /// The measured winner on the sialia raw-PT benchmark (Acc@5 = 6/13 = 46.2%,
    /// matching the PT+EN digest): specificity seeding, an UNDIRECTED walk (the
    /// import graph splits by language, so domain-locality is undirected),
    /// damping ≈ 0.60 (a strong topic bias keeps mass near the seed
    /// neighbourhood) and a fan-in penalty of 1.0 (demote the deep shared sinks
    /// a walk piles onto). A robust plateau — Acc@5 holds across damping 0.58–
    /// 0.62 and fan-in 0.87–1.13, and 50 iterations already converge.
    fn default() -> Self {
        RankConfig {
            direction: Direction::Undirected,
            damping_x1024: 614,
            iterations: 50,
            top: 10,
            seed_weight: SeedWeight::Specificity,
            propagate: true,
            hub_penalty_x1024: 0,
            fanin_penalty_x1024: 1024,
            direct_seed: true,
            direct_base_x1024: 100_000,
        }
    }
}

/// One ranked file (byte-stable score for the audit trail).
#[derive(Serialize)]
pub struct ScoredFile {
    pub file: String,
    /// PageRank mass ×1024 relative to the total (`r_i * 1024 / PR_SCALE`).
    pub score_x1024: u64,
    /// ADDITIVE per-file evidence: the matched dictionary terms and the direct
    /// identifier query tokens that seeded this file (sorted asc, deduped —
    /// byte-stable). Empty when only graph propagation carried the file, and
    /// omitted from the JSON then, so pre-existing consumers read unchanged
    /// rows.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub terms: Vec<String>,
}

/// One query term the request bridged to, with the rarity it seeded at and how
/// many eligible model files it resolved to — the localization audit.
#[derive(Serialize)]
pub struct MatchedTerm {
    pub term: String,
    pub idf_x1024: u64,
    pub specificity_x1024: u64,
    pub seeds: usize,
}

/// The byte-stable ranker result.
#[derive(Serialize)]
pub struct RankResult {
    /// The query tokens actually searched (glue-filtered, deduped).
    pub query: Vec<String>,
    /// Ranker settings echoed back so a result is reproducible from the JSON.
    pub direction: String,
    pub seed_weight: String,
    pub damping_x1024: u64,
    pub iterations: usize,
    pub propagated: bool,
    /// Matched dictionary terms, rarest (highest idf) first.
    pub matched_terms: Vec<MatchedTerm>,
    /// Distinct eligible model files that carried any personalization mass.
    pub seed_files: usize,
    /// Files ranked by PageRank score DESC (path ASC tiebreak), capped at `top`.
    pub files: Vec<ScoredFile>,
}

/// Rank the model's files for `query_terms` (a raw request, e.g. Portuguese),
/// seeded by `dict`. Pure given its inputs and the embedded ladder data;
/// deterministic and fail-open (see the module note).
pub fn rank(model: &ProjectModel, dict: &Dictionary, query_terms: &[String], cfg: &RankConfig) -> RankResult {
    let n = model.modules.len();
    let empty = |ql: Vec<String>| RankResult {
        query: ql,
        direction: cfg.direction.label().to_string(),
        seed_weight: cfg.seed_weight.label().to_string(),
        damping_x1024: cfg.damping_x1024,
        iterations: cfg.iterations,
        propagated: cfg.propagate,
        matched_terms: Vec::new(),
        seed_files: 0,
        files: Vec::new(),
    };

    // Query tokens: split on non-alphanumerics, lowercase, floor 3 chars, drop
    // identifier glue AND PT+EN natural-language glue, order-preserving dedup —
    // the same shape the digest query and dict-lookup use, so the PT→term bridge
    // is identical.
    let ql = query_tokens(query_terms);
    if ql.is_empty() || n == 0 {
        return empty(ql);
    }

    // Per-module eligibility: only a hand-written, non-test module may seed or
    // rank (a machine-written or test file is never the file to read/edit) —
    // the generated-demotion the task requires, applied BEFORE ranking.
    let eligible: Vec<bool> = model
        .modules
        .iter()
        .map(|m| crate::classify::anchor_eligible(&m.file_class) && !mustard_core::domain::ast::is_test_path(&m.path))
        .collect();
    let pos: BTreeMap<&str, usize> = model.modules.iter().enumerate().map(|(i, m)| (m.path.as_str(), i)).collect();

    // Anchor promiscuity per module: how many distinct dictionary terms list it
    // as an anchor. Read straight off the dictionary — the hub-penalty signal.
    let mut promisc_by_path: BTreeMap<&str, u64> = BTreeMap::new();
    for e in &dict.terms {
        for a in &e.anchors {
            *promisc_by_path.entry(a.as_str()).or_insert(0) += 1;
        }
    }
    let promisc: Vec<u64> = model.modules.iter().map(|m| promisc_by_path.get(m.path.as_str()).copied().unwrap_or(0)).collect();
    // FINAL-score demotion of the two model-derived hub signals — anchor
    // promiscuity (comment-dense cross-cutting files) and global import fan-in
    // (deep shared sinks a forward walk piles onto). Additive in the divisor:
    // `mass · SCALE / (SCALE + hub·promiscuity + fanin·fan_in)`. A no-op when
    // both penalties are off; applied to the SCORE only (never the seeds), so it
    // re-ranks the already-scored candidates instead of starving propagation.
    let hub_k = cfg.hub_penalty_x1024 as u128;
    let fanin_k = cfg.fanin_penalty_x1024 as u128;
    let demote = |mass: u128, i: usize| -> u128 {
        if hub_k == 0 && fanin_k == 0 {
            return mass;
        }
        mass * SCALE / (SCALE + hub_k * promisc[i] as u128 + fanin_k * model.modules[i].fan_in as u128)
    };

    // Model seed indices: folded declaration-name AND path tokens → the eligible
    // modules carrying them. An English term ("aging", "zod") resolves to real
    // declarations here; a PT comment-term ("contrato") resolves to nothing and
    // falls to its dictionary anchors below.
    let mut token_seeds: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    // Distinct identifier-token count per eligible module — the "document length"
    // for the direct-match BM25 length normalization below (a short, focused file
    // that matches the query's rare words must beat a huge file that declares
    // everything and so matches many words incidentally).
    let mut tok_count: Vec<u128> = vec![0; n];
    for (i, m) in model.modules.iter().enumerate() {
        if !eligible[i] {
            continue;
        }
        let mut folds: BTreeSet<String> = BTreeSet::new();
        for tok in crate::digest::tokenize(&m.path) {
            let f = fold(&tok);
            if f.len() >= 3 {
                folds.insert(f);
            }
        }
        for d in &m.declarations {
            for tok in crate::digest::tokenize(&d.name) {
                let f = fold(&tok);
                if f.len() >= 3 {
                    folds.insert(f);
                }
            }
        }
        tok_count[i] = folds.len() as u128;
        for f in folds {
            token_seeds.entry(f).or_default().insert(i);
        }
    }

    // Match each dictionary term against the query (fold-equality OR folded
    // prefix, min side ≥4), collect the matched terms with their idf/specificity
    // and resolved seed set. A term seeds the UNION of its model-token seeds and
    // its dictionary anchors, restricted to eligible modules.
    let qfolds: Vec<String> = ql.iter().map(|q| fold(q)).collect();
    struct Matched {
        term: String,
        idf_x1024: u64,
        specificity_x1024: u64,
        weight: u128,
        seeds: BTreeSet<usize>,
    }
    let mut matched: Vec<Matched> = Vec::new();
    for e in &dict.terms {
        let ft = fold(&e.term);
        if ft.len() < 3 || !qfolds.iter().any(|q| term_matches(q, &ft)) {
            continue;
        }
        // idf recovered from the stored TF·IDF: specificity = count · idf.
        let idf_x1024 = if e.count > 0 { e.specificity_x1024 / e.count as u64 } else { 0 };
        let mut seeds: BTreeSet<usize> = BTreeSet::new();
        if let Some(model_seeds) = token_seeds.get(&ft) {
            seeds.extend(model_seeds.iter().copied());
        }
        for a in &e.anchors {
            if let Some(&i) = pos.get(a.replace('\\', "/").as_str()) {
                if eligible[i] {
                    seeds.insert(i);
                }
            }
        }
        if seeds.is_empty() {
            continue;
        }
        let weight = match cfg.seed_weight {
            SeedWeight::Idf => idf_x1024 as u128,
            SeedWeight::Specificity => e.specificity_x1024 as u128,
            SeedWeight::Balanced => isqrt(e.specificity_x1024 as u128 * idf_x1024 as u128),
            SeedWeight::Uniform => SCALE,
        }
        .max(1);
        matched.push(Matched { term: e.term.clone(), idf_x1024, specificity_x1024: e.specificity_x1024, weight, seeds });
    }
    // Per-file term evidence (the ADDITIVE `files[].terms` audit): which
    // matched dictionary terms — and, below, which direct identifier query
    // tokens — seeded each file. BTreeSet keeps each list sorted + deduped
    // (byte-stable); a file only graph propagation carries stays absent
    // (empty evidence, honestly).
    let mut evidence: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for m in &matched {
        for &i in &m.seeds {
            evidence.entry(i).or_default().insert(m.term.clone());
        }
    }

    // Personalization vector: each matched term contributes its weight SPLIT
    // across its seeds — so a rare term (few seeds) concentrates mass on the
    // files that carry it, while a ubiquitous term spreads thin. Accumulated at
    // SEED_UNIT resolution, then normalized once to PR_SCALE.
    let mut acc: Vec<u128> = vec![0; n];
    for m in &matched {
        let per_seed = (m.weight * SEED_UNIT) / m.seeds.len() as u128;
        for &i in &m.seeds {
            acc[i] += per_seed;
        }
    }

    // UNGATED direct-identifier MATCH SCORE (the Wave-2b fix). For every
    // glue-filtered query token, find the eligible modules whose declaration/path
    // identifiers contain it (prefix-or-equal fold, the SAME `term_matches` rung),
    // with NO dictionary-membership gate, and add that token's inverse-df weight
    // (idf — a rare identifier token counts far more than a ubiquitous one) to
    // each such module. Summing across tokens gives a BM25-lite match QUALITY: a
    // file that matches MANY RARE query words scores high, the exact
    // discrimination the English-gloss baseline uses to win the English-identifier
    // targets (id7 `channel`+`sales`+`hook`, id8 `bank`+`approval`, id14
    // `receivable`+`configuration`). It never touches the dict route's
    // propagation `acc`, so the graph walk is unchanged — this is a pure,
    // fan-in-EXEMPT base floor. Deterministic: sorted `qfolds` × sorted `token_seeds`.
    let mut direct_score: Vec<u128> = vec![0; n];
    if cfg.direct_seed {
        let n_elig = (eligible.iter().filter(|&&e| e).count() as u128).max(1);
        for (qi, qf) in qfolds.iter().enumerate() {
            if qf.len() < 3 {
                continue;
            }
            let mut seeds_q: BTreeSet<usize> = BTreeSet::new();
            for (tk, mods) in &token_seeds {
                if term_matches(qf, tk) {
                    seeds_q.extend(mods.iter().copied());
                }
            }
            if seeds_q.is_empty() {
                continue;
            }
            let idf = ((n_elig * SCALE) / seeds_q.len() as u128).max(1);
            for &i in &seeds_q {
                direct_score[i] += idf;
                // The ORIGINAL query token (qfolds[qi] folds ql[qi]) is the
                // evidence a direct identifier match leaves on the file.
                if let Some(tok) = ql.get(qi) {
                    evidence.entry(i).or_default().insert(tok.clone());
                }
            }
        }
        // BM25 length normalization (b ≈ 0.75): divide each file's summed idf by
        // `(1−b) + b·len/avglen`, so a short focused match outranks a long file
        // that matched many words only by declaring everything. Integer, ×SCALE.
        let sum_len: u128 = tok_count.iter().sum();
        let avg_len = (sum_len / n_elig).max(1);
        const B: u128 = 768; // 0.75 × SCALE
        for i in 0..n {
            if direct_score[i] > 0 {
                let denom = (SCALE - B) + B * tok_count[i] / avg_len;
                direct_score[i] = direct_score[i] * SCALE / denom.max(1);
            }
        }
    }

    let total: u128 = acc.iter().sum();
    let max_ds = direct_score.iter().copied().max().unwrap_or(0);
    if total == 0 && max_ds == 0 {
        return empty(ql);
    }
    // Dict-route personalization on the PR scale (UNCHANGED — the walk seeds it).
    let p: Vec<u128> = if total > 0 { acc.iter().map(|&a| a * PR_SCALE / total).collect() } else { vec![0; n] };
    // The direct-match floor is the ABSOLUTE length-normed match score (NOT
    // max-normalized per query): a query whose best identifier match is a single
    // broad word yields a SMALL floor that leaves the dict-route ranking intact,
    // while a query with several RARE identifier matches (id7 sales+channel+hook,
    // id8 bank+approval+status) yields a LARGE floor that surfaces its target.
    // (`max_ds`, above, only gates the all-empty case.)
    let seed_files = acc.iter().zip(&direct_score).filter(|(&a, &d)| a > 0 || d > 0).count();

    // Rank the personalization directly (ablation) or after the random walk.
    let r = if cfg.propagate && total > 0 {
        let (out, outw) = adjacency(&model.modules, cfg.direction, &eligible);
        power_iterate(&p, &out, &outw, cfg.damping_x1024 as u128, cfg.iterations)
    } else {
        p
    };

    // Order eligible files by score DESC, path ASC (byte-stable), cap at `top`.
    // Score = fan-in-penalized propagated mass  +  the direct-match base floor.
    // The fan-in penalty stays on PROPAGATION only; a file the query literally
    // names keeps its own match floor (`base_k · floor`) so a low-centrality
    // target (EF config, enum) ranks on its match, not on graph centrality.
    let base_k = cfg.direct_base_x1024 as u128;
    let mut ranked: Vec<(usize, u128)> =
        (0..n).filter(|&i| eligible[i] && (r[i] > 0 || direct_score[i] > 0)).map(|i| (i, demote(r[i], i) + direct_score[i] * base_k / SCALE)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(model.modules[a.0].path.cmp(&model.modules[b.0].path)));
    ranked.truncate(cfg.top);
    let files = ranked
        .into_iter()
        .map(|(i, mass)| ScoredFile {
            file: model.modules[i].path.clone(),
            score_x1024: (mass * SCALE / PR_SCALE) as u64,
            terms: evidence.get(&i).map(|s| s.iter().cloned().collect()).unwrap_or_default(),
        })
        .collect();

    // Matched terms rarest-first (idf desc, term asc) for the audit.
    let mut matched_terms: Vec<MatchedTerm> = matched
        .iter()
        .map(|m| MatchedTerm { term: m.term.clone(), idf_x1024: m.idf_x1024, specificity_x1024: m.specificity_x1024, seeds: m.seeds.len() })
        .collect();
    matched_terms.sort_by(|a, b| b.idf_x1024.cmp(&a.idf_x1024).then(a.term.cmp(&b.term)));

    RankResult {
        query: ql,
        direction: cfg.direction.label().to_string(),
        seed_weight: cfg.seed_weight.label().to_string(),
        damping_x1024: cfg.damping_x1024,
        iterations: cfg.iterations,
        propagated: cfg.propagate,
        matched_terms,
        seed_files,
        files,
    }
}

/// Prepare the request's query tokens: split on non-alphanumerics, lowercase,
/// floor 3 chars, drop identifier glue (`stopwords.toml`) and PT+EN
/// natural-language glue (vendored Snowball stoplists), order-preserving dedup.
fn query_tokens(terms: &[String]) -> Vec<String> {
    let ident_glue = crate::digest::stopwords();
    let nl_glue = crate::dictionary::natural_language_glue();
    let mut seen = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in terms {
        for tok in raw.split(|c: char| !c.is_alphanumeric()) {
            let t = tok.to_lowercase();
            if t.len() < 3 || !t.chars().any(|c| c.is_alphabetic()) {
                continue;
            }
            let f = fold(&t);
            if ident_glue.contains(&t) || nl_glue.contains(&t) || nl_glue.contains(&f) {
                continue;
            }
            if seen.insert(t.clone()) {
                out.push(t);
            }
        }
    }
    out
}

/// One query token matches one dictionary term (both already folded) when they
/// are equal OR one is a prefix of the other with the SHORTER side ≥4 chars —
/// the rungs `dict-lookup.ps1` validated at 13/13 domain recall. The ≥4 floor is
/// what keeps a 3-letter glue fragment from prefix-matching a real term.
fn term_matches(q: &str, t: &str) -> bool {
    if q == t {
        return true;
    }
    let shorter = q.len().min(t.len());
    shorter >= 4 && (t.starts_with(q) || q.starts_with(t))
}

/// Build the walk adjacency (out-neighbours + summed out-weight per node) for
/// `direction`, over the resolved import edges. Both endpoints of a kept edge
/// must be eligible — machine-written / test nodes carry no mass and route
/// none, the generated-demotion applied to the graph itself. `outw[j] == 0`
/// marks a dangling node (its mass teleports).
fn adjacency(modules: &[crate::model::Module], direction: Direction, eligible: &[bool]) -> (Vec<Vec<(usize, u64)>>, Vec<u128>) {
    let n = modules.len();
    let edges = crate::graph::resolve_edges(modules, &None);
    let mut out: Vec<Vec<(usize, u64)>> = vec![Vec::new(); n];
    let mut push = |from: usize, to: usize, w: u64| {
        if eligible[from] && eligible[to] {
            out[from].push((to, w));
        }
    };
    for (a, b, w) in edges {
        match direction {
            Direction::Forward => push(a, b, w),
            Direction::Reverse => push(b, a, w),
            Direction::Undirected => {
                push(a, b, w);
                push(b, a, w);
            }
        }
    }
    let outw: Vec<u128> = out.iter().map(|es| es.iter().map(|&(_, w)| w as u128).sum()).collect();
    (out, outw)
}

/// Personalized-PageRank power iteration in u128 fixed point. `p` is the
/// personalization/teleport vector (Σ = `PR_SCALE`); the walk restarts to `p`
/// with probability `1 − damping`, and a dangling node's mass is redistributed
/// via `p` so total mass is conserved (modulo integer-division leakage, which is
/// deterministic and irrelevant to the ordering). Fixed `iterations`, no float.
fn power_iterate(p: &[u128], out: &[Vec<(usize, u64)>], outw: &[u128], damping_x1024: u128, iterations: usize) -> Vec<u128> {
    let n = p.len();
    let d = damping_x1024.min(SCALE);
    let mut r = p.to_vec();
    for _ in 0..iterations {
        let mut inflow = vec![0u128; n];
        let mut dangling = 0u128;
        for j in 0..n {
            if outw[j] == 0 {
                dangling += r[j];
                continue;
            }
            let rj = r[j];
            for &(i, w) in &out[j] {
                inflow[i] += rj * w as u128 / outw[j];
            }
        }
        // Teleport coefficient folds the restart term and the dangling
        // redistribution, both proportional to `p`:
        //   base_i = p_i · ((1−d)·PR_SCALE + d·dangling) / (SCALE·PR_SCALE)
        let teleport_coeff = (SCALE - d) * PR_SCALE + d * dangling;
        let denom = SCALE * PR_SCALE;
        for i in 0..n {
            r[i] = p[i] * teleport_coeff / denom + d * inflow[i] / SCALE;
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::DictEntry;
    use crate::model::{Decl, Module};

    fn decl(name: &str) -> Decl {
        Decl { kind: "function".to_string(), name: name.to_string(), ..Default::default() }
    }

    /// A hand-written module (empty class → eligible) with declarations + imports.
    fn module(path: &str, decls: &[&str], imports: &[&str]) -> Module {
        Module {
            path: path.to_string(),
            language: "typescript".to_string(),
            imports: imports.iter().map(|s| s.to_string()).collect(),
            declarations: decls.iter().map(|d| decl(d)).collect(),
            ..Default::default()
        }
    }

    fn entry(term: &str, count: usize, df: usize, spec: u64, anchors: &[&str], source: &str) -> DictEntry {
        DictEntry {
            term: term.to_string(),
            specificity_x1024: spec,
            count,
            df,
            anchors: anchors.iter().map(|s| s.to_string()).collect(),
            source: source.to_string(),
        }
    }

    fn ranked_files(r: &RankResult) -> Vec<&str> {
        r.files.iter().map(|f| f.file.as_str()).collect()
    }

    /// SEEDING — a query bridges to a dictionary term, which resolves to the
    /// modules that DECLARE it (model seed) plus the term's anchors, and those
    /// files rank; an unrelated file does not.
    #[test]
    fn seeds_from_model_declarations_and_anchors() {
        let model = ProjectModel {
            modules: vec![
                module("src/payables/aging-bar.tsx", &["AgingBar"], &[]),
                module("src/unrelated/widget.tsx", &["Widget"], &[]),
            ],
            ..Default::default()
        };
        // "aging" is an identifier token of aging-bar.tsx → model seed.
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("aging", 4, 2, 2048, &[], "both")] };
        let cfg = RankConfig { propagate: false, ..Default::default() };
        let r = rank(&model, &dict, &["ajustar o aging".to_string()], &cfg);
        assert_eq!(ranked_files(&r), vec!["src/payables/aging-bar.tsx"], "the declaring file seeds and ranks; the widget does not");
        assert_eq!(r.matched_terms.len(), 1);
        assert_eq!(r.matched_terms[0].term, "aging");
    }

    /// A PT comment-term whose identifiers are English resolves through its
    /// stored anchors (its only seeds), not model declarations.
    #[test]
    fn seeds_a_portuguese_comment_term_via_anchors() {
        let model = ProjectModel {
            modules: vec![
                module("src/contracts/form-context.tsx", &["ContractForm"], &[]),
                module("src/other/thing.tsx", &["Thing"], &[]),
            ],
            ..Default::default()
        };
        // "contrato" appears in NO identifier; its anchor is the contract file.
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("contrato", 10, 3, 3072, &["src/contracts/form-context.tsx"], "comment")] };
        let cfg = RankConfig { propagate: false, ..Default::default() };
        let r = rank(&model, &dict, &["campo no contrato".to_string()], &cfg);
        assert_eq!(ranked_files(&r), vec!["src/contracts/form-context.tsx"], "PT term localizes through its anchor");
    }

    /// GENERATED DEMOTION — a machine-written module never seeds and never
    /// ranks, even when it declares the term AND is import-central.
    #[test]
    fn generated_module_never_seeds_or_ranks() {
        let mut gen = module("src/generated/contracts.g.ts", &["Contract"], &[]);
        gen.file_class = "generated".to_string();
        let model = ProjectModel {
            modules: vec![gen, module("src/contracts/service.ts", &["ContractService"], &["src/generated/contracts.g.ts"])],
            ..Default::default()
        };
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("contract", 6, 2, 2048, &["src/generated/contracts.g.ts"], "both")] };
        let cfg = RankConfig { propagate: true, ..Default::default() };
        let r = rank(&model, &dict, &["contract".to_string()], &cfg);
        assert!(!ranked_files(&r).iter().any(|f| f.contains("generated")), "generated file demoted from ranking: {:?}", ranked_files(&r));
    }

    /// PROPAGATION — with a single seed, forward PageRank moves mass along the
    /// import edge onto the imported dependency, so a graph-central file the
    /// query never named still surfaces (the localization the flat anchor sum
    /// cannot do).
    #[test]
    fn propagation_lifts_a_graph_central_neighbour() {
        let model = ProjectModel {
            modules: vec![
                module("src/ui/form.tsx", &["Form"], &["src/core/schema.ts"]),
                module("src/core/schema.ts", &["Schema"], &[]),
            ],
            ..Default::default()
        };
        // Only the form is seeded; the schema it imports is never named. Fan-in
        // penalty off to isolate propagation from the demotion.
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("form", 4, 1, 4096, &["src/ui/form.tsx"], "both")] };
        let seeded = rank(&model, &dict, &["form".to_string()], &RankConfig { propagate: false, fanin_penalty_x1024: 0, ..Default::default() });
        assert_eq!(ranked_files(&seeded), vec!["src/ui/form.tsx"], "seed-only ranks only the seed");
        let walked =
            rank(&model, &dict, &["form".to_string()], &RankConfig { direction: Direction::Forward, propagate: true, fanin_penalty_x1024: 0, ..Default::default() });
        assert!(ranked_files(&walked).contains(&"src/core/schema.ts"), "forward walk surfaces the imported neighbour: {:?}", ranked_files(&walked));
    }

    /// EVIDENCE — each ranked row carries the ADDITIVE `terms` audit: the
    /// dictionary terms and direct query tokens that seeded it, sorted and
    /// deduped; a propagation-only file carries an empty list (omitted from
    /// the JSON so pre-existing consumers read unchanged rows).
    #[test]
    fn ranked_rows_carry_term_evidence() {
        let model = ProjectModel {
            modules: vec![
                module("src/payables/aging-bar.tsx", &["AgingBar"], &["src/core/schema.ts"]),
                module("src/core/schema.ts", &["Schema"], &[]),
            ],
            ..Default::default()
        };
        // "aging" seeds the bar twice over (the dict term resolves to its
        // declaration AND the raw token direct-matches the identifier — dedup
        // keeps one); "bar" lands as a direct identifier token only.
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("aging", 4, 2, 2048, &[], "both")] };
        let walked = rank(
            &model,
            &dict,
            &["ajustar o aging bar".to_string()],
            &RankConfig { direction: Direction::Forward, fanin_penalty_x1024: 0, ..Default::default() },
        );
        let bar = walked.files.iter().find(|f| f.file.contains("aging-bar")).expect("seeded file ranks");
        assert_eq!(bar.terms, vec!["aging".to_string(), "bar".to_string()], "dict term + direct tokens, sorted+deduped: {:?}", bar.terms);
        let schema = walked.files.iter().find(|f| f.file.contains("schema"));
        if let Some(s) = schema {
            assert!(s.terms.is_empty(), "propagation-only file carries no term evidence: {:?}", s.terms);
        }
        // JSON: empty evidence is OMITTED (unchanged rows for old consumers).
        let json = serde_json::to_string(&walked).expect("serialize");
        assert!(json.contains("\"terms\":[\"aging\",\"bar\"]"), "evidence serialized: {json}");
        assert!(!json.contains("\"terms\":[]"), "empty evidence omitted: {json}");
    }

    /// DETERMINISM — two runs over the same inputs serialize to identical bytes.
    #[test]
    fn is_byte_stable() {
        let model = ProjectModel {
            modules: vec![
                module("src/a/one.ts", &["Alpha"], &["src/a/two.ts"]),
                module("src/a/two.ts", &["Beta"], &["src/a/one.ts"]),
                module("src/b/three.ts", &["Gamma"], &["src/a/one.ts"]),
            ],
            ..Default::default()
        };
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("alpha", 3, 1, 3072, &["src/a/one.ts"], "both"), entry("gamma", 2, 1, 2048, &["src/b/three.ts"], "both")] };
        let cfg = RankConfig::default();
        let a = serde_json::to_string(&rank(&model, &dict, &["alpha gamma".to_string()], &cfg)).expect("serialize");
        let b = serde_json::to_string(&rank(&model, &dict, &["alpha gamma".to_string()], &cfg)).expect("serialize");
        assert_eq!(a, b, "two runs are byte-identical");
    }

    /// FAIL-OPEN — no matched term, an all-glue query and an empty model each
    /// yield an empty ranked list, never a panic.
    #[test]
    fn fails_open() {
        let model = ProjectModel { modules: vec![module("src/a.ts", &["Alpha"], &[])], ..Default::default() };
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("alpha", 2, 1, 2048, &["src/a.ts"], "both")] };
        let cfg = RankConfig::default();
        // Nothing in the dictionary matches.
        assert!(rank(&model, &dict, &["zzzznomatch".to_string()], &cfg).files.is_empty(), "no bridge → empty");
        // Query is pure glue (PT + EN function words).
        assert!(rank(&model, &dict, &["de para com the and".to_string()], &cfg).files.is_empty(), "all-glue → empty");
        // Empty model.
        let none = rank(&ProjectModel::default(), &dict, &["alpha".to_string()], &cfg);
        assert!(none.files.is_empty(), "empty model → empty");
    }

    /// FAN-IN PENALTY — two files seeded equally (equal personalization mass);
    /// the one with high global fan-in (a deep shared sink) is demoted below the
    /// leaf. The path tiebreak is set so the sink would lead WITHOUT the penalty,
    /// isolating the demotion's effect.
    #[test]
    fn fanin_penalty_demotes_a_global_sink() {
        let mut sink = module("src/aaa_sink.ts", &["Sink"], &[]);
        sink.fan_in = 40; // a deep shared sink — imported everywhere
        let service = module("src/zzz_service.ts", &["Service"], &[]); // leaf, fan_in 0
        let model = ProjectModel { modules: vec![sink, service], ..Default::default() };
        // "alpha" anchors both equally; neither declares it (seeded via anchors).
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("alpha", 4, 2, 2048, &["src/aaa_sink.ts", "src/zzz_service.ts"], "comment")] };
        let off = rank(&model, &dict, &["alpha".to_string()], &RankConfig { propagate: false, fanin_penalty_x1024: 0, ..Default::default() });
        assert_eq!(off.files.first().map(|f| f.file.as_str()), Some("src/aaa_sink.ts"), "no penalty: equal mass, path-asc → sink leads: {:?}", ranked_files(&off));
        let on = rank(&model, &dict, &["alpha".to_string()], &RankConfig { propagate: false, fanin_penalty_x1024: 1024, ..Default::default() });
        assert_eq!(on.files.first().map(|f| f.file.as_str()), Some("src/zzz_service.ts"), "penalty demotes the fan-in-40 sink below the leaf: {:?}", ranked_files(&on));
    }

    /// The integer square root under the `balanced` weight is exact on perfect
    /// squares and floors between — the float-free primitive keeps that weight
    /// byte-stable.
    #[test]
    fn isqrt_is_exact_on_squares_and_floors_between() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(2), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(1_000_000), 1000);
        assert_eq!(isqrt(999_999), 999);
    }

    /// IDF WEIGHTING — a rare term (high idf, one seed) outweighs a ubiquitous
    /// term (low idf) that seeds the same query, so the rare term's file leads —
    /// the fix for the anchor-sum bug where a broad high-count term dominated.
    #[test]
    fn idf_weighting_favours_the_rare_term_file() {
        let mut modules = vec![
            module("src/rare/desdobramento.ts", &["Xqz"], &[]),
            module("src/common/conta.ts", &["Yqz"], &[]),
        ];
        // A large corpus so the ubiquitous term's idf really is small.
        for i in 0..40 {
            modules.push(module(&format!("src/f/f{i}.ts"), &[&format!("F{i}")], &[]));
        }
        let model = ProjectModel { modules, ..Default::default() };
        // "conta": high count, high df → low idf (spec/count small).
        // "desdobramento": low count, df 1 → high idf.
        let dict = Dictionary {
            version: 1,
            non_english_comments: 0,
            terms: vec![
                entry("conta", 200, 40, 200_000, &["src/common/conta.ts"], "comment"),
                entry("desdobramento", 3, 1, 30_000, &["src/rare/desdobramento.ts"], "comment"),
            ],
        };
        let cfg = RankConfig { propagate: false, seed_weight: SeedWeight::Idf, ..Default::default() };
        let r = rank(&model, &dict, &["desdobramento de conta".to_string()], &cfg);
        assert_eq!(r.files.first().map(|f| f.file.as_str()), Some("src/rare/desdobramento.ts"), "rare-term file leads under idf weighting: {:?}", ranked_files(&r));
        // And the matched-term audit is rarest-first.
        assert_eq!(r.matched_terms.first().map(|m| m.term.as_str()), Some("desdobramento"));
    }

    /// UNGATED DIRECT SEEDING (Wave-2b fix) — an English query token that is NOT
    /// a dictionary term still seeds the file whose IDENTIFIER contains it, and
    /// the direct-match base floor lifts that file above a high-fan-in hub the
    /// walk would otherwise flood. With the gate restored (`direct_seed=false`)
    /// the same word seeds nothing, reproducing the pre-fix miss.
    #[test]
    fn ungated_query_token_seeds_by_identifier_and_floors_low_centrality() {
        let mut hub = module("src/aaa_hub.ts", &["Hub"], &[]);
        hub.fan_in = 30; // a central hub that would dominate propagation + tiebreak
        let target = module("src/features/sales-channel-list.ts", &["useSalesChannelList"], &["src/aaa_hub.ts"]);
        let model = ProjectModel { modules: vec![hub, target], ..Default::default() };
        // The dictionary bridges ONLY an unrelated term; `channel` is absent.
        let dict = Dictionary { version: 1, non_english_comments: 0, terms: vec![entry("hub", 100, 50, 2048, &["src/aaa_hub.ts"], "comment")] };

        // Gate ON (default): the English identifier token seeds its file directly
        // and its base floor surfaces it despite the hub's fan-in-30 centrality.
        let on = rank(&model, &dict, &["listar os canais channel".to_string()], &RankConfig::default());
        assert!(ranked_files(&on).contains(&"src/features/sales-channel-list.ts"), "ungated: the identifier match seeds and ranks: {:?}", ranked_files(&on));

        // Gate OFF: `channel` matches no dictionary term → the file is never seeded.
        let off = rank(&model, &dict, &["listar os canais channel".to_string()], &RankConfig { direct_seed: false, ..Default::default() });
        assert!(!ranked_files(&off).contains(&"src/features/sales-channel-list.ts"), "gated: no dict term for 'channel' -> not seeded: {:?}", ranked_files(&off));
    }
}
