//! ranking — pure, domain-agnostic BM25 relevance arithmetic.
//!
//! The single home for the Okapi BM25 score shape, shared by every consumer
//! that ranks documents by a query without re-implementing the math:
//! - the scan crate's `digest` (per-term sample ranking over the repo model), and
//! - the rt crate's persistent-memory recall (knowledge/decision bodies vs the
//!   prompt).
//!
//! Fixed-point integer arithmetic (scores ×1024): floats never enter a
//! comparison, so every ranking is byte-stable across runs and platforms. The
//! tuning constants `k1`/`b` are NOT embedded here — they are passed in (each
//! ×1024), so each caller owns its tuning: scan reads `ranking.toml`; a simple
//! consumer uses the classic 1.2 / 0.75 defaults via [`bm25_x1024_default`].
//! Nothing here knows a language, framework, file name or document kind.

/// Fixed-point scale: scores and ratios carry 10 fractional bits.
pub const SCALE: u64 = 1024;

/// Classic BM25 `k1 = 1.2` ×1024 — term-frequency saturation.
pub(crate) const DEFAULT_K1_X1024: u64 = 1229; // round(1.2 * 1024)

/// Classic BM25 `b = 0.75` ×1024 — length-normalization strength.
pub(crate) const DEFAULT_B_X1024: u64 = 768; // round(0.75 * 1024)

/// Average document length ×1024 over a corpus of `docs` documents totalling
/// `total_len` tokens. Floored at 1 so it can always divide.
#[must_use]
pub fn avgdl_x1024(total_len: usize, docs: usize) -> u64 {
    if docs == 0 {
        return SCALE;
    }
    ((total_len as u64 * SCALE) / docs as u64).max(1)
}

/// BM25 score ×1024 for `tf` occurrences of a term in a document of length
/// `dl`, against the corpus average `avgdl_x1024`. Classic Okapi shape WITHOUT
/// the IDF factor — a premise that holds PER TERM: when documents compete FOR
/// one term, the term's IDF is a common constant and cancels.
///
/// `k1_x1024` / `b_x1024` are the tuning constants ×1024; the caller MUST clamp
/// `b_x1024` to `[0, SCALE]` (the subtraction relies on it). Pure integer
/// arithmetic; ties are broken by the caller (e.g. path asc) for byte-stable
/// output.
#[must_use]
pub fn bm25_x1024(tf: usize, dl: usize, avgdl_x1024: u64, k1_x1024: u64, b_x1024: u64) -> u64 {
    if tf == 0 {
        return 0;
    }
    let tf = tf as u64;
    // dl / avgdl, ×1024.
    let len_ratio_x1024 = (dl as u64 * SCALE * SCALE) / avgdl_x1024.max(1);
    // (1 - b) + b * dl/avgdl, ×1024. b clamped to [0, SCALE] by the caller, so
    // the subtraction never underflows.
    let norm_x1024 = SCALE - b_x1024 + (b_x1024 * len_ratio_x1024) / SCALE;
    // tf + k1 * norm, ×1024.
    let denom_x1024 = tf * SCALE + (k1_x1024 * norm_x1024) / SCALE;
    // tf * (k1 + 1) / denom, ×1024.
    (tf * (SCALE + k1_x1024) * SCALE) / denom_x1024.max(1)
}

/// [`bm25_x1024`] with the classic 1.2 / 0.75 tuning — the convenience a simple
/// consumer (no `ranking.toml`) uses.
#[must_use]
pub fn bm25_x1024_default(tf: usize, dl: usize, avgdl_x1024: u64) -> u64 {
    bm25_x1024(tf, dl, avgdl_x1024, DEFAULT_K1_X1024, DEFAULT_B_X1024)
}

/// Inverse document frequency ×1024 of a term seen in `df` of `n_docs`
/// documents: `log2((n_docs + 1) / (df + 1))`, never negative. Rarer terms
/// (small `df`) score higher; a term in (nearly) every document tends to 0.
/// `df` is clamped to `[1, n_docs]` so an occurrence count that exceeds the
/// document count never underflows. This is the cross-term factor BM25 omits
/// (see [`bm25_x1024`]): summed per document over the matched terms, it ranks
/// ACROSS terms so a rare term outweighs a ubiquitous one regardless of how
/// each matched. A pure corpus statistic — NO tuning knob — float-free and
/// byte-stable.
#[must_use]
pub fn idf_x1024(df: usize, n_docs: usize) -> u64 {
    let n = n_docs.max(1);
    let df = df.clamp(1, n);
    log2_x1024(n as u64 + 1).saturating_sub(log2_x1024(df as u64 + 1))
}

/// Domain-specificity ×1024 of a term: its term frequency times its inverse
/// document frequency — the classic TF·IDF, fixed-point. `count` is the term's
/// total occurrences (saturated into the ×1024 multiply so a flood cannot
/// overflow), `df` the number of documents it appears in, `n_docs` the corpus
/// size. The product PEAKS IN THE MIDDLE of the frequency range: it demotes the
/// ubiquitous term (high `df` → `idf_x1024` tends to 0) AND the hapax (low
/// `count` → small TF), so the discriminative mid-frequency vocabulary scores
/// highest. Reuses [`idf_x1024`] for the corpus-rarity factor — no tuning knob,
/// float-free and byte-stable like every primitive here.
#[must_use]
pub fn domain_specificity_x1024(count: usize, df: usize, n_docs: usize) -> u64 {
    (count as u64).saturating_mul(idf_x1024(df, n_docs))
}

/// Fixed-point (×1024) base-2 logarithm of `x`: `floor(log2 x)` from the integer
/// `ilog2`, plus a 10-bit fractional part linearly interpolated above that power
/// of two. Monotone non-decreasing and continuous across powers;
/// `log2_x1024(0) == log2_x1024(1) == 0`. The float-free primitive under
/// [`idf_x1024`].
fn log2_x1024(x: u64) -> u64 {
    if x <= 1 {
        return 0;
    }
    let i = u64::from(x.ilog2()); // floor(log2 x); x >= 2 ⇒ i >= 1
    let pow = 1u64 << i; // 2^i, with 2^i <= x < 2^(i+1)
    i * SCALE + ((x - pow) * SCALE) / pow // i + fractional (x/2^i − 1), ×1024
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bm25_grows_with_tf_and_saturates() {
        let avg = avgdl_x1024(10, 10);
        let s1 = bm25_x1024_default(1, 1, avg);
        let s2 = bm25_x1024_default(2, 1, avg);
        let s8 = bm25_x1024_default(8, 1, avg);
        assert!(s2 > s1, "more tf scores higher");
        assert!(s8 > s2);
        // Saturation: the jump 1→2 exceeds the jump 7→8.
        assert!(s2 - s1 > s8 - bm25_x1024_default(7, 1, avg));
    }

    #[test]
    fn bm25_normalizes_by_document_length() {
        let avg = avgdl_x1024(102, 2); // corpus: one 100-decl + one 2-decl module
        let sprawling = bm25_x1024_default(2, 100, avg);
        let focused = bm25_x1024_default(1, 2, avg);
        assert!(focused > sprawling, "a focused short doc outranks a sprawling one");
    }

    #[test]
    fn bm25_zero_tf_scores_zero() {
        assert_eq!(bm25_x1024_default(0, 5, avgdl_x1024(10, 2)), 0);
    }

    #[test]
    fn avgdl_floors_at_one_and_handles_empty_corpus() {
        assert_eq!(avgdl_x1024(0, 0), SCALE, "empty corpus → SCALE sentinel");
        assert_eq!(avgdl_x1024(0, 5), 1, "floored at 1 so it can always divide");
    }

    #[test]
    fn defaults_match_classic_constants() {
        assert_eq!(DEFAULT_K1_X1024, (1.2_f64 * SCALE as f64).round() as u64);
        assert_eq!(DEFAULT_B_X1024, (0.75_f64 * SCALE as f64).round() as u64);
    }

    #[test]
    fn log2_x1024_is_exact_on_powers_and_interpolates_between() {
        assert_eq!(log2_x1024(0), 0);
        assert_eq!(log2_x1024(1), 0);
        assert_eq!(log2_x1024(2), SCALE, "log2 2 = 1");
        assert_eq!(log2_x1024(8), 3 * SCALE, "log2 8 = 3");
        assert!(
            log2_x1024(2) < log2_x1024(3) && log2_x1024(3) < log2_x1024(4),
            "monotone, interpolating between powers of two",
        );
    }

    #[test]
    fn idf_falls_with_document_frequency() {
        let n = 1000;
        assert!(idf_x1024(2, n) > idf_x1024(200, n), "a rarer term scores higher");
        assert!(idf_x1024(n, n) < idf_x1024(n / 50, n), "a term in (nearly) every doc tends to zero");
        // `df` clamps to [1, n]: an occurrence count above the doc count neither
        // panics nor underflows, it saturates at the ubiquitous floor.
        assert_eq!(idf_x1024(5 * n, n), idf_x1024(n, n));
        assert_eq!(idf_x1024(0, n), idf_x1024(1, n));
    }

    #[test]
    fn domain_specificity_peaks_in_the_mid_frequency() {
        let n = 1000;
        // Ubiquitous term (high df, "type"/"response" style): high count but
        // near-zero idf → low specificity.
        let ubiquitous = domain_specificity_x1024(900, 900, n);
        // Mid-frequency term with a reasonable count ("tenant"/"category"
        // style): both factors substantial → the discriminative peak.
        let mid = domain_specificity_x1024(60, 40, n);
        // Hapax (df == 1, count low): high idf but tiny tf → low specificity.
        let hapax = domain_specificity_x1024(1, 1, n);
        assert!(mid > ubiquitous, "mid-frequency outscores the ubiquitous term: {mid} vs {ubiquitous}");
        assert!(mid > hapax, "mid-frequency outscores the hapax: {mid} vs {hapax}");
    }
}
