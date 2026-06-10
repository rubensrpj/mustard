//! matching — the tiered term-match ladder for the digest query.
//!
//! Replaces the old bidirectional prefix(>=4) heuristic, which manufactured
//! false cognates: a request token in one natural language matched an
//! unrelated identifier token in another ("cores" ~ "core", "cancelado" ~
//! "cancel"). Every tier below is an EXACT key equality — no prefix or
//! substring test survives anywhere on the ladder:
//!
//!   T1 `exact`   — raw lowercased token (or whole-identifier) equality;
//!   T2 `fold`    — equality after folding Latin diacritics to ASCII;
//!   T3 `stem`    — SAME-language stem equality (rust-stemmers), never
//!                  across languages, guarded against truncation pairs;
//!   T4 `lexicon` — curated bilingual domain glossary; translations act as
//!                  OR-synonyms (Pirkola-style), never as a replacement.
//!
//! Weights drop ~10x per tier (Zoekt-style: exact >> fold >> stem >>
//! glossary), so a real vocabulary hit always outranks a derived one.
//!
//! Anti-truncation guard (T3): a stemmer happily maps a word ONTO another
//! that is merely its prefix (both vendored stemmers reduce "cores" and
//! "core" to one key; the same happens to "cancelado" and "cancel"). That
//! relation is the dead prefix heuristic wearing a stemmer hat — it carries
//! zero evidence the prefix test didn't already carry — so stem equality is
//! accepted ONLY between surfaces that are NOT prefix-related ("studies" ~
//! "study", "faturar" ~ "faturamento"). A truncation pair without a lexicon
//! entry is an honest miss; cross-language equivalence is the lexicon's job
//! alone (the spec's documented trade: a missed true cognate over a false
//! one).
//!
//! Languages are DECLARED, never detected: `dedup([request language,
//! stemmers::FALLBACK_LANG])`
//! where the request language comes from the root project config (or the
//! CLI flag). Which languages have a stemmer/stoplist/lexicon is data
//! mirrored in `stemmers.rs` (the approved natural-language carve-out);
//! nothing in THIS module names one. Fully deterministic: sorted data,
//! stable iteration, no floats.

use std::collections::BTreeSet;

use rust_stemmers::Stemmer;

/// Tier weight multipliers, ~10x per step. Applied to the fixed-point BM25
/// anchor arithmetic in `digest::query` (integer multiplier — byte-stable).
pub fn weight(tier: u8) -> u64 {
    match tier {
        1 => 1000,
        2 => 100,
        3 => 10,
        _ => 1,
    }
}

/// Stable tier names for the per-term match report.
pub fn tier_name(tier: u8) -> &'static str {
    match tier {
        1 => "exact",
        2 => "fold",
        3 => "stem",
        4 => "lexicon",
        _ => "none",
    }
}

/// A tier hit: which rung matched and the natural-language evidence behind it
/// (the stemmer language for T3, the lexicon pair label for T4, empty for
/// T1/T2 — those are language-free equalities).
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
}

/// One parsed lexicon entry, pre-folded and pre-stemmed at load:
/// `key` (+ its key-language stem) on one side, the synonym translations
/// (+ their value-language stems) on the other.
struct Entry {
    key: String,
    key_stem: Option<String>,
    syns: Vec<String>,
    syn_stems: Vec<String>,
}

/// A vendored bilingual lexicon, active when both its languages are in the
/// query language set. Bidirectional: the REQUEST token may sit on either
/// side (inflected forms reach an entry via the same-language stem), but the
/// CODE side must equal a key or a synonym exactly — no fuzziness on the
/// index side, ever ("igualdade exata de chave").
struct Lexicon {
    label: String,
    /// Index of each side's stemmer in `Ladder::stemmers` (None = no stemmer
    /// vendored for that side: entry lookup is exact-fold only).
    key_si: Option<usize>,
    val_si: Option<usize>,
    entries: Vec<Entry>,
}

/// The match ladder for one query: active languages, their stemmers and
/// stoplists, and the lexicons bridging them. Built once per `digest --query`.
pub struct Ladder {
    /// (language code, stemmer) rows in deterministic order: request language
    /// first, then the always-on fallback.
    stemmers: Vec<(String, Stemmer)>,
    /// Natural-language stop words (raw + folded) for the active languages —
    /// query-token glue, on top of the identifier-glue stopwords.toml list.
    stop: BTreeSet<String>,
    lexicons: Vec<Lexicon>,
}

impl Ladder {
    /// Build the ladder for a declared request language (a BCP-47-ish code;
    /// only its primary subtag is used). The active set is
    /// `dedup([request, stemmers::FALLBACK_LANG])` — zero language detection, and an unknown
    /// code simply has no stemmer/stoplist rows (degraded, never an error).
    pub fn new(request_lang: &str) -> Self {
        let primary = primary_subtag(request_lang);
        let mut langs: Vec<String> = Vec::new();
        for l in [primary.as_str(), crate::stemmers::FALLBACK_LANG] {
            if !l.is_empty() && !langs.iter().any(|x| x == l) {
                langs.push(l.to_string());
            }
        }
        let stemmers: Vec<(String, Stemmer)> =
            langs.iter().filter_map(|l| crate::stemmers::stemmer(l).map(|s| (l.clone(), s))).collect();
        let mut stop = BTreeSet::new();
        for l in &langs {
            for line in crate::stemmers::stoplist(l).lines() {
                let w = line.trim().to_lowercase();
                if w.is_empty() || w.starts_with('#') {
                    continue;
                }
                stop.insert(fold(&w));
                stop.insert(w);
            }
        }
        let mut lexicons = Vec::new();
        for i in 0..langs.len() {
            for j in (i + 1)..langs.len() {
                if let Some(seed) = crate::stemmers::lexicon(&langs[i], &langs[j]) {
                    lexicons.push(parse_lexicon(&seed, &stemmers));
                }
            }
        }
        Self { stemmers, stop, lexicons }
    }

    /// Whether `q` is natural-language glue in any active language (checked
    /// raw and accent-folded) — such a token must never act as a
    /// discriminator, mirroring the stopwords.toml contract.
    pub fn query_stopword(&self, q: &str) -> bool {
        self.stop.contains(q) || self.stop.contains(&fold(q))
    }

    /// Prepare a lowercased token for tier comparison.
    pub fn sig(&self, token: &str) -> Sig {
        let fold = fold(token);
        let stems = self.stemmers.iter().map(|(_, st)| st.stem(&fold).into_owned()).collect();
        Sig { raw: token.to_string(), fold, stems }
    }

    /// Climb the ladder: the index token `key` against the request token `q`.
    /// First rung that holds wins; `None` is an honest miss.
    pub fn tier(&self, key: &Sig, q: &Sig) -> Option<Hit> {
        if key.raw == q.raw {
            return Some(Hit { tier: 1, lang: String::new() });
        }
        if key.fold == q.fold {
            return Some(Hit { tier: 2, lang: String::new() });
        }
        // T3 — same-language stem equality, never on a truncation pair (a
        // bare prefix relation is the dead heuristic, not morphology).
        if !truncation_related(&key.fold, &q.fold) {
            for (i, (lang, _)) in self.stemmers.iter().enumerate() {
                if key.stems[i] == q.stems[i] {
                    return Some(Hit { tier: 3, lang: lang.clone() });
                }
            }
        }
        // T4 — bilingual domain lexicon, translations as OR-synonyms.
        for lx in &self.lexicons {
            if lx.bridges(key, q) {
                return Some(Hit { tier: 4, lang: lx.label.clone() });
            }
        }
        None
    }
}

impl Lexicon {
    /// Does this lexicon bridge request token `q` onto index token `key`?
    /// The request side may be inflected (same-language stem reaches the
    /// entry: "cancelado" finds "cancelar") under the SAME anti-truncation
    /// guard as tier 3 — otherwise a curated word would resurrect the dead
    /// prefix relation through its own entry ("charges" must not reach
    /// "charge" by stemming onto the synonym). The index side is exact-fold
    /// equality against a key or a synonym, never stemmed.
    fn bridges(&self, key: &Sig, q: &Sig) -> bool {
        let q_eq = |word: &str, stem: Option<&String>, si: Option<usize>| -> bool {
            q.fold == word
                || (!truncation_related(&q.fold, word)
                    && matches!((si, stem), (Some(i), Some(st)) if &q.stems[i] == st))
        };
        for e in &self.entries {
            // Request on the KEY side -> a translation must equal the index key.
            if q_eq(&e.key, e.key_stem.as_ref(), self.key_si) && e.syns.contains(&key.fold) {
                return true;
            }
            // Request on the VALUE side -> the key (or a sibling synonym, OR
            // semantics) must equal the index key.
            let q_on_val = e.syns.iter().enumerate().any(|(k, s)| q_eq(s, e.syn_stems.get(k), self.val_si));
            if q_on_val && (e.key == key.fold || e.syns.contains(&key.fold)) {
                return true;
            }
        }
        false
    }
}

/// Parse a vendored lexicon seed into pre-folded, pre-stemmed entries. A
/// malformed EMBEDDED file is a programmer error caught by any test run —
/// the same contract as ranking.toml and stopwords.toml.
fn parse_lexicon(seed: &crate::stemmers::LexiconSeed, stemmers: &[(String, Stemmer)]) -> Lexicon {
    let v: toml::Value = toml::from_str(seed.toml).expect("embedded lexicon is not valid TOML");
    let key_si = stemmers.iter().position(|(l, _)| l == seed.key_lang);
    let val_si = stemmers.iter().position(|(l, _)| l == seed.val_lang);
    let stem_with = |si: Option<usize>, w: &str| si.map(|i| stemmers[i].1.stem(w).into_owned());
    let mut entries: Vec<Entry> = v
        .get("terms")
        .and_then(|t| t.as_table())
        .expect("embedded lexicon must contain a [terms] table")
        .iter()
        .map(|(k, val)| {
            let key = fold(&k.to_lowercase());
            let syns: Vec<String> = val
                .as_array()
                .expect("each lexicon entry must be an array of synonyms")
                .iter()
                .map(|s| fold(&s.as_str().expect("each synonym must be a string").to_lowercase()))
                .collect();
            let syn_stems = syns.iter().map(|s| stem_with(val_si, s).unwrap_or_else(|| s.clone())).collect();
            Entry { key_stem: stem_with(key_si, &key), key, syns, syn_stems }
        })
        .collect();
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    Lexicon { label: seed.label.to_string(), key_si, val_si, entries }
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

/// Primary BCP-47 subtag, lowercased: leading ASCII letters only, so
/// region/script suffixes and malformed input degrade to a plain code (or to
/// empty, which leaves only the fallback language active).
fn primary_subtag(raw: &str) -> String {
    raw.trim().to_lowercase().chars().take_while(|c| c.is_ascii_alphabetic()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(ladder: &Ladder, key: &str, q: &str) -> Option<(u8, String)> {
        ladder.tier(&ladder.sig(key), &ladder.sig(q)).map(|h| (h.tier, h.lang))
    }

    #[test]
    fn false_cognates_die_on_every_rung() {
        // The motivating pairs: prefix-related surfaces whose stems collide
        // in BOTH vendored stemmers — the guard must refuse them at T3, and
        // without a lexicon entry they are honest misses.
        for lang in ["", "pt-BR", "en-US"] {
            let l = Ladder::new(lang);
            assert!(hit(&l, "core", "cores").is_none(), "cores~core must miss (lang={lang:?})");
            assert!(hit(&l, "charge", "charges").is_none(), "truncation pair is never stem evidence");
        }
        // cancelado~cancel: dead without the pt-en lexicon...
        let en_only = Ladder::new("en-US");
        assert!(hit(&en_only, "cancel", "cancelado").is_none(), "no glossary, no bridge");
    }

    #[test]
    fn ladder_tiers_report_honestly() {
        let l = Ladder::new("pt-BR");
        assert_eq!(hit(&l, "parentid", "parentid"), Some((1, String::new())), "whole-ident exact");
        assert_eq!(hit(&l, "cobranca", "cobrança"), Some((2, String::new())), "accent fold");
        // Same-language stems, non-truncation surfaces: real morphology.
        assert_eq!(hit(&l, "study", "studies"), Some((3, "en".into())));
        assert_eq!(hit(&l, "faturamento", "faturar"), Some((3, "pt".into())));
        // ...and the glossary bridge, tier + pair reported. The inflected
        // request reaches the `cancelar` entry via the same-language stem.
        assert_eq!(hit(&l, "cancel", "cancelado"), Some((4, "pt-en".into())));
        // Bidirectional: an `en` request token reaches a `pt` identifier.
        assert_eq!(hit(&l, "fatura", "invoice"), Some((4, "pt-en".into())));
    }

    #[test]
    fn query_stopwords_cover_both_languages_raw_and_folded() {
        let l = Ladder::new("pt-BR");
        assert!(l.query_stopword("the"));
        assert!(l.query_stopword("não"), "raw accented form");
        assert!(l.query_stopword("nao"), "folded form equally inert");
        assert!(!l.query_stopword("cobranca"), "domain vocabulary stays");
        let en = Ladder::new("");
        assert!(!en.query_stopword("não"), "inactive language list is not loaded");
    }

    #[test]
    fn unknown_language_degrades_to_fallback_rows() {
        let l = Ladder::new("fr-FR");
        // No vendored stemmer/lexicon for the request language: only the
        // fallback rows are active — degraded, never an error.
        assert_eq!(hit(&l, "study", "studies"), Some((3, "en".into())));
        assert!(hit(&l, "cancel", "cancelado").is_none());
    }
}
