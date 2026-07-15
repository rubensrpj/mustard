//! Lang-aware spec slug helper (Wave 4 of `mustard-unification`).
//!
//! Spec slugs (`.claude/spec/{slug}/spec.md`) are kebab-case identifiers
//! derived from a free-form title (e.g. `"Configuração de Idioma e Tom"` →
//! `"configuracao-idioma-tom"`). The pt-BR path needs accent stripping; the
//! en-US path does not. Stopword lists differ per locale.
//!
//! This is a thin facade over [`mustard_core::slugify`] kept here so any rt
//! caller can `crate::commands::spec::spec_slug::for_lang` without having to think about
//! the BCP-47 parse / fail-open dance every time.
//!
//! ## Fail-open
//!
//! Every helper accepts free-form input. An empty or fully non-alphanumeric
//! input degrades to `"x"` (the floor inherited from the legacy slug contract).
//!
//! ## W6 — subcommand entry point
//!
//! `mustard-rt run spec-lang resolve` is listed in the W4 spec but was
//! explicitly deferred to W6. It lives next to this module.

use mustard_core::{slugify, LocaleError, SupportedLocale as Locale};
use std::str::FromStr;

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
        assert_eq!(for_lang("", "pt-BR"), "x");
        assert_eq!(for_lang("///", "en-US"), "x");
    }
}
