//! `map_fill` — os blocos do mapa que o scan ainda tem de encher.
//!
//! A passada do scan grava cada bloco de [`BLOCKS`] com a marca dela. O
//! bloco cujo formato muda volta vazio e sem marca na abertura do banco
//! ([`crate::io::map_db`]), e só a passada seguinte o enche de novo. Até ela
//! rodar, o bloco sem marca ao lado de um bloco marcado diz que o mapa guarda
//! menos do que o projeto tem: a pergunta que o lê responderia vazio, e por
//! isso recusa ([`refuse`]).
//!
//! O mapa escrito à mão num teste não tem marca em bloco nenhum, e nele nada
//! falta encher.

use crate::domain::project_map::MapRefusal;
use crate::io::map_db::MapDb;
use crate::io::project_map::{unreadable, MapBlock, Need, BLOCKS, CENSUS, DECLS, FILES, GRAPH, HISTORY, ROUTES};
use crate::platform::error::Result;

/// Os blocos de [`BLOCKS`] que a pergunta `need` lê: as tabelas deles ou,
/// no caso do censo, a marca da passada que o gravou. Os blocos que o scan
/// não enche — a história de cada declaração, os pull requests e as specs —
/// não entram.
pub(crate) fn read_by(need: Need<'_>) -> &'static [&'static MapBlock] {
    match need {
        Need::Nothing | Need::Pull(_) => &[],
        Need::Terrain | Need::Lineage(_) => &[&CENSUS],
        Need::Paths => &[&FILES],
        Need::Importers(_) | Need::Tests(_) => &[&FILES, &GRAPH],
        Need::Parts(_) => &[&FILES, &DECLS],
        Need::Declarations { .. } => &[&FILES, &DECLS, &ROUTES],
        Need::Summary => &[&CENSUS, &FILES, &GRAPH, &HISTORY],
        Need::Examples { .. } => &[&FILES, &DECLS, &GRAPH, &HISTORY],
        Need::History { .. } => &[&CENSUS, &FILES, &DECLS, &ROUTES, &HISTORY],
    }
}

/// Os blocos que a lista de candidatos do filtro lê: os do índice de busca
/// ([`SEARCHED`](crate::io::project_map::SEARCHED)) e a história da base, de
/// onde saem os títulos dos commits mais novos do arquivo de cada candidato.
pub(crate) const READ_BY_CANDIDATES: [&MapBlock; 4] = [&FILES, &DECLS, &GRAPH, &HISTORY];

/// A recusa de quem lê os blocos `among` num mapa em que algum deles voltou
/// vazio numa troca de formato e o scan ainda não o encheu de novo
/// ([`unfilled`]): [`MapRefusal::MapUnfilled`], com o nome de cada um. Sem
/// ela, a pergunta responderia como se o projeto não tivesse o que o bloco
/// guarda.
pub(crate) fn refuse(db: &MapDb, among: &[&MapBlock]) -> std::result::Result<(), MapRefusal> {
    if among.is_empty() {
        return Ok(());
    }
    let blocks = unfilled(db, among.iter().copied()).map_err(unreadable)?;
    if blocks.is_empty() {
        return Ok(());
    }
    Err(MapRefusal::MapUnfilled { blocks: blocks.into_iter().map(str::to_string).collect() })
}

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
        emptied_by_an_older_scan(root, &DECLS);
    }

    /// O mesmo com o bloco `block`: o scan mais velho o declara noutra
    /// versão, e a abertura seguinte o refaz vazio e sem marca.
    fn emptied_by_an_older_scan(root: &Path, block: &MapBlock) {
        let older = Block { name: block.name(), version: 1, tables: &[], schema: "", kind: Kind::Rebuilt(|_, _| Ok(())) };
        MapDb::open(&model_path(root), root, &[older]).unwrap();
    }

    /// Um mapa com o que cada pergunta lê de cada bloco que o scan grava: as
    /// línguas, o subprojeto e a camada da pasta, no censo; os arquivos; as
    /// declarações e a medida de qualidade do arquivo; a rota; as
    /// importações, os testes e o arquivo mais importado, nas ligações; e a
    /// história da base.
    fn map_of_every_block() -> Value {
        json!({
            "state": {},
            "languages": [{"language": "rust", "files": 2, "loc": 15}],
            "projects": [{"name": "loja", "dir": "", "kind": "cargo", "code_files": 2}],
            "skeleton": [{"dir": "src", "role": "L0"}],
            "modules": [
                {"path": "src/pedido.rs", "loc": 10, "has_tests": true, "tests": ["tests/pedido.rs"],
                 "quality": {"size": 8, "imports": 1},
                 "declarations": [{"kind": "function", "name": "gravar_pedido", "line": 1, "end_line": 3,
                                   "signature": "pub fn gravar_pedido()", "doc": "Grava o pedido.",
                                   "used_by": ["src/uso.rs:2:usar"]}],
                 "routes": [{"method": "POST", "path": "pedidos", "written": "/pedidos", "handler": "gravar_pedido",
                             "line": 1, "framework": "axum"}]},
                {"path": "src/uso.rs", "loc": 5, "deps": ["src/pedido.rs"],
                 "declarations": [{"kind": "function", "name": "usar", "line": 2, "end_line": 4}]}
            ],
            "graph": {"nodes": 2, "edges": 1, "top_fan_in": [{"module": "src/pedido.rs", "degree": 1}]},
            "history": {"base": "dev", "paths": ["src/pedido.rs", "src/uso.rs"],
                        "commits": [{"id": "c1", "at": 10, "title": "Cria o pedido (#12)", "pr": 12, "added": [0, 1]}]}
        })
    }

    /// Cada pergunta ao mapa, pelos nomes de [`map_of_every_block`].
    const QUESTIONS: [Need<'static>; 14] = [
        Need::Nothing,
        Need::Summary,
        Need::Terrain,
        Need::Paths,
        Need::Importers("src/pedido.rs"),
        Need::Tests("src/pedido.rs"),
        Need::Declarations { file: None, name: "gravar_pedido" },
        Need::Declarations { file: Some("src/pedido.rs"), name: "gravar_pedido" },
        Need::Examples { words: false },
        Need::Examples { words: true },
        Need::History { file: None, name: "gravar_pedido" },
        Need::History { file: Some("src/pedido.rs"), name: "gravar_pedido" },
        Need::Lineage("src/pedido.rs"),
        Need::Pull(12),
    ];

    /// O mapa escrito à mão, sem marca em bloco nenhum, responde toda
    /// pergunta mesmo depois da troca de formato que esvazia as declarações;
    /// no mapa da passada, quem usa recusa até a passada seguinte gravar as
    /// declarações de novo, e a busca volta a responder junto com ela.
    #[test]
    fn a_hand_written_map_answers_every_question_and_a_map_written_again_answers_again() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project_map::write_text(root, &map_of_every_block().to_string()).unwrap();
        opened_by_an_older_scan(root);
        for need in QUESTIONS {
            assert!(project_map::read_for(root, need).is_ok(), "{need:?} on a hand-written map");
        }

        let users = Need::Declarations { file: None, name: "gravar_pedido" };
        project_map::save_at(&model_path(root), &map_of_every_block(), "scan 1", &languages()).unwrap();
        opened_by_an_older_scan(root);
        let refused = MapRefusal::MapUnfilled { blocks: vec!["decls".to_string()] };
        assert_eq!(project_map::read_for(root, users).unwrap_err(), refused);

        project_map::save_at(&model_path(root), &map_of_every_block(), "scan 1", &languages()).unwrap();
        let found = project_map::read_for(root, users).unwrap();
        assert_eq!(found.declared("gravar_pedido"), [("src/pedido.rs".to_string(), 1)]);
        let found = map_search::candidates(root, "pedido", "", &languages(), map_search::any_path).unwrap();
        assert!(found.candidates.iter().any(|c| c.name == "gravar_pedido"), "the search answers again too");
    }

    /// `true` quando este processo mantém aberto o arquivo do mapa em `model`,
    /// ou um dos que andam ao lado dele (o diário e o registro de gravações).
    /// O Windows não deixa apagar nem trocar arquivo aberto; o Linux deixa, e
    /// é pela lista de arquivos abertos dele que a alça esquecida aparece. Onde
    /// essa lista não existe, não há o que ver.
    fn holds_open(model: &Path) -> bool {
        let Ok(open) = std::fs::read_dir("/proc/self/fd") else { return false };
        let model = std::fs::canonicalize(model).unwrap_or_else(|_| model.to_path_buf());
        let model = model.to_string_lossy().into_owned();
        open.flatten()
            .filter_map(|fd| std::fs::read_link(fd.path()).ok())
            .any(|target| target.to_string_lossy().starts_with(&model))
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
        drop(db);

        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        assert!(unfilled_in(root).is_empty(), "the next pass fills it again");

        // Escrever o mapa à mão apaga o arquivo do banco: nenhuma conexão pode
        // seguir aberta, ou o Windows recusa com arquivo em uso.
        assert!(!holds_open(&model_path(root)), "the map is closed before it is written by hand");
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
        let map = map_with(json!({"head": now.head, "listing": now.digest(), "base": now.base.name, "base_tip": now.base.tip}));
        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        assert!(!project_map::is_behind(root, &|| None), "the map is the one of the commit and the content of now");

        opened_by_an_older_scan(root);
        assert!(project_map::is_behind(root, &|| None), "the declarations came back empty");
        assert!(project_map::is_behind(root, &|| None), "and stay behind until a pass fills them");

        project_map::save_at(&model_path(root), &map, "scan 1", &languages()).unwrap();
        assert!(!project_map::is_behind(root, &|| None));
    }

    /// A busca por "pedido" e a lista de candidatos do filtro no mapa em
    /// `root`, cada uma escrita para comparar e com os blocos que lê.
    fn searched_in(root: &Path) -> [(std::result::Result<String, MapRefusal>, &'static [&'static MapBlock]); 2] {
        [
            (map_search::search(root, "pedido", &languages(), 5).map(|found| format!("{found:?}")), &SEARCHED),
            (
                map_search::candidates(root, "pedido", "", &languages(), map_search::any_path).map(|found| format!("{found:?}")),
                &READ_BY_CANDIDATES,
            ),
        ]
    }

    /// Com cada bloco que o scan grava vazio numa troca de formato, cada
    /// pergunta que o lê — quem usa, o trecho, os exemplos, quem importa, o
    /// resumo e as outras — e a busca e a lista de candidatos do filtro que o
    /// leem recusam com o nome dele em vez de responder vazio, e só elas: o
    /// que não o lê responde o mesmo que no mapa inteiro. A busca lê os
    /// arquivos, as declarações e as ligações; a lista, também a história, de
    /// onde vêm os títulos dos commits de cada candidato. A recusa da busca é
    /// a mesma da pergunta que lê o mesmo bloco. Lado a lado, a leitura sem a
    /// conferência mostra que os blocos de cada pergunta são os que mudam a
    /// resposta dela. Com dois blocos vazios, a recusa dá os dois nomes na
    /// mesma ordem da pergunta que lê os dois.
    #[test]
    fn a_block_emptied_by_a_format_change_is_refused_by_every_question_and_search_that_reads_it() {
        for block in &BLOCKS {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let model = model_path(root);
            project_map::save_at(&model, &map_of_every_block(), "scan 1", &languages()).unwrap();
            let whole = open_existing(&model).unwrap();
            let before: Vec<String> =
                QUESTIONS.iter().map(|&need| format!("{:?}", project_map::part_of(&whole, need).unwrap())).collect();
            drop(whole);
            let searched_before = searched_in(root);
            assert!(searched_before.iter().all(|(answer, _)| answer.as_ref().is_ok_and(|text| text.contains("src/pedido.rs"))));

            emptied_by_an_older_scan(root, block);
            let name = block.name();
            let refused = MapRefusal::MapUnfilled { blocks: vec![name.to_string()] };
            let db = open_existing(&model).unwrap();
            let mut refusing = 0;
            for (&need, before) in QUESTIONS.iter().zip(&before) {
                let reads = read_by(need).iter().any(|read| read.name() == name);
                let unchecked = format!("{:?}", project_map::part_of(&db, need).unwrap());
                assert_eq!(&unchecked != before, reads, "{need:?} with the block {name} emptied");
                match project_map::read_for(root, need) {
                    Ok(answer) => {
                        assert!(!reads, "{need:?} answered with the block {name} emptied");
                        assert_eq!(&format!("{answer:?}"), before, "{need:?} with the block {name} emptied");
                    }
                    Err(refusal) => {
                        assert!(reads, "{need:?} refused with the block {name} emptied: {refusal:?}");
                        assert_eq!(refusal, refused);
                        refusing += 1;
                    }
                }
            }
            for ((answer, read), (before, _)) in searched_in(root).into_iter().zip(&searched_before) {
                if !read.iter().any(|read| read.name() == name) {
                    assert_eq!(&answer, before, "a search that does not read the block {name} emptied");
                    continue;
                }
                assert_eq!(answer.unwrap_err(), refused, "the search and the questions with the block {name} emptied");
                assert!(refusing > 0, "some question reads the block {name}");
            }
        }

        let dir = tempdir().unwrap();
        let root = dir.path();
        project_map::save_at(&model_path(root), &map_of_every_block(), "scan 1", &languages()).unwrap();
        emptied_by_an_older_scan(root, &GRAPH);
        emptied_by_an_older_scan(root, &FILES);
        let importers = project_map::read_for(root, Need::Importers("src/pedido.rs")).unwrap_err();
        assert_eq!(importers, MapRefusal::MapUnfilled { blocks: vec!["files".to_string(), "graph".to_string()] });
        for (answer, _) in searched_in(root) {
            assert_eq!(answer.unwrap_err(), importers);
        }
    }
}
