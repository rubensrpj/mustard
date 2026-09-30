//! `map_order` — a ordem única dos arquivos da busca do mapa.
//!
//! A busca tinha ordens que discordavam. A lista das declarações que vai ao
//! filtro ([`map_search::sources`]) junta quatro listas por rodízio e vê o
//! pedido inteiro, a frase e as palavras, nos campos das declarações. A
//! resposta do banco, a que sai sem filtro, ordenava os arquivos só pela nota
//! deles ([`map_search::ranked_files`]). Na régua de 360 buscas cada uma
//! acerta arquivos que a outra perde: com os nomes, 50 dos erros da resposta
//! tinham o certo entre os 5 primeiros da lista; só com a frase, a lista
//! perde para o banco em 47 buscas e ganha em 27.
//!
//! Aqui as ordens se somam numa só, por posição recíproca: cada arquivo vale
//! `peso/(60+posição)` em cada uma destas ordens, contando os arquivos
//! distintos de cada lista, e ganha o de maior soma; no empate, o que está
//! antes na lista inteira e depois no banco.
//!
//! - a lista inteira do rodízio, com peso 0,5;
//! - a lista de todos os campos das declarações, com peso 0,5;
//! - a lista dos arquivos, com peso 1;
//! - a consulta agrupada ([`map_grouped`]), com peso 0,5: as cem primeiras
//!   declarações de uma consulta só ao índice viram arquivos distintos;
//! - o banco dos arquivos, com peso 1.
//!
//! A lista de base e a dos nomes não somam por conta própria: elas escolhem a
//! declaração de nome ou assinatura curtos que casa com uma palavra da
//! pergunta, e numa frase longa esse é o primeiro do rodízio (no Suzano, o
//! arquivo certo é o primeiro da lista de base em 2 das 120 buscas só com a
//! frase). Elas entram só pelo rodízio, com o peso pequeno dele.
//!
//! Os [`map_check::CHECKED`] primeiros dessa ordem passam pela conferência, que
//! os reordena pela cobertura das palavras raras da pergunta; o que segue
//! vale para a ordem já conferida.
//!
//! A resposta ao Claude e a cabeça da lista de candidatos saem dessa ordem: a
//! primeira declaração de cada um dos [`TOP`] primeiros arquivos vai para o
//! começo da lista, na ordem deles, e o resto segue na ordem do rodízio.
//! Assim os cinco primeiros arquivos da lista são os cinco da resposta, e o
//! filtro não perde nenhuma declaração que a lista já trazia além das poucas
//! que a cabeça empurra para baixo.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::project_map::Found;
use crate::domain::search::TOP;
use crate::domain::triage::Lead;
use crate::io::map_check;
use crate::io::map_search::{decl_files, ranked_files, sources};
use crate::io::map_words::question;
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
    /// O que a conferência dos primeiros candidatos diz da frente da lista
    /// ([`crate::io::map_check`]).
    pub lead: Lead,
}

/// Quantas ordens de declarações entram na soma, além do banco dos arquivos:
/// a lista inteira do rodízio, a de todos os campos das declarações, a dos
/// arquivos e a consulta agrupada, nesta ordem.
const LISTS: usize = 4;

/// O peso de cada ordem na soma das posições.
///
/// A lista inteira do rodízio abre com o melhor de cada uma das quatro listas,
/// e a de base e a dos nomes escolhem pela declaração de nome curto que casa
/// com uma palavra da pergunta: na régua de 360 buscas só com a frase, o
/// arquivo certo é o primeiro da lista de base em 2 das 120 do Suzano, e o
/// rodízio herda esse primeiro. As duas listas que leem o arquivo — a dos
/// arquivos e o banco — juntam as palavras da pergunta no mesmo arquivo, e
/// por isso pesam o dobro das que só leem a declaração. Os pesos saem da
/// medida nos assuntos pares da régua e da conferência nos ímpares, com os
/// três projetos de prova, só com a frase e com os nomes. A consulta agrupada
/// pesa 0,5, como no laboratório que a mediu: sozinha ela põe o certo em
/// primeiro mais vezes e perde nos cinco primeiros; somada com esse peso, ganha
/// nos dois.
const WEIGHTS: Weights = Weights { lists: [0.5, 0.5, 1.0, 0.5], bank: 1.0 };

#[derive(Clone, Copy)]
struct Weights {
    lists: [f64; LISTS],
    bank: f64,
}

/// Os pesos da tabela; nos testes, `MAP_ORDER_WEIGHTS` (`ordem=peso`, com as
/// ordens `whole`, `everything`, `files`, `grouped` e `bank`, separadas por espaço) põe
/// outros no lugar, para uma medida comparar com eles.
fn weights() -> Weights {
    #[cfg(test)]
    if let Ok(spec) = std::env::var("MAP_ORDER_WEIGHTS") {
        let mut out = Weights { lists: [0.0; LISTS], bank: 0.0 };
        for pair in spec.split_whitespace() {
            let (name, weight) = pair.split_once('=').unwrap();
            let weight: f64 = weight.parse().unwrap();
            match name {
                "whole" => out.lists[0] = weight,
                "everything" => out.lists[1] = weight,
                "files" => out.lists[2] = weight,
                "grouped" => out.lists[3] = weight,
                "bank" => out.bank = weight,
                other => panic!("{other}"),
            }
        }
        return out;
    }
    WEIGHTS
}

/// Um arquivo na soma das ordens.
struct Standing {
    path: String,
    /// O número do arquivo, quando veio de alguma das listas.
    file: Option<i64>,
    /// A posição entre os arquivos distintos de cada lista de declarações.
    ranks: [Option<usize>; LISTS],
    bank_rank: Option<usize>,
    bank_score: u64,
}

impl Standing {
    /// A soma das posições recíprocas, cada uma com o peso da sua ordem.
    fn sum(&self, weight: &Weights) -> f64 {
        let part = |rank: Option<usize>| rank.map_or(0.0, |at| 1.0 / (RECIPROCAL_FROM + at as f64));
        let lists: f64 = self.ranks.iter().zip(weight.lists).map(|(rank, weight)| weight * part(*rank)).sum();
        lists + weight.bank * part(self.bank_rank)
    }
}

/// Se os primeiros candidatos da ordem passam pela conferência.
#[derive(Clone, Copy)]
pub(super) enum Check<'a> {
    /// A ordem é a soma das listas, como saiu; os testes das listas leem assim.
    #[cfg(test)]
    Off,
    /// Os primeiros candidatos passam pela conferência ([`map_check`]), que
    /// os reordena pela cobertura das palavras raras da pergunta, com o corpo
    /// das declarações lido do disco a partir da raiz quando ela vem.
    On(Option<&'a Path>),
}

/// A ordem única da busca de `query` e `intent` no banco aberto. Com a
/// conferência ligada, a lista das declarações candidatas e os arquivos
/// seguem a ordem conferida.
pub(super) fn ordered(
    conn: &Connection,
    check: Check<'_>,
    query: &str,
    intent: &str,
    languages: &Languages,
) -> Result<Ordered> {
    let sources = sources(conn, query, intent, languages)?;
    let whole = sources.whole();
    let file_of: HashMap<i64, i64> = decl_files(conn)?.into_iter().collect();
    let mut first_decl: HashMap<i64, i64> = HashMap::new();
    for id in &whole {
        if let Some(&file) = file_of.get(id) {
            first_decl.entry(file).or_insert(*id);
        }
    }
    for id in &sources.grouped {
        if let Some(&file) = file_of.get(id) {
            first_decl.entry(file).or_insert(*id);
        }
    }
    let bank = ranked_files(conn, query, languages, BANK_DEPTH)?;
    let weight = weights();
    let mut path_of = conn.prepare("SELECT path FROM files WHERE rowid = ?1")?;
    let mut entries: Vec<Standing> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    let lists: [&[i64]; LISTS] = [&whole, &sources.everything, &sources.files, &sources.grouped];
    for (slot, list) in lists.iter().enumerate() {
        let mut seen: HashSet<i64> = HashSet::new();
        for id in list.iter() {
            let Some(&file) = file_of.get(id) else { continue };
            if !seen.insert(file) {
                continue;
            }
            if seen.len() > LIST_DEPTH {
                break;
            }
            let Some(path) = path_of.query_row([file], |row| row.get::<_, String>(0)).optional()? else { continue };
            let index = *at.entry(path.clone()).or_insert_with(|| {
                entries.push(Standing { path, file: Some(file), ranks: [None; LISTS], bank_rank: None, bank_score: 0 });
                entries.len() - 1
            });
            entries[index].ranks[slot] = Some(seen.len());
        }
    }
    for (rank, found) in bank.iter().enumerate() {
        let index = *at.entry(found.path.clone()).or_insert_with(|| {
            entries.push(Standing { path: found.path.clone(), file: None, ranks: [None; LISTS], bank_rank: None, bank_score: 0 });
            entries.len() - 1
        });
        entries[index].bank_rank = Some(rank + 1);
        entries[index].bank_score = found.score;
    }
    // `sort_by` é estável: no empate, a ordem da lista inteira e depois a do banco.
    entries.sort_by(|a, b| b.sum(&weight).total_cmp(&a.sum(&weight)));
    let found: Vec<Found> =
        entries.iter().map(|e| Found { path: e.path.clone(), score: e.bank_score, text: None }).collect();
    let checked = match check {
        #[cfg(test)]
        Check::Off => map_check::Verdict { files: found, lead: Lead::default() },
        Check::On(root) => {
            let mut normalizer = Normalizer::new(languages);
            let words = question(conn, &mut normalizer, query)?;
            map_check::check(conn, root, &mut normalizer, &words, found)?
        }
    };
    let position: HashMap<&str, usize> =
        checked.files.iter().enumerate().map(|(at, found)| (found.path.as_str(), at)).collect();
    entries.sort_by_key(|e| position.get(e.path.as_str()).copied().unwrap_or(usize::MAX));
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
    let (files, lead) = (checked.files, checked.lead);
    Ok(Ordered { list, files, bank: bank.into_iter().take(BANK_TOP).collect(), lead })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::map_search::{candidates_at, sources};
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

    /// Onde está o arquivo certo na ordem de cada uma das listas que a ordem
    /// única junta (`MAP_RANKS_RULER`, o arquivo JSON da régua; com
    /// `MAP_RANKS_PHRASE`, a pergunta é só a frase): por busca, a posição do
    /// primeiro arquivo certo entre os arquivos distintos de cada lista (0:
    /// fora), uma linha em `MAP_RANKS_OUT`.
    #[test]
    #[ignore = "mede com os mapas dos projetos de prova"]
    fn measure_where_each_list_puts_the_right_file() {
        let ruler = std::env::var("MAP_RANKS_RULER").unwrap();
        let out = std::env::var("MAP_RANKS_OUT").unwrap();
        let phrase = std::env::var("MAP_RANKS_PHRASE").is_ok();
        let ruler: Value = serde_json::from_str(&std::fs::read_to_string(ruler).unwrap()).unwrap();
        let languages = languages();
        let mut lines: Vec<String> = Vec::new();
        for search in ruler["searches"].as_array().unwrap() {
            let text = |key: &str| search[key].as_str().unwrap().to_string();
            let (key, intent) = (text("key"), text("intent"));
            let query = if phrase { intent.clone() } else { text("query") };
            let db = crate::io::map_search::indexed(std::path::Path::new(&text("model")), &languages, &crate::io::map_fill::READ_BY_CANDIDATES).unwrap();
            let conn = db.conn();
            let right: Vec<String> =
                search["targets"].as_array().unwrap().iter().map(|t| t[0].as_str().unwrap().to_string()).collect();
            let file_of: HashMap<i64, i64> = decl_files(conn).unwrap().into_iter().collect();
            let mut path_of = conn.prepare("SELECT path FROM files WHERE rowid = ?1").unwrap();
            let sources = crate::io::map_search::sources(conn, &query, &intent, &languages).unwrap();
            let mut where_is = |list: &[i64]| -> usize {
                let mut seen: Vec<i64> = Vec::new();
                for id in list {
                    let Some(&file) = file_of.get(id) else { continue };
                    if seen.contains(&file) {
                        continue;
                    }
                    seen.push(file);
                    let path: String = path_of.query_row([file], |row| row.get(0)).unwrap();
                    if right.contains(&path) {
                        return seen.len();
                    }
                }
                0
            };
            let whole = sources.whole();
            let bank = ranked_files(conn, &query, &languages, 100).unwrap();
            let bank_rank = bank.iter().position(|f| right.contains(&f.path)).map_or(0, |at| at + 1);
            lines.push(
                json!({
                    "key": key, "base": where_is(&sources.base), "names": where_is(&sources.names),
                    "everything": where_is(&sources.everything), "files": where_is(&sources.files),
                    "whole": where_is(&whole), "bank": bank_rank,
                })
                .to_string(),
            );
        }
        std::fs::write(out, lines.join("\n")).unwrap();
    }

    /// Uma pergunta em frase longa, num projeto onde a declaração de nome
    /// curto de outros arquivos casa com uma palavra só: `unidade`,
    /// `material` e `densidade` são a assinatura de uma propriedade cada, e o
    /// arquivo certo só traz as palavras nas mensagens que escreve, que a
    /// lista de base não lê.
    fn phrase_map() -> Value {
        let single = |path: &str, name: &str, signature: &str| {
            json!({ "path": path, "declarations": [
                {"kind": "property", "name": name, "line": 1, "end_line": 1, "signature": signature}] })
        };
        let mut modules = vec![
            single("src/dto/unidade.dto.ts", "campoA", "unidade"),
            single("src/dto/material.dto.ts", "campoB", "material"),
            single("src/dto/densidade.dto.ts", "campoC", "densidade"),
            json!({ "path": "src/service/importacao.service.ts",
                "declarations": [
                    {"kind": "function", "name": "executar", "line": 1, "end_line": 3, "signature": "executar()"},
                    {"kind": "function", "name": "salvar", "line": 4, "end_line": 6, "signature": "salvar()"}],
                "texts": [
                    {"line": 2, "kind": "log", "value": "importada a planilha de densidade por unidade e material genetico", "owner": "executar"},
                    {"line": 5, "kind": "log", "value": "gravado no banco", "owner": "salvar"}] }),
        ];
        modules.extend((1..=8).map(|n| {
            json!({ "path": format!("src/outro/arquivo{n}.ts"), "declarations": [
                {"kind": "function", "name": format!("faz{n}"), "line": 1, "end_line": 2, "signature": format!("faz{n}()")}] })
        }));
        json!({ "modules": modules })
    }

    /// A lista inteira do rodízio abre com o primeiro da lista de base, e ele é
    /// a propriedade de nome curto que casa com uma palavra da frase. A resposta
    /// e a lista de candidatos abrem com o arquivo que traz as palavras da frase
    /// juntas, que a soma põe na frente porque a lista dos arquivos e o banco
    /// pesam mais que a lista inteira.
    #[test]
    fn a_long_phrase_answers_with_the_file_that_holds_its_words_and_not_with_the_short_declaration_of_another() {
        let dir = saved(&phrase_map());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let phrase = "importa a planilha de densidade por unidade e material genetico";
        let rotation = files_of(&dir, &sources(db.conn(), phrase, phrase, &languages()).unwrap().whole());
        assert_eq!(rotation[0], "src/dto/unidade.dto.ts", "the rotation opens with the short declaration: {rotation:?}");
        let answer = ordered(db.conn(), Check::Off, phrase, phrase, &languages()).unwrap();
        assert_eq!(paths(&answer.files)[0], "src/service/importacao.service.ts", "{:?}", paths(&answer.files));
        let sent = candidates_at(&model_path(dir.path()), phrase, phrase, &languages(), 100).unwrap();
        assert_eq!(sent.candidates[0].path, "src/service/importacao.service.ts");
    }

    /// O caminho entra no índice quebrado em palavras, como o nome: a pergunta
    /// `contract end points` acha o arquivo `ContractEndPoints.cs` e a de
    /// `plantio plan repository` o `plantio-plan.repository.ts`, quando as
    /// palavras só estão no caminho.
    #[test]
    fn the_path_of_a_file_is_indexed_as_words() {
        let one = |path: &str, name: &str| {
            json!({ "path": path, "declarations": [
                {"kind": "function", "name": name, "line": 1, "end_line": 2, "signature": format!("{name}()")}] })
        };
        let mut modules = vec![
            one("src/api/ContractEndPoints.cs", "handle"),
            one("src/plantio/plantio-plan.repository.ts", "run"),
        ];
        modules.extend((1..=6).map(|n| one(&format!("src/outro/arquivo{n}.ts"), &format!("faz{n}"))));
        let dir = saved(&json!({ "modules": modules }));
        let db = open_existing(&model_path(dir.path())).unwrap();
        let first = |question: &str| ranked_files(db.conn(), question, &languages(), 10).unwrap().remove(0).path;
        assert_eq!(first("contract end points"), "src/api/ContractEndPoints.cs");
        assert_eq!(first("plantio plan repository"), "src/plantio/plantio-plan.repository.ts");
    }

    /// A palavra da camada vem do caminho: dois arquivos com a mesma
    /// declaração e o mesmo nome de assunto se distinguem pela palavra
    /// `repository` da pergunta, na resposta e no começo da lista de candidatos.
    #[test]
    fn the_layer_word_of_the_question_picks_the_file_by_its_path() {
        let layer = |path: &str| {
            json!({ "path": path, "declarations": [
                {"kind": "function", "name": "reject", "line": 1, "end_line": 4, "signature": "reject()"}] })
        };
        let mut modules = vec![
            layer("src/contract/contract.service.ts"),
            layer("src/contract/contract.repository.ts"),
            layer("src/contract/contract.controller.ts"),
        ];
        modules.extend((1..=6).map(|n| layer(&format!("src/outro/arquivo{n}.ts"))));
        let dir = saved(&json!({ "modules": modules }));
        let db = open_existing(&model_path(dir.path())).unwrap();
        let answer = ordered(db.conn(), Check::Off, "reject repository", "reject repository", &languages()).unwrap();
        assert_eq!(paths(&answer.files)[0], "src/contract/contract.repository.ts", "{:?}", paths(&answer.files));
        let sent = candidates_at(&model_path(dir.path()), "reject repository", "reject repository", &languages(), 100).unwrap();
        assert_eq!(sent.candidates[0].path, "src/contract/contract.repository.ts");
    }

    #[test]
    fn the_answer_keeps_the_file_only_the_list_finds_and_the_one_only_the_bank_finds() {
        let dir = saved(&clock_map());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let by_bank = ranked_files(db.conn(), "timestamp", &languages(), 100).unwrap();
        let by_list = files_of(&dir, &sources(db.conn(), "timestamp", "", &languages()).unwrap().whole());
        assert!(!paths(&by_bank).contains(&"src/relogio.rs"), "the bank does not read signatures: {by_bank:?}");
        assert!(!by_list.contains(&"src/timestamp.rs".to_string()), "a file with no declaration is not in the list: {by_list:?}");
        let answer = ordered(db.conn(), Check::Off, "timestamp", "", &languages()).unwrap().files;
        let top = paths(&answer);
        assert!(top[..TOP.min(top.len())].contains(&"src/relogio.rs"), "list-only file missing: {top:?}");
        assert!(top[..TOP.min(top.len())].contains(&"src/timestamp.rs"), "bank-only file missing: {top:?}");
        assert_eq!(top[0], "src/notas.rs", "the file both orders name goes first: {top:?}");
    }

    #[test]
    fn the_candidate_list_opens_with_the_files_of_the_answer_in_the_same_order() {
        let dir = saved(&clock_map());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let old = files_of(&dir, &sources(db.conn(), "timestamp", "", &languages()).unwrap().whole());
        let one = ordered(db.conn(), Check::Off, "timestamp", "", &languages()).unwrap();
        assert_eq!(old[0], "src/relogio.rs", "the round robin alone opens with the signature file: {old:?}");
        let listed = files_of(&dir, &one.list);
        let with_declarations: Vec<&str> = paths(&one.files).into_iter().filter(|p| *p != "src/timestamp.rs").collect();
        assert_eq!(listed.iter().map(String::as_str).collect::<Vec<_>>(), with_declarations);
        assert_eq!(listed[0], "src/notas.rs");
        // A lista que o filtro recebe é essa: a mesma cabeça, sem perder declaração.
        let sent = candidates_at(&model_path(dir.path()), "timestamp", "", &languages(), 100).unwrap();
        let conferred = ordered(db.conn(), Check::On(Some(dir.path())), "timestamp", "", &languages()).unwrap();
        assert_eq!(sent.whole, conferred.list, "the filter gets the list the check ordered");
        assert_eq!(files_of(&dir, &sent.whole).len(), listed.len());
        let mut sorted_new = one.list.clone();
        let mut sorted_old = sources(db.conn(), "timestamp", "", &languages()).unwrap().whole();
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
        let old = files_of(&dir, &sources(db.conn(), "timestamp", "", &languages()).unwrap().whole());
        assert!(old[0].starts_with("src/relogio"), "the round robin alone opens with a signature file: {old:?}");
        let one = ordered(db.conn(), Check::Off, "timestamp", "", &languages()).unwrap();
        let listed = files_of(&dir, &one.list);
        let answered: Vec<&str> = paths(&one.files).into_iter().take(TOP).collect();
        assert_eq!(listed.iter().take(TOP).map(String::as_str).collect::<Vec<_>>(), answered);
        assert!(answered.iter().take(4).all(|path| path.starts_with("src/notas")), "{answered:?}");
    }

    /// A consulta agrupada soma à ordem o arquivo que só o começo da palavra
    /// acha: `cobrar` não acha o cobrador pela raiz, e o arquivo dele entra na
    /// ordem única pela lista agrupada.
    #[test]
    fn the_grouped_list_brings_into_the_order_a_file_only_the_start_of_the_word_finds() {
        let modules: Vec<Value> = (0..10)
            .map(|n| json!({"path": format!("src/outro{n}.rs"), "declarations": [
                {"kind": "function", "name": format!("fazer{n}"), "line": 1, "end_line": 5, "doc": "algo bem diferente aqui"}]}))
            .chain([json!({"path": "src/mes.rs", "declarations": [
                {"kind": "function", "name": "fechar", "line": 1, "end_line": 5, "doc": "fecha o cobrador do mes"}]})])
            .collect();
        let dir = saved(&json!({ "modules": modules }));
        let db = open_existing(&model_path(dir.path())).unwrap();
        let got = ordered(db.conn(), Check::Off, "cobrar", "", &languages()).unwrap();
        assert_eq!(got.files.first().map(|file| file.path.as_str()), Some("src/mes.rs"), "{:?}", got.files);
    }
}
