//! `citation` — a conferência das fontes que um texto cita.
//!
//! Um fato do levantamento, e uma tarefa do plano, dizem de onde saíram: o
//! arquivo e a linha, o comando com o resultado, ou o número da mensagem do
//! usuário. Quando a fonte cita um arquivo, ele tem de existir e ter a linha
//! citada. Os nomes de código que o texto cita entre crases (`SpecLog`,
//! `check_citations`, `a::b`, `run()`) têm de existir no mapa do projeto, de
//! preferência no arquivo citado.
//!
//! A regra mora aqui uma vez, sem disco: quem responde se o arquivo existe e
//! onde o nome é declarado é o [`CitationWorld`], e o disco está em
//! `io::citation`. A gravação de um ponto e a conferência do plano chamam a
//! mesma [`check`].
//!
//! Só o arquivo que não existe e a linha que passa do fim recusam. O nome que
//! o mapa acha em outro arquivo, o nome que ele não conhece e a falta do mapa
//! só avisam: o mapa pode estar atrás do código, e um aviso não prende o
//! assistente num laço de repetição.

use crate::domain::project_map::clean_path;
use crate::domain::spec_events::Refusal;
use crate::platform::i18n::{translate, Locale};

/// A fonte de um fato que cita um arquivo: `caminho:linha` ou
/// `caminho:início-fim`, sem espaço. Devolve o caminho, com barras normais, e
/// a última linha citada. `None` para as outras fontes — um comando com o
/// resultado, o número de uma mensagem, um endereço.
#[must_use]
pub fn file_citation(source: &str) -> Option<(String, u64)> {
    let source = source.trim();
    if source.is_empty() || source.contains(char::is_whitespace) || source.contains("://") {
        return None;
    }
    let (path, lines) = source.rsplit_once(':')?;
    let (start, end) = lines.split_once('-').unwrap_or((lines, lines));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(start) || !digits(end) {
        return None;
    }
    let path = path.replace('\\', "/");
    if path.is_empty() || !(path.contains('/') || path.contains('.')) {
        return None;
    }
    let start: u64 = start.parse().ok()?;
    let end: u64 = end.parse().ok()?;
    Some((path, start.max(end)))
}

/// Os nomes de código que um texto cita entre crases, na ordem em que
/// aparecem, sem repetir. Conta como nome o identificador com `_`, o que tem
/// letra maiúscula (`SpecLog`, `State`), o qualificado (`a::b`, que vale pelo
/// último pedaço) e o chamado (`run()`, que vale sem os parênteses). Não conta
/// o caminho (com `/` ou `.`), a opção (`--spec`), o código com hífen, o que
/// tem espaço, a palavra solta minúscula (`survey`) nem o JSON.
#[must_use]
pub fn cited_names(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, span) in text.split('`').enumerate() {
        if i % 2 == 0 {
            continue;
        }
        if let Some(name) = code_name(span.trim()) {
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// O nome que um trecho entre crases cita, quando ele é um nome de código.
fn code_name(span: &str) -> Option<String> {
    let called = span.strip_suffix("()");
    let bare = called.unwrap_or(span);
    if !bare.split("::").all(is_identifier) {
        return None;
    }
    let qualified = bare.contains("::");
    let last = bare.rsplit("::").next()?;
    let looks_like_code = called.is_some()
        || qualified
        || last.contains('_')
        || last.chars().any(|c| c.is_ascii_uppercase());
    looks_like_code.then(|| last.to_string())
}

/// Um identificador de código: letras, números e `_`, em ASCII, começando por
/// letra ou `_`, com pelo menos uma letra.
fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else { return false };
    (first.is_ascii_alphabetic() || first == '_')
        && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && word.chars().any(|c| c.is_ascii_alphabetic())
}

/// O que a conferência achou numa fonte e no texto que ela sustenta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// O arquivo citado não existe.
    MissingFile { path: String },
    /// O arquivo existe, e a linha citada é a zero ou passa do fim dele.
    MissingLine { path: String, line: u64, lines: u64 },
    /// O nome citado não é declarado no arquivo citado, e sim nestes lugares
    /// (caminho e linha, em ordem de caminho).
    NameElsewhere { name: String, path: String, found: Vec<(String, u64)> },
    /// O mapa do projeto não conhece o nome citado.
    NameUnknown { name: String },
    /// Não há mapa do projeto, então os nomes citados não foram conferidos.
    NoMap,
}

impl Finding {
    /// `true` para o arquivo que não existe e para a linha que passa do fim:
    /// só esses recusam. Os achados de nome só avisam.
    #[must_use]
    pub fn is_refusal(&self) -> bool {
        matches!(self, Self::MissingFile { .. } | Self::MissingLine { .. })
    }

    /// A recusa da gravação do fato número `fact`, quando o achado recusa.
    #[must_use]
    pub fn refusal(&self, fact: usize) -> Option<Refusal> {
        match self {
            Self::MissingFile { path } => Some(Refusal::CitedFileMissing { fact, path: path.clone() }),
            Self::MissingLine { path, line, lines } => {
                Some(Refusal::CitedLineMissing { fact, path: path.clone(), line: *line, lines: *lines })
            }
            _ => None,
        }
    }

    /// O aviso sobre o fato número `fact`, no idioma pedido, quando o achado
    /// só avisa.
    #[must_use]
    pub fn warning(&self, fact: usize, lang: Locale) -> Option<String> {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::NameElsewhere { name, path, found } => {
                let found: Vec<String> = found.iter().map(|(path, line)| format!("{path}:{line}")).collect();
                Some(fill(
                    "spec_events.name_elsewhere",
                    &[
                        ("{fact}", fact.to_string()),
                        ("{name}", name.clone()),
                        ("{path}", path.clone()),
                        ("{found}", found.join(", ")),
                    ],
                ))
            }
            Self::NameUnknown { name } => {
                Some(fill("spec_events.name_unknown", &[("{fact}", fact.to_string()), ("{name}", name.clone())]))
            }
            Self::NoMap => Some(fill("spec_events.names_unchecked", &[])),
            Self::MissingFile { .. } | Self::MissingLine { .. } => None,
        }
    }
}

/// O que a conferência pergunta ao mundo: se o arquivo existe e onde o nome é
/// declarado. O disco responde em `io::citation`; o teste responde de memória.
pub trait CitationWorld {
    /// Quantas linhas o arquivo citado tem; `None` quando ele não existe.
    fn file_lines(&self, path: &str) -> Option<u64>;
    /// Onde o mapa declara o nome: caminho e linha, em ordem de caminho.
    fn declared(&self, name: &str) -> Vec<(String, u64)>;
    /// `true` quando há mapa para conferir os nomes.
    fn has_map(&self) -> bool;
}

/// Confere uma fonte e o texto que ela sustenta.
///
/// Quando a fonte cita um arquivo, ele tem de existir e ter a linha citada;
/// senão, o achado recusa e os nomes nem são olhados. Depois, cada nome de
/// código citado no texto é procurado no mapa: achado no arquivo citado (ou
/// em qualquer arquivo, quando a fonte não cita arquivo), passa; achado só em
/// outro arquivo, avisa onde está; não achado, avisa. Sem mapa, um aviso só
/// diz que os nomes não foram conferidos. Uma fonte que não cita arquivo e um
/// texto sem nome não acham nada.
#[must_use]
pub fn check(world: &impl CitationWorld, source: &str, text: &str) -> Vec<Finding> {
    let cited = file_citation(source);
    if let Some((path, line)) = &cited {
        let Some(lines) = world.file_lines(path) else {
            return vec![Finding::MissingFile { path: path.clone() }];
        };
        if *line == 0 || *line > lines {
            return vec![Finding::MissingLine { path: path.clone(), line: *line, lines }];
        }
    }
    let names = cited_names(text);
    if names.is_empty() {
        return Vec::new();
    }
    if !world.has_map() {
        return vec![Finding::NoMap];
    }
    let mut out = Vec::new();
    for name in names {
        let found = world.declared(&name);
        if found.is_empty() {
            out.push(Finding::NameUnknown { name });
            continue;
        }
        if let Some((path, _)) = &cited {
            if !found.iter().any(|(declared_in, _)| same_file(declared_in, path)) {
                out.push(Finding::NameElsewhere { name, path: path.clone(), found });
            }
        }
    }
    out
}

/// `true` quando o caminho do mapa é o arquivo citado. A citação pode vir de
/// uma subpasta e trazer só o fim do caminho, como `src/a.rs` para
/// `apps/rt/src/a.rs`.
fn same_file(map_path: &str, cited: &str) -> bool {
    let cited = clean_path(cited);
    map_path == cited || map_path.ends_with(&format!("/{cited}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Um mundo de memória: arquivos com o número de linhas e as declarações
    /// do mapa, nome a nome.
    #[derive(Default)]
    struct World {
        files: BTreeMap<&'static str, u64>,
        decls: Vec<(&'static str, &'static str, u64)>,
        map: bool,
    }

    impl CitationWorld for World {
        fn file_lines(&self, path: &str) -> Option<u64> {
            self.files.get(path).copied()
        }
        fn declared(&self, name: &str) -> Vec<(String, u64)> {
            self.decls.iter().filter(|(_, n, _)| *n == name).map(|(p, _, l)| ((*p).to_string(), *l)).collect()
        }
        fn has_map(&self) -> bool {
            self.map
        }
    }

    fn world() -> World {
        World {
            files: BTreeMap::from([("src/a.rs", 40), ("src/b.rs", 10)]),
            decls: vec![("src/a.rs", "SpecLog", 12), ("src/b.rs", "check_citations", 3), ("src/b.rs", "run", 7)],
            map: true,
        }
    }

    #[test]
    fn a_cited_name_declared_in_the_cited_file_passes() {
        assert_eq!(check(&world(), "src/a.rs:12", "o `SpecLog` guarda a leitura"), Vec::new());
        assert_eq!(check(&world(), "src\\a.rs:3-20", "o `SpecLog` guarda a leitura"), Vec::new());
        // Sem arquivo citado, basta o mapa conhecer o nome.
        assert_eq!(check(&world(), "cargo test → 3 passed", "o `SpecLog` guarda a leitura"), Vec::new());
    }

    #[test]
    fn a_cited_name_declared_elsewhere_warns_with_where_it_is() {
        let found = check(&world(), "src/a.rs:2", "quem confere é `check_citations`");
        let expected = Finding::NameElsewhere {
            name: "check_citations".into(),
            path: "src/a.rs".into(),
            found: vec![("src/b.rs".into(), 3)],
        };
        assert_eq!(found, vec![expected.clone()]);
        assert!(!expected.is_refusal());
        assert_eq!(expected.refusal(2), None);
        let pt = expected.warning(2, Locale::PtBr).unwrap();
        assert!(pt.contains("O fato 2") && pt.contains("`check_citations`") && pt.contains("src/b.rs:3"), "{pt}");
        let en = expected.warning(2, Locale::EnUs).unwrap();
        assert!(en.contains("Fact 2") && en.contains("src/a.rs") && en.contains("src/b.rs:3"), "{en}");
    }

    #[test]
    fn a_cited_name_the_map_does_not_know_warns() {
        let found = check(&world(), "src/a.rs:2", "o `Inexistente` e o `SpecLog`");
        assert_eq!(found, vec![Finding::NameUnknown { name: "Inexistente".into() }]);
        assert!(found[0].warning(1, Locale::PtBr).unwrap().contains("não conhece"));
    }

    #[test]
    fn words_in_backticks_that_are_not_code_names_are_not_checked() {
        let text = "a fase `survey`, a `plan`, `src/a.rs`, `mod.rs`, `--spec`, `MSTD-RULE-0025`, \
                    `cargo test -p x`, `{\"a\":1}`, `P-12`, `Não`, `Vec<String>`, `1024`, ``";
        assert_eq!(cited_names(text), Vec::<String>::new());
        assert_eq!(check(&World::default(), "src/nada.rs", text), Vec::new(), "no names, no map warning");
        let text = "`State`, `SpecLog`, `check_citations`, `spanOf`, `TOP`, `State`";
        assert_eq!(cited_names(text), vec!["State", "SpecLog", "check_citations", "spanOf", "TOP"]);
    }

    #[test]
    fn a_qualified_or_called_name_is_checked_by_its_last_segment() {
        assert_eq!(cited_names("`io::citation::check_at` e `run()` e `Finding::NoMap()`"), vec![
            "check_at", "run", "NoMap"
        ]);
        assert_eq!(check(&world(), "src/b.rs:7", "chama `commands::run()`"), Vec::new());
        assert_eq!(check(&world(), "src/b.rs:7", "chama `a::SpecLog`"), vec![Finding::NameElsewhere {
            name: "SpecLog".into(),
            path: "src/b.rs".into(),
            found: vec![("src/a.rs".into(), 12)],
        }]);
    }

    #[test]
    fn a_missing_file_or_line_refuses_and_the_names_are_not_looked_at() {
        let missing = check(&world(), "src/nao-existe.rs:10", "o `Inexistente`");
        assert_eq!(missing, vec![Finding::MissingFile { path: "src/nao-existe.rs".into() }]);
        assert_eq!(missing[0].refusal(1), Some(Refusal::CitedFileMissing { fact: 1, path: "src/nao-existe.rs".into() }));
        let past_end = check(&world(), "src/b.rs:3-11", "");
        assert_eq!(past_end, vec![Finding::MissingLine { path: "src/b.rs".into(), line: 11, lines: 10 }]);
        assert!(past_end[0].is_refusal() && past_end[0].warning(1, Locale::PtBr).is_none());
        assert_eq!(check(&world(), "src/b.rs:0", ""), vec![Finding::MissingLine {
            path: "src/b.rs".into(),
            line: 0,
            lines: 10
        }]);
    }

    #[test]
    fn without_a_map_one_finding_says_the_names_were_not_checked() {
        let world = World { map: false, ..world() };
        assert_eq!(check(&world, "src/a.rs:1", "o `SpecLog` e o `Outro`"), vec![Finding::NoMap]);
        assert!(Finding::NoMap.warning(1, Locale::EnUs).unwrap().contains("mustard-rt run scan"));
        assert_eq!(check(&world, "src/a.rs:1", "sem nome"), Vec::new());
    }
}
