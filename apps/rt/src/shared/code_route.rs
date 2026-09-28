//! `code_route` — o caminho do código pelo mapa, para as travas que veem cada
//! leitura e cada busca.
//!
//! A leitura inteira de um arquivo grande de código volta com as partes dele
//! e o comando que traz só a parte certa ([`whole_read`]). A busca de um nome
//! de declaração em pastas de código volta com o comando de quem usa o nome e
//! o da busca por assunto ([`folder_search`]). O que o mapa não guarda passa:
//! documento, configuração, dados, pasta fora do projeto.
//!
//! Nunca falha: sem mapa, com o mapa ilegível ou o banco travado, a resposta
//! é nenhuma, e a ação segue.

use std::path::{Path, PathBuf};

use mustard_core::domain::project_map::{self, FileParts};
use mustard_core::io::project_map::{self as store, Need};
use mustard_core::io::workspace::{is_git_repo_root, linked_worktree_main};
use mustard_core::platform::i18n::Locale;

use crate::hooks::write::write_gate::say;
use crate::shared::paths::relative_to_cwd;

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
    let top = start.ancestors().find(|folder| is_git_repo_root(folder))?;
    let main = linked_worktree_main(top)?;
    let same = |a: &Path, b: &Path| match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same(&main, Path::new(root)) {
        return None;
    }
    let rel = relative_to_cwd(&top.to_string_lossy(), &abs_text)?;
    Some(ProjectPath { tree: top.to_path_buf(), rel: rel.trim_start_matches("./").to_string(), abs })
}

/// A recusa da leitura inteira do arquivo `file` (relativo à raiz), que
/// traria `lines` linhas: as partes dele e o comando do trecho, quando o mapa
/// de `root` o guarda e ele passa de [`WHOLE_READ_MAX_LINES`]. `None` no
/// arquivo pequeno, no que o mapa não guarda, sem mapa e em todo erro de
/// leitura do mapa.
pub(crate) fn whole_read(root: &Path, file: &str, lines: usize, lang: Locale) -> Option<String> {
    if lines <= WHOLE_READ_MAX_LINES {
        return None;
    }
    let map = store::read_for(root, Need::Parts(file)).ok()?;
    let found = project_map::parts(&map, file).ok()?;
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

/// `true` quando `pattern` pode ser um nome de declaração: letras, números e
/// sublinhado, sem começar por número. Texto com espaço ou expressão não é.
pub(crate) fn is_name(pattern: &str) -> bool {
    let mut chars = pattern.chars();
    chars.next().is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
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

/// A recusa da busca de `pattern` nas pastas `folders` (relativas à raiz;
/// vazia é a raiz), com os filtros de nome de arquivo `globs`: o comando de
/// quem usa o nome e o da busca por assunto. Só quando o padrão é um nome de
/// declaração que o mapa de `root` conhece e alguma das pastas guarda código
/// do mapa que os filtros deixam passar. `None` em todo o resto, sem mapa e
/// em todo erro de leitura do mapa.
pub(crate) fn folder_search(root: &Path, pattern: &str, folders: &[String], globs: &[String], lang: Locale) -> Option<String> {
    if !is_name(pattern) || folders.is_empty() {
        return None;
    }
    // Os filtros juntam o que aceitam; um sem extensão aceita tudo.
    let mut only: Vec<String> = Vec::new();
    for glob in globs {
        match glob_extensions(glob) {
            Some(extensions) => only.extend(extensions),
            None => {
                only.clear();
                break;
            }
        }
    }
    let paths = store::read_for(root, Need::Paths).ok()?;
    let inside = |path: &str, folder: &str| folder.is_empty() || path.starts_with(&format!("{}/", folder.trim_end_matches('/')));
    let wanted = |path: &str| {
        only.is_empty()
            || Path::new(path).extension().and_then(|ext| ext.to_str()).is_some_and(|ext| only.iter().any(|o| o == ext))
    };
    let holds_code =
        paths.modules.iter().any(|module| wanted(&module.path) && folders.iter().any(|folder| inside(&module.path, folder)));
    if !holds_code {
        return None;
    }
    let named = store::read_for(root, Need::Declarations { file: None, name: pattern }).ok()?;
    if named.modules.iter().all(|module| module.declarations.is_empty()) {
        return None;
    }
    Some(say("code_route.name_search", lang, &[("{name}", pattern)]))
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
