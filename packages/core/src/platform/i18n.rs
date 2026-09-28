//! `i18n` — central language module for Mustard banners.
//!
//! ## Why
//!
//! Before this module, hardcoded pt-BR strings lived in
//! `apps/rt/src/hooks/*.rs` (e.g. `amend_capture.rs`) and in
//! `apps/cli/src/commands/*.rs`. Bilingual lookup tables were copy-pasted
//! across three or more files. There was no single place to:
//!
//! - declare the canonical locale codes (BCP-47, never short forms);
//! - translate a banner key into the user's language;
//! - slugify free-form text in a way that respects PT-vs-EN accent rules.
//!
//! This module is now that single place, a boundary-typed module exported
//! from `mustard_core`.
//!
//! ## Locale vocabulary
//!
//! - [`Locale`] — BCP-47 typed locale. Only `pt-BR` and `en-US` are accepted;
//!   the legacy short forms `pt` / `en` are rejected with
//!   [`LocaleError::ShortForm`] (see memory `project_locale_codes`).
//! - [`I18n`] — the locale callers thread through banner rendering.
//!
//! The catalogue is written in one voice only, plain and didactic: there is
//! no tone to choose, so nothing here rewrites a translation after lookup.
//!
//! ## Canonical banner keys
//!
//! Banners are keyed by dotted-namespace identifiers. The texts live in the
//! parts of the catalogue under `i18n/`, one file per subject, and every
//! reader goes through [`translate`], the catalogue's single door.
//!
//! ## Forward compatibility
//!
//! New keys land in the catalogue part of their subject, not in consumer
//! crates. A missing key returns the key string itself so the caller still
//! emits *something*; this is the fail-open contract that keeps a typo in a
//! hook from blocking user work.

use std::fmt;
use std::str::FromStr;

/// BCP-47 locale code used by the spec/header cascade.
///
/// Only `pt-BR` and `en-US` are valid Mustard locales. Short forms (`pt`,
/// `en`) are rejected by [`Locale::from_str`] with [`LocaleError::ShortForm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    /// Brazilian Portuguese, BCP-47 `pt-BR`.
    PtBr,
    /// United States English, BCP-47 `en-US`.
    EnUs,
}

impl Locale {
    /// Canonical BCP-47 code for this locale.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PtBr => "pt-BR",
            Self::EnUs => "en-US",
        }
    }
}

impl Default for Locale {
    /// pt-BR is the default for Mustard banners (the project's primary user
    /// locale per `project_locale_codes`).
    fn default() -> Self {
        Self::PtBr
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Locale parse errors.
///
/// `ShortForm` is intentionally distinct from `Unknown` so callers can warn the
/// user that their config still uses the legacy `pt`/`en` short codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocaleError {
    /// The input is the legacy short form (`pt` / `en`). Reject and ask the
    /// caller to upgrade to BCP-47.
    ShortForm(String),
    /// The input is not a recognised Mustard locale.
    Unknown(String),
}

impl fmt::Display for LocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShortForm(s) => write!(
                f,
                "locale {s:?} is a legacy short form; use a BCP-47 code (pt-BR / en-US)"
            ),
            Self::Unknown(s) => write!(f, "unknown locale {s:?}; expected pt-BR or en-US"),
        }
    }
}

impl std::error::Error for LocaleError {}

impl FromStr for Locale {
    type Err = LocaleError;

    /// Parse a BCP-47 code. Trimming + case-insensitive on the region part.
    /// Short forms (`pt` / `en`) are explicitly rejected — callers should
    /// surface the error and ask the user to update their config to the
    /// canonical BCP-47 spelling.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        // Reject short forms up-front with a typed error.
        let lc = trimmed.to_ascii_lowercase();
        if lc == "pt" || lc == "en" {
            return Err(LocaleError::ShortForm(trimmed.to_string()));
        }
        // BCP-47: `xx-YY` — language lowercase, region uppercase. Accept
        // mixed-case input by normalising.
        match lc.as_str() {
            "pt-br" => Ok(Self::PtBr),
            "en-us" => Ok(Self::EnUs),
            _ => Err(LocaleError::Unknown(trimmed.to_string())),
        }
    }
}

/// Banner-rendering context: the locale.
///
/// Threaded through hook / CLI banner code so a single struct call replaces
/// the bilingual lookup tables that used to sit in each module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct I18n {
    /// User locale (drives [`translate`]).
    pub lang: Locale,
}

impl I18n {
    /// Build an `I18n` for `lang`.
    #[must_use]
    pub fn new(lang: Locale) -> Self {
        Self { lang }
    }

    /// Translate `key` into this locale.
    #[must_use]
    pub fn render(&self, key: &str) -> String {
        translate(key, self.lang).to_string()
    }
}

mod flow;
mod survey;
mod prompt;
mod gates;
mod pending;
mod session;
mod map;
mod events;
mod page;
mod install;
mod spec_text;

/// Uma parte do catálogo: os começos de chave que ela responde e a leitura dela.
type Part = (&'static [&'static str], fn(&str, Locale) -> Option<&'static str>);

/// As partes do catálogo de textos, uma por arquivo em `i18n/`. Cada parte
/// cuida de um assunto, e a chave nova entra na parte do assunto dela:
///
/// - `flow` — os passos do fluxo de uma spec;
/// - `survey` — o levantamento;
/// - `prompt` — o pedido da onda;
/// - `gates` — os portões dos ganchos;
/// - `pending` — as pendências;
/// - `session` — a sessão e a barra de status;
/// - `map` — o mapa do projeto;
/// - `events` — o arquivo de eventos da spec;
/// - `page` — as páginas;
/// - `install` — o diagnóstico da instalação;
/// - `spec_text` — o texto da spec em markdown.
const PARTS: [Part; 11] = [
    (flow::PREFIXES, flow::text),
    (survey::PREFIXES, survey::text),
    (prompt::PREFIXES, prompt::text),
    (gates::PREFIXES, gates::text),
    (pending::PREFIXES, pending::text),
    (session::PREFIXES, session::text),
    (map::PREFIXES, map::text),
    (events::PREFIXES, events::text),
    (page::PREFIXES, page::text),
    (install::PREFIXES, install::text),
    (spec_text::PREFIXES, spec_text::text),
];

/// Translate `key` into a literal banner string for `lang`.
///
/// A missing key returns the key itself (fail-open: the caller still emits
/// *something*). The key is read only in the catalogue part that declares its
/// prefix (the text before the first dot); `PARTS` lists the parts and the
/// subject of each. Adding a new banner = adding one arm per locale to the
/// `match` of the part of its subject.
///
/// Lifetime: returns `&'static str` because every entry is a string literal —
/// no allocation in the hot banner path.
#[must_use]
pub fn translate(key: &str, lang: Locale) -> &'static str {
    let prefix = key.split_once('.').map_or(key, |(head, _)| head);
    PARTS
        .iter()
        .find(|(prefixes, _)| prefixes.contains(&prefix))
        .and_then(|(_, text)| text(key, lang))
        // Fail-open: unknown key returns the key itself so callers always have
        // *something* to render. This is what `karpathy-guidelines` calls a
        // "safe default" — never panic on a typo in a hook.
        .unwrap_or_else(|| key_as_static(key))
}

/// Promote a `&str` to `&'static str` *only* for the fail-open path of
/// [`translate`]. Returns the well-known literal `<missing-key>` so we never
/// leak arbitrary unbounded `&str` into a static slot.
#[must_use]
fn key_as_static(_key: &str) -> &'static str {
    "<missing-key>"
}

/// Slugify `text` to a kebab-case identifier, in the language it is written in.
///
/// `language` is a BCP-47 code (`pt-BR`, `es-ES`) or just the language (`es`),
/// any of them, not only the locales Mustard ships messages for. The words
/// dropped are the `slug:` line of that language's file
/// (`domain::normalize::slug_words`): basic articles and prepositions only; a
/// language without the line keeps every word.
///
/// Latin diacritics are stripped (`ç → c`, `ñ → n`, …) in every language,
/// before kebab-casing: the slug is ASCII, and an accented letter left in
/// would split its word in two (`configuraci-n`).
///
/// The output never contains leading/trailing dashes and never collapses to an
/// empty string — fully non-alphanumeric input degrades to `"x"`, mirroring
/// the existing `apps/rt/src/run/scan/interpret.rs::slugify` contract.
#[must_use]
pub fn slugify(text: &str, language: &str) -> String {
    let normalised = crate::domain::text::fold_accents(text);
    let languages = crate::domain::normalize::Languages::new([language]);
    let stopwords: Vec<&str> =
        languages.codes().iter().map(String::as_str).flat_map(crate::domain::normalize::slug_words).collect();
    // 1. lowercase + split on non-alphanumeric.
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in normalised.chars() {
        let lc = ch.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() {
            cur.push(lc);
        } else if !cur.is_empty() {
            tokens.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    // 2. drop stopwords — but only when at least one token in the input is
    //    longer than a single character. Pure single-char inputs (e.g.
    //    `"ç ã õ"`) and pure-stopword inputs keep every token so callers
    //    always get *something* slug-shaped back. After filtering, if nothing
    //    is left, fall back to the original tokens.
    let has_long = tokens.iter().any(|tok| tok.chars().count() > 1);
    let kept: Vec<String> = if has_long {
        let filtered: Vec<String> = tokens
            .iter()
            .filter(|tok| !stopwords.contains(&tok.as_str()))
            .cloned()
            .collect();
        if filtered.is_empty() { tokens } else { filtered }
    } else {
        tokens
    };
    let joined = kept.join("-");
    if joined.is_empty() {
        "x".to_string()
    } else {
        joined
    }
}

// ---------------------------------------------------------------------------
// Type aliases — `SupportedLocale` (catalogue) + `UserLocale` (open BCP-47)
// ---------------------------------------------------------------------------

/// Catalogue-backed locale — the closed set Mustard ships translations for.
///
/// `SupportedLocale` is a type alias for the original [`Locale`] enum, so each
/// callsite could move to the new name without breaking every consumer at once.
pub type SupportedLocale = Locale;

/// User-declared BCP-47 locale, as a spec records it.
///
/// Unlike [`SupportedLocale`] (closed, two variants), `UserLocale` accepts any
/// syntactically valid BCP-47 code so users can write specs in `fr-FR`, `de-DE`,
/// etc. Parse the raw tag into a [`SupportedLocale`] when a banner needs to
/// render, falling back to the default when the locale is not in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserLocale {
    /// The raw BCP-47 tag as supplied by the user.
    pub raw: String,
}

impl UserLocale {
    /// Construct a `UserLocale` from a BCP-47 string.  No validation is
    /// performed — any non-empty string is accepted so fail-open callers never
    /// have to handle an error for syntactically arbitrary user input.
    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }
}

impl fmt::Display for UserLocale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for UserLocale {
    type Err = UserLocaleError;

    /// Parse a BCP-47 string into a `UserLocale`. Rejects empty strings and
    /// shapes that are not `<lang>-<REGION>` (2-3 lowercase letters, hyphen,
    /// 2 uppercase letters). Short forms like `pt`/`en` and unhyphenated
    /// blobs like `ptbr` are rejected so callers can rely on a canonical tag.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(UserLocaleError::Empty);
        }
        let (lang, region) = trimmed
            .split_once('-')
            .ok_or_else(|| UserLocaleError::Malformed(trimmed.to_string()))?;
        let lang_ok = (2..=3).contains(&lang.len())
            && lang.chars().all(|c| c.is_ascii_lowercase());
        let region_ok = region.len() == 2 && region.chars().all(|c| c.is_ascii_uppercase());
        if !lang_ok || !region_ok {
            return Err(UserLocaleError::Malformed(trimmed.to_string()));
        }
        Ok(Self { raw: trimmed.to_string() })
    }
}

/// Errors returned by [`UserLocale::from_str`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserLocaleError {
    /// Empty or whitespace-only input.
    Empty,
    /// Input does not match the `<lang>-<REGION>` shape.
    Malformed(String),
}

impl fmt::Display for UserLocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("locale string is empty"),
            Self::Malformed(raw) => write!(f, "locale `{raw}` is not BCP-47 `<lang>-<REGION>`"),
        }
    }
}

impl std::error::Error for UserLocaleError {}

/// Render a wave label given a locale + 1-based wave index.
///
/// `Locale::PtBr` → `"Onda 3"`, `Locale::EnUs` → `"W3"`. Reused by the rt
/// dispatch layer and the dashboard banners so the format stays in sync.
#[must_use]
pub fn wave_label(n: u32, lang: Locale) -> String {
    match lang {
        Locale::PtBr => format!("{} {n}", translate("wave.label", lang)),
        // EN uses the compact `W3` form — no separating space.
        Locale::EnUs => format!("{}{n}", translate("wave.label", lang)),
    }
}

// ---------------------------------------------------------------------------
// File-operation markers (`## Files` bullet annotations)
// ---------------------------------------------------------------------------

/// Every catalogue locale, EN canonical first — the iteration order of
/// [`file_marker_synonyms`], so the EN spelling is always `synonyms[0]`.
const CATALOGUE_LOCALES: &[Locale] = &[Locale::EnUs, Locale::PtBr];

/// A file-operation marker recognised in a spec's `## Files` bullet lines —
/// e.g. ``- `src/Payable.cs` (create)``. `Create` declares a net-new file
/// (validators must not flag it as missing); `Edit` declares a change to an
/// existing file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileMarker {
    /// Net-new file — `(create)` / `(new)` / `(novo)` / `(criar)`.
    Create,
    /// Existing file to change — `(edit)` / `(editar)`.
    Edit,
}

impl FileMarker {
    /// Catalogue key carrying this marker's per-locale synonyms.
    fn catalogue_key(self) -> &'static str {
        match self {
            Self::Create => "marker.create",
            Self::Edit => "marker.edit",
        }
    }
}

/// Every accepted spelling of `marker`, across ALL catalogue locales, deduped,
/// EN canonical first (`(create)` for [`FileMarker::Create`]). The synonyms
/// are data in the [`translate`] catalogue (`marker.*` keys, `|`-separated per
/// locale) — the SINGLE origin shared by the drafter and every validator
/// (`analyze-validation`, scope-classify), so a localized marker like the
/// pt-BR `(novo)` can never drift out of recognition.
///
/// Spellings are lowercase literals including the surrounding parentheses;
/// match with [`line_has_file_marker`] (case-insensitive `contains`).
#[must_use]
pub fn file_marker_synonyms(marker: FileMarker) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for lang in CATALOGUE_LOCALES {
        for syn in translate(marker.catalogue_key(), *lang).split('|') {
            let syn = syn.trim();
            if !syn.is_empty() && !out.contains(&syn) {
                out.push(syn);
            }
        }
    }
    out
}

/// Whether `line` carries `marker` in ANY of its accepted spellings
/// (case-insensitive substring, like the historical `(create)` check).
/// Fail-open helper for `## Files` bullet validation: a line such as
/// ``- `src/Payable.cs` (novo)`` matches [`FileMarker::Create`].
#[must_use]
pub fn line_has_file_marker(line: &str, marker: FileMarker) -> bool {
    let lower = line.to_lowercase();
    file_marker_synonyms(marker).iter().any(|syn| lower.contains(syn))
}

// ---------------------------------------------------------------------------
// Palavras em maiúsculas nas frases do programa
// ---------------------------------------------------------------------------

/// As palavras em maiúsculas que podem ficar fora de crase numa frase do
/// programa: siglas e unidades de uso comum. Uma sigla nova só passa se
/// alguém a puser aqui de propósito.
pub const CAPS_ALLOWED: [&str; 4] = ["JSON", "UTF", "MB", "GB"];

/// Cada palavra de duas letras ou mais, toda em maiúsculas e fora de crase,
/// uma vez só, na ordem em que aparece, salvo as de [`CAPS_ALLOWED`].
///
/// É a regra única das frases fixas do programa, as do catálogo e as da
/// ajuda dos comandos: elas são escritas por nós, então a conferência não
/// adivinha se a palavra é grito ou sigla, e o nome de código vai entre
/// crases. Lista vazia quer dizer que o texto segue a regra.
#[must_use]
pub fn uppercase_words(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for outside in text.split('`').step_by(2) {
        for word in outside.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            let letters = word.chars().filter(|c| c.is_alphabetic()).count();
            let upper = letters >= 2 && !word.chars().any(char::is_lowercase);
            if upper && !CAPS_ALLOWED.contains(&word) && !found.contains(&word) {
                found.push(word);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Confere uma parte do catálogo pelo texto do arquivo dela. Cada chave
    /// começa por um dos começos que a parte declara, a porta responde cada
    /// uma nos dois idiomas, e o número de chaves e a impressão dos textos
    /// são os que a parte gravou. Uma chave que sai, que vai para a parte
    /// errada ou que muda de texto derruba a conferência.
    pub(super) fn assert_part_unchanged(source: &str, prefixes: &[&str], keys: usize, fingerprint: u64) {
        let found = part_keys(source);
        // A impressão é o FNV-1a de 64 bits sobre chave, idioma e texto, na
        // ordem das chaves: estável entre versões do Rust, ao contrário do
        // hasher da biblioteca padrão.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for key in &found {
            let prefix = key.split_once('.').map_or(*key, |(head, _)| head);
            assert!(prefixes.contains(&prefix), "{key} starts with {prefix}, which this part does not declare");
            for lang in [Locale::PtBr, Locale::EnUs] {
                let text = translate(key, lang);
                assert_ne!(text, "<missing-key>", "{key} has no text in {lang}");
                for byte in key.bytes().chain([0]).chain(lang.as_str().bytes()).chain([0]).chain(text.bytes()).chain([0xff]) {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        let shown = format!(
            "0x{:04x}_{:04x}_{:04x}_{:04x}",
            hash >> 48,
            (hash >> 32) & 0xffff,
            (hash >> 16) & 0xffff,
            hash & 0xffff
        );
        assert!(
            (found.len(), hash) == (keys, fingerprint),
            "the part changed: it now holds {} keys with fingerprint {shown}; if the change is on purpose, \
             write these two numbers in the part's test",
            found.len()
        );
    }

    /// As chaves de uma parte, lidas do texto do arquivo dela: cada braço do
    /// `match` antes dos testes, no formato `("chave", Locale::PtBr) =>`,
    /// `("chave", Locale::EnUs) =>` ou `("chave", _) =>`.
    fn part_keys(source: &str) -> BTreeSet<&str> {
        let arms = source.split("#[cfg(test)]").next().unwrap_or(source);
        let mut found = BTreeSet::new();
        for line in arms.lines().map(str::trim_start).filter(|line| line.starts_with("(\"")) {
            let (key, pattern) = line[2..].split_once('"').expect("the key closes its quotes");
            assert!(
                [", Locale::PtBr) =>", ", Locale::EnUs) =>", ", _) =>"].iter().any(|shape| pattern.starts_with(shape)),
                "each arm is written (\"key\", Locale::PtBr) =>, (\"key\", Locale::EnUs) => or (\"key\", _) =>: {line}"
            );
            found.insert(key);
        }
        found
    }

    /// O texto de cada parte do catálogo, na mesma ordem de `PARTS`. O tamanho
    /// vem de `PARTS`: uma parte nova não compila sem o texto dela aqui.
    const PART_SOURCES: [&str; PARTS.len()] = [
        include_str!("i18n/flow.rs"),
        include_str!("i18n/survey.rs"),
        include_str!("i18n/prompt.rs"),
        include_str!("i18n/gates.rs"),
        include_str!("i18n/pending.rs"),
        include_str!("i18n/session.rs"),
        include_str!("i18n/map.rs"),
        include_str!("i18n/events.rs"),
        include_str!("i18n/page.rs"),
        include_str!("i18n/install.rs"),
        include_str!("i18n/spec_text.rs"),
    ];

    /// O texto com cada marcador (`{spec}`, `{count}`…) trocado por uma
    /// palavra, como ele chega a quem lê. Chave com outra coisa dentro, como o
    /// JSON de um exemplo, fica como está.
    fn with_markers_filled(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            match after.find('}') {
                Some(close) if close > 0 && after[..close].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                    out.push_str("item");
                    rest = &after[close + 1..];
                }
                _ => {
                    out.push('{');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// Os defeitos de escrita de um texto do catálogo, cada um com a chave e o
    /// idioma: os da conferência das respostas e, além deles, cada palavra
    /// toda em maiúsculas fora de crase. O texto é medido com as lacunas já
    /// preenchidas.
    fn catalog_text_defects(key: &str, lang: Locale, raw: &str) -> Vec<String> {
        let text = with_markers_filled(raw);
        let report = crate::domain::clarity::measure(&text, &[], Some(lang));
        let mut defects: Vec<String> =
            report.defects(lang).into_iter().map(|defect| format!("{key} ({lang}): {defect}")).collect();
        for word in uppercase_words(&text) {
            defects.push(format!("{key} ({lang}): uppercase word {word} outside backticks"));
        }
        defects
    }

    /// Toda frase do catálogo, nos dois idiomas, passa na mesma conferência de
    /// escrita das respostas: frase de até 25 palavras, sigla explicada, nenhum
    /// código interno, leitura fácil e o idioma certo. Além dela, nenhuma
    /// palavra toda em maiúsculas fora de crase, salvo a lista curta de
    /// exceções. Cada texto é medido sozinho, com os
    /// marcadores já trocados por uma palavra, porque é assim que ele chega a
    /// quem lê. A falha lista cada chave, o idioma e o defeito.
    #[test]
    fn every_catalog_text_reads_clearly() {
        let mut failures = Vec::new();
        let mut measured = 0;
        for source in PART_SOURCES {
            for key in part_keys(source) {
                for lang in [Locale::PtBr, Locale::EnUs] {
                    measured += 1;
                    failures.extend(catalog_text_defects(key, lang, translate(key, lang)));
                }
            }
        }
        assert!(measured > 1_000, "the measure reached every part, in both languages: {measured} texts");
        assert!(
            failures.is_empty(),
            "{} catalog texts fail the writing check:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// A conferência do catálogo mede o texto como ele chega a quem lê: o
    /// marcador vira uma palavra, e a frase que passa de 25 palavras só com
    /// ele cai.
    #[test]
    fn a_marker_counts_as_one_word_in_the_catalog_measure() {
        assert_eq!(with_markers_filled("A spec {spec} tem {count} itens."), "A spec item tem item itens.");
        assert_eq!(with_markers_filled("O exemplo {\"class\":\"x\"} fica."), "O exemplo {\"class\":\"x\"} fica.");
        assert_eq!(with_markers_filled("Sem par { aqui."), "Sem par { aqui.");
        let words = |n: usize| vec!["palavra"; n].join(" ");
        let measure = |text: &str| crate::domain::clarity::measure(&with_markers_filled(text), &[], None);
        assert!(measure(&format!("{} {{spec}}.", words(24))).long_sentences.is_empty(), "25 words pass");
        assert_eq!(measure(&format!("{} {{spec}}.", words(25))).long_sentences.len(), 1, "26 words fail");
    }

    /// Palavra toda em maiúsculas fora de crase derruba o texto do catálogo,
    /// com a chave, o idioma e a palavra: o "NÃO" solto, o nome de código e a
    /// sigla fora da lista, mesmo explicada. Entre crases ela passa, e as
    /// exceções da lista também.
    #[test]
    fn an_uppercase_word_outside_backticks_fails_the_catalog_measure() {
        let key = "spec_events.report_carries_return_line";
        let defect = |word: &str| format!("{key} (pt-BR): uppercase word {word} outside backticks");
        assert_eq!(catalog_text_defects(key, Locale::PtBr, "Isso NÃO apaga nada."), vec![defect("NÃO")]);
        assert_eq!(
            catalog_text_defects(key, Locale::PtBr, "O relatório leva as linhas USAGE, PAUSED e USAGE de novo."),
            vec![defect("USAGE"), defect("PAUSED")]
        );
        assert_eq!(catalog_text_defects(key, Locale::PtBr, "Busque pelo código, como DEC-0142."), vec![defect("DEC")]);
        assert_eq!(catalog_text_defects(key, Locale::PtBr, "O PR (pull request) de {spec} saiu."), vec![defect("PR")]);
        for calm in [
            "Isso `NÃO` apaga nada.",
            "O relatório leva as linhas `USAGE` e `PAUSED`.",
            "Busque pelo código, como `DEC-0142`.",
            "O JSON em UTF-8 passa de 2 MB, longe de 1 GB.",
            "O item A de {spec} saiu.",
        ] {
            assert_eq!(catalog_text_defects(key, Locale::PtBr, calm), Vec::<String>::new(), "{calm}");
        }
    }

    /// Cada começo de chave é respondido por uma parte só do catálogo.
    #[test]
    fn each_key_prefix_belongs_to_one_part() {
        let mut seen = BTreeSet::new();
        for (prefixes, _) in PARTS {
            for prefix in prefixes {
                assert!(seen.insert(*prefix), "{prefix} is declared by two parts");
            }
        }
    }

    // Short forms are rejected with a typed error.
    #[test]
    fn i18n_rejects_short_form() {
        assert_eq!(
            Locale::from_str("pt").unwrap_err(),
            LocaleError::ShortForm("pt".to_string())
        );
        assert_eq!(
            Locale::from_str("en").unwrap_err(),
            LocaleError::ShortForm("en".to_string())
        );
        // Trim + case-insensitive still rejects.
        assert_eq!(
            Locale::from_str("  PT ").unwrap_err(),
            LocaleError::ShortForm("PT".to_string())
        );
    }

    #[test]
    fn locale_parses_bcp47() {
        assert_eq!(Locale::from_str("pt-BR").unwrap(), Locale::PtBr);
        assert_eq!(Locale::from_str("en-US").unwrap(), Locale::EnUs);
        // Case-insensitive on the region tag.
        assert_eq!(Locale::from_str("PT-br").unwrap(), Locale::PtBr);
        assert_eq!(Locale::from_str("EN-US").unwrap(), Locale::EnUs);
        // Foreign / unsupported codes → Unknown, not ShortForm.
        assert!(matches!(
            Locale::from_str("es-MX").unwrap_err(),
            LocaleError::Unknown(_)
        ));
    }

    // Known keys translate to the canonical literals.
    #[test]
    fn i18n_translates_known_keys() {
        assert_eq!(
            translate("banner.close.success", Locale::PtBr),
            "Pipeline fechado com sucesso."
        );
        assert_eq!(
            translate("banner.close.success", Locale::EnUs),
            "Pipeline closed successfully."
        );
        assert_eq!(translate("wave.label", Locale::PtBr), "Onda");
        assert_eq!(translate("wave.label", Locale::EnUs), "W");
    }

    #[test]
    fn translate_unknown_key_is_failopen() {
        // Missing keys return a stable sentinel rather than panicking.
        assert_eq!(translate("banner.missing.xyz", Locale::PtBr), "<missing-key>");
        assert_eq!(translate("banner.missing.xyz", Locale::EnUs), "<missing-key>");
    }

    #[test]
    fn slugify_pt_strips_accents() {
        assert_eq!(slugify("Configuração do Idioma", "pt-BR"), "configuracao-idioma");
        assert_eq!(slugify("São Paulo é grande", "pt-BR"), "sao-paulo-grande");
        assert_eq!(slugify("ç ã õ", "pt-BR"), "c-a-o");
    }

    #[test]
    fn slugify_pt_drops_em_a_contractions() {
        // `no` ("em o") is a stopword now: it must not eat a token slot and leave
        // a `...-erro-no` tail — the meaningful word (`nome`) survives instead.
        assert_eq!(slugify("erro no nome", "pt-BR"), "erro-nome");
        assert_eq!(slugify("tratamento na base", "pt-BR"), "tratamento-base");
        assert_eq!(slugify("volta ao topo", "pt-BR"), "volta-topo");
    }

    #[test]
    fn slugify_en_drops_only_the_english_words() {
        assert_eq!(slugify("The Quick Brown Fox", "en-US"), "quick-brown-fox");
        // PT stopwords are NOT applied in EN mode.
        assert_eq!(slugify("de para", "en-US"), "de-para");
    }

    /// O acento sai em toda língua, e não só no português: a letra acentuada
    /// que ficasse partiria a palavra em duas no nome, que só leva ASCII.
    #[test]
    fn slugify_strips_accents_in_every_language() {
        assert_eq!(slugify("Configuración del idioma", "es-ES"), "configuracion-del-idioma");
        assert_eq!(slugify("Fix the naïve café parser", "en-US"), "fix-naive-cafe-parser");
    }

    /// A língua que o Mustard não fala nas mensagens vale do mesmo jeito: o
    /// nome em espanhol deixa de fora só as palavras do arquivo do espanhol,
    /// e nunca as do português — o "no" do espanhol é negação, e sair do nome
    /// inverteria o sentido.
    #[test]
    fn slugify_reads_any_declared_language_and_never_falls_back_to_portuguese() {
        let spanish = crate::domain::normalize::slug_words("es");
        assert!(!spanish.contains(&"no"), "{spanish:?}");
        assert_eq!(slugify("El botón no guarda", "es-ES"), "el-boton-no-guarda");
        assert_eq!(slugify("El botón no guarda", "pt-BR"), "el-boton-guarda");
        // Sem arquivo da língua, nenhuma palavra sai.
        assert_eq!(slugify("a de no", "xx-YY"), "a-de-no");
    }

    /// O nome da spec deixa de fora cada palavra da linha `slug:` do arquivo
    /// da língua, e só elas: a palavra de ligação fora da linha, como "sem"
    /// e "not", fica no nome, porque mudaria o sentido.
    #[test]
    fn slugify_drops_the_slug_words_of_the_language_file_and_only_them() {
        for (tag, language) in [("pt-BR", "pt"), ("en-US", "en")] {
            let words = crate::domain::normalize::slug_words(language);
            assert!(!words.is_empty(), "{language}");
            for word in words {
                assert_eq!(slugify(&format!("erro {word} nome"), tag), "erro-nome", "{language}: {word}");
            }
        }
        assert_eq!(slugify("spec sem dono", "pt-BR"), "spec-sem-dono");
        assert_eq!(slugify("commit nao fecha", "pt-BR"), "commit-nao-fecha");
        assert_eq!(slugify("gate not closed", "en-US"), "gate-not-closed");
    }

    #[test]
    fn slugify_handles_empty_and_punctuation() {
        // Mirror the existing `interpret::slugify` floor — degrade to "x".
        assert_eq!(slugify("///", "pt-BR"), "x");
        assert_eq!(slugify("", "en-US"), "x");
        // A single-token input is preserved even if it would be a stopword,
        // so callers always get *something* slug-shaped back.
        assert_eq!(slugify("the", "en-US"), "the");
    }

    #[test]
    fn i18n_render_is_the_translation() {
        let i = I18n::new(Locale::EnUs);
        assert_eq!(i.render("banner.close.success"), "Pipeline closed successfully.");
    }

    #[test]
    fn wave_label_formats_per_locale() {
        assert_eq!(wave_label(3, Locale::PtBr), "Onda 3");
        assert_eq!(wave_label(3, Locale::EnUs), "W3");
    }

    #[test]
    fn file_marker_synonyms_merge_locales_en_canonical_first() {
        let create = file_marker_synonyms(FileMarker::Create);
        assert_eq!(create[0], "(create)", "EN canonical leads: {create:?}");
        for syn in ["(create)", "(new)", "(novo)", "(criar)"] {
            assert!(create.contains(&syn), "{syn} accepted: {create:?}");
        }
        let edit = file_marker_synonyms(FileMarker::Edit);
        assert_eq!(edit[0], "(edit)");
        assert!(edit.contains(&"(editar)"));
        // Deduped — no spelling twice.
        let mut sorted = create.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), create.len(), "no duplicates: {create:?}");
    }

    #[test]
    fn line_has_file_marker_matches_localized_and_case_insensitive() {
        assert!(line_has_file_marker("- `a.rs` (create)", FileMarker::Create));
        assert!(line_has_file_marker("- `a.rs` (novo)", FileMarker::Create));
        assert!(line_has_file_marker("- `a.rs` (criar)", FileMarker::Create));
        assert!(line_has_file_marker("- `a.rs` (NOVO)", FileMarker::Create));
        assert!(line_has_file_marker("- `a.rs` (editar)", FileMarker::Edit));
        // No marker / wrong marker → no match.
        assert!(!line_has_file_marker("- `a.rs`", FileMarker::Create));
        assert!(!line_has_file_marker("- `a.rs` (editar)", FileMarker::Create));
        // A prose parenthetical is not a marker.
        assert!(!line_has_file_marker("- `a.rs` (new format)", FileMarker::Create));
    }
}
