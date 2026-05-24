//! Lang-aware spec slug helper (Wave 4 of `mustard-unification`).
//!
//! Spec slugs (`.claude/spec/{slug}/spec.md`) are kebab-case identifiers
//! derived from a free-form title (e.g. `"Configuração de Idioma e Tom"` →
//! `"configuracao-idioma-tom"`). The pt-BR path needs accent stripping; the
//! en-US path does not. Stopword lists differ per locale.
//!
//! This is a thin facade over [`mustard_core::slugify`] kept here so any rt
//! caller can `crate::run::spec_slug::for_lang` without having to think about
//! the BCP-47 parse / fail-open dance every time.
//!
//! ## Fail-open
//!
//! Every helper accepts free-form input. An empty or fully non-alphanumeric
//! input degrades to `"x"` (the floor inherited from the existing
//! `scan::interpret::slugify` contract).
//!
//! ## W6 — subcommand entry point
//!
//! `mustard-rt run i18n translate-heading` and `mustard-rt run spec-lang
//! resolve` are listed in the W4 spec but explicitly deferred to W6. They
//! will live next to this module; for now the rt run-face does not register
//! a `SpecLang` variant.

// W6: the public helpers below are the entry point that the deferred
// `i18n translate-heading` / `spec-lang resolve` subcommands will call. They
// stay `#[allow(dead_code)]` until W6 wires the dispatch — exposing them as
// public API today keeps the surface stable across the wave gap.
#![allow(dead_code)]

use mustard_core::{slugify, Locale, LocaleError};
use std::str::FromStr;

/// Slugify `title` for `lang`. PT strips accents, EN does not.
///
/// This is the typed-locale variant of the legacy ascii-only
/// `scan::interpret::slugify(&str)`; callers that already hold a
/// [`Locale`] should prefer this entry point so the choice is explicit.
#[must_use]
pub fn for_locale(title: &str, lang: Locale) -> String {
    slugify(title, lang)
}

/// Slugify `title` using a BCP-47 locale string. A missing / malformed code
/// degrades to [`Locale::default`] (`pt-BR`) — fail-open so the slug never
/// blocks a write.
///
/// The legacy short forms (`pt` / `en`) are accepted on read and expanded to
/// their BCP-47 peers, mirroring the lenient parse in `meta.json`.
#[must_use]
pub fn for_lang(title: &str, raw_lang: &str) -> String {
    let lang = match Locale::from_str(raw_lang) {
        Ok(l) => l,
        Err(LocaleError::ShortForm(s)) => {
            if s.eq_ignore_ascii_case("pt") {
                Locale::PtBr
            } else {
                Locale::EnUs
            }
        }
        Err(_) => Locale::default(),
    };
    slugify(title, lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_locale_strips_pt_accents() {
        assert_eq!(
            for_locale("Configuração de Idioma e Tom", Locale::PtBr),
            "configuracao-idioma-tom"
        );
    }

    #[test]
    fn for_locale_keeps_en_intact() {
        assert_eq!(
            for_locale("Language and Tone Settings", Locale::EnUs),
            "language-tone-settings"
        );
    }

    #[test]
    fn for_lang_accepts_bcp47_and_short_forms() {
        // BCP-47 canonical.
        assert_eq!(for_lang("Olá Mundo", "pt-BR"), "ola-mundo");
        // Legacy short form is normalised, not rejected.
        assert_eq!(for_lang("Olá Mundo", "pt"), "ola-mundo");
        // Unknown locale falls back to default (PtBr).
        assert_eq!(for_lang("Configuração", "klingon"), "configuracao");
    }

    #[test]
    fn empty_input_degrades_to_x() {
        assert_eq!(for_locale("", Locale::PtBr), "x");
        assert_eq!(for_locale("///", Locale::EnUs), "x");
    }
}
