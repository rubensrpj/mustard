//! Lang-aware spec slug helper (Wave 4 of `mustard-unification`).
//!
//! Spec slugs (`.claude/spec/{slug}/spec.md`) are kebab-case identifiers
//! derived from a free-form title (e.g. `"Configuração de Idioma e Tom"` →
//! `"configuracao-idioma-tom"`). The pt-BR path needs accent stripping; the
//! en-US path does not. Stopword lists differ per locale.
//!
//! [`canonical`] is the ONE derivation that names a work unit, and it lives
//! here so the three callers that must agree about it — the base gate that
//! mints the name, the `{base}_{slug}` work branch, and the `spec-draft` that
//! files the spec directory — call the same function instead of each writing
//! their own BCP-47 parse / cap / fail-open dance.
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
use std::path::Path;
use std::str::FromStr;

/// Max number of words kept in a work unit's canonical slug. A paragraph-length
/// intent is cut here — on a word boundary, never mid-word (the old 60-char
/// `.take` decapitated the final word, e.g. `…contas-a-r`).
const SLUG_MAX_TOKENS: usize = 5;

/// Resolve a BCP-47 locale string, fail-open. A missing / malformed code
/// degrades to [`Locale::default`] (`pt-BR`) so a slug never blocks a write; the
/// legacy short forms (`pt` / `en`) are accepted on read and expanded to their
/// BCP-47 peers, mirroring the lenient parse in `meta.json`.
fn parse_locale(raw_lang: &str) -> Locale {
    match Locale::from_str(raw_lang) {
        Ok(l) => l,
        Err(LocaleError::ShortForm(s)) => {
            if s.eq_ignore_ascii_case("pt") {
                Locale::PtBr
            } else {
                Locale::EnUs
            }
        }
        Err(_) => Locale::default(),
    }
}

/// The CANONICAL name of a work unit, derived from its free-text intent: the
/// per-locale [`mustard_core::slugify`] capped to [`SLUG_MAX_TOKENS`] words.
///
/// ONE derivation with several callers, deliberately. The base gate mints the
/// name here (`emit-pipeline --kind pipeline.kind`), [`compute_work_branch`]
/// builds `{base}_{slug}` on top of it, and `spec-draft` names the spec
/// directory with it. A second spelling anywhere is how a unit ends up carrying
/// TWO names — the branch under one, the spec directory under another — which
/// `inside_own_work_branch` then reports as "not inside your own unit".
///
/// A shared function is not a shared ARGUMENT, though: two callers passing
/// different intents (or different locales) still get different names. That is
/// why the name is minted ONCE, at the gate, and carried from there.
///
/// [`compute_work_branch`]: crate::commands::event::work_branch::compute_work_branch
/// [`inside_own_work_branch`]: crate::commands::pipeline::resume_bootstrap
#[must_use]
pub fn canonical(intent: &str, lang: Locale) -> String {
    slugify(intent, lang)
        .split('-')
        .take(SLUG_MAX_TOKENS)
        .collect::<Vec<_>>()
        .join("-")
}

/// [`canonical`] against the language the PROJECT declares — `mustard.json#lang`
/// then the legacy `specLang`, defaulting to `pt-BR` (the precedence
/// [`mustard_core::ProjectConfig::i18n`] applies).
///
/// The callers that hold only a project root — the base gate and the
/// work-branch name — resolve the locale through here, so they cannot each pick
/// a different one. Fail-open: an unreadable `mustard.json` yields the default.
#[must_use]
pub fn canonical_for_project(intent: &str, project: &Path) -> String {
    let config = mustard_core::ProjectConfig::load(project);
    let declared = config.lang.as_deref().or(config.spec_lang.as_deref());
    canonical(intent, declared.map_or_else(Locale::default, parse_locale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_parse_accepts_bcp47_and_short_forms() {
        // BCP-47 canonical.
        assert_eq!(canonical("Olá Mundo", parse_locale("pt-BR")), "ola-mundo");
        // Legacy short form is normalised, not rejected.
        assert_eq!(canonical("Olá Mundo", parse_locale("pt")), "ola-mundo");
        // Unknown locale falls back to the default (PtBr).
        assert_eq!(canonical("Configuração", parse_locale("klingon")), "configuracao");
    }

    #[test]
    fn empty_input_degrades_to_x() {
        assert_eq!(canonical("", Locale::PtBr), "x");
        assert_eq!(canonical("///", Locale::EnUs), "x");
    }

    #[test]
    fn canonical_is_kebab_and_word_bounded() {
        assert_eq!(canonical("Add user CRUD", Locale::EnUs), "add-user-crud");
        assert_eq!(canonical("  ---  Fix login   bug  ", Locale::EnUs), "fix-login-bug");
    }

    #[test]
    fn canonical_caps_on_a_word_boundary() {
        // 10 content words → first 5 kept, cut on a boundary (no partial word).
        let s = canonical("alpha beta gamma delta epsilon zeta eta theta iota kappa", Locale::EnUs);
        assert_eq!(s, "alpha-beta-gamma-delta-epsilon");
    }

    #[test]
    fn canonical_drops_stopwords_and_never_cuts_mid_word() {
        // Field report (sialia): the hand-rolled slug kept "em/a/de" and cut
        // "receber" → "r". Delegating to slugify drops stopwords per-locale and
        // the token cap lands on a word boundary.
        let s = canonical(
            "Espelhar em contas a pagar a visão de listagem de contas a receber",
            Locale::PtBr,
        );
        assert_eq!(s, "espelhar-contas-pagar-visao-listagem");
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn canonical_for_project_reads_the_declared_language() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let intent = "Corrigir o botao de login";

        // No `mustard.json` at all ⇒ the pt-BR default: `o` and `de` are
        // Portuguese stopwords and go.
        assert_eq!(canonical_for_project(intent, root), "corrigir-botao-login");

        // A declared `lang` decides — the SAME string names a different unit
        // under en-US, where neither word is a stopword. That is exactly why
        // the callers that hold only a project root resolve the locale here,
        // in one place, instead of each picking their own.
        std::fs::write(root.join("mustard.json"), r#"{"lang":"en-US"}"#).unwrap();
        assert_eq!(canonical_for_project(intent, root), "corrigir-o-botao-de-login");
    }
}
