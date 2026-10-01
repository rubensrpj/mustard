//! A busca numa pasta, ou num tipo de arquivo, só manda ao filtro e só
//! devolve o que está nela: a lista de candidatos do filtro e as peças da
//! resposta saem da pasta e dos tipos que o agente pediu, e a busca sem pasta
//! segue a do projeto inteiro.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mustard_core::domain::map_filter::{
    judged, FilterError, FilterRequest, FilterUsage, Filtered, MapFilter, Scored, CUT_SHARE, EXISTS_FROM, MAX_KEPT,
};
use mustard_core::domain::map_select::MAX_RETURNED;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::search::CANDIDATES;
use mustard_core::io::map_triage;
use mustard_core::io::project_map as store;
use serde_json::json;

use super::fixture::{repo_with, Judge};
use super::{reply, unrecorded, Dialect, Reply, Said, Scene, Search, RANKED_FILES};
use crate::shared::code_route::{project_path, ProjectPath};
use crate::shared::config_key::{NameFilter, Walk};
use crate::shared::search_door::{self as door, Ask, Assembled, Numbers, Outcome};
use mustard_core::io::map_search;
use mustard_core::platform::i18n::Locale;
use mustard_core::ProjectConfig;

/// O fonte de quatro linhas de `name`, com o comentário `comment` no corpo.
fn source(name: &str, comment: &str) -> String {
    format!("pub fn {name}() -> u32 {{\n    // {comment}\n    1\n}}\n")
}

/// A declaração de `name`, nas linhas 1 a 4, com o comentário `comment` no
/// corpo.
fn declaration(name: &str, comment: &str) -> serde_json::Value {
    json!({ "kind": "function", "name": name, "line": 1, "end_line": 4, "body_comment": comment })
}

/// O projeto das duas pastas, com a palavra `imposto` só em comentários:
/// `src/frete` com dois fontes `.rs` e um `.ts`, e `src/pedido` com dois `.rs`.
pub(crate) fn project() -> (tempfile::TempDir, PathBuf) {
    project_with(&[])
}

/// O projeto de [`project`] com mais os arquivos `extra`, cada um com o
/// caminho, o nome da função, a língua e o comentário.
fn project_with(extra: &[(&str, &str, &str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let comment = "imposto embutido";
    let mut files = vec![
        ("src/frete/calculo.rs", "calcular_frete", "rust", comment),
        ("src/frete/tabela.rs", "tabela_frete", "rust", comment),
        ("src/frete/calculo.ts", "calcular_frete_ts", "typescript", comment),
        ("src/pedido/total.rs", "total_pedido", "rust", comment),
        ("src/pedido/recibo.rs", "emitir_recibo", "rust", comment),
    ];
    files.extend_from_slice(extra);
    let modules: Vec<_> = files
        .iter()
        .map(|(path, name, language, comment)| {
            json!({ "path": path, "language": language, "loc": 4, "declarations": [declaration(name, comment)] })
        })
        .collect();
    let texts: Vec<(&str, String)> = files.iter().map(|(path, name, _, comment)| (*path, source(name, comment))).collect();
    let texts: Vec<(&str, &str)> = texts.iter().map(|(path, text)| (*path, text.as_str())).collect();
    repo_with("{}", &texts, json!({ "modules": modules }))
}

fn both() -> Languages {
    Languages::new(["pt-BR", "en-US"])
}

/// A busca por `imposto` que o gancho lê, nas pastas `folders` e com os
/// filtros de nome `filters`, respondida com o filtro `judge`.
fn searched(root: &Path, folders: &[&str], (filters, walk): (&[NameFilter], Walk), judge: &Judge) -> Reply {
    let patterns = ["imposto".to_string()];
    let folders: Vec<ProjectPath> = folders
        .iter()
        .map(|folder| project_path(&root.to_string_lossy(), &root.to_string_lossy(), folder).expect("a project folder"))
        .collect();
    let search = Search {
        patterns: &patterns,
        dialect: if walk == Walk::Grep { Dialect::Basic } else { Dialect::Rust },
        ignore_case: false,
        whole_word: false,
        folders: &folders,
        filters,
        walk,
        shows_lines: true,
    };
    let config = ProjectConfig::load(root);
    let languages = both();
    let assemble = judge.assemble();
    let scene = Scene {
        root,
        model: &store::model_path(root),
        memory: None,
        session: Some("teste"),
        lang: Locale::PtBr,
        languages: &languages,
        config: &config,
        assemble: &assemble,
        record: &unrecorded,
        described: "",
        said: Said::Given(""),
    };
    reply(&scene, &search)
}

/// Os caminhos dos candidatos que o filtro recebeu na última chamada.
fn sent(judge: &Judge) -> Vec<String> {
    judge.last().candidates.into_iter().map(|candidate| candidate.path).collect()
}

fn include(glob: &str) -> NameFilter {
    NameFilter { exclude: false, glob: glob.to_string() }
}

fn exclude(glob: &str) -> NameFilter {
    NameFilter { exclude: true, glob: glob.to_string() }
}

const RG: Walk = Walk::Rg { unignored: false };

/// A busca numa pasta manda ao filtro só as declarações de dentro dela; pedida
/// no projeto inteiro, a mesma busca manda também as de fora.
#[test]
fn a_search_in_a_folder_hands_the_filter_only_candidates_from_inside_it() {
    let (_dir, root) = project();
    let judge = Judge::sure_of(&[("calcular_frete", 0.9)]);

    let reply = searched(&root, &["src/frete"], (&[], RG), &judge);
    assert!(matches!(reply, Reply::Note(_)), "the premise: the map finds part and the filter is asked: {reply:?}");
    let inside = sent(&judge);
    assert_eq!(inside.len(), 3, "{inside:?}");
    assert!(inside.iter().all(|path| path.starts_with("src/frete/")), "{inside:?}");

    searched(&root, &["."], (&[], RG), &judge);
    let whole = sent(&judge);
    assert!(whole.iter().any(|path| path.starts_with("src/pedido/")), "the whole project sends the other folder too: {whole:?}");
    assert_eq!(whole.len(), 5, "{whole:?}");
}

/// O `--include`, o glob e o tipo da ferramenta deixam de fora do pedido ao
/// filtro os arquivos dos outros tipos, na leitura do `rg` e na do `grep`.
#[test]
fn a_file_type_filter_keeps_the_other_types_out_of_what_the_filter_is_asked() {
    let (_dir, root) = project();
    let judge = Judge::sure_of(&[("calcular_frete", 0.9)]);
    for (filters, walk) in [
        (vec![include("*.rs")], RG),
        (vec![include("*.rs")], Walk::Grep),
        (vec![exclude("*.ts")], RG),
    ] {
        searched(&root, &["."], (&filters, walk), &judge);
        let rust = sent(&judge);
        assert_eq!(rust.len(), 4, "{filters:?} {walk:?}: {rust:?}");
        assert!(rust.iter().all(|path| path.ends_with(".rs")), "{filters:?} {walk:?}: {rust:?}");
    }
    searched(&root, &["src/frete"], (&[include("*.ts")], RG), &judge);
    assert_eq!(sent(&judge), ["src/frete/calculo.ts"], "the folder and the type together");
}

/// Sem pasta nem filtro, a lista que vai ao filtro é a do projeto inteiro, a
/// mesma que o mapa dá sem nenhuma admissão.
#[test]
fn a_search_without_a_folder_hands_the_filter_the_list_of_the_whole_project() {
    let (_dir, root) = project();
    let judge = Judge::sure_of(&[("calcular_frete", 0.9)]);
    searched(&root, &["."], (&[], RG), &judge);
    let whole = map_search::candidates(&root, "imposto", "imposto", &both(), CANDIDATES, map_search::any_path).unwrap();
    let ids: Vec<i64> = whole.candidates.iter().map(|candidate| candidate.id).collect();
    assert_eq!(judge.last().candidates.iter().map(|candidate| candidate.id).collect::<Vec<_>>(), ids);
    assert!(!ids.is_empty());
}

/// Os números da busca com o teto de `candidates` candidatos.
fn numbers(candidates: usize) -> Numbers {
    Numbers {
        candidates,
        cut_share: (CUT_SHARE * 100.0).round() as usize,
        exists_from: (EXISTS_FROM * 100.0).round() as usize,
        max_kept: MAX_KEPT,
        max_returned: MAX_RETURNED,
    }
}

/// O filtro de mentira que dá a chance 0,9 aos candidatos do arquivo
/// `favourite` e 0,01 aos outros, e guarda o caminho de cada candidato que
/// recebeu.
struct ByPath {
    favourite: &'static str,
    received: Rc<RefCell<Vec<String>>>,
}

impl MapFilter for ByPath {
    fn filter(&self, request: &FilterRequest) -> Result<Filtered, FilterError> {
        *self.received.borrow_mut() = request.candidates.iter().map(|candidate| candidate.path.clone()).collect();
        let scores: Vec<Scored> = request
            .candidates
            .iter()
            .map(|candidate| Scored { id: candidate.id, score: if candidate.path == self.favourite { 0.9 } else { 0.01 } })
            .collect();
        let (verdict, kept) = judged(&scores, 0.99, request.cut);
        Ok(Filtered { verdict, kept, usage: FilterUsage::default() })
    }
}

/// A busca por `query` que a porta classifica nas pastas `rels` e com os
/// filtros de nome `filters`, com o teto de `limit` candidatos e o filtro que
/// escolhe o arquivo `favourite`: os caminhos dos candidatos que o filtro
/// recebeu e os das peças que voltaram.
fn classified(
    root: &Path,
    (query, limit): (&str, usize),
    (rels, filters): (&[String], &[NameFilter]),
    favourite: &'static str,
) -> (Vec<String>, Vec<String>) {
    let languages = both();
    let triaged = map_triage::triage_at(&store::model_path(root), (query, ""), &languages, RANKED_FILES).expect("triage");
    let numbers = numbers(limit);
    let ask = Ask {
        root,
        query,
        intent: "",
        described: "",
        said: "",
        lang: Locale::PtBr,
        languages: &languages,
        numbers: &numbers,
        triaged: &triaged,
        rels,
        filters,
        walk: RG,
    };
    let received = Rc::new(RefCell::new(Vec::new()));
    let filter = ByPath { favourite, received: Rc::clone(&received) };
    let assembled = Assembled { name: "jev", filter: Box::new(filter), warning: None };
    let result = door::classify(&ask, &assembled).expect("the map reads");
    let Outcome::Classified { pieces, .. } = result.outcome else { panic!("the filter answers") };
    let sent = received.borrow().clone();
    (sent, pieces.into_iter().map(|piece| piece.path).collect())
}

/// O corte da pasta vem antes do teto: com o teto de dois candidatos, a pasta
/// com três declarações manda duas das suas, e não as duas melhores do
/// projeto, que são de fora dela.
#[test]
fn the_limit_is_filled_from_the_folder_even_when_the_best_ones_are_outside_it() {
    let (_dir, root) = project_with(&[("src/pedido/imposto.rs", "calcular_imposto", "rust", "imposto")]);
    let (open, _) = classified(&root, ("imposto", 2), (&[], &[]), "src/pedido/imposto.rs");
    assert!(open.contains(&"src/pedido/imposto.rs".to_string()), "the premise: the best one is outside src/frete: {open:?}");

    let folders = ["src/frete".to_string()];
    let (inside, pieces) = classified(&root, ("imposto", 2), (&folders, &[]), "src/frete/calculo.rs");
    assert_eq!(inside.len(), 2, "{inside:?}");
    assert!(inside.iter().all(|path| path.starts_with("src/frete/")), "{inside:?}");
    assert!(pieces.iter().all(|path| path.starts_with("src/frete/")), "{pieces:?}");
}

/// O contrato e a implementação dele em outra pasta.
pub(crate) fn contract_project() -> (tempfile::TempDir, PathBuf) {
    let map = json!({ "modules": [
        { "path": "src/pay/port.rs", "language": "rust", "loc": 4, "declarations": [
            { "kind": "trait", "name": "PaymentPort", "line": 1, "end_line": 4, "signature": "pub trait PaymentPort",
              "members": ["src/pay/port.rs:2:charge"] },
            { "kind": "method", "name": "charge", "line": 2, "end_line": 2, "signature": "fn charge(&self, total: u32)",
              "owner": ["PaymentPort"], "implemented_by": ["src/bank/card.rs:3:charge"] }
        ] },
        { "path": "src/bank/card.rs", "language": "rust", "loc": 9, "declarations": [
            { "kind": "struct", "name": "CardGateway", "line": 1, "end_line": 1 },
            { "kind": "method", "name": "charge", "line": 3, "end_line": 9, "signature": "fn charge(&self, total: u32)",
              "owner": ["CardGateway"], "contract": ["PaymentPort"], "implements": ["src/pay/port.rs:2:charge"] }
        ] }
    ] });
    let port = "pub trait PaymentPort {\n    fn charge(&self, total: u32);\n    // fim\n}\n";
    let card = "struct CardGateway;\nimpl PaymentPort for CardGateway {\n    fn charge(&self, total: u32) {\n        // um\n        // dois\n        // tres\n        // quatro\n        // cinco\n    }\n}\n";
    repo_with("{}", &[("src/pay/port.rs", port), ("src/bank/card.rs", card)], map)
}

/// A resposta de uma busca numa pasta nunca traz peça de fora dela: o método
/// de contrato do corte puxa a implementação dele quando ela está na pasta, e
/// deixa de puxá-la quando ela mora em outra. Sem pasta, a implementação volta.
#[test]
fn a_contract_never_pulls_its_implementation_from_outside_the_folder() {
    let (_dir, root) = contract_project();
    let (_, pulled) = classified(&root, ("charge", 2), (&[], &[]), "src/pay/port.rs");
    assert!(pulled.contains(&"src/bank/card.rs".to_string()), "the premise: the whole project pulls the implementation: {pulled:?}");

    let folders = ["src/pay".to_string()];
    let (_, kept) = classified(&root, ("charge", 2), (&folders, &[]), "src/pay/port.rs");
    assert!(!kept.is_empty(), "the contract method itself comes back");
    assert!(kept.iter().all(|path| path.starts_with("src/pay/")), "{kept:?}");
}
