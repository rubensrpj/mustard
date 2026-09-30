// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os agentes acham e leem o código pelos comandos do mapa.
//!
//! Os quatro moldes (onda e revisão, nos dois idiomas) e os textos do
//! catálogo que mandam conferir, juntar ou planejar tarefas citam a busca, o
//! trecho e os usos do `mustard-rt run map`. Cada subcomando e cada opção
//! citada é conferido contra a ajuda do binário de verdade, e o molde da onda
//! diz que a falha pequena achada no caminho se conserta na própria onda.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::process::Command;

use mustard_core::platform::i18n::{translate, Locale};

/// As chaves do catálogo que citam os comandos do mapa: a conferência das
/// tarefas, a citação que a onda deixou, a sobra juntada a uma tarefa aberta
/// e os dois textos que mandam gravar as tarefas de um pedido.
const CATALOG_KEYS: [&str; 5] = [
    "round.analysis_check",
    "round.after_wave.leftover",
    "round.leftover_joined",
    "request.new_waves",
    "request.adjust_waves",
];

/// Os comandos que todo texto citado traz, cada um com uma opção que ele
/// exige: ler só a declaração e ver quem a usa. A busca vem com o texto do
/// `Grep`, sem opção (ver [`cites_the_search_with_the_text_of_grep`]).
const CITED: [(&str, &str); 3] = [("slice", "--file"), ("slice", "--name"), ("users", "--name")];

/// `true` quando `text` cita a busca do mapa com o texto entre aspas, como o
/// `Grep` o recebe, e nenhuma das duas opções que ela aceita só para a medida.
fn cites_the_search_with_the_text_of_grep(text: &str) -> bool {
    let mut cited = citations(text).into_iter().filter(|c| c.question == "search").peekable();
    cited.peek().is_some()
        && cited.all(|c| c.options.is_empty())
        && text.contains("run map search \"<")
        && !text.contains("--query")
        && !text.contains("--intent")
}

fn template(lang: &str, name: &str) -> String {
    let root = manifest_dir::manifest_dir().join("../..");
    std::fs::read_to_string(root.join(format!("packages/core/templates/agents/{lang}/{name}.md")))
        .unwrap_or_else(|e| panic!("the {lang} `{name}` template is missing: {e}"))
}

/// O que `mustard-rt run map` aceita, lido da ajuda do binário: os
/// subcomandos e as opções.
struct Accepted {
    questions: Vec<String>,
    options: Vec<String>,
}

fn accepted_by_the_command() -> Accepted {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt")).args(["run", "map", "--help"]).output().expect("the binary runs");
    assert!(out.status.success(), "{out:?}");
    let help = String::from_utf8_lossy(&out.stdout).to_string();
    let questions: Vec<String> = help
        .split("[possible values: ")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap_or_else(|| panic!("the help lists no subcommand: {help}"))
        .split(", ")
        .map(str::to_string)
        .collect();
    let options: Vec<String> = help
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("--"))
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect();
    assert!(questions.len() >= 5 && options.contains(&"--file".to_string()), "the help was not read: {help}");
    Accepted { questions, options }
}

/// Um comando do mapa que um texto cita: o subcomando e as opções.
struct Citation {
    question: String,
    options: Vec<String>,
}

/// Os comandos do mapa que `text` cita: cada `run map <subcomando> ...` até o
/// fim do trecho entre crases (ou da linha).
fn citations(text: &str) -> Vec<Citation> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("run map ") {
        let after = &rest[at + "run map ".len()..];
        let line = after.lines().next().unwrap_or_default();
        let span = line.split('`').next().unwrap_or_default();
        let mut words = span.split_whitespace();
        let question = words.next().unwrap_or_default().trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        let options = words.filter(|word| word.starts_with("--")).map(str::to_string).collect();
        found.push(Citation { question, options });
        rest = after;
    }
    found
}

/// O que `text` cita e o `run map` não aceita: subcomando ou opção fora da
/// ajuda do binário.
fn not_accepted(text: &str, accepted: &Accepted) -> Vec<String> {
    let mut wrong = Vec::new();
    for citation in citations(text) {
        if !accepted.questions.contains(&citation.question) {
            wrong.push(format!("subcommand `{}`", citation.question));
        }
        for option in &citation.options {
            if !accepted.options.contains(option) {
                wrong.push(format!("option `{option}` of `{}`", citation.question));
            }
        }
    }
    wrong
}

/// Os textos que têm de citar os comandos, com o nome de cada um: os quatro
/// moldes e as cinco chaves do catálogo nos dois idiomas.
fn cited_texts() -> Vec<(String, String)> {
    let mut texts = Vec::new();
    for (lang, locale) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        for name in ["wave", "review"] {
            texts.push((format!("the {lang} `{name}` mold"), template(lang, name)));
        }
        for key in CATALOG_KEYS {
            texts.push((format!("the {lang} text `{key}`"), translate(key, locale).to_string()));
        }
    }
    texts
}

/// Os quatro moldes e os textos do catálogo citam a busca com o texto do
/// `Grep`, o trecho de uma declaração e quem a usa, cada um com a opção que
/// exige, e nada do que citam sobra fora do que o `run map` aceita.
#[test]
fn the_molds_and_the_catalog_texts_cite_search_slice_and_users() {
    let accepted = accepted_by_the_command();
    for (name, text) in cited_texts() {
        let cited = citations(&text);
        for (question, option) in CITED {
            assert!(
                cited.iter().any(|c| c.question == question && c.options.iter().any(|o| o == option)),
                "{name} does not cite `run map {question} {option}`: {text}"
            );
        }
        assert!(cites_the_search_with_the_text_of_grep(&text), "{name} does not cite `run map search \"<pattern>\"`: {text}");
        let wrong = not_accepted(&text, &accepted);
        assert!(wrong.is_empty(), "{name} cites what `run map` does not accept: {wrong:?}");
    }
}

/// O molde de onda e o de revisão citam também o resumo do arquivo, os testes
/// e a história de uma declaração, cada um com a hora de usar: a lista tem os
/// seis comandos, e o molde só cita subcomando que o binário aceita.
#[test]
fn the_molds_list_each_map_command_with_its_moment_of_use() {
    let accepted = accepted_by_the_command();
    for lang in ["pt-BR", "en-US"] {
        for name in ["wave", "review"] {
            let mold = template(lang, name);
            let cited = citations(&mold);
            for question in ["search", "summary", "slice", "users", "tests", "history"] {
                assert!(cited.iter().any(|c| c.question == question), "the {lang} `{name}` mold does not list `run map {question}`");
                let line = mold
                    .lines()
                    .find(|line| line.contains(&format!("run map {question} ")))
                    .unwrap_or_else(|| panic!("the {lang} `{name}` mold has no line for `run map {question}`"));
                let moment = line.split("`: ").nth(1).unwrap_or_default();
                assert!(moment.split_whitespace().count() >= 3, "the {lang} `{name}` line of `{question}` has no moment of use: {line}");
            }
            assert!(not_accepted(&mold, &accepted).is_empty(), "the {lang} `{name}` mold cites what `run map` does not accept");
        }
    }
}

/// A conferência pega o texto que cita subcomando ou opção que o `run map`
/// não aceita, e deixa passar o que ele aceita.
#[test]
fn a_cited_subcommand_or_option_the_command_does_not_accept_is_caught() {
    let accepted = accepted_by_the_command();
    let good = "Use `mustard-rt run map slice --file a.rs --name f` e `run map importers`.";
    assert!(not_accepted(good, &accepted).is_empty(), "{:?}", not_accepted(good, &accepted));
    let bad_question = "Use `mustard-rt run map fetch --file a.rs`.";
    assert_eq!(not_accepted(bad_question, &accepted), vec!["subcommand `fetch`".to_string()]);
    let bad_option = "Use `mustard-rt run map slice --lines 3 --name f`.";
    assert_eq!(not_accepted(bad_option, &accepted), vec!["option `--lines` of `slice`".to_string()]);
}

/// O molde da onda, nos dois idiomas, diz que a falha pequena nos arquivos da
/// tarefa ou nos vizinhos se conserta na onda, com um teste que falha sem o
/// conserto, e que só vira sobra o que pede decisão do usuário ou toca outra
/// área. O molde de revisão não traz a frase: quem revisa não conserta.
#[test]
fn the_wave_molds_say_a_small_failure_is_fixed_in_the_wave() {
    for (lang, phrases) in [
        (
            "pt-BR",
            [
                "Falha pequena nos arquivos da tarefa ou nos vizinhos se conserta na onda, com um teste que falha sem o conserto.",
                "Só vira sobra o que pede decisão do usuário ou toca outra área.",
            ],
        ),
        (
            "en-US",
            [
                "A small failure in the task's files or their neighbors is fixed in the wave, with a test that fails without the fix.",
                "Only what needs the user's decision or touches another area becomes a leftover.",
            ],
        ),
    ] {
        let wave = template(lang, "wave");
        for phrase in phrases {
            assert!(wave.contains(phrase), "the {lang} wave mold lost `{phrase}`");
        }
        let review = template(lang, "review");
        assert!(!review.contains(phrases[0]), "the {lang} reviewer must not fix small failures: it only points them out");
    }
}
