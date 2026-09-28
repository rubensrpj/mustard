//! Lang-aware spec slug helper.
//!
//! Spec slugs (`.claude/spec/{slug}/spec.md`) are kebab-case identifiers
//! derived from a free-form title (e.g. `"Configuração de Idioma e Tom"` →
//! `"configuracao-idioma-tom"`). Accents are stripped in every language; the
//! words left out are the `slug:` line of the language file of the language
//! the title is written in.
//!
//! [`canonical`] is the ONE derivation that names a work unit from its intent,
//! and it lives here so every caller that must agree about the name calls the
//! same function instead of each writing their own BCP-47 parse / cap /
//! fail-open dance. Today the only one is the base gate's overlap check, and
//! the gate itself has no caller.
//!
//! ## Fail-open
//!
//! Every helper accepts free-form input. An empty or fully non-alphanumeric
//! input degrades to `"x"` (the floor inherited from the legacy slug contract).

use mustard_core::domain::normalize::text_language;
use mustard_core::slugify;
use std::path::Path;

/// Max number of words kept in a work unit's canonical slug. A paragraph-length
/// intent is cut here — on a word boundary, never mid-word (the old 60-char
/// `.take` decapitated the final word, e.g. `…contas-a-r`).
const SLUG_MAX_TOKENS: usize = 5;

/// The CANONICAL name of a work unit, derived from its free-text intent: the
/// [`mustard_core::slugify`] of the intent in `language` (a BCP-47 code, any
/// language) capped to [`SLUG_MAX_TOKENS`] words.
///
/// A shared function is not a shared ARGUMENT, though: two callers passing
/// different intents (or different languages) still get different names. That
/// is why the name is minted ONCE and carried from there.
#[must_use]
pub fn canonical(intent: &str, language: &str) -> String {
    slugify(intent, language)
        .split('-')
        .take(SLUG_MAX_TOKENS)
        .collect::<Vec<_>>()
        .join("-")
}

/// [`canonical`] in the text language the PROJECT declares (`language.text`),
/// as written — `es-ES` stays `es-ES`, even though Mustard has no messages in
/// it — and `pt-BR` when none was declared. Read through
/// [`text_language`], the same reader the search uses, and never through the
/// closed set of message languages, which would name a Spanish unit by the
/// Portuguese rules.
///
/// The callers that hold only a project root resolve the language through
/// here, so they cannot each pick a different one. Fail-open: an unreadable
/// `mustard.json` yields the default.
#[must_use]
pub fn canonical_for_project(intent: &str, project: &Path) -> String {
    let language = text_language(&mustard_core::ProjectConfig::load(project));
    canonical(intent, &language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_strips_accents_in_portuguese() {
        assert_eq!(canonical("Olá Mundo", "pt-BR"), "ola-mundo");
        assert_eq!(canonical("Configuração", "pt-BR"), "configuracao");
    }

    #[test]
    fn empty_input_degrades_to_x() {
        assert_eq!(canonical("", "pt-BR"), "x");
        assert_eq!(canonical("///", "en-US"), "x");
    }

    #[test]
    fn canonical_is_kebab_and_word_bounded() {
        assert_eq!(canonical("Add user CRUD", "en-US"), "add-user-crud");
        assert_eq!(canonical("  ---  Fix login   bug  ", "en-US"), "fix-login-bug");
    }

    #[test]
    fn canonical_caps_on_a_word_boundary() {
        // 10 content words → first 5 kept, cut on a boundary (no partial word).
        let s = canonical("alpha beta gamma delta epsilon zeta eta theta iota kappa", "en-US");
        assert_eq!(s, "alpha-beta-gamma-delta-epsilon");
    }

    #[test]
    fn canonical_drops_stopwords_and_never_cuts_mid_word() {
        // Field report (sialia): the hand-rolled slug kept "em/a/de" and cut
        // "receber" → "r". Delegating to slugify drops stopwords per-locale and
        // the token cap lands on a word boundary.
        let s = canonical(
            "Espelhar em contas a pagar a visão de listagem de contas a receber",
            "pt-BR",
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

        // A declared `language.text` decides — the SAME string names a
        // different unit under en-US, where neither word is a stopword. That
        // is exactly why the callers that hold only a project root resolve the
        // locale here, in one place, instead of each picking their own.
        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        assert_eq!(canonical_for_project(intent, root), "corrigir-o-botao-de-login");

        // The old language key is not read: the project declared nothing.
        std::fs::write(root.join("mustard.json"), r#"{"lang":"en-US"}"#).unwrap();
        assert_eq!(canonical_for_project(intent, root), "corrigir-botao-login");
    }

    /// A língua declarada fora das mensagens do Mustard dá o nome pelas regras
    /// dela, e não pelas do português: o "no" do espanhol é negação e fica no
    /// nome, e o acento sai do mesmo jeito.
    #[test]
    fn canonical_for_project_names_by_a_language_outside_the_messages() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let intent = "El botón no guarda";

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"es-ES"}}"#).unwrap();
        assert_eq!(canonical_for_project(intent, root), "el-boton-no-guarda");

        // A língua sem arquivo também é a declarada: nenhuma palavra sai, e o
        // padrão do português não entra no lugar dela.
        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":" xx-YY "}}"#).unwrap();
        assert_eq!(canonical_for_project("Corrigir o botão de login", root), "corrigir-o-botao-de-login");

        // Em branco é não declarada: vale o padrão, o português.
        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"  "}}"#).unwrap();
        assert_eq!(canonical_for_project(intent, root), "el-boton-guarda");
    }
}
