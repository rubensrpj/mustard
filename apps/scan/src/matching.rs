//! matching — the tiered term-match ladder for the digest query.
//!
//! Intra-language (ENGLISH) matching only: the request and the code are taken
//! to share one vocabulary, so there is no per-request language and no
//! cross-language bridge. Every tier below is an EXACT key equality — no prefix
//! or substring test survives anywhere on the ladder:
//!
//!   T1 `exact`   — raw lowercased token (or whole-identifier) equality;
//!   T2 `fold`    — equality after folding Latin diacritics to ASCII;
//!   T3 `stem`    — English Snowball stem equality; a truncation pair needs
//!                  unanimous morphological backing (see the guard note below);
//!   T5 `trigram` — opt-in fuzzy RESCUE (pg_trgm-style Jaccard) the caller turns
//!                  on only to salvage an otherwise weak/none query.
//!
//! Weights drop ~10x per tier (Zoekt-style: exact >> fold >> stem), so a real
//! vocabulary hit always outranks a derived one.
//!
//! Anti-truncation guard (T3): a stemmer happily maps a word ONTO another that
//! is merely its prefix, so for surfaces that ARE prefix-related ("payables" ~
//! "payable") one language's lone stem collision is the dead prefix heuristic
//! wearing a stemmer hat. A truncation pair is therefore accepted ONLY on
//! UNANIMOUS morphological backing: every active stemmer must collapse both
//! surfaces to one non-empty key ("payables" ~ "payable" — genuine plural). A
//! bare prefix without that backing stays dead ("pay" ~ "payables", stems
//! distinct).
//!
//! The active language is the always-on English fallback
//! (`stemmers::FALLBACK_LANG`); nothing here is declared or detected per request.
//! Fully deterministic: sorted data, stable iteration, no floats.

use std::collections::BTreeSet;

use rust_stemmers::Stemmer;

/// Stable tier names for the per-term match report.
pub fn tier_name(tier: u8) -> &'static str {
    match tier {
        1 => "exact",
        2 => "fold",
        3 => "stem",
        5 => "trigram",
        _ => "none",
    }
}

/// Minimum trigram Jaccard similarity (×1000) for the T5 fuzzy RESCUE rung.
/// pg_trgm's default is 0.3; we use a stricter 0.5 to curb the false cognates
/// the exact ladder was built to avoid (`card`~`discard` ≈ 0.4 stays a miss),
/// while shared-root + morphology bridge (`calculados`~`calculate` ≈ 0.5,
/// `invalidadas`~`invalidate` ≈ 0.55). T5 fires ONLY when the caller opts in
/// (`tier(.., allow_fuzzy=true)`) — the digest enables it only to RESCUE a query
/// the strict ladder leaves weak/none, so the precision cost lands only on
/// queries that were already failing; a strong query never sees it.
const TRIGRAM_SIM_MIN_X1000: u64 = 500;

/// A tier hit: which rung matched and the natural-language evidence behind it
/// (the stemmer language for T3, the literal `trigram` for T5, empty for T1/T2 —
/// those are language-free equalities).
pub struct Hit {
    pub tier: u8,
    pub lang: String,
}

/// A token prepared for tier comparison: its raw lowercased form, its
/// accent-folded form, and its stem in each active language (parallel to
/// `Ladder::stemmers`). Computed once per token so the digest's term-index
/// sweep never re-stems.
pub struct Sig {
    raw: String,
    fold: String,
    stems: Vec<String>,
    /// Overlapping 3-char windows of the folded form (the pg_trgm/Google-Code-
    /// Search substrate) — compared by Jaccard on the opt-in T5 fuzzy rung.
    /// Language-free: no tokenization or stemming.
    tri: BTreeSet<String>,
}

/// The match ladder for one query: the active English stemmer and its stoplist.
/// Built once per `digest --query`.
pub struct Ladder {
    /// (language code, stemmer) rows in deterministic order — English only.
    stemmers: Vec<(String, Stemmer)>,
    /// Natural-language stop words (raw + folded) for the active language —
    /// query-token glue, on top of the identifier-glue stopwords.toml list.
    stop: BTreeSet<String>,
}

impl Default for Ladder {
    fn default() -> Self {
        Self::new()
    }
}

impl Ladder {
    /// Build the English-only ladder. The active set is just
    /// `stemmers::FALLBACK_LANG` — zero language detection, no request language.
    pub fn new() -> Self {
        let langs: [&str; 1] = [crate::stemmers::FALLBACK_LANG];
        let stemmers: Vec<(String, Stemmer)> =
            langs.iter().filter_map(|&l| crate::stemmers::stemmer(l).map(|s| (l.to_string(), s))).collect();
        let mut stop = BTreeSet::new();
        for &l in &langs {
            for line in crate::stemmers::stoplist(l).lines() {
                let w = line.trim().to_lowercase();
                if w.is_empty() || w.starts_with('#') {
                    continue;
                }
                stop.insert(fold(&w));
                stop.insert(w);
            }
        }
        Self { stemmers, stop }
    }

    /// Whether `q` is natural-language glue in the active language (checked raw
    /// and accent-folded) — such a token must never act as a discriminator,
    /// mirroring the stopwords.toml contract.
    pub fn query_stopword(&self, q: &str) -> bool {
        self.stop.contains(q) || self.stop.contains(&fold(q))
    }

    /// Prepare a lowercased token for tier comparison.
    pub fn sig(&self, token: &str) -> Sig {
        let fold = fold(token);
        let stems = self.stemmers.iter().map(|(_, st)| st.stem(&fold).into_owned()).collect();
        let tri = trigrams(&fold);
        Sig { raw: token.to_string(), fold, stems, tri }
    }

    /// Climb the ladder: the index token `key` against the request token `q`.
    /// First rung that holds wins; `None` is an honest miss. `allow_fuzzy` opens
    /// the T5 trigram RESCUE rung (off by default — the caller turns it on only
    /// to rescue an otherwise weak/none query, never on the strict path).
    pub fn tier(&self, key: &Sig, q: &Sig, allow_fuzzy: bool) -> Option<Hit> {
        if key.raw == q.raw {
            return Some(Hit { tier: 1, lang: String::new() });
        }
        if key.fold == q.fold {
            return Some(Hit { tier: 2, lang: String::new() });
        }
        // T3 — same-language stem equality. A truncation pair needs UNANIMOUS
        // backing (every active stemmer collapses both surfaces to one non-empty
        // key — see the module note); one row's lone collision on such a pair is
        // truncation, not morphology. Non-truncation surfaces keep the any-row
        // rule.
        if truncation_related(&key.fold, &q.fold) {
            let unanimous = !self.stemmers.is_empty()
                && key.stems.iter().zip(&q.stems).all(|(k, qs)| !k.is_empty() && k == qs);
            if unanimous {
                return Some(Hit { tier: 3, lang: self.stemmers[0].0.clone() });
            }
        } else {
            for (i, (lang, _)) in self.stemmers.iter().enumerate() {
                if key.stems[i] == q.stems[i] {
                    return Some(Hit { tier: 3, lang: lang.clone() });
                }
            }
        }
        // T5 — trigram Jaccard RESCUE (pg_trgm-style), opt-in only. A language-
        // free fuzzy rung BELOW the strict ladder: bridges shared-root +
        // morphology by form, gated by a similarity floor.
        if allow_fuzzy {
            let inter = q.tri.intersection(&key.tri).count();
            if inter > 0 {
                let uni = (q.tri.len() + key.tri.len() - inter) as u64;
                if uni > 0 && (inter as u64) * 1000 / uni >= TRIGRAM_SIM_MIN_X1000 {
                    return Some(Hit { tier: 5, lang: "trigram".into() });
                }
            }
        }
        None
    }
}

/// Fold Latin diacritics to their ASCII base letter. A pure character table
/// (Unicode data, no language named); input is already lowercased. Anything
/// outside the table passes through untouched.
pub(crate) fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            _ => c,
        })
        .collect()
}

/// One folded surface is a bare prefix of the other — the relation the old
/// heuristic trusted and this ladder never does (equal forms are tier 2).
fn truncation_related(a: &str, b: &str) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// The set of overlapping 3-char windows of `s` (already folded). A surface
/// shorter than 3 chars contributes itself as a single gram so a 3-letter token
/// still compares. Pure char windows: no language knowledge.
fn trigrams(s: &str) -> BTreeSet<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return BTreeSet::new();
    }
    if chars.len() < 3 {
        return std::iter::once(s.to_string()).collect();
    }
    chars.windows(3).map(|w| w.iter().collect::<String>()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(ladder: &Ladder, key: &str, q: &str) -> Option<(u8, String)> {
        ladder.tier(&ladder.sig(key), &ladder.sig(q), false).map(|h| (h.tier, h.lang))
    }

    /// Same, but with the T5 fuzzy RESCUE rung enabled (the digest's weak/none path).
    fn fuzzy(ladder: &Ladder, key: &str, q: &str) -> Option<(u8, String)> {
        ladder.tier(&ladder.sig(key), &ladder.sig(q), true).map(|h| (h.tier, h.lang))
    }

    #[test]
    fn trigram_rescue_bridges_shared_roots_only_when_enabled() {
        let l = Ladder::new();
        // OFF by default: the strict ladder never fuzzes (the existing contract).
        assert!(hit(&l, "calculate", "calculados").is_none(), "strict path stays exact");
        // ON (rescue): shared-root + morphology bridge at T5.
        assert_eq!(fuzzy(&l, "calculate", "calculados").map(|(t, _)| t), Some(5), "calculados~calculate");
        assert_eq!(fuzzy(&l, "invalidate", "invalidadas").map(|(t, _)| t), Some(5), "invalidadas~invalidate");
        // The similarity floor still rejects a low-overlap pair, even fuzzy.
        assert!(fuzzy(&l, "discard", "card").is_none(), "0.4 overlap stays below the 0.5 floor");
        // A genuine exact match never downgrades to T5 just because fuzzy is on.
        assert_eq!(fuzzy(&l, "payable", "payable").map(|(t, _)| t), Some(1), "exact still wins under fuzzy");
    }

    #[test]
    fn truncation_pair_needs_unanimous_stem_backing() {
        // A pair the English stemmer collapses to one key is genuine
        // plural/singular morphology and matches at T3; a bare prefix with
        // distinct stems stays dead.
        let l = Ladder::new();
        assert_eq!(hit(&l, "payable", "payables").map(|(t, _)| t), Some(3), "genuine plural");
        assert!(hit(&l, "payables", "pay").is_none(), "bare prefix, stems distinct");
        assert!(hit(&l, "pay", "payables").is_none(), "direction-independent");
    }

    #[test]
    fn ladder_tiers_report_honestly() {
        let l = Ladder::new();
        assert_eq!(hit(&l, "parentid", "parentid"), Some((1, String::new())), "whole-ident exact");
        assert_eq!(hit(&l, "cobranca", "cobrança"), Some((2, String::new())), "accent fold");
        // Same-language stems, non-truncation surfaces: real morphology.
        assert_eq!(hit(&l, "study", "studies"), Some((3, "en".into())));
    }

    #[test]
    fn query_stopwords_are_english_raw_and_folded() {
        let l = Ladder::new();
        assert!(l.query_stopword("the"));
        assert!(!l.query_stopword("cobranca"), "domain vocabulary stays");
    }
}
