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
//!
//! A citação que traz só o nome do arquivo, sem a pasta, ganha o caminho
//! certo: [`resolve`] põe a pasta quando um arquivo só do projeto tem esse
//! nome, e a recusa do arquivo que não existe diz os arquivos com o nome dele.

use crate::domain::clarity::is_file_path;
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

/// As citações de arquivo e linha que um texto traz, na ordem em que
/// aparecem e sem repetir: cada pedaço dele que [`file_citation`] reconhece,
/// sem a pontuação da frase em volta.
///
/// Serve ao levantamento: o pedido que já traz o diagnóstico, com arquivo e
/// linha, não precisa ser refeito do zero. Num caso real o pedido trazia o
/// diagnóstico inteiro, e o levantamento perguntou sintoma, causa e
/// reprodução de novo.
#[must_use]
pub fn file_citations(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for piece in pieces(text) {
        if file_citation(piece).is_some() && !out.iter().any(|seen| seen == piece) {
            out.push(piece.to_string());
        }
    }
    out
}

/// Os arquivos que um texto cita, na ordem em que aparecem e sem repetir, com
/// barras normais: o pedaço no formato de [`file_citation`] vale pelo
/// caminho, sem a linha, e o caminho sem linha vale quando o detector único
/// ([`is_file_path`]) o reconhece e ele tem pasta (`apps/rt/src/a.rs`,
/// `.claude/spec`). O nome sem pasta e sem linha (`Cargo.toml`,
/// `self.facts`) não vale, porque não se distingue de um nome de código.
///
/// Serve ao levantamento: dois textos que citam o mesmo arquivo falam do
/// mesmo trecho do código.
#[must_use]
pub fn cited_files(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for piece in pieces(text) {
        let path = match file_citation(piece) {
            Some((path, _)) => path,
            None if is_file_path(piece) && piece.contains(['/', '\\']) => piece.replace('\\', "/"),
            None => continue,
        };
        let path = clean_path(&path);
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// Os pedaços de um texto que podem citar um arquivo: separados por espaço,
/// crase, aspas e abertura ou fecho de grupo, sem a pontuação da frase no
/// fim.
fn pieces(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || "`\"'()[]{}<>,;".contains(c)).map(|piece| piece.trim_end_matches(['.', ':']))
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
        if let Some(name) = code_name(span.trim())
            && !out.contains(&name)
        {
            out.push(name);
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
    /// O arquivo citado não existe. Quando a citação traz só o nome, sem a
    /// pasta, `same_name` são os arquivos do projeto que têm esse nome, em
    /// ordem de caminho; senão, fica vazio.
    MissingFile { path: String, same_name: Vec<String> },
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
            Self::MissingFile { path, same_name } => {
                Some(Refusal::CitedFileMissing { fact, path: path.clone(), same_name: same_name.clone() })
            }
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
    /// Os arquivos do projeto cujo nome, sem a pasta, é `name`: o caminho de
    /// cada um, em ordem de caminho.
    fn files_named(&self, name: &str) -> Vec<String>;
}

/// O achado do arquivo citado em `path` que não existe. Quando a citação traz
/// só o nome do arquivo, sem a pasta, ele leva os arquivos do projeto que têm
/// esse nome: é o caminho que a recusa sugere.
#[must_use]
pub fn missing_file(world: &impl CitationWorld, path: &str) -> Finding {
    let same_name = if path.contains('/') { Vec::new() } else { world.files_named(path) };
    Finding::MissingFile { path: path.to_string(), same_name }
}

/// A fonte `source` com o caminho certo, pela conferência que recusaria a
/// gravação dela.
///
/// A fonte que se confere volta como veio, e a que não cita arquivo também.
/// A que cita só o nome de um arquivo que não está ali volta com a pasta,
/// quando um arquivo só do projeto tem esse nome e a linha citada cabe nele.
/// As outras voltam com o achado que recusaria: o arquivo que não existe, com
/// os arquivos que têm o nome dele quando são dois ou mais, ou a linha que
/// passa do fim.
///
/// # Errors
///
/// O achado que recusa, quando a fonte não se confere nem com a pasta.
pub fn resolve(world: &impl CitationWorld, source: &str) -> Result<String, Finding> {
    let refused = |source: &str| check(world, source, "").into_iter().find(Finding::is_refusal);
    match refused(source) {
        None => Ok(source.to_string()),
        Some(Finding::MissingFile { same_name, .. }) if same_name.len() == 1 => {
            let lines = source.trim().rsplit_once(':').map_or("", |(_, lines)| lines);
            let found = format!("{}:{lines}", same_name[0]);
            match refused(&found) {
                None => Ok(found),
                Some(finding) => Err(finding),
            }
        }
        Some(finding) => Err(finding),
    }
}

/// O lugar `place` que um achado cita, com o caminho certo, pela mesma
/// conferência das fontes ([`resolve`]).
///
/// O lugar com linha se confere como a fonte de um fato. O que é só um
/// caminho de arquivo (`apps/a.rs`, `a.rs`) tem de existir, e o que traz só o
/// nome ganha a pasta quando um arquivo só do projeto tem esse nome. O lugar
/// que não cita arquivo — um nome de código, um comando — volta como veio.
///
/// # Errors
///
/// O achado que recusa: o arquivo que não existe, com os arquivos que têm o
/// nome dele quando são dois ou mais, ou a linha que passa do fim.
pub fn resolve_place(world: &impl CitationWorld, place: &str) -> Result<String, Finding> {
    let place = place.trim();
    if file_citation(place).is_some() || !is_file_path(place) {
        return resolve(world, place);
    }
    let path = clean_path(&place.replace('\\', "/"));
    if world.file_lines(&path).is_some() {
        return Ok(path);
    }
    match missing_file(world, &path) {
        Finding::MissingFile { same_name, .. } if same_name.len() == 1 => Ok(same_name[0].clone()),
        finding => Err(finding),
    }
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
            return vec![missing_file(world, path)];
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
        if let Some((path, _)) = &cited
            && !found.iter().any(|(declared_in, _)| same_file(declared_in, path))
        {
            out.push(Finding::NameElsewhere { name, path: path.clone(), found });
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
        fn files_named(&self, name: &str) -> Vec<String> {
            self.files.keys().filter(|path| path.rsplit('/').next() == Some(name)).map(|path| (*path).to_string()).collect()
        }
    }

    fn world() -> World {
        World {
            files: BTreeMap::from([("src/a.rs", 40), ("src/b.rs", 10)]),
            decls: vec![("src/a.rs", "SpecLog", 12), ("src/b.rs", "check_citations", 3), ("src/b.rs", "run", 7)],
            map: true,
        }
    }

    /// O arquivo citado vale pelo caminho: sem a linha, com barras normais e
    /// sem repetir. O caminho sem linha precisa de pasta e extensão; o nome
    /// solto, a frase, o endereço e o número não valem.
    #[test]
    fn the_files_a_text_cites_are_told_apart_from_names_and_addresses() {
        let text = "Veja `apps/rt/src/a.rs:10-20`, src\\b.rs e (docs/c.md). Depois ./src/d.rs: de novo apps/rt/src/a.rs.";
        assert_eq!(cited_files(text), ["apps/rt/src/a.rs", "src/b.rs", "docs/c.md", "src/d.rs"]);
        assert_eq!(cited_files("`Cargo.toml`, `self.facts`, https://example.com/a.html, e/ou, docs/v1.2, 3/4"), Vec::<String>::new());
        assert_eq!(cited_files("Cargo.toml:3 e README.md:10"), ["Cargo.toml", "README.md"]);
    }

    /// O caminho sem linha segue o detector de caminho da conferência de
    /// escrita: a pasta oculta e a que parte da casa contam, mesmo sem
    /// extensão; a extensão longa conta, porque o caminho tem pasta, e a só de
    /// maiúsculas não conta.
    #[test]
    fn a_path_without_a_line_is_cited_as_the_writing_check_detects_it() {
        assert_eq!(cited_files("Grava em .claude/spec e lê ~/projetos/notas."), [".claude/spec", "~/projetos/notas"]);
        assert_eq!(
            cited_files("Veja src/App.svelte, prisma/schema.prisma, docs/guia.markdown e docs/LEIA.MD."),
            ["src/App.svelte", "prisma/schema.prisma", "docs/guia.markdown"]
        );
    }

    /// O lugar de um achado se confere com e sem a linha: o que cita só o
    /// nome de um arquivo que um arquivo só do projeto tem ganha a pasta; o
    /// arquivo que não existe, o nome que dois arquivos têm e a linha depois
    /// do fim recusam; o lugar que não cita arquivo volta como veio.
    #[test]
    fn a_finding_place_gets_its_folder_and_a_missing_one_is_refused() {
        let mut two = world();
        two.files.insert("lib/a.rs", 5);
        assert_eq!(resolve_place(&world(), "src/a.rs:12"), Ok("src/a.rs:12".into()));
        assert_eq!(resolve_place(&world(), " a.rs:12 "), Ok("src/a.rs:12".into()));
        assert_eq!(resolve_place(&world(), "b.rs"), Ok("src/b.rs".into()));
        assert_eq!(resolve_place(&world(), "./src/b.rs"), Ok("src/b.rs".into()));
        assert_eq!(resolve_place(&world(), "SpecLog::read"), Ok("SpecLog::read".into()));
        assert_eq!(resolve_place(&world(), "src/c.rs"), Err(Finding::MissingFile { path: "src/c.rs".into(), same_name: Vec::new() }));
        assert_eq!(resolve_place(&world(), "src/b.rs:11"), Err(Finding::MissingLine { path: "src/b.rs".into(), line: 11, lines: 10 }));
        assert_eq!(
            resolve_place(&two, "a.rs"),
            Err(Finding::MissingFile { path: "a.rs".into(), same_name: vec!["lib/a.rs".into(), "src/a.rs".into()] })
        );
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
        assert_eq!(check(&World::default(), "src/nothing.rs", text), Vec::new(), "no names, no map warning");
        let text = "`State`, `SpecLog`, `check_citations`, `spanOf`, `TOP`, `State`";
        assert_eq!(cited_names(text), vec!["State", "SpecLog", "check_citations", "spanOf", "TOP"]);
    }

    #[test]
    fn a_qualified_or_called_name_is_checked_by_its_last_segment() {
        assert_eq!(cited_names("`io::citation::citation_roots` e `run()` e `Finding::NoMap()`"), vec![
            "citation_roots", "run", "NoMap"
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
        assert_eq!(missing, vec![Finding::MissingFile { path: "src/nao-existe.rs".into(), same_name: Vec::new() }]);
        assert_eq!(
            missing[0].refusal(1),
            Some(Refusal::CitedFileMissing { fact: 1, path: "src/nao-existe.rs".into(), same_name: Vec::new() })
        );
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
