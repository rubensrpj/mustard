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
//! O texto guarda a raiz de todas as línguas, e a pergunta da busca com filtro
//! ([`Normalizer::query_in_text_language`]) leva só a da primeira, a língua do
//! texto do projeto, e a das outras só na palavra cuja forma da primeira não
//! acha nada no índice. As outras buscas cortam a pergunta em todas as línguas
//! ([`Normalizer::query`]).
//!
//! As línguas vêm do projeto e são passadas por quem busca ([`Languages`]).
//! Cada língua é um arquivo de dados, `languages/<código>.txt`, embutido no
//! binário: a linha `stem:` dá o algoritmo de raiz pelo nome, e as outras
//! linhas são as palavras de ligação. A língua
//! nova é um arquivo a mais na pasta, sem linha de código. [`STEMMERS`] é a única tabela que liga o nome
//! ao algoritmo da biblioteca. A língua sem arquivo, ou sem a linha `stem:`,
//! fica sem raiz, e a palavra dela é comparada inteira; a sem palavras de
//! ligação não tira palavra nenhuma.
//!
//! Função pura: sem disco e sem relógio, a não ser [`Languages::of_project`],
//! que lê a configuração do projeto.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rust_stemmers::{Algorithm, Stemmer};

use crate::domain::config::ProjectConfig;
use crate::domain::text;

/// A tabela única que liga o nome do algoritmo de raiz, como o arquivo da
/// língua o escreve na linha `stem:`, ao algoritmo da biblioteca: os 18 que
/// ela tem. Nenhuma outra parte do código escolhe algoritmo.
const STEMMERS: &[(&str, Algorithm)] = &[
    ("arabic", Algorithm::Arabic),
    ("danish", Algorithm::Danish),
    ("dutch", Algorithm::Dutch),
    ("english", Algorithm::English),
    ("finnish", Algorithm::Finnish),
    ("french", Algorithm::French),
    ("german", Algorithm::German),
    ("greek", Algorithm::Greek),
    ("hungarian", Algorithm::Hungarian),
    ("italian", Algorithm::Italian),
    ("norwegian", Algorithm::Norwegian),
    ("portuguese", Algorithm::Portuguese),
    ("romanian", Algorithm::Romanian),
    ("russian", Algorithm::Russian),
    ("spanish", Algorithm::Spanish),
    ("swedish", Algorithm::Swedish),
    ("tamil", Algorithm::Tamil),
    ("turkish", Algorithm::Turkish),
];

/// Os arquivos das línguas, pela parte da língua do código BCP-47: um por
/// arquivo da pasta `languages/`, com o texto dele, na tabela que o script de
/// compilação grava. Em cada arquivo, a linha que começa com `#` é
/// comentário, a linha `stem:` dá o nome do algoritmo de raiz, e cada outra
/// linha é uma palavra de ligação.
const LANGUAGE_FILES: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/languages.rs"));

/// O começo da linha que dá, no arquivo da língua, o nome do algoritmo de raiz.
const STEM_LINE: &str = "stem:";

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
        let text = text_language(config);
        let code = written(config.language.code.as_deref())
            .unwrap_or_else(|| config.language().code_or_default().as_str().to_string());
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

/// A língua do texto do projeto: `language.text` do `mustard.json` como está
/// escrita, mesmo quando o Mustard não tem mensagens nela (`es-ES` fica
/// `es-ES`), e o padrão que [`ProjectConfig::language`] dá a ela quando não
/// foi declarada. As mensagens do próprio Mustard seguem a língua fechada de
/// [`crate::domain::config::Language::text_or_default`]; o que depende do texto
/// que a pessoa escreveu, como a busca e o nome de uma spec, lê esta.
#[must_use]
pub fn text_language(config: &ProjectConfig) -> String {
    written(config.language.text.as_deref())
        .unwrap_or_else(|| config.language().text_or_default().as_str().to_string())
}

/// O código de língua escrito, sem espaço em volta; nenhum quando em branco.
fn written(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim).filter(|tag| !tag.is_empty()).map(str::to_string)
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
        let forms = self.cut(word, self.stemmers.len());
        self.known.insert(word.to_string(), forms.clone());
        forms
    }

    /// As formas de uma palavra com a raiz das `languages` primeiras línguas,
    /// na ordem em que o projeto as declarou.
    fn cut(&self, word: &str, languages: usize) -> Vec<String> {
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
        for stemmer in self.stemmers.iter().take(languages) {
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

    /// Source spellings also retain contiguous identifier components. A
    /// lowercase query can then find `QuartzPayClient` as `quartzpay`, while
    /// ordinary neighbouring prose words never become a synthetic identifier.
    pub fn written_forms(&mut self, text: &str) -> std::collections::BTreeSet<String> {
        let mut out:std::collections::BTreeSet<_>=self.forms(text).into_iter().flatten().collect();
        for token in text.split(|c:char|!c.is_alphanumeric() && c!='_').filter(|word|!word.is_empty()) {
            let split=split_identifier(token).to_lowercase();let parts:Vec<_>=split.split_whitespace().collect();
            if parts.len()<2 {continue;}
            out.extend(self.word_forms(&parts.concat()));
            // Bounded windows avoid quadratic work on pathological names.
            for width in 2..=parts.len().min(4) {
                for window in parts.windows(width) {out.extend(self.word_forms(&window.concat()));}
            }
        }
        out
    }

    /// As palavras de uma pergunta com as suas formas, como [`Self::forms`],
    /// sem as palavras de ligação: sem isso, o "a" de "apagando a pasta"
    /// casaria qualquer texto com um "a". A pergunta só com palavras de
    /// ligação fica vazia.
    pub fn query(&mut self, text: &str) -> Vec<Vec<String>> {
        self.collect(text, true)
    }

    /// A pergunta da busca com filtro: como [`Self::query`], mas cada palavra
    /// leva só a raiz da primeira língua, a do texto do projeto, e as
    /// palavras que dividem alguma forma viram uma só, com as formas de
    /// todas. A raiz inglesa de `commands` é `command`, que espalharia a
    /// pergunta por todo lugar que escreve `command`, caminho de pasta
    /// inclusive; só com a do português, a pergunta acha o que escreve
    /// `commands`. A forma que duas palavras têm em comum (`simula` e
    /// `simulação`) conta uma vez na nota, e não uma por palavra. O texto do
    /// índice segue com a raiz de todas as línguas.
    ///
    /// `finds` diz se as formas da primeira língua acham alguma coisa no
    /// índice. A palavra que não acha nada ganha também a raiz das outras
    /// línguas: em um projeto de texto em português e código em inglês, `users`
    /// não existe no índice e passa a achar `UserRepository` pela raiz
    /// `user`; a palavra que já acha algo não muda.
    ///
    /// # Errors
    ///
    /// O erro de `finds`, sem tentar de novo.
    pub fn query_in_text_language<E>(
        &mut self,
        text: &str,
        mut finds: impl FnMut(&[String]) -> Result<bool, E>,
    ) -> Result<Vec<Vec<String>>, E> {
        let mut words: Vec<Vec<String>> = Vec::new();
        for word in plain_words(text) {
            if self.is_function_word(&word) {
                continue;
            }
            let first = self.cut(&word, 1);
            let every = self.word_forms(&word);
            words.push(if every.len() == first.len() || finds(&first)? { first } else { every });
        }
        Ok(merge_shared_forms(words))
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

/// As palavras da pergunta que dividem alguma forma viram uma palavra só, com
/// as formas de todas, na ordem em que aparecem: a forma que duas palavras
/// têm em comum conta uma vez na nota (`link` e `linked`, `capacity` e
/// `capacidade`), e não uma por palavra.
fn merge_shared_forms(words: Vec<Vec<String>>) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::with_capacity(words.len());
    for word in words {
        let mut merged = word;
        let mut at = 0;
        while at < out.len() {
            if out[at].iter().any(|form| merged.contains(form)) {
                let mut earlier = out.remove(at);
                for form in merged {
                    if !earlier.contains(&form) {
                        earlier.push(form);
                    }
                }
                merged = earlier;
            } else {
                at += 1;
            }
        }
        out.push(merged);
    }
    out
}

/// As linhas de dado do arquivo da língua, sem as vazias e sem os
/// comentários; nenhuma quando ela não tem arquivo.
fn entries(language: &str) -> impl Iterator<Item = &'static str> + '_ {
    LANGUAGE_FILES
        .iter()
        .filter(move |(code, _)| *code == language)
        .flat_map(|(_, file)| file.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

/// O nome do algoritmo de raiz que o arquivo da língua dá na linha `stem:`.
fn stem_name(language: &str) -> Option<&'static str> {
    entries(language).find_map(|line| line.strip_prefix(STEM_LINE)).map(str::trim)
}

/// O algoritmo de raiz da língua: o nome que o arquivo dela dá, pela tabela
/// [`STEMMERS`].
fn algorithm(language: &str) -> Option<Algorithm> {
    let name = stem_name(language)?;
    STEMMERS.iter().find(|(known, _)| *known == name).map(|(_, algorithm)| *algorithm)
}

/// As palavras de ligação da língua: as linhas do arquivo dela fora a do
/// algoritmo; nenhuma quando ela não tem arquivo.
fn stopwords(language: &str) -> impl Iterator<Item = &'static str> + '_ {
    entries(language).filter(|line| !line.starts_with(STEM_LINE))
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

    /// A pergunta da busca com filtro quando a primeira língua sempre acha algo
    /// no índice: só a raiz dela.
    fn first_language(normalizer: &mut Normalizer, text: &str) -> Vec<Vec<String>> {
        normalizer.query_in_text_language(text, |_| Ok::<bool, std::convert::Infallible>(true)).unwrap()
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

    /// A pergunta da busca com filtro leva só a raiz da primeira língua, a do
    /// texto do projeto, e o texto leva a de todas: `commands` na pergunta não
    /// vira `command`, que espalharia a busca por todo lugar que escreve
    /// `command`, e o texto que escreve `commands` ou `command` continua
    /// achado pelas duas perguntas. A pergunta das outras buscas segue com a
    /// raiz de todas as línguas.
    #[test]
    fn the_filter_question_keeps_only_the_root_of_the_first_language() {
        let languages = Languages::new(["pt-BR", "en-US"]);
        let mut normalizer = Normalizer::new(&languages);
        let written = normalizer.word_forms("commands");
        assert!(written.contains(&"commands".to_string()) && written.contains(&"command".to_string()), "{written:?}");
        let asked = only(&first_language(&mut normalizer, "commands"));
        assert_eq!(asked, ["commands"], "the english root is not in the question");
        let singular = only(&first_language(&mut normalizer, "command"));
        assert!(singular.iter().any(|form| written.contains(form)), "the singular still finds the plural: {singular:?}");
        let english_first = Languages::new(["en-US", "pt-BR"]);
        assert_eq!(only(&first_language(&mut Normalizer::new(&english_first), "commands")), ["command"]);
        assert!(only(&normalizer.query("commands")).contains(&"command".to_string()), "the other searches keep every root");
    }

    /// As palavras da pergunta com filtro que dividem uma forma viram uma
    /// palavra só, e a forma comum conta uma vez na nota; as que não dividem
    /// nada seguem separadas, e a ordem das palavras não muda o resultado.
    #[test]
    fn two_filter_question_words_that_share_a_form_count_it_once() {
        let languages = Languages::new(["pt-BR", "en-US"]);
        let mut normalizer = Normalizer::new(&languages);
        let plain = first_language(&mut normalizer, "simula");
        let accented = first_language(&mut normalizer, "simulação");
        assert_eq!((plain.len(), accented.len()), (1, 1));
        let shared = "simul".to_string();
        assert!(plain[0].contains(&shared) && accented[0].contains(&shared), "{plain:?} / {accented:?}");
        for question in ["simula simulação", "simulação simula"] {
            let words = first_language(&mut normalizer, question);
            assert_eq!(words.len(), 1, "{question}: {words:?}");
            assert_eq!(words.iter().filter(|word| word.contains(&shared)).count(), 1, "{question}: {words:?}");
            assert_eq!(words[0].len(), 2, "{question}: the forms of both words, once each: {words:?}");
        }
        assert_eq!(first_language(&mut normalizer, "simula pasta").len(), 2, "words with no form in common stay apart");
        assert_eq!(first_language(&mut normalizer, "a de para").len(), 0, "function words leave the question");
    }

    /// A palavra da pergunta com filtro cuja forma da primeira língua não acha
    /// nada no índice ganha também a raiz das outras línguas; a que acha algo
    /// fica só com a da primeira, palavra por palavra. A pergunta a `finds` é
    /// sempre com as formas da primeira língua.
    #[test]
    fn a_filter_question_word_that_finds_nothing_in_the_first_language_tries_the_other_languages() {
        let languages = Languages::new(["pt-BR", "en-US"]);
        let mut normalizer = Normalizer::new(&languages);
        let mut asked: Vec<Vec<String>> = Vec::new();
        let words = normalizer
            .query_in_text_language("users commands", |forms| {
                asked.push(forms.to_vec());
                Ok::<bool, std::convert::Infallible>(forms.iter().any(|form| form == "commands"))
            })
            .unwrap();
        let first_users = first_language(&mut normalizer, "users");
        assert_eq!(asked, vec![only(&first_users), vec!["commands".to_string()]], "only the first-language forms are asked");
        assert_eq!(words.len(), 2, "{words:?}");
        assert_eq!(words[0], normalizer.word_forms("users"), "the word that finds nothing gets the roots of every language");
        assert!(words[0].contains(&"user".to_string()), "{words:?}");
        assert_eq!(words[1], ["commands"], "the word that finds something keeps only the first language");
    }

    /// A pergunta de uma língua só e a palavra de ligação não perguntam nada
    /// ao índice, e o erro de quem pergunta sobe sem devolver a pergunta.
    #[test]
    fn the_filter_question_asks_the_index_only_about_words_that_could_gain_a_form() {
        let one = Languages::new(["en-US"]);
        let mut only_english = Normalizer::new(&one);
        let words = only_english
            .query_in_text_language("commands of the queue", |_| -> Result<bool, ()> { panic!("one language has no other root") })
            .unwrap();
        assert_eq!(words, [vec!["command".to_string()], vec!["queue".to_string()]]);
        let languages = Languages::new(["pt-BR", "en-US"]);
        let mut normalizer = Normalizer::new(&languages);
        let mut asked = 0;
        let widened = normalizer
            .query_in_text_language("a users de", |_| {
                asked += 1;
                Ok::<bool, std::convert::Infallible>(false)
            })
            .unwrap();
        assert_eq!((asked, widened.len()), (1, 1), "the function words are not asked and leave the question");
        let refused = normalizer.query_in_text_language("users", |_| Err("the index is unreadable"));
        assert_eq!(refused, Err("the index is unreadable"));
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
        use crate::io::{map_triage, project_map as store};
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
            let from_map = map_triage::triage(project.path(), (question, ""), &languages, TOP).unwrap().files;
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

    /// A mesma pergunta, em português e em inglês, acha o mesmo arquivo no
    /// mapa: a raiz de "validado" encontra a de "validates", e as palavras
    /// que só ligam a frase saem nas duas línguas ("onde", "é", "cada" e
    /// "antes" saem como "where", "is", "each" e "before"). Sem isso, a
    /// pergunta em português acharia primeiro o arquivo cuja documentação,
    /// em português, repete essas palavras.
    #[test]
    fn the_same_question_in_portuguese_and_english_finds_the_same_target() {
        use crate::domain::project_map::{MapDecl, MapModule, ProjectMap};
        use crate::io::{map_triage, project_map as store};

        let languages = Languages::new(["pt-BR", "en-US"]);
        let mut normalizer = Normalizer::new(&languages);
        assert!(normalizer.query("Onde é que cada um antes de ser").is_empty());
        assert!(normalizer.query("Where is each of these before it is").is_empty());

        let module = |path: &str, name: &str, doc: &str| MapModule {
            path: path.to_string(),
            declarations: vec![MapDecl { name: name.to_string(), doc: doc.to_string(), ..MapDecl::default() }],
            ..MapModule::default()
        };
        let map = ProjectMap {
            modules: vec![
                module("src/orders.rs", "validate_order", "Validates the order before it is saved."),
                module(
                    "src/notes.rs",
                    "notes",
                    "Onde cada nota é guardada: é aqui, onde cada parte é lida antes e onde cada linha é escrita antes.",
                ),
                module("src/page.rs", "render_page", "Renders the page."),
                module("src/line.rs", "parse_line", "Parses one line."),
            ],
            ..ProjectMap::default()
        };
        let project = tempfile::tempdir().unwrap();
        store::write(project.path(), &map).unwrap();
        let first = |question: &str| {
            let found = map_triage::triage(project.path(), (question, ""), &languages, 5).unwrap().files;
            found.first().map(|file| file.path.clone()).unwrap_or_default()
        };
        let portuguese = first("Onde é que cada pedido é validado antes de ser salvo?");
        let english = first("Where is each order validated before it is saved?");
        assert_eq!(english, "src/orders.rs");
        assert_eq!(portuguese, english, "the same question in both languages finds the same file");
    }

    /// Cada língua é um arquivo da pasta das línguas, e cada arquivo da pasta
    /// entra no binário: a língua nova é um arquivo a mais, sem linha de
    /// código. A linha `stem:` de cada arquivo nomeia um algoritmo da tabela,
    /// e cada algoritmo da tabela tem a sua língua.
    #[test]
    fn each_language_is_one_data_file_of_the_folder() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("domain").join("normalize").join("languages");
        let mut on_disk: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
            .filter_map(|path| path.file_stem().and_then(|stem| stem.to_str()).map(str::to_string))
            .collect();
        on_disk.sort();
        let mut built_in: Vec<String> = LANGUAGE_FILES.iter().map(|(code, _)| (*code).to_string()).collect();
        built_in.sort();
        assert_eq!(built_in, on_disk, "every file of the folder is built in");

        for (code, _) in LANGUAGE_FILES {
            if let Some(name) = stem_name(code) {
                assert!(STEMMERS.iter().any(|(known, _)| *known == name), "{code}: unknown stem {name}");
            }
        }
        for (name, _) in STEMMERS {
            assert!(LANGUAGE_FILES.iter().any(|(code, _)| stem_name(code) == Some(*name)), "no language uses {name}");
        }
        assert_eq!(stem_name("pt"), Some("portuguese"));
        assert!(!stopwords("pt").any(|word| word.starts_with(STEM_LINE)), "the stem line is not a function word");
        assert!(stopwords("pt").any(|word| word == "onde"));
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
