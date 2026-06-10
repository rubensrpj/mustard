//! rank — deterministic relevance scoring for the digest projections.
//!
//! One module, one responsibility (mirrors classify.rs / graph.rs): the
//! arithmetic that orders term samples and anchor candidates. BM25 over the
//! term index (tf saturation + document-length normalization) plus the two
//! structural anchor signals — a small fan-in tiebreak and the stop-file
//! cutoff. The corpus itself (postings, document lengths) is built and owned
//! by `digest`; this module only scores.
//!
//! All arithmetic is fixed-point integer (scores ×1024): floats never enter a
//! comparison, so every ranking is byte-stable across runs and platforms.
//! k1 / b / alpha / the stop-file percent are DATA in `ranking.toml`
//! (embedded at compile time, same contract as stopwords.toml and
//! generated-markers.toml) — tuning relevance is a data change, never a
//! logic change here. Nothing in this module knows a language, framework or
//! file name: every signal is a statistic of the scanned repo itself.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Fixed-point scale: scores and ratios carry 10 fractional bits.
const SCALE: u64 = 1024;

struct Params {
    /// BM25 k1 ×1024 — term-frequency saturation.
    k1_x1024: u64,
    /// BM25 b ×1024, clamped to [0, SCALE] — length-normalization strength.
    b_x1024: u64,
    /// Anchor fan-in tiebreak weight α ×1024 — small by contract.
    alpha_x1024: u64,
    /// Stop-file cutoff: fan-in above this percent of the total module count
    /// leaves anchor eligibility.
    hub_fanin_pct: u64,
    /// Published-catalog weight ×1024 of one TYPE-kind occurrence.
    type_weight_x1024: u64,
    /// Published-catalog weight ×1024 of one member-kind occurrence.
    member_weight_x1024: u64,
    /// Capture kinds (the generic `@definition.<kind>` suffixes) ranked as
    /// types — DATA from ranking.toml, never named in code.
    type_kinds: BTreeSet<String>,
    /// MMR λ ×1024, clamped to [0, SCALE] — relevance vs diversity.
    mmr_lambda_x1024: u64,
    /// MMR pool bound: top (pool_per_slot × slots) candidates compete.
    mmr_pool_per_slot: usize,
}

/// Parsed once per process. A malformed embedded file is a programmer error
/// caught by any test run — same contract as `digest::stopwords` over
/// stopwords.toml and `classify::catalog` over generated-markers.toml.
fn params() -> &'static Params {
    static P: OnceLock<Params> = OnceLock::new();
    P.get_or_init(|| parse_params(include_str!("../ranking.toml")))
}

fn parse_params(src: &str) -> Params {
    let v: toml::Value = toml::from_str(src).expect("ranking.toml is not valid TOML");
    // TOML floats are converted to fixed point HERE, once, at load — after
    // this every computation is integer-only. `as u64` saturates a negative
    // to 0, so a bad sign can demote a signal but never wrap or panic.
    let fx = |val: Option<&toml::Value>, default: f64| -> u64 {
        let f = val.and_then(|x| x.as_float()).unwrap_or(default);
        (f * SCALE as f64).round() as u64
    };
    Params {
        k1_x1024: fx(v.get("bm25").and_then(|t| t.get("k1")), 1.2),
        b_x1024: fx(v.get("bm25").and_then(|t| t.get("b")), 0.75).min(SCALE),
        alpha_x1024: fx(v.get("anchors").and_then(|t| t.get("alpha")), 0.03125),
        hub_fanin_pct: v
            .get("anchors")
            .and_then(|t| t.get("hub_fanin_pct"))
            .and_then(|x| x.as_integer())
            .map(|n| n.max(0) as u64)
            .unwrap_or(30),
        type_weight_x1024: fx(v.get("catalog").and_then(|t| t.get("type_weight")), 2.5),
        member_weight_x1024: fx(v.get("catalog").and_then(|t| t.get("member_weight")), 1.0),
        type_kinds: v
            .get("catalog")
            .and_then(|t| t.get("type_kinds"))
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .filter_map(|k| k.as_str().map(|s| s.to_lowercase()))
            .collect(),
        mmr_lambda_x1024: fx(v.get("samples").and_then(|t| t.get("mmr_lambda")), 0.75).min(SCALE),
        mmr_pool_per_slot: v
            .get("samples")
            .and_then(|t| t.get("mmr_pool_per_slot"))
            .and_then(|x| x.as_integer())
            .map(|n| n.max(1) as usize)
            .unwrap_or(8),
    }
}

/// Average document length ×1024 over an indexed corpus of `docs` documents
/// totalling `total_len` declarations. Floored at 1 so it can always divide.
pub fn avgdl_x1024(total_len: usize, docs: usize) -> u64 {
    if docs == 0 {
        return SCALE;
    }
    ((total_len as u64 * SCALE) / docs as u64).max(1)
}

/// BM25 score ×1024 for `tf` occurrences of a term in a document of length
/// `dl`, against the corpus average `avgdl_x1024`. Classic Okapi shape
/// WITHOUT the IDF factor: every comparison made with this score holds the
/// term set fixed (modules competing FOR one term, or candidates summed over
/// the same matched terms), so the per-term IDF constant cancels — rarity is
/// already the explicit matched-term order in `digest::query`. Pure integer
/// arithmetic; ties are broken by the caller (path asc) for byte-stable output.
pub fn bm25_x1024(tf: usize, dl: usize, avgdl_x1024: u64) -> u64 {
    if tf == 0 {
        return 0;
    }
    let p = params();
    let tf = tf as u64;
    // dl / avgdl, ×1024.
    let len_ratio_x1024 = (dl as u64 * SCALE * SCALE) / avgdl_x1024.max(1);
    // (1 - b) + b * dl/avgdl, ×1024. b is clamped to [0, SCALE] at load, so
    // the subtraction never underflows.
    let norm_x1024 = SCALE - p.b_x1024 + (p.b_x1024 * len_ratio_x1024) / SCALE;
    // tf + k1 * norm, ×1024.
    let denom_x1024 = tf * SCALE + (p.k1_x1024 * norm_x1024) / SCALE;
    // tf * (k1 + 1) / denom, ×1024.
    (tf * (SCALE + p.k1_x1024) * SCALE) / denom_x1024.max(1)
}

/// Anchor fan-in tiebreak ×1024: α·log2(1 + fan_in), with integer (floor)
/// log2 — deterministic, and monotone enough for a tiebreak. Zero fan-in
/// contributes zero; by data contract α keeps this strictly below one BM25
/// unit, so fan-in orders candidates the term match already tied, never
/// candidates the match separated.
pub fn fanin_boost_x1024(fan_in: usize) -> u64 {
    params().alpha_x1024 * (fan_in as u64 + 1).ilog2() as u64
}

/// Structural stop-file: a module whose import-graph fan-in exceeds
/// `hub_fanin_pct`% of the repo's total module count is glue the whole repo
/// leans on (a prelude, a shared-utility barrel) — never the file to read
/// for one capability, so it leaves anchor eligibility. A statistic of the
/// scanned repo itself, no name knowledge. Cross-multiplied comparison: the
/// exact percent test, no integer-division loss.
pub fn anchor_stopfile(fan_in: usize, total_modules: usize) -> bool {
    (fan_in as u64) * 100 > (total_modules as u64) * params().hub_fanin_pct
}

/// Published-catalog weight ×1024 of one occurrence under a declaration of
/// `kind`. Which kinds are type-class is DATA (ranking.toml `type_kinds` —
/// the generic `@definition.<kind>` capture suffixes); everything else is a
/// member. Type names are the entity vocabulary, so they outweigh the member
/// flood under the published term cap.
pub fn kind_weight_x1024(kind: &str) -> u64 {
    let p = params();
    if p.type_kinds.contains(kind) {
        p.type_weight_x1024
    } else {
        p.member_weight_x1024
    }
}

/// Fold a per-module Σ of kind weights ×1024 back into an integer count
/// (half-up rounding), floored at 1 so a term that occurs at all keeps a
/// nonzero rank — the same findability floor `classify::index_weight` applies.
pub fn weighted_count(sum_x1024: u64) -> usize {
    ((sum_x1024 + SCALE / 2) / SCALE).max(1) as usize
}

/// One candidate for a sample slot: a module carrying the term, its relevance
/// (BM25 ×1024), the project stratum it lives under and the two similarity
/// surfaces MMR compares (path subtokens, import neighborhood). Borrowed
/// views only — the corpus stays owned by `digest`.
pub struct SampleCand<'a> {
    pub path: &'a str,
    pub score_x1024: u64,
    /// The model's `projects[].dir` that prefixes this path ("" = none).
    pub stratum: &'a str,
    /// Lowercased subtokens of the path (digest tokenizer).
    pub subtokens: &'a BTreeSet<String>,
    /// Verbatim import strings — the module's dependency neighborhood.
    pub neighbors: &'a BTreeSet<&'a str>,
}

/// Pick up to `max` sample paths, in slot order. Two phases, both
/// deterministic (every tie breaks on path asc):
///
/// 1. STRATIFICATION — when ≥2 strata (project dirs) carry a candidate, each
///    gets one guaranteed slot via its best candidate, winners ordered by
///    relevance. A repo where ≤1 stratum matches skips this phase entirely:
///    the global ranking degenerates with no effect.
/// 2. MMR — the remaining slots go to the greedy maximal-marginal-relevance
///    pick over the top candidates by relevance (pool bounded by data):
///    λ·relevance − (1−λ)·max-similarity-to-already-picked, similarity =
///    equal-parts mean of subtoken Jaccard, shared directory depth and
///    import-neighborhood Jaccard. With nothing picked yet the penalty is
///    zero, so the first MMR slot is the pure relevance winner.
pub fn select_samples(cands: &[SampleCand], max: usize) -> Vec<String> {
    if max == 0 || cands.is_empty() {
        return Vec::new();
    }
    let p = params();
    // Canonical relevance order: score desc, path asc.
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|&a, &b| cands[b].score_x1024.cmp(&cands[a].score_x1024).then(cands[a].path.cmp(cands[b].path)));

    let mut selected: Vec<usize> = Vec::new();
    let mut taken = vec![false; cands.len()];

    // Phase 1: per-stratum guarantee. The first candidate met in canonical
    // order is the stratum's best; BTreeMap keeps the stratum walk stable.
    let mut best_of: BTreeMap<&str, usize> = BTreeMap::new();
    for &i in &order {
        if !cands[i].stratum.is_empty() {
            best_of.entry(cands[i].stratum).or_insert(i);
        }
    }
    if best_of.len() >= 2 {
        let mut winners: Vec<usize> = best_of.into_values().collect();
        winners.sort_by(|&a, &b| cands[b].score_x1024.cmp(&cands[a].score_x1024).then(cands[a].path.cmp(cands[b].path)));
        for i in winners.into_iter().take(max) {
            taken[i] = true;
            selected.push(i);
        }
    }

    // Phase 2: greedy MMR over the relevance-top pool of the leftovers.
    let mut pool: Vec<usize> =
        order.iter().copied().filter(|&i| !taken[i]).take(p.mmr_pool_per_slot.saturating_mul(max)).collect();
    let max_score = cands.iter().map(|c| c.score_x1024).max().unwrap_or(1).max(1);
    while selected.len() < max && !pool.is_empty() {
        let mut best: Option<(i64, usize)> = None; // (mmr score, pool position)
        for (pos, &i) in pool.iter().enumerate() {
            let rel = (cands[i].score_x1024.saturating_mul(SCALE) / max_score) as i64;
            let sim = selected.iter().map(|&s| similarity_x1024(&cands[i], &cands[s])).max().unwrap_or(0) as i64;
            let mmr = p.mmr_lambda_x1024 as i64 * rel - (SCALE - p.mmr_lambda_x1024) as i64 * sim;
            let wins = match best {
                None => true,
                Some((bm, bpos)) => mmr > bm || (mmr == bm && cands[i].path < cands[pool[bpos]].path),
            };
            if wins {
                best = Some((mmr, pos));
            }
        }
        let Some((_, pos)) = best else { break };
        selected.push(pool.remove(pos));
    }
    selected.into_iter().map(|i| cands[i].path.to_string()).collect()
}

/// Equal-parts similarity ×1024 between two candidates: path-subtoken
/// Jaccard + shared-directory depth + import-neighborhood Jaccard, each in
/// [0, SCALE]. Pure integer arithmetic over repo-relative surfaces — no name
/// knowledge.
fn similarity_x1024(a: &SampleCand, b: &SampleCand) -> u64 {
    (jaccard_x1024(a.subtokens, b.subtokens) + shared_dir_x1024(a.path, b.path) + jaccard_x1024(a.neighbors, b.neighbors)) / 3
}

/// Jaccard ×1024. Empty-vs-anything is 0 — absence of evidence is never
/// similarity.
fn jaccard_x1024<T: Ord>(a: &BTreeSet<T>, b: &BTreeSet<T>) -> u64 {
    let inter = a.intersection(b).count() as u64;
    if inter == 0 {
        return 0;
    }
    let union = (a.len() + b.len()) as u64 - inter;
    inter * SCALE / union
}

/// Shared leading directory depth ×1024, normalized by the deeper path's
/// directory depth. Two root-level files share no directory evidence (0).
fn shared_dir_x1024(a: &str, b: &str) -> u64 {
    fn dir(p: &str) -> &str {
        p.rsplit_once('/').map_or("", |(d, _)| d)
    }
    let (da, db) = (dir(a), dir(b));
    let depth = |d: &str| if d.is_empty() { 0 } else { d.split('/').count() };
    let deepest = depth(da).max(depth(db)) as u64;
    if deepest == 0 {
        return 0;
    }
    let shared = da.split('/').zip(db.split('/')).take_while(|(x, y)| x == y).count() as u64;
    shared * SCALE / deepest
}

#[cfg(test)]
mod tests {
    use super::*;

    // Engine-shape tests only: relative orderings that hold for ANY sane
    // k1/b/alpha data, so retuning ranking.toml never breaks them.

    #[test]
    fn bm25_grows_with_tf_and_saturates() {
        let avg = avgdl_x1024(10, 10);
        let s1 = bm25_x1024(1, 1, avg);
        let s2 = bm25_x1024(2, 1, avg);
        let s8 = bm25_x1024(8, 1, avg);
        assert!(s2 > s1, "more occurrences score higher: {s1} vs {s2}");
        // Saturation: the upper bound is (k1 + 1), i.e. each extra occurrence
        // pays less — score growth from 2 to 8 stays under 4x.
        assert!(s8 < s2 * 4, "tf saturates: {s2} -> {s8}");
    }

    #[test]
    fn bm25_normalizes_by_document_length() {
        let avg = avgdl_x1024(102, 2); // corpus: one 100-decl + one 2-decl module
        let sprawling = bm25_x1024(2, 100, avg);
        let focused = bm25_x1024(1, 2, avg);
        assert!(focused > sprawling, "a focused module beats raw count in a sprawling one: {focused} vs {sprawling}");
    }

    #[test]
    fn bm25_zero_tf_scores_zero() {
        assert_eq!(bm25_x1024(0, 5, avgdl_x1024(10, 2)), 0);
    }

    #[test]
    fn fanin_boost_is_zero_at_zero_and_stays_small() {
        assert_eq!(fanin_boost_x1024(0), 0, "no fan-in, no boost");
        let lone = bm25_x1024(1, 1, avgdl_x1024(1, 1));
        assert!(fanin_boost_x1024(1_000_000) < lone, "tiebreak never outranks a single term match");
        assert!(fanin_boost_x1024(8) > fanin_boost_x1024(1), "more fan-in, more boost");
    }

    #[test]
    fn stopfile_is_a_repo_relative_percent() {
        // 30% of 10 modules = 3: fan-in 3 stays, 4 leaves.
        assert!(!anchor_stopfile(3, 10));
        assert!(anchor_stopfile(4, 10));
        // Scale-free: the same fan-in is fine in a bigger repo.
        assert!(!anchor_stopfile(4, 100));
        assert!(!anchor_stopfile(0, 0), "empty corpus never divides or trips");
    }

    // Sampling tests, engine-shape too: they hold for any sane data — a
    // nonempty type_kinds list with type_weight > member_weight, and any
    // mmr_lambda strictly inside (0, 1). The kind names themselves never
    // appear here (they are catalog vocabulary; tests/term_index.rs asserts
    // them outside src/, mirroring the classify/generated_class split).

    #[test]
    fn kind_weight_types_outweigh_the_member_baseline() {
        let member = kind_weight_x1024("zzz-not-a-kind");
        assert!(!params().type_kinds.is_empty(), "catalog declares type kinds");
        for k in &params().type_kinds {
            assert!(kind_weight_x1024(k) > member, "type kind `{k}` must outweigh a member");
        }
    }

    #[test]
    fn weighted_count_rounds_half_up_and_floors_at_one() {
        assert_eq!(weighted_count(SCALE), 1);
        assert_eq!(weighted_count(SCALE * 5 / 2), 3, "2.5 rounds half-up");
        assert_eq!(weighted_count(1), 1, "any occurrence keeps a nonzero rank");
    }

    /// A candidate over test-owned sets; `score` is plain (×SCALE inside).
    fn cand<'a>(path: &'a str, score: u64, stratum: &'a str, toks: &'a BTreeSet<String>, nbrs: &'a BTreeSet<&'a str>) -> SampleCand<'a> {
        SampleCand { path, score_x1024: score * SCALE, stratum, subtokens: toks, neighbors: nbrs }
    }

    fn toks(words: &[&str]) -> BTreeSet<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn samples_guarantee_one_slot_per_matched_stratum() {
        let (t, n) = (toks(&[]), BTreeSet::new());
        // Three strong candidates in stratum `a`, one weak in `b`: the global
        // top-3 would be all-`a`, but `b` must keep its guaranteed slot.
        let cands = [
            cand("a/one.x", 300, "a", &t, &n),
            cand("a/two.x", 290, "a", &t, &n),
            cand("a/three.x", 280, "a", &t, &n),
            cand("b/solo.x", 10, "b", &t, &n),
        ];
        let picked = select_samples(&cands, 3);
        assert_eq!(picked[0], "a/one.x", "winners keep relevance order");
        assert!(picked.contains(&"b/solo.x".to_string()), "matched stratum guaranteed: {picked:?}");
    }

    #[test]
    fn samples_single_stratum_degenerates_to_global_ranking() {
        let (t, n) = (toks(&[]), BTreeSet::new());
        // Root-level paths: no shared dirs, no subtokens, no neighbors — every
        // similarity is 0, so MMR reduces to pure relevance and only the
        // stratum guarantee could reorder. With one stratum it must not.
        let cands = [
            cand("one.x", 300, "a", &t, &n),
            cand("two.x", 200, "a", &t, &n),
            cand("three.x", 100, "", &t, &n),
        ];
        assert_eq!(select_samples(&cands, 3), vec!["one.x", "two.x", "three.x"]);
    }

    #[test]
    fn samples_mmr_prefers_the_diverse_candidate_on_equal_relevance() {
        let core = toks(&["pay", "core"]);
        let dup = toks(&["pay", "sync"]);
        let far = toks(&["ship", "view"]);
        let n = BTreeSet::new();
        // Equal relevance everywhere: slot 0 goes to path asc; slot 1 must
        // skip the near-duplicate (same dir + shared subtokens) and take the
        // diverse candidate — for any lambda strictly under 1.
        let cands = [
            cand("m/pay/core.x", 50, "", &core, &n),
            cand("m/pay/sync.x", 50, "", &dup, &n),
            cand("m/ship/view.x", 50, "", &far, &n),
        ];
        assert_eq!(select_samples(&cands, 3), vec!["m/pay/core.x", "m/ship/view.x", "m/pay/sync.x"]);
    }

    #[test]
    fn samples_import_neighborhood_penalizes_the_twin() {
        let t = toks(&[]);
        let shared: BTreeSet<&str> = ["common/db", "common/http"].into();
        let lone = BTreeSet::new();
        // Same dir, no subtokens: the only similarity signal is the import
        // neighborhood. The twin of the first pick loses the second slot.
        let cands = [
            cand("m/a.x", 50, "", &t, &shared),
            cand("m/b.x", 50, "", &t, &shared),
            cand("m/c.x", 50, "", &t, &lone),
        ];
        assert_eq!(select_samples(&cands, 2), vec!["m/a.x", "m/c.x"]);
    }

    #[test]
    fn samples_similarity_legs_are_bounded_and_empty_safe() {
        let a = toks(&["pay", "core"]);
        let empty = toks(&[]);
        assert_eq!(jaccard_x1024(&a, &a), SCALE, "identical sets saturate");
        assert_eq!(jaccard_x1024(&a, &empty), 0, "absence of evidence is not similarity");
        assert_eq!(shared_dir_x1024("m/pay/a.x", "m/pay/b.x"), SCALE);
        assert_eq!(shared_dir_x1024("a.x", "b.x"), 0, "root files share no directory evidence");
        assert!(shared_dir_x1024("m/pay/a.x", "m/ship/b.x") < SCALE);
    }
}
