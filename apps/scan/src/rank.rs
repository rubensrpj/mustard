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
}
