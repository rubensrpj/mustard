//! `map_order` — a ordem única dos arquivos da busca do mapa.
//!
//! A busca tinha duas ordens que discordavam. A lista das declarações que
//! vai ao filtro ([`map_search::whole_list`]) junta quatro listas por rodízio
//! e vê o pedido inteiro, a frase e as palavras, nos campos das declarações.
//! A resposta do banco, a que sai sem filtro, ordenava os arquivos só pela
//! nota deles ([`map_search::ranked_files`]). Na régua de 360 buscas cada uma
//! acerta arquivos que a outra perde: com os nomes, 50 dos erros da resposta
//! tinham o certo entre os 5 primeiros da lista; só com a frase, a lista
//! perde para o banco em 47 buscas e ganha em 27.
//!
//! Aqui as duas se somam numa ordem só, por posição recíproca: cada arquivo
//! vale `1/(60+posição)` na lista, contando os arquivos distintos dela, mais
//! `1/(60+posição)` no banco; ganha o de maior soma e, no empate, o que está
//! antes na lista. A resposta ao Claude e a cabeça da lista de candidatos
//! saem dessa ordem: a primeira declaração de cada um dos [`TOP`] primeiros
//! arquivos vai para o começo da lista, na ordem deles, e o resto segue na
//! ordem do rodízio. Assim os cinco primeiros arquivos da lista são os cinco
//! da resposta, e o filtro não perde nenhuma declaração que a lista já
//! trazia além das poucas que a cabeça empurra para baixo.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use crate::domain::normalize::Languages;
use crate::domain::project_map::Found;
use crate::domain::search::TOP;
use crate::io::map_search::{decl_files, ranked_files, whole_list};
use crate::platform::error::Result;

/// A constante da posição recíproca: `1/(60+posição)`, a de sempre.
const RECIPROCAL_FROM: f64 = 60.0;

/// Quantos arquivos do banco entram na soma.
const BANK_DEPTH: usize = 100;

/// Quantos arquivos distintos da lista entram na soma e na ordem única.
const LIST_DEPTH: usize = 200;

/// Quantos arquivos do topo do banco a ordem guarda para os sinais da
/// triagem: o primeiro e o segundo.
const BANK_TOP: usize = 2;

/// O que a ordem única devolve.
pub(super) struct Ordered {
    /// A lista das declarações candidatas: a cabeça na ordem dos arquivos, e
    /// depois o rodízio, sem repetir.
    pub list: Vec<i64>,
    /// Os arquivos na ordem única, cada um com a nota do banco (zero quando
    /// o banco não o achou) e sem o texto fixo.
    pub files: Vec<Found>,
    /// Os primeiros arquivos do banco, na ordem da nota dele: de onde saem os
    /// sinais da triagem.
    pub bank: Vec<Found>,
}

/// Um arquivo na soma das duas ordens.
struct Standing {
    path: String,
    /// O número do arquivo, quando veio da lista.
    file: Option<i64>,
    list_rank: Option<usize>,
    bank_rank: Option<usize>,
    bank_score: u64,
}

impl Standing {
    fn sum(&self) -> f64 {
        let part = |rank: Option<usize>| rank.map_or(0.0, |at| 1.0 / (RECIPROCAL_FROM + at as f64));
        part(self.list_rank) + part(self.bank_rank)
    }
}

/// A ordem única da busca de `query` e `intent` no banco aberto.
pub(super) fn ordered(conn: &Connection, query: &str, intent: &str, languages: &Languages) -> Result<Ordered> {
    let whole = whole_list(conn, query, intent, languages)?;
    let file_of: HashMap<i64, i64> = decl_files(conn)?.into_iter().collect();
    let mut first_decl: HashMap<i64, i64> = HashMap::new();
    let mut listed: Vec<i64> = Vec::new();
    for id in &whole {
        let Some(&file) = file_of.get(id) else { continue };
        if let Entry::Vacant(slot) = first_decl.entry(file) {
            slot.insert(*id);
            listed.push(file);
        }
    }
    let bank = ranked_files(conn, query, languages, BANK_DEPTH)?;
    let mut path_of = conn.prepare("SELECT path FROM files WHERE rowid = ?1")?;
    let mut entries: Vec<Standing> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for (rank, file) in listed.iter().take(LIST_DEPTH).enumerate() {
        let Some(path) = path_of.query_row([file], |row| row.get::<_, String>(0)).optional()? else { continue };
        at.insert(path.clone(), entries.len());
        entries.push(Standing { path, file: Some(*file), list_rank: Some(rank + 1), bank_rank: None, bank_score: 0 });
    }
    for (rank, found) in bank.iter().enumerate() {
        match at.get(&found.path) {
            Some(&index) => {
                entries[index].bank_rank = Some(rank + 1);
                entries[index].bank_score = found.score;
            }
            None => {
                at.insert(found.path.clone(), entries.len());
                entries.push(Standing {
                    path: found.path.clone(),
                    file: None,
                    list_rank: None,
                    bank_rank: Some(rank + 1),
                    bank_score: found.score,
                });
            }
        }
    }
    // `sort_by` é estável: no empate, a ordem da lista e depois a do banco.
    entries.sort_by(|a, b| b.sum().total_cmp(&a.sum()));
    let mut head: Vec<i64> = Vec::new();
    let mut by_path = conn.prepare("SELECT rowid FROM files WHERE path = ?1")?;
    for entry in entries.iter().take(TOP) {
        let file = match entry.file {
            Some(file) => Some(file),
            None => by_path.query_row([&entry.path], |row| row.get::<_, i64>(0)).optional()?,
        };
        if let Some(id) = file.and_then(|file| first_decl.get(&file)) {
            head.push(*id);
        }
    }
    let mut list = head.clone();
    list.extend(whole.into_iter().filter(|id| !head.contains(id)));
    let files = entries.into_iter().map(|e| Found { path: e.path, score: e.bank_score, text: None }).collect();
    Ok(Ordered { list, files, bank: bank.into_iter().take(BANK_TOP).collect() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::map_search::candidates_at;
    use crate::io::project_map::{self as store, model_path, open_existing};
    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// Um projeto com o mapa `map` gravado como o scan grava, com o índice.
    fn saved(map: &Value) -> TempDir {
        let dir = tempdir().unwrap();
        store::save_at(&model_path(dir.path()), map, "scan 1", &languages()).unwrap();
        dir
    }

    /// Três arquivos para a palavra `timestamp`: `relogio.rs` só a traz na
    /// assinatura de uma função, que o banco dos arquivos não lê; `notas.rs`
    /// só a traz no comentário do arquivo, que a lista lê no nível dos
    /// arquivos; `timestamp.rs` a traz no nome e não declara nada, então só o
    /// banco o vê.
    fn clock_map() -> Value {
        json!({ "modules": [
            { "path": "src/relogio.rs", "declarations": [
                {"kind": "function", "name": "agora", "line": 1, "end_line": 3, "signature": "pub fn agora() -> Timestamp"}] },
            { "path": "src/notas.rs", "file_comment": "guarda o timestamp de cada nota", "declarations": [
                {"kind": "function", "name": "gravar", "line": 1, "end_line": 3, "signature": "pub fn gravar()"}] },
            { "path": "src/timestamp.rs", "declarations": [] },
        ]})
    }

    /// Os arquivos distintos de uma lista de declarações, na ordem em que
    /// aparecem.
    fn files_of(dir: &TempDir, list: &[i64]) -> Vec<String> {
        let db = open_existing(&model_path(dir.path())).unwrap();
        let mut out: Vec<String> = Vec::new();
        for id in list {
            let path: String = db
                .conn()
                .query_row("SELECT file FROM decls WHERE rowid = ?1", [id], |row| row.get(0))
                .unwrap();
            if !out.contains(&path) {
                out.push(path);
            }
        }
        out
    }

    fn paths(files: &[Found]) -> Vec<&str> {
        files.iter().map(|file| file.path.as_str()).collect()
    }

    #[test]
    fn the_answer_keeps_the_file_only_the_list_finds_and_the_one_only_the_bank_finds() {
        let dir = saved(&clock_map());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let by_bank = ranked_files(db.conn(), "timestamp", &languages(), 100).unwrap();
        let by_list = files_of(&dir, &whole_list(db.conn(), "timestamp", "", &languages()).unwrap());
        assert!(!paths(&by_bank).contains(&"src/relogio.rs"), "the bank does not read signatures: {by_bank:?}");
        assert!(!by_list.contains(&"src/timestamp.rs".to_string()), "a file with no declaration is not in the list: {by_list:?}");
        let answer = ordered(db.conn(), "timestamp", "", &languages()).unwrap().files;
        let top = paths(&answer);
        assert!(top[..TOP.min(top.len())].contains(&"src/relogio.rs"), "list-only file missing: {top:?}");
        assert!(top[..TOP.min(top.len())].contains(&"src/timestamp.rs"), "bank-only file missing: {top:?}");
        assert_eq!(top[0], "src/notas.rs", "the file both orders name goes first: {top:?}");
    }

    #[test]
    fn the_candidate_list_opens_with_the_files_of_the_answer_in_the_same_order() {
        let dir = saved(&clock_map());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let old = files_of(&dir, &whole_list(db.conn(), "timestamp", "", &languages()).unwrap());
        let one = ordered(db.conn(), "timestamp", "", &languages()).unwrap();
        assert_eq!(old[0], "src/relogio.rs", "the round robin alone opens with the signature file: {old:?}");
        let listed = files_of(&dir, &one.list);
        let with_declarations: Vec<&str> = paths(&one.files).into_iter().filter(|p| *p != "src/timestamp.rs").collect();
        assert_eq!(listed.iter().map(String::as_str).collect::<Vec<_>>(), with_declarations);
        assert_eq!(listed[0], "src/notas.rs");
        // A lista que o filtro recebe é essa: a mesma cabeça, sem perder declaração.
        let sent = candidates_at(&model_path(dir.path()), "timestamp", "", &languages(), 100).unwrap();
        assert_eq!(sent.whole, one.list);
        let mut sorted_new = one.list.clone();
        let mut sorted_old = whole_list(db.conn(), "timestamp", "", &languages()).unwrap();
        sorted_new.sort_unstable();
        sorted_old.sort_unstable();
        assert_eq!(sorted_new, sorted_old, "the head reorders the list and drops nothing");
    }

    /// Com mais de cinco arquivos, os cinco primeiros da lista de candidatos
    /// são os cinco da resposta, na mesma ordem: a lista sozinha abriria com
    /// os três arquivos que só têm a palavra na assinatura, e a soma põe na
    /// frente os quatro que as duas ordens nomeiam.
    #[test]
    fn the_five_first_files_of_the_list_are_the_five_of_the_answer() {
        let mut modules: Vec<Value> = (1..=3)
            .map(|n| {
                json!({ "path": format!("src/relogio{n}.rs"), "declarations": [
                    {"kind": "function", "name": format!("agora{n}"), "line": 1, "end_line": 3,
                     "signature": "pub fn agora() -> Timestamp"}] })
            })
            .collect();
        modules.extend((1..=4).map(|n| {
            json!({ "path": format!("src/notas{n}.rs"), "file_comment": "guarda o timestamp de cada nota", "declarations": [
                {"kind": "function", "name": format!("gravar{n}"), "line": 1, "end_line": 3, "signature": "pub fn gravar()"}] })
        }));
        let dir = saved(&json!({ "modules": modules }));
        let db = open_existing(&model_path(dir.path())).unwrap();
        let old = files_of(&dir, &whole_list(db.conn(), "timestamp", "", &languages()).unwrap());
        assert!(old[0].starts_with("src/relogio"), "the round robin alone opens with a signature file: {old:?}");
        let one = ordered(db.conn(), "timestamp", "", &languages()).unwrap();
        let listed = files_of(&dir, &one.list);
        let answered: Vec<&str> = paths(&one.files).into_iter().take(TOP).collect();
        assert_eq!(listed.iter().take(TOP).map(String::as_str).collect::<Vec<_>>(), answered);
        assert!(answered.iter().take(4).all(|path| path.starts_with("src/notas")), "{answered:?}");
    }
}
