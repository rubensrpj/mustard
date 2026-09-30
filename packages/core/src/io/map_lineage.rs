//! `map_lineage` — quais arquivos do mapa esperam a história de cada
//! declaração. O scan a lê do git em segundo plano, arquivo por arquivo, e
//! grava em lotes ([`crate::io::project_map::save_lineages_at`]); aqui se diz
//! quais arquivos ainda faltam.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::domain::ast::is_test_path;
use crate::domain::project_map::{lineage_fresh_in, FileLineage, MapRefusal};
use crate::io::project_map::{history_at, lineage_heads, lineages, open_existing, unreadable, CENSUS};
use crate::platform::error::Result;

/// O arquivo de trava da leitura da história do mapa em `model`: na pasta
/// temporária, com o nome tirado do caminho do mapa, para não deixar arquivo no
/// projeto. Quem o segura é o único que lê a história daquele mapa.
#[must_use]
pub fn reading_lock_path(model: &Path) -> PathBuf {
    let whole = std::path::absolute(model).unwrap_or_else(|_| model.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    whole.hash(&mut hasher);
    std::env::temp_dir().join(format!("mustard-history-{:016x}.lock", hasher.finish()))
}

/// Os arquivos do mapa em `model` cuja história por declaração falta ou
/// venceu, em ordem de caminho: os que o índice de busca lê — sem os escritos
/// por máquina e sem os de teste — e que declaram alguma coisa. A história
/// guardada vale pela regra da pergunta da história
/// ([`lineage_fresh_in`]): a base, a marca do scan que a leu, as mudanças de
/// arquivo seguidas (`moves`), os comentários de revisão presos ao arquivo e o
/// commit mais novo dele na história do mapa. O arquivo cuja marca não mudou
/// não volta. Sem a base de onde a história se lê, nenhum.
///
/// # Errors
///
/// As recusas de todo leitor do mapa.
pub fn wanted_at(model: &Path, moves: usize) -> std::result::Result<Vec<String>, MapRefusal> {
    let history = history_at(model)?;
    if history.base.is_empty() || history.missing.is_some() {
        return Ok(Vec::new());
    }
    let db = open_existing(model)?;
    let census_mark = db.mark(CENSUS.name()).map_err(unreadable)?.unwrap_or_default();
    let read = || -> Result<Vec<String>> {
        let conn = db.conn();
        let stored: HashMap<String, _> =
            lineage_heads(conn, None)?.into_iter().map(|head| (head.path.clone(), head)).collect();
        let mut comments: HashMap<String, usize> = HashMap::new();
        let mut counted = conn.prepare("SELECT path, COUNT(*) FROM pr_comments GROUP BY path")?;
        for row in counted.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))? {
            let (path, count) = row?;
            comments.insert(path, usize::try_from(count).unwrap_or(0));
        }
        let mut files = conn.prepare(
            "SELECT path FROM files WHERE COALESCE(file_class, '') = '' \
             AND EXISTS (SELECT 1 FROM decls WHERE decls.file = files.path) ORDER BY path",
        )?;
        let mut wanted = Vec::new();
        for path in files.query_map([], |row| row.get::<_, String>(0))? {
            let path = path?;
            if is_test_path(&path) {
                continue;
            }
            let fresh = stored.get(&path).is_some_and(|head| {
                lineage_fresh_in(head, &history, &census_mark, comments.get(&path).copied().unwrap_or(0), moves)
            });
            if !fresh {
                wanted.push(path);
            }
        }
        Ok(wanted)
    };
    read().map_err(unreadable)
}

/// Quantos caminhos vão numa pergunta só ao banco: o limite de variáveis de
/// uma consulta é de centenas nas versões mais antigas do SQLite.
const PATHS_PER_QUERY: usize = 500;

/// A história guardada dos arquivos `paths` do mapa em `model`, inteira — os
/// commits e as declarações —, na ordem em que se gravou. O arquivo sem
/// história guardada não vem. É de onde a leitura que só soma o que é novo
/// parte: a história que já vale de cada arquivo.
///
/// # Errors
///
/// As recusas de todo leitor do mapa.
pub fn stored_at(model: &Path, paths: &[&str]) -> std::result::Result<Vec<FileLineage>, MapRefusal> {
    let db = open_existing(model)?;
    let mut found = Vec::new();
    for chunk in paths.chunks(PATHS_PER_QUERY) {
        found.extend(lineages(db.conn(), Some(chunk)).map_err(unreadable)?);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::normalize::Languages;
    use crate::domain::project_map::{DeclChange, DeclLineage, LineageCommit};
    use crate::io::project_map::{model_path, save_at, save_lineage_at};
    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};

    /// A marca com que o teste grava o mapa: a da passada do scan que o fez.
    const MARK: &str = "scan 1";

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// Um mapa com a base `dev` e seis arquivos: três de código com
    /// declaração, um de teste, um escrito por máquina e um sem declaração.
    fn map() -> Value {
        let code = |path: &str| json!({"path": path, "loc": 5, "declarations": [{"kind": "function", "name": "f", "line": 1, "end_line": 2}]});
        json!({
            "modules": [
                code("src/b.rs"),
                code("src/a.rs"),
                code("src/c.rs"),
                code("tests/a_test.rs"),
                {"path": "src/gen.rs", "loc": 5, "file_class": "generated", "declarations": [
                    {"kind": "function", "name": "g", "line": 1, "end_line": 2}]},
                {"path": "src/empty.rs", "loc": 1, "declarations": []},
            ],
            "state": {"head": "abc", "base": "dev", "base_tip": "fed"},
            "history": {"base": "dev", "paths": ["src/a.rs", "src/b.rs", "src/c.rs"],
                        "commits": [{"id": "c1", "at": 10, "title": "cria", "changed": [0, 1, 2]}]}
        })
    }

    fn saved(map: &Value) -> TempDir {
        let dir = tempdir().unwrap();
        assert!(save_at(&model_path(dir.path()), map, MARK, &languages()).unwrap());
        dir
    }

    /// A história de `path` como o scan a grava: da base `dev`, com a marca do
    /// mapa, `moves` mudanças de arquivo seguidas e `comments` comentários.
    fn read(path: &str, moves: u32, comments: u32) -> FileLineage {
        FileLineage {
            path: path.into(),
            base: "dev".into(),
            last_commit: "c1".into(),
            mark: MARK.into(),
            moves,
            comments,
            ..FileLineage::default()
        }
    }

    #[test]
    fn the_files_the_search_reads_and_that_declare_something_wait_for_their_history_in_path_order() {
        let dir = saved(&map());
        let wanted = wanted_at(&model_path(dir.path()), 3).unwrap();
        assert_eq!(wanted, ["src/a.rs", "src/b.rs", "src/c.rs"], "no test file, no machine-written file, no file with no declaration");
    }

    #[test]
    fn a_file_whose_history_was_read_with_the_same_mark_does_not_wait_again() {
        let dir = saved(&map());
        let model = model_path(dir.path());
        save_lineage_at(&model, &read("src/b.rs", 3, 0)).unwrap();
        assert_eq!(wanted_at(&model, 3).unwrap(), ["src/a.rs", "src/c.rs"]);
    }

    #[test]
    fn a_history_read_with_another_mark_another_move_count_or_another_base_waits_again() {
        let dir = saved(&map());
        let model = model_path(dir.path());
        let all = ["src/a.rs", "src/b.rs", "src/c.rs"];
        for path in all {
            save_lineage_at(&model, &read(path, 3, 0)).unwrap();
        }
        assert!(wanted_at(&model, 3).unwrap().is_empty(), "every history is valid");
        assert_eq!(wanted_at(&model, 4).unwrap(), all, "the moves asked for changed");

        save_lineage_at(&model, &FileLineage { mark: "another scan".into(), ..read("src/a.rs", 3, 0) }).unwrap();
        save_lineage_at(&model, &FileLineage { base: "main".into(), ..read("src/b.rs", 3, 0) }).unwrap();
        assert_eq!(wanted_at(&model, 3).unwrap(), ["src/a.rs", "src/b.rs"], "the mark of the scan and the base are checked too");
    }

    #[test]
    fn a_map_with_no_base_to_read_from_waits_for_nothing() {
        let mut none = map();
        none["history"] = json!({"missing": "no_base", "paths": [], "commits": []});
        let dir = saved(&none);
        assert!(wanted_at(&model_path(dir.path()), 3).unwrap().is_empty());
    }

    #[test]
    fn the_stored_history_of_the_asked_files_comes_whole_with_the_tip_it_was_read_at() {
        let dir = saved(&map());
        let model = model_path(dir.path());
        let with_tip = |path: &str, tip: &str| FileLineage {
            tip: tip.into(),
            commits: vec![LineageCommit { id: "c1".into(), at: 10, title: "cria".into(), ..LineageCommit::default() }],
            declarations: vec![DeclLineage {
                name: "f".into(),
                commits: vec![DeclChange { id: "c1".into(), form: false }],
                ..DeclLineage::default()
            }],
            ..read(path, 3, 0)
        };
        save_lineage_at(&model, &with_tip("src/a.rs", "tip-a")).unwrap();
        save_lineage_at(&model, &with_tip("src/b.rs", "tip-b")).unwrap();

        let found = stored_at(&model, &["src/b.rs", "src/c.rs"]).unwrap();
        assert_eq!(found, [with_tip("src/b.rs", "tip-b")], "the asked file with a history comes whole, and the one without does not");

        // Mais caminhos que o banco aceita numa pergunta só: nenhum some.
        let many: Vec<String> = (0..1_200).map(|at| format!("src/none{at}.rs")).chain(["src/a.rs".to_string()]).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(stored_at(&model, &many).unwrap(), [with_tip("src/a.rs", "tip-a")]);
    }

    #[test]
    fn the_lock_of_a_reading_is_of_each_map_and_lives_outside_the_project() {
        let dir = tempdir().unwrap();
        let (one, other) = (dir.path().join("a/grain.db"), dir.path().join("b/grain.db"));
        assert_eq!(reading_lock_path(&one), reading_lock_path(&one));
        assert_ne!(reading_lock_path(&one), reading_lock_path(&other));
        assert!(!reading_lock_path(&one).starts_with(dir.path()), "{:?}", reading_lock_path(&one));
    }
}
