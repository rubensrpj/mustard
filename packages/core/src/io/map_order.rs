//! `map_order` — a ordem única dos arquivos da busca do mapa.
//!
//! A busca tinha ordens que discordavam. A lista das declarações que vai ao
//! filtro ([`map_search::sources_near`]) junta quatro listas por rodízio e vê o
//! pedido inteiro, a frase e as palavras, nos campos das declarações. A
//! resposta do banco, a que sai sem filtro, ordenava os arquivos só pela nota
//! deles ([`map_search::ranked_files_near`]). Cada uma acerta arquivos que a
//! outra perde: com os nomes, a lista traz entre os primeiros o certo que a
//! resposta do banco erra; só com a frase, a lista perde para o banco em
//! parte das buscas e ganha em outra.
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
//! pergunta, e numa frase longa esse é o primeiro do rodízio (o arquivo certo
//! raramente é o primeiro da lista de base só com a frase). Elas entram só
//! pelo rodízio, com o peso pequeno dele.
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
//!
//! **O sentido** ([`crate::io::map_sense`]). No mapa com vetores, a ordem
//! das palavras lê também as formas vizinhas de cada palavra da pergunta
//! (o sinônimo, a palavra da outra língua), com metade do peso da forma
//! escrita. O resto da lista, depois da cabeça, soma por posição recíproca a
//! ordem dos vetores — o pedido contra todas as declarações — com metade do
//! peso da ordem das palavras: são os candidatos do filtro, e a
//! declaração que só o sentido acha entra por aí. Sem vetores no mapa, nada
//! disso existe e a ordem é a de sempre.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::domain::normalize::{Languages, Normalizer};
use crate::domain::search::Found;
use crate::domain::search::{folders_first, fuse, RECIPROCAL_FROM, TOP, VECTOR_WEIGHT};
use crate::domain::triage::Lead;
use crate::io::map_check;
use crate::io::map_lists::{decl_files, ranked_files_near, sources_near};
use crate::io::map_sense::{Meaning, Sense};
use crate::io::map_words::question;
use crate::platform::error::Result;

/// Quantos arquivos do banco entram na soma.
const BANK_DEPTH: usize = 100;

/// Quantos arquivos distintos da lista entram na soma e na ordem única.
const LIST_DEPTH: usize = 200;

/// Quantos arquivos o banco lê quando a busca é numa pasta: os de dentro dela
/// passam à frente, e o que está fora dos cem primeiros do projeto inteiro
/// ainda pode ser o primeiro da pasta.
const BANK_SCOPED_DEPTH: usize = 1000;

/// Quantos arquivos do topo do banco a ordem guarda para os sinais da
/// triagem: o primeiro e o segundo.
const BANK_TOP: usize = 2;

/// O que a ordem única devolve.
pub(super) struct Ordered {
    /// A lista das declarações candidatas: a cabeça na ordem dos arquivos, e
    /// depois o rodízio, sem repetir.
    pub list: Vec<i64>,
    /// As declarações da cabeça da lista, na ordem dos arquivos.
    pub head: Vec<i64>,
    /// Os arquivos na ordem única, cada um com a nota do banco (zero quando
    /// o banco não o achou) e sem o texto fixo. Com vetores no mapa, é a
    /// ordem das palavras somada à dos vetores.
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
/// com uma palavra da pergunta: só com a frase, o arquivo certo raramente é o
/// primeiro da lista de base, e o rodízio herda esse primeiro. As duas listas
/// que leem o arquivo — a dos arquivos e o banco — juntam as palavras da
/// pergunta no mesmo arquivo, e por isso pesam o dobro das que só leem a
/// declaração. A consulta agrupada pesa 0,5: sozinha ela põe o certo em
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
///
/// A lista de candidatos é a das palavras que a pergunta escreve, inteira, e
/// só depois dela vêm os achados da forma vizinha de uma palavra que o mapa
/// não escreve e da ordem dos vetores: nenhum dos dois põe um arquivo na
/// frente de um que as palavras escritas acharam. Os arquivos e a conferência
/// ficam os da ordem com o sentido.
pub(super) fn ordered(
    conn: &Connection,
    check: Check<'_>,
    query: &str,
    intent: &str,
    languages: &Languages,
) -> Result<Ordered> {
    let sense = Sense::read(conn, languages, (query, intent), true)?;
    let sensed = ordered_with(conn, check, &sense, query, intent, languages)?;
    let written = ordered_with(conn, check, &Sense::off(), query, intent, languages)?;
    let seen: HashSet<i64> = written.list.iter().copied().collect();
    let mut list = written.list.clone();
    list.extend(sensed.list.iter().filter(|id| !seen.contains(id)).copied());
    Ok(Ordered { list, head: written.head, ..sensed })
}

/// A ordem única de [`ordered`] com o `sense` dado: [`Sense::off`] lê só as
/// palavras escritas.
pub(super) fn ordered_with(
    conn: &Connection,
    check: Check<'_>,
    sense: &Sense,
    query: &str,
    intent: &str,
    languages: &Languages,
) -> Result<Ordered> {
    ordered_in(conn, check, sense, (query, intent), (languages, &[]))
}

/// A ordem única de [`ordered_with`] para uma busca pedida em `scope`, as
/// pastas dela: os arquivos de dentro passam à frente em cada lista e na ordem
/// final, e o primeiro e o segundo do banco, de onde saem os sinais do grau,
/// são os de dentro. Sem pasta (`scope` vazio), é a ordem de sempre.
pub(super) fn ordered_in(
    conn: &Connection,
    check: Check<'_>,
    sense: &Sense,
    (query, intent): (&str, &str),
    (languages, scope): (&Languages, &[String]),
) -> Result<Ordered> {
    let request = format!("{query} {intent}");
    let request = request.trim();
    let sources = sources_near(conn, query, intent, languages, &sense.near)?;
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
    let mut bank = ranked_files_near(conn, query, languages, if scope.is_empty() { BANK_DEPTH } else { BANK_SCOPED_DEPTH }, &sense.near)?;
    folders_first(&mut bank, scope, |found| &found.path);
    bank.truncate(BANK_DEPTH);
    let weight = weights();
    let mut path_of = conn.prepare("SELECT path FROM files WHERE rowid = ?1")?;
    let mut entries: Vec<Standing> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    let lists: [&[i64]; LISTS] = [&whole, &sources.everything, &sources.files, &sources.grouped];
    for (slot, list) in lists.iter().enumerate() {
        let mut seen: HashSet<i64> = HashSet::new();
        let mut files: Vec<(i64, String)> = Vec::new();
        for id in *list {
            let Some(&file) = file_of.get(id) else { continue };
            if !seen.insert(file) {
                continue;
            }
            let Some(path) = path_of.query_row([file], |row| row.get::<_, String>(0)).optional()? else { continue };
            files.push((file, path));
        }
        folders_first(&mut files, scope, |(_, path)| path);
        for (rank, (file, path)) in files.into_iter().take(LIST_DEPTH).enumerate() {
            let index = *at.entry(path.clone()).or_insert_with(|| {
                entries.push(Standing { path, file: Some(file), ranks: [None; LISTS], bank_rank: None, bank_score: 0 });
                entries.len() - 1
            });
            entries[index].ranks[slot] = Some(rank + 1);
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
    folders_first(&mut entries, scope, |entry| &entry.path);
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
    let (mut files, lead) = (checked.files, checked.lead);
    // A conferência reordena os primeiros: a pasta pedida volta à frente.
    folders_first(&mut files, scope, |found| &found.path);
    let meaning = if sense.meaning { Meaning::of(conn, request, &file_of, LIST_DEPTH)? } else { Meaning::default() };
    // Os vetores entram na lista de candidatos, depois da cabeça; a ordem dos
    // arquivos da resposta fica a das palavras.
    let order = if meaning.is_empty() { whole } else { fuse(&whole, &meaning.decls, VECTOR_WEIGHT) };
    let file_id: HashMap<&str, i64> = entries.iter().filter_map(|e| Some((e.path.as_str(), e.file?))).collect();
    let mut head: Vec<i64> = Vec::new();
    let mut by_path = conn.prepare("SELECT rowid FROM files WHERE path = ?1")?;
    for found in files.iter().take(TOP) {
        let file = match file_id.get(found.path.as_str()) {
            Some(&file) => Some(file),
            None => by_path.query_row([&found.path], |row| row.get::<_, i64>(0)).optional()?,
        };
        let decl = file.and_then(|file| first_decl.get(&file).or_else(|| meaning.best.get(&file)));
        if let Some(id) = decl {
            head.push(*id);
        }
    }
    let mut list = head.clone();
    list.extend(order.into_iter().filter(|id| !head.contains(id)));
    Ok(Ordered { list, head, files, bank: bank.into_iter().take(BANK_TOP).collect(), lead })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::map_search::{any_path, candidates_at};
    use crate::io::map_sense::Near;
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
        let rotation = files_of(&dir, &sources_near(db.conn(), phrase, phrase, &languages(), &Near::none()).unwrap().whole());
        assert_eq!(rotation[0], "src/dto/unidade.dto.ts", "the rotation opens with the short declaration: {rotation:?}");
        let answer = ordered(db.conn(), Check::Off, phrase, phrase, &languages()).unwrap();
        assert_eq!(paths(&answer.files)[0], "src/service/importacao.service.ts", "{:?}", paths(&answer.files));
        let sent = candidates_at(&model_path(dir.path()), phrase, phrase, &languages(), any_path).unwrap();
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
        let first = |question: &str| ranked_files_near(db.conn(), question, &languages(), 10, &Near::none()).unwrap().remove(0).path;
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
        let sent = candidates_at(&model_path(dir.path()), "reject repository", "reject repository", &languages(), any_path).unwrap();
        assert_eq!(sent.candidates[0].path, "src/contract/contract.repository.ts");
    }

    #[test]
    fn the_answer_keeps_the_file_only_the_list_finds_and_the_one_only_the_bank_finds() {
        let dir = saved(&clock_map());
        let db = open_existing(&model_path(dir.path())).unwrap();
        let by_bank = ranked_files_near(db.conn(), "timestamp", &languages(), 100, &Near::none()).unwrap();
        let by_list = files_of(&dir, &sources_near(db.conn(), "timestamp", "", &languages(), &Near::none()).unwrap().whole());
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
        let old = files_of(&dir, &sources_near(db.conn(), "timestamp", "", &languages(), &Near::none()).unwrap().whole());
        let one = ordered(db.conn(), Check::Off, "timestamp", "", &languages()).unwrap();
        assert_eq!(old[0], "src/relogio.rs", "the round robin alone opens with the signature file: {old:?}");
        let listed = files_of(&dir, &one.list);
        let with_declarations: Vec<&str> = paths(&one.files).into_iter().filter(|p| *p != "src/timestamp.rs").collect();
        assert_eq!(listed.iter().map(String::as_str).collect::<Vec<_>>(), with_declarations);
        assert_eq!(listed[0], "src/notas.rs");
        // A lista que o filtro recebe é essa: a mesma cabeça, sem perder declaração.
        let sent = candidates_at(&model_path(dir.path()), "timestamp", "", &languages(), any_path).unwrap();
        let conferred = ordered(db.conn(), Check::On(Some(dir.path())), "timestamp", "", &languages()).unwrap();
        assert_eq!(sent.ids(), conferred.list, "the filter gets the list the check ordered");
        assert_eq!(files_of(&dir, &sent.ids()).len(), listed.len());
        let mut sorted_new = one.list.clone();
        let mut sorted_old = sources_near(db.conn(), "timestamp", "", &languages(), &Near::none()).unwrap().whole();
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
        let old = files_of(&dir, &sources_near(db.conn(), "timestamp", "", &languages(), &Near::none()).unwrap().whole());
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
