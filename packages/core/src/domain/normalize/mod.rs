//! A normalização de toda busca do projeto: a pergunta e o texto em que ela
//! procura passam pelos mesmos passos, nas línguas que o projeto declarou.
//!
//! Os passos são fixos e vão nesta ordem: o nome colado se separa nas
//! palavras dele (`gerarRelatorio` vira `gerar Relatorio`), tudo vai para
//! minúsculas, e cada palavra ganha as suas formas. Em cada língua, a forma é
//! a raiz da palavra sem acento, com o acento tirado depois da raiz e também
//! antes dela: as duas ordens dão raízes diferentes em algumas palavras, e
//! guardar as duas faz a pergunta sem acento achar o texto com acento. A
//! palavra de ligação de qualquer das línguas (`de`, `the`) não ganha raiz:
//! fica ela mesma, sem acento, para a raiz dela não coincidir com a de uma
//! palavra de conteúdo parecida (a de "some" nunca é a de "somar").
//!
//! As línguas vêm do projeto e são passadas por quem busca ([`Languages`]).
//! A raiz de cada língua vem de [`STEMMERS`], a única tabela que liga uma
//! língua a um algoritmo; a língua fora dela fica sem raiz, e a palavra dela
//! é comparada inteira. As palavras de ligação vêm de um arquivo de dados por
//! língua, embutido no binário; a língua sem arquivo não tira palavra nenhuma.
//!
//! Função pura: sem disco e sem relógio, a não ser [`Languages::of_project`],
//! que lê a configuração do projeto.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rust_stemmers::{Algorithm, Stemmer};

use crate::domain::config::ProjectConfig;
use crate::domain::text;

/// A tabela única que liga a língua, pela parte da língua do código BCP-47,
/// ao algoritmo de raiz: as 18 línguas da biblioteca de raízes. Nenhuma outra
/// parte do código escolhe algoritmo.
const STEMMERS: &[(&str, Algorithm)] = &[
    ("ar", Algorithm::Arabic),
    ("da", Algorithm::Danish),
    ("nl", Algorithm::Dutch),
    ("en", Algorithm::English),
    ("fi", Algorithm::Finnish),
    ("fr", Algorithm::French),
    ("de", Algorithm::German),
    ("el", Algorithm::Greek),
    ("hu", Algorithm::Hungarian),
    ("it", Algorithm::Italian),
    ("no", Algorithm::Norwegian),
    ("pt", Algorithm::Portuguese),
    ("ro", Algorithm::Romanian),
    ("ru", Algorithm::Russian),
    ("es", Algorithm::Spanish),
    ("sv", Algorithm::Swedish),
    ("ta", Algorithm::Tamil),
    ("tr", Algorithm::Turkish),
];

/// As palavras de ligação das línguas que o Snowball publica, um arquivo por
/// língua, uma palavra por linha; a linha que começa com `#` é comentário.
const STOPWORDS: &[(&str, &str)] = &[
    ("da", include_str!("stopwords/da.txt")),
    ("de", include_str!("stopwords/de.txt")),
    ("en", include_str!("stopwords/en.txt")),
    ("es", include_str!("stopwords/es.txt")),
    ("fi", include_str!("stopwords/fi.txt")),
    ("fr", include_str!("stopwords/fr.txt")),
    ("hu", include_str!("stopwords/hu.txt")),
    ("it", include_str!("stopwords/it.txt")),
    ("nl", include_str!("stopwords/nl.txt")),
    ("no", include_str!("stopwords/no.txt")),
    ("pt", include_str!("stopwords/pt.txt")),
    ("ru", include_str!("stopwords/ru.txt")),
    ("sv", include_str!("stopwords/sv.txt")),
];

/// As línguas em que as palavras de uma busca são cortadas: a parte da
/// língua de cada código declarado (`pt` de `pt-BR`), em minúsculas, sem
/// repetição e na ordem da declaração.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Languages(Vec<String>);

impl Languages {
    /// As línguas destes códigos, em BCP-47 (`pt-BR`, `es-ES`) ou só a
    /// língua (`pt`). O código em branco não conta.
    #[must_use]
    pub fn new<'a>(declared: impl IntoIterator<Item = &'a str>) -> Self {
        let mut out: Vec<String> = Vec::new();
        for tag in declared {
            let language = tag.trim().split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
            if !language.is_empty() && !out.contains(&language) {
                out.push(language);
            }
        }
        Self(out)
    }

    /// As línguas de um projeto: `language.text` e `language.code` do
    /// `mustard.json`, como estão escritas, e uma só quando as duas são a
    /// mesma língua. A que não foi declarada vale o padrão que
    /// [`ProjectConfig::language`] dá a ela. A língua escrita que não está
    /// entre as das mensagens do projeto, como `es-ES`, vale para a busca do
    /// mesmo jeito.
    #[must_use]
    pub fn of(config: &ProjectConfig) -> Self {
        let declared = config.language();
        let written = |raw: Option<&str>| raw.map(str::trim).filter(|tag| !tag.is_empty()).map(str::to_string);
        let text = written(config.language.text.as_deref())
            .unwrap_or_else(|| declared.text_or_default().as_str().to_string());
        let code = written(config.language.code.as_deref())
            .unwrap_or_else(|| declared.code_or_default().as_str().to_string());
        Self::new([text.as_str(), code.as_str()])
    }

    /// As línguas do projeto em `root`, pelo `mustard.json` dele.
    #[must_use]
    pub fn of_project(root: &Path) -> Self {
        Self::of(&ProjectConfig::load(root))
    }

    /// As línguas, na ordem.
    #[must_use]
    pub fn codes(&self) -> &[String] {
        &self.0
    }
}

/// O nome quebrado nas palavras dele: `ProcessadorPagamento`,
/// `processador_pagamento` e `processador-pagamento` viram
/// `processador pagamento`.
#[must_use]
pub fn split_identifier(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 8);
    let mut prev: Option<char> = None;
    for c in name.chars() {
        if matches!(c, '_' | '-' | '.' | '/' | ':') {
            out.push(' ');
        } else {
            if c.is_uppercase() && prev.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) {
                out.push(' ');
            }
            out.push(c);
        }
        prev = Some(c);
    }
    out
}

/// As palavras de um texto antes de qualquer língua: o nome colado separado,
/// tudo em minúsculas, com o acento, sem repetição e na ordem em que
/// aparecem. É o que o campo de busca de cada linha da spec guarda; as formas
/// saem delas na hora da busca, nas línguas do projeto.
#[must_use]
pub fn plain_words(text: &str) -> Vec<String> {
    let lower = split_identifier(text).to_lowercase();
    let mut seen: HashSet<&str> = HashSet::new();
    text::words(&lower).filter(|word| seen.insert(word)).map(str::to_string).collect()
}

/// Cada palavra de `text` com as suas formas, nas línguas `languages`, sem
/// repetição e na ordem: a normalização de toda busca.
#[must_use]
pub fn forms(text: &str, languages: &Languages) -> Vec<Vec<String>> {
    Normalizer::new(languages).forms(text)
}

/// A normalização nas línguas de uma busca, com as formas de cada palavra já
/// calculadas guardadas: num arquivo inteiro, a mesma palavra se repete
/// muitas vezes e é cortada uma vez só.
pub struct Normalizer {
    /// O algoritmo de cada língua; `None` para a língua sem algoritmo.
    stemmers: Vec<Option<Stemmer>>,
    /// As palavras de ligação das línguas, com e sem acento.
    function_words: HashSet<String>,
    /// As formas de cada palavra já vista.
    known: HashMap<String, Vec<String>>,
}

impl Normalizer {
    #[must_use]
    pub fn new(languages: &Languages) -> Self {
        let stemmers = languages.0.iter().map(|language| algorithm(language).map(Stemmer::create)).collect();
        let mut function_words = HashSet::new();
        for language in &languages.0 {
            for word in stopwords(language) {
                function_words.insert(word.to_string());
                function_words.insert(text::fold_accents(word));
            }
        }
        Self { stemmers, function_words, known: HashMap::new() }
    }

    /// `true` para a palavra de ligação de qualquer das línguas, já em
    /// minúsculas, com ou sem acento.
    #[must_use]
    pub fn is_function_word(&self, word: &str) -> bool {
        self.function_words.contains(word)
    }

    /// As formas de uma palavra já em minúsculas: em cada língua, a raiz sem
    /// acento nas duas ordens, sem repetição. A palavra de ligação e a da
    /// língua sem algoritmo ficam como estão, sem acento.
    pub fn word_forms(&mut self, word: &str) -> Vec<String> {
        if let Some(forms) = self.known.get(word) {
            return forms.clone();
        }
        let forms = self.cut(word);
        self.known.insert(word.to_string(), forms.clone());
        forms
    }

    fn cut(&self, word: &str) -> Vec<String> {
        let folded = text::fold_accents(word);
        if self.is_function_word(word) {
            return vec![folded];
        }
        let mut out: Vec<String> = Vec::new();
        let mut push = |form: String| {
            if !out.contains(&form) {
                out.push(form);
            }
        };
        for stemmer in &self.stemmers {
            match stemmer {
                Some(stemmer) => {
                    push(text::fold_accents(&stemmer.stem(word)));
                    push(stemmer.stem(&folded).into_owned());
                }
                None => push(folded.clone()),
            }
        }
        if out.is_empty() {
            out.push(folded);
        }
        out
    }

    /// Cada palavra de `text` com as suas formas, sem repetir a palavra que
    /// tem as mesmas formas de outra, na ordem.
    pub fn forms(&mut self, text: &str) -> Vec<Vec<String>> {
        self.collect(text, false)
    }

    /// As palavras de uma pergunta com as suas formas, como [`Self::forms`],
    /// sem as palavras de ligação: sem isso, o "a" de "apagando a pasta"
    /// casaria qualquer texto com um "a". A pergunta só com palavras de
    /// ligação fica vazia.
    pub fn query(&mut self, text: &str) -> Vec<Vec<String>> {
        self.collect(text, true)
    }

    fn collect(&mut self, text: &str, skip_function_words: bool) -> Vec<Vec<String>> {
        let mut seen: HashSet<Vec<String>> = HashSet::new();
        let mut out: Vec<Vec<String>> = Vec::new();
        for word in plain_words(text) {
            if skip_function_words && self.is_function_word(&word) {
                continue;
            }
            let forms = self.word_forms(&word);
            if seen.insert(forms.clone()) {
                out.push(forms);
            }
        }
        out
    }
}

/// O algoritmo de raiz da língua, pela tabela [`STEMMERS`].
fn algorithm(language: &str) -> Option<Algorithm> {
    STEMMERS.iter().find(|(code, _)| *code == language).map(|(_, algorithm)| *algorithm)
}

/// As palavras de ligação da língua; nenhuma quando ela não tem arquivo.
fn stopwords(language: &str) -> impl Iterator<Item = &'static str> + '_ {
    STOPWORDS
        .iter()
        .filter(move |(code, _)| *code == language)
        .flat_map(|(_, list)| list.lines())
        .map(str::trim)
        .filter(|word| !word.is_empty() && !word.starts_with('#'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::LanguageConfig;

    fn project(text: Option<&str>, code: Option<&str>) -> ProjectConfig {
        ProjectConfig {
            language: LanguageConfig { text: text.map(str::to_string), code: code.map(str::to_string) },
            ..ProjectConfig::default()
        }
    }

    fn only(words: &[Vec<String>]) -> Vec<String> {
        assert_eq!(words.len(), 1, "{words:?}");
        words[0].clone()
    }

    #[test]
    fn the_languages_are_the_text_and_code_ones_as_written_and_one_when_both_match() {
        let both = Languages::of(&project(Some("pt-BR"), Some("en-US")));
        assert_eq!(both.codes(), ["pt", "en"]);
        let same = Languages::of(&project(Some("en-US"), Some("en-GB")));
        assert_eq!(same.codes(), ["en"], "one language when both are the same");
        let spanish = Languages::of(&project(Some("es-ES"), Some("en-US")));
        assert_eq!(spanish.codes(), ["es", "en"], "a language outside the message locales still counts");
        let default = ProjectConfig::default();
        let expected = [
            default.language().text_or_default().as_str().to_string(),
            default.language().code_or_default().as_str().to_string(),
        ];
        assert_eq!(Languages::of(&default), Languages::new(expected.iter().map(String::as_str)));
    }

    /// Cada palavra ganha a raiz nas duas ordens de cada língua, e as formas
    /// de todas as línguas ficam juntas: "usuários" dá a raiz do português, e
    /// "users", a do inglês.
    #[test]
    fn each_word_gets_its_root_in_both_orders_of_each_language() {
        let languages = Languages::new(["pt-BR", "en-US"]);
        let mut normalizer = Normalizer::new(&languages);
        let users = normalizer.word_forms("users");
        assert!(users.contains(&"user".to_string()), "{users:?}");
        let accented = normalizer.word_forms("conciliação");
        assert!(accented.iter().all(|form| !form.contains('ç') && !form.contains('ã')), "{accented:?}");
        let plain = normalizer.word_forms("conciliacao");
        assert!(plain.iter().any(|form| accented.contains(form)), "{plain:?} / {accented:?}");
        assert_eq!(normalizer.word_forms("apagando")[0], normalizer.word_forms("apagar")[0]);
    }

    /// A palavra de ligação fica ela mesma, sem raiz, e sai da pergunta.
    #[test]
    fn a_function_word_keeps_itself_and_leaves_the_question() {
        let languages = Languages::new(["pt-BR", "en-US"]);
        let mut normalizer = Normalizer::new(&languages);
        assert_eq!(normalizer.word_forms("some"), ["some"]);
        assert_eq!(normalizer.word_forms("não"), ["nao"]);
        assert!(normalizer.query("a de para o the of").is_empty());
        assert_eq!(normalizer.query("apagando a pasta").len(), 2);
    }

    /// O nome colado se separa nas palavras dele, e o campo guardado dá as
    /// mesmas formas que o texto de onde saiu.
    #[test]
    fn the_stored_words_give_the_same_forms_as_the_text_they_came_from() {
        let text = "O UserRepository grava os usuários em apps/rt/work-branch.rs";
        assert_eq!(
            plain_words(text),
            ["o", "user", "repository", "grava", "os", "usuários", "em", "apps", "rt", "work", "branch", "rs"]
        );
        let languages = Languages::new(["pt-BR", "en-US"]);
        assert_eq!(forms(&plain_words(text).join(" "), &languages), forms(text, &languages));
        assert_eq!(split_identifier("ProcessadorPagamento"), "Processador Pagamento");
        assert_eq!(split_identifier("processador_pagamento"), "processador pagamento");
        assert_eq!(split_identifier("apps/rt/work-branch.rs"), "apps rt work branch rs");
    }

    /// A língua sem algoritmo de raiz nem palavras de ligação não quebra: a
    /// palavra fica inteira, sem acento, e nenhuma sai da pergunta.
    #[test]
    fn a_language_without_a_stemmer_keeps_whole_words_and_does_not_break() {
        let languages = Languages::new(["ja-JP"]);
        assert_eq!(languages.codes(), ["ja"]);
        assert_eq!(only(&forms("Usuários", &languages)), ["usuarios"]);
        let mut normalizer = Normalizer::new(&languages);
        assert_eq!(normalizer.query("de the").len(), 2, "no function words without a list");
        assert_eq!(only(&forms("x", &Languages::new([]))), ["x"], "no language at all still gives the word");
    }

    /// As quatro buscas — a da spec, a do mapa, a das lições e a das skills —
    /// cortam a mesma pergunta nas mesmas formas: lado a lado, sobre os
    /// mesmos dois textos, cada pergunta acha o mesmo texto nas quatro, e a
    /// que não casa, ou só traz palavra de ligação, não acha nada em nenhuma.
    /// O mapa é gravado e buscado pelo índice de palavras dele, como a busca
    /// do mapa faz.
    #[test]
    fn the_four_searches_cut_the_same_question_into_the_same_forms() {
        use std::collections::BTreeMap;

        use serde_json::{json, Value};

        use crate::domain::project_map::{MapDecl, MapModule, ProjectMap};
        use crate::domain::search::TOP;
        use crate::io::{map_search, project_map as store};
        use crate::domain::spec_events::{found_by, normalize, parse_log, render_line, search_field, stamp};

        let languages = Languages::new(["pt-BR", "en-US"]);
        let texts = ["UserRepository apaga as pastas", "Payment soma os centavos"];
        let object = |value: Value| value.as_object().cloned().unwrap();
        let lines: String = (1u64..)
            .zip(texts)
            .map(|(id, text)| render_line(&stamp(normalize(object(json!({"text": text, "origin": 1})), "note"), id, None, "t")) + "\n")
            .collect();
        let log = parse_log(&lines);
        assert_eq!(log.events.len(), texts.len(), "{lines}");
        let map = ProjectMap {
            modules: (1u64..)
                .zip(texts)
                .map(|(id, text)| MapModule {
                    path: format!("{id}.rs"),
                    declarations: vec![MapDecl { name: text.to_string(), ..MapDecl::default() }],
                    ..MapModule::default()
                })
                .collect(),
            ..ProjectMap::default()
        };
        let project = tempfile::tempdir().unwrap();
        store::write(project.path(), &map).unwrap();
        let skills: Vec<String> = texts.iter().map(|text| search_field(Some(text), &[])).collect();

        let sorted = |mut ids: Vec<u64>| {
            ids.sort_unstable();
            ids
        };
        for (question, expected) in [
            ("users", vec![1]),
            ("repositories", vec![1]),
            ("apagando", vec![1]),
            ("pasta", vec![1]),
            ("centavo", vec![2]),
            ("payment", vec![2]),
            ("carro", vec![]),
            ("os de the", vec![]),
        ] {
            let spec = found_by(log.events.iter().collect(), question, &BTreeMap::new(), &languages);
            let spec = sorted(spec.iter().map(|e| e.id).collect());
            let from_map = map_search::search(project.path(), question, &languages, TOP).unwrap();
            let from_map = sorted(from_map.iter().map(|f| f.path.trim_end_matches(".rs").parse().unwrap()).collect());
            let lessons = crate::domain::lessons::matching_among(&log.events.iter().collect::<Vec<_>>(), question, &languages);
            let lessons = sorted(lessons.iter().map(|hit| hit.id).collect());
            let docs = (1u64..).zip(skills.iter().map(String::as_str));
            let skill = sorted(crate::domain::search::search(docs, question, &languages).iter().map(|hit| hit.id).collect());
            assert_eq!(
                [&spec, &from_map, &lessons, &skill],
                [&expected; 4],
                "{question}: spec, map, lessons and skills side by side"
            );
        }
    }

    /// Nenhuma parte do código escolhe um algoritmo de raiz fora da tabela
    /// única: os fontes do núcleo e dos aplicativos citam o nome do algoritmo
    /// só nas linhas da tabela.
    #[test]
    fn no_stemmer_algorithm_is_chosen_outside_the_one_table() {
        let needle = concat!("Algorithm", "::");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let mut sources = Vec::new();
        collect_sources(&workspace.join("packages").join("core").join("src"), &mut sources);
        collect_sources(&workspace.join("apps"), &mut sources);
        assert!(sources.len() > 100, "the sources were found: {}", sources.len());
        let this = Path::new(file!()).file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let mut outside = Vec::new();
        let mut in_table = 0usize;
        for path in &sources {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            let count = content.matches(needle).count();
            if count == 0 {
                continue;
            }
            let own = path.ends_with(Path::new("domain").join("normalize").join(&this));
            if own {
                in_table += count;
            } else {
                outside.push(path.display().to_string());
            }
        }
        assert!(outside.is_empty(), "a stemmer chosen outside the table: {outside:?}");
        assert_eq!(in_table, STEMMERS.len(), "only the table lines name an algorithm");
    }

    fn collect_sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    collect_sources(&path, out);
                }
            } else if name.ends_with(".rs") {
                out.push(path);
            }
        }
    }
}
