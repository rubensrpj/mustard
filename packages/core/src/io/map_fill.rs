//! `map_fill` — os blocos do mapa que o scan ainda tem de encher.
//!
//! A passada do scan grava cada bloco de [`BLOCKS`] com a marca dela. O
//! bloco cujo formato muda volta vazio e sem marca na abertura do banco
//! ([`crate::io::map_db`]), e só a passada seguinte o enche de novo. Até ela
//! rodar, o bloco sem marca ao lado de um bloco marcado diz que o mapa guarda
//! menos do que o projeto tem: a pergunta que o lê responderia vazio.
//!
//! O mapa escrito à mão num teste não tem marca em bloco nenhum, e nele nada
//! falta encher.

use crate::io::map_db::MapDb;
use crate::io::project_map::{MapBlock, BLOCKS};
use crate::platform::error::Result;

/// Os nomes dos blocos de `among` que o scan ainda tem de encher: os sem
/// marca, num mapa em que algum bloco de [`BLOCKS`] traz a de uma passada.
/// Vazio quando nenhum bloco tem marca — o mapa escrito à mão — e quando
/// todos têm.
pub(crate) fn unfilled<'b>(db: &MapDb, among: impl IntoIterator<Item = &'b MapBlock>) -> Result<Vec<&'static str>> {
    let mut filled = false;
    for block in &BLOCKS {
        filled |= db.mark(block.name())?.is_some_and(|mark| !mark.is_empty());
    }
    if !filled {
        return Ok(Vec::new());
    }
    let mut empty = Vec::new();
    for block in among {
        if db.mark(block.name())?.unwrap_or_default().is_empty() {
            empty.push(block.name());
        }
    }
    Ok(empty)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::*;
    use crate::domain::normalize::Languages;
    use crate::domain::project_map::MapRefusal;
    use crate::io::map_db::{Block, Kind};
    use crate::io::map_search;
    use crate::io::project_map::{self, model_path, open_existing, DECLS, SEARCHED};

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// Um mapa com uma função que casa com "pedido", no estado `state`.
    fn map_with(state: Value) -> Value {
        json!({
            "state": state,
            "modules": [{"path": "src/pedido.rs", "loc": 10, "declarations": [
                {"kind": "function", "name": "gravar_pedido", "line": 1, "end_line": 3,
                 "signature": "pub fn gravar_pedido()", "doc": "Grava o pedido."}]}]
        })
    }

    /// O scan de uma compilação mais velha abre o mapa: ele declara o bloco
    /// das declarações noutra versão, e o bloco perde a marca da passada. A
    /// abertura seguinte, na versão deste programa, o refaz vazio.
    fn opened_by_an_older_scan(root: &Path) {
        let older = Block { name: DECLS.name(), version: 1, tables: &[], schema: "", kind: Kind::Rebuilt(|_, _| Ok(())) };
        MapDb::open(&model_path(root), root, &[older]).unwrap();
    }

    fn unfilled_in(root: &Path) -> Vec<&'static str> {
        unfilled(&open_existing(&model_path(root)).unwrap(), &BLOCKS).unwrap()
    }

    /// O mapa que o scan gravou não tem bloco a encher; o bloco que voltou
    /// vazio numa troca de formato tem, até a passada seguinte gravá-lo de
    /// novo. O mapa escrito à mão, sem marca em bloco nenhum, não tem.
    #[test]
    fn a_block_emptied_by_a_format_change_is_unfilled_until_the_scan_writes_it_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let map = map_with(json!({}));
        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        assert!(unfilled_in(root).is_empty());

        opened_by_an_older_scan(root);
        assert_eq!(unfilled_in(root), ["decls"]);
        let db = open_existing(&model_path(root)).unwrap();
        assert_eq!(unfilled(&db, SEARCHED).unwrap(), ["decls"], "the search reads the declarations");
        let rows: i64 = db.conn().query_row("SELECT count(*) FROM decls", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 0, "the block came back empty");

        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        assert!(unfilled_in(root).is_empty(), "the next pass fills it again");

        project_map::write_text(root, &map.to_string()).unwrap();
        opened_by_an_older_scan(root);
        assert!(unfilled_in(root).is_empty(), "a hand-written map has no mark to lose");
    }

    /// Com o commit e o conteúdo iguais aos da passada, o mapa só fica para
    /// trás quando um bloco voltou vazio numa troca de formato; gravado de
    /// novo pela passada, volta a estar em dia.
    #[test]
    fn a_block_emptied_by_a_format_change_puts_the_map_behind() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "x").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "semente"]);
        let now = project_map::listing(root).unwrap();
        let map = map_with(json!({"head": now.head, "listing": now.digest()}));
        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        assert!(!project_map::is_behind(root), "the map is the one of the commit and the content of now");

        opened_by_an_older_scan(root);
        assert!(project_map::is_behind(root), "the declarations came back empty");
        assert!(project_map::is_behind(root), "and stay behind until a pass fills them");

        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        assert!(!project_map::is_behind(root));
    }

    /// A busca num mapa cujas declarações voltaram vazias recusa, com o nome
    /// do bloco, em vez de responder vazio; o mapa escrito de novo pela
    /// passada volta a responder.
    #[test]
    fn a_search_on_emptied_declarations_is_refused_instead_of_answering_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let map = map_with(json!({}));
        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        opened_by_an_older_scan(root);
        let refused = MapRefusal::MapUnfilled { blocks: vec!["decls".to_string()] };
        assert_eq!(map_search::search(root, "pedido", &languages(), 5).unwrap_err(), refused);
        assert_eq!(map_search::candidates(root, "pedido", "", &languages(), 100).unwrap_err(), refused);

        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        let found = map_search::candidates(root, "pedido", "", &languages(), 100).unwrap();
        assert_eq!(found.candidates.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["gravar_pedido"]);
    }
}
