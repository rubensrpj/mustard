//! `code_route` — o caminho do código pelo mapa, para as travas que veem cada
//! leitura e cada busca.
//!
//! A leitura inteira de um arquivo grande de código volta com as partes dele
//! e o comando que traz só a parte certa ([`whole_read`]). A busca por palavra
//! em pastas de código é assunto do mapa ([`holds_code`]); a resposta dela mora
//! em [`super::word_search`]. O que o mapa não guarda passa: documento,
//! configuração, dados, pasta fora do projeto.
//!
//! Nunca falha: sem mapa, com o mapa ilegível ou o banco travado, a resposta
//! é nenhuma, e a ação segue.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mustard_core::domain::project_map::{self, FilePart, FileParts, ProjectMap};
use mustard_core::io::project_map::{self as store, Need};
use mustard_core::io::workspace::{is_git_repo_root, linked_worktree_main};
use mustard_core::platform::i18n::Locale;

use crate::shared::config_key::{surely_takes, NameFilter, Walk};
use crate::shared::paths::relative_to_cwd;
use crate::shared::say::say;

/// Acima deste tanto de linhas, a leitura inteira de um arquivo de código do
/// mapa volta com as partes dele.
pub(crate) const WHOLE_READ_MAX_LINES: usize = 300;

/// Quantas partes a recusa mostra; o resto sai no comando das partes.
const PARTS_SHOWN: usize = 40;

/// Um caminho do projeto: a raiz da árvore em que ele mora — a do projeto ou
/// a de uma cópia de trabalho dele —, o caminho relativo a ela, como o mapa o
/// guarda, e o caminho de verdade no disco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectPath {
    pub(crate) tree: PathBuf,
    pub(crate) rel: String,
    pub(crate) abs: PathBuf,
}

/// `given` como caminho do projeto em `root`: dentro da raiz, ou dentro de
/// uma cópia de trabalho ligada ao mesmo repositório, como a cópia de uma
/// onda. O caminho relativo se lê a partir de `base`. `None` fora dos dois.
/// Lê só os arquivos que o git deixa, sem rodar o git.
pub(crate) fn project_path(root: &str, base: &str, given: &str) -> Option<ProjectPath> {
    let given = given.replace('\\', "/");
    let abs = if Path::new(&given).is_absolute() {
        PathBuf::from(&given)
    } else {
        Path::new(base).join(&given)
    };
    let abs_text = abs.to_string_lossy().replace('\\', "/");
    if let Some(rel) = relative_to_cwd(root, &abs_text) {
        return Some(ProjectPath { tree: PathBuf::from(root), rel: rel.trim_start_matches("./").to_string(), abs });
    }
    let start = if abs.is_dir() { abs.as_path() } else { abs.parent()? };
    let top = linked_copy(start, Path::new(root))?;
    let rel = relative_to_cwd(&top.to_string_lossy(), &abs_text)?;
    Some(ProjectPath { tree: top, rel: rel.trim_start_matches("./").to_string(), abs })
}

/// A raiz da cópia de trabalho em que `start` mora, quando ela é uma cópia
/// ligada ao repositório do projeto `root`, como a de uma onda. `None` no
/// próprio projeto e em qualquer outra pasta. Lê só os arquivos que o git
/// deixa, sem rodar o git.
pub(crate) fn linked_copy(start: &Path, root: &Path) -> Option<PathBuf> {
    let start = std::path::absolute(start).ok()?;
    let top = start.ancestors().find(|folder| is_git_repo_root(folder))?;
    let main = linked_worktree_main(top)?;
    match (std::fs::canonicalize(&main), std::fs::canonicalize(root)) {
        (Ok(main), Ok(root)) if main == root => Some(top.to_path_buf()),
        _ => None,
    }
}

/// A recusa da leitura inteira do arquivo `file`, de texto `text`, que
/// traria `lines` linhas: as partes dele e o comando do trecho, quando o mapa
/// de `root` o guarda e ele passa de [`WHOLE_READ_MAX_LINES`]. Numa cópia de
/// trabalho, as partes saem com as linhas que têm no texto da cópia
/// ([`parts_in_copy`]). `None` no arquivo pequeno, no que o mapa não guarda,
/// sem mapa e em todo erro de leitura do mapa.
pub(crate) fn whole_read(root: &Path, file: &ProjectPath, text: &str, lines: usize, lang: Locale) -> Option<String> {
    if lines <= WHOLE_READ_MAX_LINES {
        return None;
    }
    let map = store::read_for(root, Need::Parts(&file.rel)).ok()?;
    let mut found = project_map::parts(&map, &file.rel).ok()?;
    if file.tree != root
        && let Ok(project) = std::fs::read_to_string(root.join(&file.rel))
    {
        found = parts_in_copy(found, &project, text);
    }
    let parts = parts_line(&found, lang);
    Some(say(
        "code_route.whole_read",
        lang,
        &[("{file}", &found.file), ("{lines}", &lines.to_string()), ("{parts}", &parts)],
    ))
}

/// As partes numa linha curta: `nome começo-fim`, até [`PARTS_SHOWN`], com
/// quantas ficaram de fora e a linha em que os testes começam.
fn parts_line(found: &FileParts, lang: Locale) -> String {
    let mut line: Vec<String> =
        found.parts.iter().take(PARTS_SHOWN).map(|part| format!("{} {}-{}", part.name, part.line, part.end_line)).collect();
    let left = found.parts.len().saturating_sub(PARTS_SHOWN);
    if left > 0 {
        line.push(say("code_route.more_parts", lang, &[("{count}", &left.to_string()), ("{file}", &found.file)]));
    }
    let mut text = line.join(", ");
    if let Some(tests) = found.tests_line {
        text.push_str("; ");
        text.push_str(&say("code_route.tests_from", lang, &[("{line}", &tests.to_string())]));
    }
    text
}

/// As partes `found`, com as linhas do texto `project` de onde o mapa as
/// tirou, levadas para as linhas que têm no texto `copy` do mesmo arquivo
/// numa cópia de trabalho ([`CopyLines`]), com a linha em que os testes
/// começam. A parte que a cópia apagou sai da lista.
pub(crate) fn parts_in_copy(found: FileParts, project: &str, copy: &str) -> FileParts {
    if project == copy {
        return found;
    }
    let lines = CopyLines::between(project, copy);
    let parts = found
        .parts
        .into_iter()
        .filter_map(|part| {
            let (line, end_line) = lines.range(part.line, part.end_line)?;
            Some(FilePart { line, end_line, ..part })
        })
        .collect();
    FileParts { file: found.file, parts, tests_line: found.tests_line.map(|line| lines.start(line)) }
}

/// Onde as linhas de um arquivo do projeto ficam no mesmo arquivo de uma
/// cópia de trabalho que o mudou. As linhas iguais dos dois textos se casam
/// em ordem: as do começo e as do fim que batem; no que sobra no meio, as que
/// aparecem uma vez só de cada lado; e o mesmo de novo entre elas. A linha
/// que não casou, mudada ou apagada na cópia, fica entre as vizinhas que
/// casaram, com a folga para o lado de trazer uma linha a mais, nunca a menos.
pub(crate) struct CopyLines {
    /// Para cada linha do projeto, contada a partir de 0, a da cópia que
    /// casou com ela.
    paired: Vec<Option<usize>>,
    /// Quantas linhas a cópia tem.
    copy_len: usize,
}

impl CopyLines {
    /// O casamento das linhas de `project` com as de `copy`.
    pub(crate) fn between(project: &str, copy: &str) -> Self {
        let project: Vec<&str> = project.lines().collect();
        let copy: Vec<&str> = copy.lines().collect();
        let mut paired = vec![None; project.len()];
        pair_lines(&project, &copy, (0, project.len()), (0, copy.len()), &mut paired);
        Self { paired, copy_len: copy.len() }
    }

    /// A linha da cópia, contada a partir de 1, em que começa o que no
    /// projeto começa na linha `line`: a que casou com ela ou, sem par, a
    /// seguinte à última casada antes dela.
    pub(crate) fn start(&self, line: u64) -> u64 {
        let at = line_index(line);
        if let Some(copy) = self.paired.get(at).copied().flatten() {
            return copy as u64 + 1;
        }
        let before = &self.paired[..at.min(self.paired.len())];
        before.iter().rev().find_map(|paired| *paired).map_or(1, |copy| copy as u64 + 2)
    }

    /// A linha da cópia, contada a partir de 1, em que termina o que no
    /// projeto termina na linha `line`: a que casou com ela ou, sem par, a
    /// anterior à primeira casada depois dela.
    pub(crate) fn end(&self, line: u64) -> u64 {
        let at = line_index(line);
        if let Some(copy) = self.paired.get(at).copied().flatten() {
            return copy as u64 + 1;
        }
        let after = self.paired.get(at.saturating_add(1)..).unwrap_or_default();
        after.iter().find_map(|paired| *paired).map_or(self.copy_len as u64, |copy| copy as u64)
    }

    /// O começo e o fim na cópia do que no projeto vai de `line` a
    /// `end_line`; `None` quando a cópia apagou tudo.
    pub(crate) fn range(&self, line: u64, end_line: u64) -> Option<(u64, u64)> {
        let (first, last) = (self.start(line), self.end(end_line.max(line)));
        (first <= last).then_some((first, last))
    }
}

/// A posição, contada a partir de 0, da linha `line`, contada a partir de 1.
fn line_index(line: u64) -> usize {
    usize::try_from(line.max(1) - 1).unwrap_or(usize::MAX)
}

/// Casa as linhas iguais de `project[p0..p1]` com as de `copy[c0..c1]`, em
/// ordem, e grava em `paired` a linha da cópia de cada linha do projeto que
/// casou ([`CopyLines`]).
fn pair_lines(
    project: &[&str],
    copy: &[&str],
    (mut p0, mut p1): (usize, usize),
    (mut c0, mut c1): (usize, usize),
    paired: &mut [Option<usize>],
) {
    while p0 < p1 && c0 < c1 && project[p0] == copy[c0] {
        paired[p0] = Some(c0);
        (p0, c0) = (p0 + 1, c0 + 1);
    }
    while p0 < p1 && c0 < c1 && project[p1 - 1] == copy[c1 - 1] {
        (p1, c1) = (p1 - 1, c1 - 1);
        paired[p1] = Some(c1);
    }
    // Para cada texto de linha: quantas vezes aparece e onde, no projeto e na cópia.
    let mut seen: HashMap<&str, [(usize, usize); 2]> = HashMap::new();
    for (at, text) in project.iter().enumerate().take(p1).skip(p0) {
        let side = &mut seen.entry(*text).or_default()[0];
        *side = (side.0 + 1, at);
    }
    for (at, text) in copy.iter().enumerate().take(c1).skip(c0) {
        if let Some(sides) = seen.get_mut(text) {
            sides[1] = (sides[1].0 + 1, at);
        }
    }
    let mut once: Vec<(usize, usize)> =
        seen.values().filter(|[p, c]| p.0 == 1 && c.0 == 1).map(|[p, c]| (p.1, c.1)).collect();
    if once.is_empty() {
        return;
    }
    once.sort_unstable();
    for (p, c) in in_order(&once) {
        pair_lines(project, copy, (p0, p), (c0, c), paired);
        paired[p] = Some(c);
        (p0, c0) = (p + 1, c + 1);
    }
    pair_lines(project, copy, (p0, p1), (c0, c1), paired);
}

/// A maior sequência de `pairs`, já em ordem de linha do projeto, em que as
/// linhas da cópia também crescem.
fn in_order(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut before: Vec<Option<usize>> = vec![None; pairs.len()];
    for (at, &(_, copy)) in pairs.iter().enumerate() {
        let length = tails.partition_point(|&tail| pairs[tail].1 < copy);
        before[at] = length.checked_sub(1).map(|shorter| tails[shorter]);
        if length == tails.len() {
            tails.push(at);
        } else {
            tails[length] = at;
        }
    }
    let mut chain = Vec::new();
    let mut at = tails.last().copied();
    while let Some(here) = at {
        chain.push(pairs[here]);
        at = before[here];
    }
    chain.reverse();
    chain
}

/// As extensões que `glob` aceita, quando ele termina nelas (`*.md`,
/// `docs/**/*.{md,toml}`). `None` quando o fim tem curinga ou não tem
/// extensão: a busca então vale para qualquer arquivo.
pub(crate) fn glob_extensions(glob: &str) -> Option<Vec<String>> {
    let last = glob.rsplit('/').next().unwrap_or(glob);
    let tail = if let Some(inner) = last.strip_suffix('}') {
        inner.rsplit_once(".{")?.1.split(',').map(str::to_string).collect()
    } else {
        vec![last.rsplit_once('.')?.1.to_string()]
    };
    let plain = |ext: &String| !ext.is_empty() && ext.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    tail.iter().all(plain).then_some(tail)
}

/// `true` quando os filtros `filters` (na ordem da linha) deixam de fora o
/// arquivo `path` com certeza: o último filtro de saída que casa com o nome
/// dele, e nenhum de entrada depois dele que possa casar, pois o último que
/// casa decide. Filtro de saída com pasta, `[` ou `\` não conta como certo;
/// filtro de entrada que a leitura não entende pode casar com tudo.
fn left_out(path: &str, filters: &[NameFilter], braces: bool) -> bool {
    let file = Path::new(path);
    let name = file.file_name().and_then(|name| name.to_str()).unwrap_or(path);
    let extension = file.extension().and_then(|ext| ext.to_str());
    let Some(last) = filters.iter().rposition(|filter| filter.exclude && surely_takes(&filter.glob, name, braces)) else {
        return false;
    };
    !filters[last + 1..].iter().any(|filter| {
        !filter.exclude
            && glob_extensions(&filter.glob).is_none_or(|only| extension.is_some_and(|ext| only.iter().any(|o| o == ext)))
    })
}

/// `true` quando alguma das pastas `folders` (relativas à raiz; vazia é a
/// raiz) guarda código do mapa `paths` que os filtros de nome de arquivo
/// `filters`, de entrada e de saída, na ordem da linha, deixam passar: os de
/// entrada estreitam a busca aos arquivos que nomeiam, e os de saída tiram os
/// que casam com certeza. `walk` diz como a busca lê as chaves dos filtros.
/// Só a busca que passa por código do mapa é assunto do mapa: a de um arquivo
/// só, de documentos ou de pastas sem código passa.
pub(crate) fn holds_code(paths: &ProjectMap, folders: &[String], filters: &[NameFilter], walk: Walk) -> bool {
    if folders.is_empty() {
        return false;
    }
    // Os filtros de entrada juntam o que aceitam; um sem extensão aceita tudo.
    let mut only: Vec<String> = Vec::new();
    for filter in filters.iter().filter(|filter| !filter.exclude) {
        match glob_extensions(&filter.glob) {
            Some(extensions) => only.extend(extensions),
            None => {
                only.clear();
                break;
            }
        }
    }
    let braces = walk != Walk::Grep;
    let inside = |path: &str, folder: &str| folder.is_empty() || path.starts_with(&format!("{}/", folder.trim_end_matches('/')));
    let wanted = |path: &str| {
        (only.is_empty()
            || Path::new(path).extension().and_then(|ext| ext.to_str()).is_some_and(|ext| only.iter().any(|o| o == ext)))
            && !left_out(path, filters, braces)
    };
    paths.modules.iter().any(|module| wanted(&module.path) && folders.iter().any(|folder| inside(&module.path, folder)))
}

/// O projeto que as travas da leitura e da busca usam nos testes.
#[cfg(test)]
pub(crate) mod fixture {
    use std::path::{Path, PathBuf};

    /// Uma chave inventada para os testes; nenhuma chave de verdade entra
    /// aqui.
    pub(crate) const FAKE_KEY: &str = "chave-falsa-0123456789";

    /// O mapa do projeto: `src/big.rs` com o tipo `Alpha` e a função
    /// `alpha`; `src/small.rs` com `beta`; `src/tested.rs`, com a parte de
    /// produção curta e os testes da linha 101 em diante; e
    /// `src/long_tested.rs`, com a parte de produção longa e os testes da
    /// linha 351 em diante.
    pub(crate) const MAP: &str = r#"{"modules":[
        {"path":"src/big.rs","declarations":[
            {"kind":"struct","name":"Alpha","line":1,"end_line":150},
            {"kind":"field","name":"size","line":2,"end_line":2},
            {"kind":"function","name":"alpha","line":151,"end_line":400}]},
        {"path":"src/small.rs","declarations":[{"kind":"function","name":"beta","line":1,"end_line":1}]},
        {"path":"src/tested.rs","test_lines":[[101,401]],"declarations":[
            {"kind":"function","name":"gamma","line":1,"end_line":100},
            {"kind":"function","name":"gamma_test","line":110,"end_line":120}]},
        {"path":"src/long_tested.rs","test_lines":[[351,401]],"declarations":[
            {"kind":"function","name":"delta","line":1,"end_line":350}]}
    ]}"#;

    /// `count` linhas de texto comum.
    pub(crate) fn lines(count: usize) -> String {
        "// uma linha\n".repeat(count)
    }

    /// Um arquivo com `code` linhas de produção e `tests` linhas de testes
    /// logo depois da marca que os abre.
    fn with_tests(code: usize, tests: usize) -> String {
        format!("{}#[cfg(test)]\n{}", lines(code), lines(tests))
    }

    /// Grava os arquivos do [`MAP`] em `root`, com `docs/big.md`, de 400
    /// linhas, que o mapa não guarda.
    pub(crate) fn write_files(root: &Path) {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::create_dir_all(root.join("docs")).expect("docs");
        std::fs::write(root.join("src/big.rs"), lines(400)).expect("big");
        std::fs::write(root.join("src/small.rs"), "fn beta() {}\n").expect("small");
        std::fs::write(root.join("src/tested.rs"), with_tests(100, 300)).expect("tested");
        std::fs::write(root.join("src/long_tested.rs"), with_tests(350, 50)).expect("long tested");
        std::fs::write(root.join("docs/big.md"), lines(400)).expect("docs");
    }

    /// Um projeto com o `mustard.json` `config`, os arquivos de
    /// [`write_files`] e, com `mapped`, o [`MAP`]. A raiz vem resolvida,
    /// como a do despachante.
    pub(crate) fn project(config: &str, mapped: bool) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = std::fs::canonicalize(dir.path()).expect("resolved tempdir");
        // No Windows o caminho resolvido volta com o prefixo `\\?\`.
        let root = PathBuf::from(resolved.to_string_lossy().trim_start_matches(r"\\?\").to_string());
        std::fs::write(root.join("mustard.json"), config).expect("config");
        write_files(&root);
        if mapped {
            mustard_core::io::project_map::write_text(&root, MAP).expect("map");
        }
        (dir, root)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{CopyLines, linked_copy, project_path};

    /// Um repositório com um commit em `dir`.
    fn repo_in(dir: &Path) {
        std::fs::create_dir_all(dir.join("src")).expect("src");
        std::fs::write(dir.join("src/a.rs"), "fn a() {}\n").expect("file");
        for args in [&["init", "-q"][..], &["add", "-A"], &["commit", "-q", "-m", "semente"]] {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }

    /// Liga a `copy` como cópia de trabalho do repositório em `root`.
    fn link_copy(root: &Path, copy: &Path) {
        let out = std::process::Command::new("git")
            .args(["worktree", "add", "-q", "-b", "onda"])
            .arg(copy)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(out.status.success(), "git worktree: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// De dentro da cópia de trabalho ligada ao projeto, a raiz da cópia sai
    /// de qualquer pasta dela, e o caminho de um arquivo dela é do projeto,
    /// com a árvore da cópia. O próprio projeto, a cópia de outro repositório
    /// e uma pasta fora do git não são cópia do projeto.
    #[test]
    fn only_a_copy_linked_to_the_project_repository_is_its_working_copy() {
        let project = tempfile::tempdir().expect("project");
        let other = tempfile::tempdir().expect("other");
        let copies = tempfile::tempdir().expect("copies");
        repo_in(project.path());
        repo_in(other.path());
        let copy = copies.path().join("c");
        let foreign = copies.path().join("f");
        link_copy(project.path(), &copy);
        link_copy(other.path(), &foreign);

        assert_eq!(linked_copy(&copy.join("src"), project.path()), Some(copy.clone()));
        assert_eq!(linked_copy(&copy, project.path()), Some(copy.clone()));
        assert_eq!(linked_copy(project.path(), project.path()), None);
        assert_eq!(linked_copy(&foreign, project.path()), None);
        assert_eq!(linked_copy(copies.path(), project.path()), None);

        let root = project.path().to_string_lossy().to_string();
        let file = copy.join("src/a.rs").to_string_lossy().to_string();
        let found = project_path(&root, &root, &file).expect("a file of the copy");
        assert_eq!((found.tree, found.rel.as_str()), (copy, "src/a.rs"));
        let foreign_file = foreign.join("src/a.rs").to_string_lossy().to_string();
        assert_eq!(project_path(&root, &root, &foreign_file), None);
    }

    /// A declaração cuja primeira linha a cópia trocou vai da linha seguinte
    /// à última igual antes dela até a igual em que ela termina, e o tipo
    /// que a contém segue do começo ao fim dele na cópia: um tipo de 1 a 7,
    /// com a função de 3 a 5, ganha uma linha no topo e uma no corpo, e fica
    /// de 2 a 9, com a função de 4 a 7.
    #[test]
    fn a_declaration_with_a_changed_first_line_keeps_the_copy_lines() {
        let project = "struct Pedido {\n\n    fn gravar(&self) {\n        banco();\n    }\n\n}\n";
        let copy = concat!(
            "// novo\nstruct Pedido {\n\n    fn gravar(&self, agora: bool) {\n",
            "        banco();\n        avisar();\n    }\n\n}\n",
        );
        let lines = CopyLines::between(project, copy);
        assert_eq!(lines.range(1, 7), Some((2, 9)));
        assert_eq!(lines.range(3, 5), Some((4, 7)));
    }
}
