//! stemmers — data-mirror from NATURAL-language codes to vendored language data.
//!
//! CARVE-OUT (the single approved exception to the "no language names in
//! src/" guard, recorded in the spec and in docs/REDESENHO-INDICE-DIGEST-
//! AGNOSTICO.md): this module may name NATURAL languages — `pt`, `en` — to
//! select a `rust_stemmers` algorithm, a vendored stoplist and a lexicon
//! pair. Natural languages are the one registry `languages.toml` cannot
//! carry (the stemmer algorithm is a crate enum, the stoplist an embedded
//! file), so the mapping lives here as a pure mirror of data. PROGRAMMING
//! languages, file extensions, grammar nodes and frameworks remain banned
//! from this file like from every other module.
//!
//! Zero behavior per language beyond selection: every function is a lookup
//! row. Adding a language = one stoplist file under `stoplists/` + one row
//! in [`stemmer`] and [`stoplist`]; adding a language pair = one file under
//! `lexicons/` + one row in [`lexicon`]. The matching engine (`matching.rs`)
//! never branches on a code — it consumes whatever this mirror returns.

use rust_stemmers::{Algorithm, Stemmer};

/// Pivot language of the identifier vocabulary — the always-on fallback code
/// the match ladder activates alongside the declared request language.
pub const FALLBACK_LANG: &str = "en";

/// Snowball stemmer for a natural-language code (primary BCP-47 subtag,
/// lowercased). `None` = no stemmer vendored: tier-3 simply has no row for
/// that language — degraded, never an error.
pub fn stemmer(code: &str) -> Option<Stemmer> {
    match code {
        "en" => Some(Stemmer::create(Algorithm::English)),
        "pt" => Some(Stemmer::create(Algorithm::Portuguese)),
        _ => None,
    }
}

/// Vendored Snowball stop-word list for a natural-language code — query-token
/// glue in that language. Empty = none vendored (the query just keeps more
/// tokens; harmless).
pub fn stoplist(code: &str) -> &'static str {
    match code {
        "en" => include_str!("../stoplists/en.txt"),
        "pt" => include_str!("../stoplists/pt.txt"),
        _ => "",
    }
}

/// A vendored bilingual domain lexicon: the embedded TOML plus which side of
/// the file each language sits on (`key_lang` = the TOML keys, `val_lang` =
/// the value arrays). The label names the pair in match reports.
pub struct LexiconSeed {
    pub label: &'static str,
    pub key_lang: &'static str,
    pub val_lang: &'static str,
    pub toml: &'static str,
}

/// Lexicon for a language pair, order-insensitive (`pt`+`en` and `en`+`pt`
/// resolve to the same file). `None` = no curated pair vendored: tier-4 has
/// no bridge for those languages and a cross-language term is an honest miss.
pub fn lexicon(a: &str, b: &str) -> Option<LexiconSeed> {
    match (a, b) {
        ("pt", "en") | ("en", "pt") => Some(LexiconSeed {
            label: "pt-en",
            key_lang: "pt",
            val_lang: "en",
            toml: include_str!("../lexicons/pt-en.toml"),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_rows_resolve_and_unknown_degrades() {
        assert!(stemmer("en").is_some());
        assert!(stemmer("pt").is_some());
        assert!(stemmer("xx").is_none(), "unknown code has no row, no error");
        assert!(!stoplist("en").is_empty());
        assert!(!stoplist("pt").is_empty());
        assert!(stoplist("xx").is_empty());
    }

    #[test]
    fn lexicon_pair_is_order_insensitive() {
        let ab = lexicon("pt", "en").expect("seed pair");
        let ba = lexicon("en", "pt").expect("seed pair reversed");
        assert_eq!(ab.label, ba.label);
        assert_eq!(ab.key_lang, "pt");
        assert_eq!(ab.val_lang, "en");
        assert!(lexicon("pt", "xx").is_none());
        assert!(toml::from_str::<toml::Value>(ab.toml).is_ok(), "seed lexicon parses");
    }
}
