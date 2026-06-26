//! stemmers — data-mirror from NATURAL-language codes to vendored language data.
//!
//! CARVE-OUT (the single approved exception to the "no language names in
//! src/" guard, recorded in the spec and in docs/REDESENHO-INDICE-DIGEST-
//! AGNOSTICO.md): this module may name NATURAL languages to select a
//! `rust_stemmers` algorithm and a vendored stoplist. Natural languages are the
//! one registry `languages.toml` cannot carry (the stemmer algorithm is a crate
//! enum, the stoplist an embedded file), so the mapping lives here as a pure
//! mirror of data. PROGRAMMING languages, file extensions, grammar nodes and
//! frameworks remain banned from this file like from every other module.
//!
//! The retrieval ladder is ENGLISH-only (intra-language); there is no
//! cross-language lexicon bridge. Zero behavior per language beyond selection:
//! every function is a lookup row. The matching engine (`matching.rs`) never
//! branches on a code — it consumes whatever this mirror returns.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_rows_resolve_and_unknown_degrades() {
        assert!(stemmer("en").is_some());
        assert!(stemmer("xx").is_none(), "unknown code has no row, no error");
        assert!(!stoplist("en").is_empty());
        assert!(stoplist("xx").is_empty());
    }
}
