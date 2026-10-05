//! `map_specs` — os itens das specs do projeto no mapa, no bloco
//! [`crate::io::project_map::SPECS`], para a busca achar o que foi combinado
//! sem abrir nenhum `spec.ndjson`.
//!
//! Entram, de cada spec em `.claude/spec/<nome>/spec.ndjson`, aberta ou
//! fechada, os itens vigentes de decisão, regra, pedido, tarefa, limite,
//! contrato, erro, caso de borda e fora do escopo: a versão mais nova de
//! cada um, sem os removidos. A conversa e os registros da obra (mensagem,
//! resposta, gancho, envio, entrega, passo) ficam fora. Cada item leva os
//! arquivos que cita (`applies_to`) e, na tarefa, os que ela muda, que
//! valem também para os itens que ela cobre. Cada commit de onda leva os
//! itens das ondas dele e os arquivos que mudou; a função ligada a um item
//! sai, na hora da resposta, da história por função já montada do arquivo,
//! e sem ela o commit liga só ao arquivo. Este módulo nunca monta história:
//! diz de quais arquivos ela falta ou venceu ([`untraced`]), e quem responde
//! a busca a monta antes, um arquivo por vez.
//!
//! O commit de onda casa com a história do arquivo pelo hash. O squash e o
//! rebase dão ao commit outro hash na base; aí ele casa pelos commits da base
//! do pull request da spec: cada spec guarda os números dos pull requests
//! que a rodada abriu para ela, e o commit da base tem o número no título ou
//! o que o provedor achou para ele.
//!
//! O bloco se põe em dia antes de cada resposta do mapa e depois de cada
//! gravação na spec ([`sync`] e [`sync_spec`]): o tamanho e a hora de cada
//! arquivo de eventos se comparam com os gravados no bloco, sem abrir o
//! arquivo. O que cresceu só com linhas que não mexem nos itens (mensagem,
//! passo, entrega) só avança o último número; o resto relê a spec inteira e
//! troca as linhas dela. A spec que sumiu da pasta sai do bloco.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection, OptionalExtension, Statement};
use serde_json::Value;

use crate::domain::normalize::Languages;
use crate::domain::project_map::{lineage_is_fresh, spec_sentence, FileLineage, MapRefusal, ProjectMap, SpecNote};
use crate::domain::spec_events::{parse_log, search_field, SpecEvent, SpecLog};
use crate::io::claude_paths::ClaudePaths;
use crate::io::map_search;
use crate::io::project_map::{
    exists_at, history_at, lineage_heads, model_path, open_existing, pull_comments_at, unreadable, CENSUS,
};
use crate::platform::error::Result;

/// Os tipos de item que entram no bloco, na ordem em que um commit de onda
/// escolhe o item que a história mostra: a decisão primeiro, a tarefa por
/// último.
const KINDS: [&str; 9] =
    ["decision", "rule", "contract", "limit", "error", "edge_case", "out_of_scope", "request", "task"];

/// Os tipos que, sem ser item, mudam o que o bloco guarda: a remoção e o
/// expurgo tiram ou mudam itens, e o commit liga itens a arquivos.
const TOUCHING: [&str; 3] = ["remove", "purge", "commit"];

/// O arquivo de eventos de cada spec.
const EVENTS_FILE: &str = "spec.ndjson";

/// O bloco das specs volta vazio na troca de versão: a montagem do mapa não
/// lê spec nenhuma, e a próxima resposta do mapa ([`sync`]) o enche de novo,
/// sem marca de spec nenhuma.
#[allow(clippy::unnecessary_wraps)] // a assinatura é a de todo bloco refeito
pub(crate) fn rebuild(_: &Connection, _: &Path) -> Result<()> {
    Ok(())
}

/// Põe em dia, no mapa do projeto em `root`, os itens de todas as specs, com
/// as palavras nas línguas `languages`. Sem mapa, nada se cria.
pub fn sync(root: &Path, languages: &Languages) -> std::result::Result<(), MapRefusal> {
    sync_with(root, languages, None)
}

/// Como [`sync`], só a spec `spec`: a que acabou de receber uma gravação.
pub fn sync_spec(root: &Path, spec: &str, languages: &Languages) -> std::result::Result<(), MapRefusal> {
    sync_with(root, languages, Some(spec.trim()))
}

/// O tamanho e a hora de um arquivo de eventos, e o último número lido dele.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    last_id: u64,
    size: u64,
    modified: i64,
}

/// O que muda no bloco para uma spec.
enum Change {
    /// O arquivo cresceu sem mexer nos itens: só a marca muda.
    Mark(String, Mark),
    /// A spec relida inteira: as linhas dela e a marca nova.
    Rows(String, SpecRows, Mark),
    /// A spec saiu da pasta.
    Gone(String),
}

fn sync_with(root: &Path, languages: &Languages, only: Option<&str>) -> std::result::Result<(), MapRefusal> {
    let model = model_path(root);
    if !exists_at(&model) {
        return Ok(());
    }
    let Ok(paths) = ClaudePaths::for_project(root) else { return Ok(()) };
    let on_disk = spec_files(&paths, only);
    let mut db = open_existing(&model)?;
    let marks = stored_marks(db.conn()).map_err(unreadable)?;
    let mut changes: Vec<Change> = Vec::new();
    for (name, path, size, modified) in &on_disk {
        let stamp = Mark { last_id: 0, size: *size, modified: *modified };
        if let Some(mark) = marks.get(name) {
            if (mark.size, mark.modified) == (stamp.size, stamp.modified) {
                continue;
            }
            if let Some(last_id) = quiet_tail(path, *mark, stamp.size) {
                changes.push(Change::Mark(name.clone(), Mark { last_id, ..stamp }));
                continue;
            }
        }
        let Ok(Some(log)) = crate::io::spec_events::read(path) else { continue };
        let last_id = log.max_id();
        changes.push(Change::Rows(name.clone(), rows_of(&log), Mark { last_id, ..stamp }));
    }
    if only.is_none() {
        let kept: HashSet<&str> = on_disk.iter().map(|(name, ..)| name.as_str()).collect();
        changes.extend(marks.keys().filter(|name| !kept.contains(name.as_str())).map(|name| Change::Gone(name.clone())));
    }
    if changes.is_empty() {
        return Ok(());
    }
    db.write(|tx| {
        let indexed = map_search::specs_indexed_in(tx, languages)?;
        for change in &changes {
            apply(tx, change, indexed.then_some(languages))?;
        }
        Ok(())
    })
    .map_err(unreadable)
}

/// O nome, o arquivo de eventos, o tamanho e a hora de cada spec da pasta
/// das specs — só a de `only`, quando ela vem —, em ordem de nome. A pasta
/// das descartadas e a pasta sem arquivo de eventos ficam de fora.
fn spec_files(paths: &ClaudePaths, only: Option<&str>) -> Vec<(String, PathBuf, u64, i64)> {
    let names: Vec<String> = match only {
        Some(name) => vec![name.to_string()],
        None => {
            let mut names: Vec<String> = crate::io::fs::read_dir(paths.spec_dir())
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| entry.is_dir && !entry.file_name.starts_with('.'))
                .map(|entry| entry.file_name)
                .collect();
            names.sort();
            names
        }
    };
    names
        .into_iter()
        .filter(|name| paths.for_spec(name).is_ok())
        .filter_map(|name| {
            let path = paths.spec_dir().join(&name).join(EVENTS_FILE);
            let meta = std::fs::metadata(&path).ok().filter(std::fs::Metadata::is_file)?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                .and_then(|at| i64::try_from(at.as_nanos()).ok())
                .unwrap_or_default();
            Some((name, path, meta.len(), modified))
        })
        .collect()
}

/// A marca gravada de cada spec.
fn stored_marks(conn: &Connection) -> Result<HashMap<String, Mark>> {
    let mut stmt = conn.prepare("SELECT spec, last_id, size, modified FROM spec_marks")?;
    let rows = stmt.query_map([], |row| {
        let number = |at: usize| row.get::<_, Option<i64>>(at).map(Option::unwrap_or_default);
        Ok((
            row.get::<_, String>(0)?,
            Mark {
                last_id: u64::try_from(number(1)?).unwrap_or_default(),
                size: u64::try_from(number(2)?).unwrap_or_default(),
                modified: number(3)?,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}

/// O último número das linhas que o arquivo em `path` ganhou depois da marca
/// `mark`, até `size` bytes, quando nenhuma delas mexe nos itens. `None`
/// quando o arquivo não só cresceu (encolheu, ou mudou antes do fim
/// gravado), quando uma linha nova não se entende ou não vem depois do
/// último número, ou quando alguma é item, remoção, expurgo, commit ou o
/// estado que traz o número de um pull request: aí a spec se relê inteira.
fn quiet_tail(path: &Path, mark: Mark, size: u64) -> Option<u64> {
    if mark.size == 0 || size <= mark.size {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(mark.size - 1)).ok()?;
    let mut bytes = Vec::new();
    file.take(size - mark.size + 1).read_to_end(&mut bytes).ok()?;
    let (&first, rest) = bytes.split_first()?;
    if first != b'\n' {
        return None;
    }
    let log = parse_log(std::str::from_utf8(rest).ok()?);
    let quiet = log.skipped.is_empty()
        && log.events.iter().all(|event| {
            let kind = event.event_type.as_str();
            event.id > mark.last_id && !KINDS.contains(&kind) && !TOUCHING.contains(&kind) && pull_number(event).is_none()
        });
    quiet.then(|| log.max_id().max(mark.last_id))
}

/// O número do pull request que o estado `event` traz, gravado pela rodada
/// ao abrir o pull request da spec.
fn pull_number(event: &SpecEvent) -> Option<u64> {
    (event.event_type == "state").then(|| event.fields.get("pr")?.get("number")?.as_u64()).flatten()
}

/// Um item vigente, como o bloco o guarda.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemRow {
    id: u64,
    code: String,
    kind: String,
    title: String,
    text: String,
    agent: String,
    search: String,
    files: Vec<String>,
}

/// Um commit de onda: os itens das ondas dele e os arquivos que mudou.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitRow {
    sha: String,
    items: Vec<u64>,
    files: Vec<String>,
}

/// As linhas de uma spec no bloco.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SpecRows {
    items: Vec<ItemRow>,
    commits: Vec<CommitRow>,
    pulls: Vec<u64>,
}

/// Os itens vigentes da spec lida, os commits das ondas dela e os números
/// dos pull requests que os estados dela trazem. Cada item leva os arquivos
/// que cita e, na tarefa, os que ela muda; a tarefa passa os dela aos itens
/// que cobre. Cada commit leva as tarefas das ondas dele, os itens que dizem
/// essas ondas e os que as tarefas cobrem. A tarefa e o item citados por uma
/// versão antiga valem pela versão vigente, que tem o mesmo código.
fn rows_of(log: &SpecLog) -> SpecRows {
    let codes = log.codes();
    let visible = log.visible();
    let items: Vec<&SpecEvent> = visible.iter().copied().filter(|event| KINDS.contains(&event.event_type.as_str())).collect();
    let current: HashMap<&str, u64> =
        items.iter().filter_map(|event| codes.get(&event.id).map(|code| (code.as_str(), event.id))).collect();
    let now = |id: u64| codes.get(&id).and_then(|code| current.get(code.as_str())).copied();
    let mut files: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    let mut of_wave: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for item in &items {
        let own = files.entry(item.id).or_default();
        push_new(own, applied_paths(item.fields.get("applies_to")));
        push_new(own, task_paths(item.fields.get("files")));
        for wave in item.int("wave").into_iter().chain(item.ints("waves")) {
            of_wave.entry(wave).or_default().push(item.id);
        }
    }
    let covers: BTreeMap<u64, Vec<u64>> = items
        .iter()
        .filter(|item| item.event_type == "task")
        .map(|task| (task.id, task.ints("covers").into_iter().filter_map(&now).collect()))
        .collect();
    for (task, covered) in &covers {
        let paths = files.get(task).cloned().unwrap_or_default();
        for id in covered {
            push_new(files.entry(*id).or_default(), paths.clone());
        }
    }
    let commits = visible
        .iter()
        .filter(|event| event.event_type == "commit")
        .filter_map(|event| {
            let sha = event.str_field("sha")?.trim().to_string();
            let mut linked: Vec<u64> = Vec::new();
            for wave in event.ints("waves") {
                for id in of_wave.get(&wave).into_iter().flatten() {
                    push_new(&mut linked, [*id]);
                    push_new(&mut linked, covers.get(id).cloned().unwrap_or_default());
                }
            }
            let mut changed = Vec::new();
            push_new(&mut changed, task_paths(event.fields.get("files")));
            (!sha.is_empty()).then_some(CommitRow { sha, items: linked, files: changed })
        })
        .collect();
    let items = items
        .iter()
        .map(|item| {
            let text = item.str_field("text").unwrap_or_default().to_string();
            let title = item.str_field("title").unwrap_or_default().to_string();
            let agent = item.str_field("agent").unwrap_or_default().to_string();
            let search = item
                .str_field("search")
                .map_or_else(|| search_field(Some(&text), &[title.as_str(), agent.as_str()]), str::to_string);
            ItemRow {
                id: item.id,
                code: codes.get(&item.id).cloned().unwrap_or_default(),
                kind: item.event_type.clone(),
                title,
                text,
                agent,
                search,
                files: files.remove(&item.id).unwrap_or_default(),
            }
        })
        .collect();
    let mut pulls = Vec::new();
    push_new(&mut pulls, visible.iter().copied().filter_map(pull_number));
    SpecRows { items, commits, pulls }
}

/// Acrescenta a `list` o que ela ainda não tem, na ordem.
fn push_new<T: PartialEq>(list: &mut Vec<T>, more: impl IntoIterator<Item = T>) {
    for value in more {
        if !list.contains(&value) {
            list.push(value);
        }
    }
}

/// Os arquivos que `applies_to` cita: a lista `files` do objeto, ou o texto
/// que é um caminho. O padrão (`**`, `src/*.rs`) não é arquivo e fica fora.
fn applied_paths(value: Option<&Value>) -> Vec<String> {
    let listed: Vec<&str> = match value {
        Some(Value::String(path)) if !path.trim().contains(char::is_whitespace) => vec![path.as_str()],
        Some(Value::Object(object)) => {
            object.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect()
        }
        _ => Vec::new(),
    };
    listed.into_iter().map(str::trim).filter(|path| !path.is_empty() && !path.contains('*')).map(str::to_string).collect()
}

/// Os caminhos de uma lista `files`, que vem como objetos com o caminho ou
/// já como caminhos em texto.
fn task_paths(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().or_else(|| item.get("path").and_then(Value::as_str)))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

/// Grava a mudança de uma spec na transação `tx`. Com `languages`, o índice
/// da busca dos itens já foi feito nessas línguas e acompanha as linhas;
/// sem, a primeira busca o refaz inteiro.
fn apply(tx: &Connection, change: &Change, languages: Option<&Languages>) -> Result<()> {
    let (spec, rows, mark) = match change {
        Change::Mark(spec, mark) => (spec, None, Some(mark)),
        Change::Rows(spec, rows, mark) => (spec, Some(rows), Some(mark)),
        Change::Gone(spec) => (spec, None, None),
    };
    tx.execute("DELETE FROM spec_marks WHERE spec = ?1", [spec])?;
    if let Some(mark) = mark {
        tx.execute(
            "INSERT INTO spec_marks(spec, last_id, size, modified) VALUES (?1, ?2, ?3, ?4)",
            params![spec, i64::try_from(mark.last_id).unwrap_or(i64::MAX), i64::try_from(mark.size).unwrap_or(i64::MAX), mark.modified],
        )?;
    }
    if matches!(change, Change::Mark(..)) {
        return Ok(());
    }
    let old: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT rowid FROM spec_items WHERE spec = ?1")?;
        stmt.query_map([spec], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    tx.execute("DELETE FROM spec_items WHERE spec = ?1", [spec])?;
    tx.execute("DELETE FROM spec_commits WHERE spec = ?1", [spec])?;
    tx.execute("DELETE FROM spec_pulls WHERE spec = ?1", [spec])?;
    if let Some(rows) = rows {
        let mut item = tx.prepare(
            "INSERT INTO spec_items(spec, id, code, kind, title, text, agent, search, files) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for row in &rows.items {
            item.execute(params![
                spec,
                i64::try_from(row.id).unwrap_or(i64::MAX),
                row.code,
                row.kind,
                row.title,
                row.text,
                row.agent,
                row.search,
                json_list(&row.files),
            ])?;
        }
        let mut commit = tx.prepare("INSERT INTO spec_commits(spec, sha, items, files) VALUES (?1, ?2, ?3, ?4)")?;
        for row in &rows.commits {
            commit.execute(params![spec, row.sha, serde_json::to_string(&row.items).unwrap_or_default(), json_list(&row.files)])?;
        }
        let mut pull = tx.prepare("INSERT INTO spec_pulls(spec, pr) VALUES (?1, ?2)")?;
        for number in &rows.pulls {
            pull.execute(params![spec, i64::try_from(*number).unwrap_or(i64::MAX)])?;
        }
    }
    if let Some(languages) = languages {
        map_search::unindex_specs(tx, &old)?;
        map_search::index_specs(tx, languages, Some(spec))?;
    }
    Ok(())
}

/// A lista em JSON; vazia, a coluna fica sem valor.
fn json_list(list: &[String]) -> Option<String> {
    (!list.is_empty()).then(|| serde_json::to_string(list).unwrap_or_default())
}

/// A nota escolhida até aqui e a ordem dela: o lugar do tipo em [`KINDS`] e
/// o número do item.
type Ranked = ((usize, i64), SpecNote);

/// O item de spec de cada commit cujo começo do hash está em `ids` — de
/// todos, sem `ids` —, pelo começo do hash. O commit de onda leva o dos
/// itens das ondas dele; o commit da base que não é de onda e veio pelo
/// pull request de uma spec, como o do squash, leva o de todos os commits de
/// onda dela. Nos dois, a decisão antes da regra, e a tarefa por último
/// ([`KINDS`]); no mesmo tipo, o de número menor.
pub(crate) fn notes_of(conn: &Connection, ids: Option<&[&str]>) -> Result<BTreeMap<String, SpecNote>> {
    let wanted: Option<BTreeSet<&str>> = ids.map(|ids| ids.iter().copied().collect());
    if wanted.as_ref().is_some_and(BTreeSet::is_empty) {
        return Ok(BTreeMap::new());
    }
    let asked = |id: &str| wanted.as_ref().is_none_or(|wanted| wanted.contains(id));
    let squashed: Vec<(String, String)> = {
        let mut stmt = conn.prepare(&format!("SELECT b.id, s.spec FROM ({BASE_PULL}) b JOIN spec_pulls s ON s.pr = b.pr"))?;
        let rows: Vec<(String, String)> =
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        rows.into_iter().filter(|(id, _)| asked(id)).collect()
    };
    let pulled: BTreeSet<&str> = squashed.iter().map(|(_, spec)| spec.as_str()).collect();
    let mut stmt = conn.prepare(
        "SELECT c.sha, i.spec, i.code, i.kind, i.text, i.id FROM spec_commits c, json_each(c.items) j \
         JOIN spec_items i ON i.spec = c.spec AND i.id = j.value ORDER BY c.rowid",
    )?;
    let mut rows = stmt.query([])?;
    let mut best: BTreeMap<String, Ranked> = BTreeMap::new();
    let mut of_spec: BTreeMap<String, Ranked> = BTreeMap::new();
    let beats = |kept: Option<&Ranked>, rank: (usize, i64)| kept.is_none_or(|(seen, _)| *seen > rank);
    while let Some(row) = rows.next()? {
        let sha: String = row.get(0)?;
        let short: String = sha.chars().take(SHORT_ID).collect();
        let spec: String = row.get(1)?;
        let kind: String = row.get(3)?;
        let rank = (KINDS.iter().position(|k| *k == kind).unwrap_or(KINDS.len()), row.get::<_, i64>(5)?);
        let for_commit = asked(&short) && beats(best.get(&short), rank);
        let for_spec = pulled.contains(spec.as_str()) && beats(of_spec.get(&spec), rank);
        if !for_commit && !for_spec {
            continue;
        }
        let note = SpecNote { spec: spec.clone(), code: row.get(2)?, sentence: spec_sentence(&row.get::<_, String>(4)?) };
        if for_spec {
            of_spec.insert(spec, (rank, note.clone()));
        }
        if for_commit {
            best.insert(short, (rank, note));
        }
    }
    if !squashed.is_empty() {
        let waves: BTreeSet<String> = {
            let mut stmt = conn.prepare("SELECT sha FROM spec_commits")?;
            let shas = stmt.query_map([], |row| row.get::<_, String>(0))?;
            shas.map(|sha| sha.map(|sha| sha.chars().take(SHORT_ID).collect())).collect::<rusqlite::Result<_>>()?
        };
        for (id, spec) in squashed {
            let Some((rank, note)) = of_spec.get(&spec) else { continue };
            if !waves.contains(&id) && beats(best.get(&id), *rank) {
                best.insert(id, (*rank, note.clone()));
            }
        }
    }
    Ok(best.into_iter().map(|(short, (_, note))| (short, note)).collect())
}

/// Quantos caracteres do hash a história por função guarda.
const SHORT_ID: usize = 10;

/// Os commits da história por arquivo, cada um com o número do pull request
/// que o trouxe à base: o do título e, sem ele, o que o provedor achou.
const BASE_PULL: &str = "SELECT c.id AS id, COALESCE(NULLIF(c.pr, 0), p.pr) AS pr FROM lineage_commits c \
     LEFT JOIN pr_commits p ON p.id = c.id AND p.pr > 0";

/// Os lugares ligados ao item `id` da spec `spec`, até `limit`: primeiro as
/// funções que os commits das ondas dele mudaram, do commit mais novo ao
/// mais velho, pela história por função do arquivo, como `arquivo:nome`; o
/// arquivo do commit que ainda não tem essa história, ou em que ela não
/// achou função do commit, entra inteiro; depois os arquivos que o item
/// cita ou que as tarefas dele mudam. O commit de onda que a história do
/// arquivo não tem casa pelos commits da base do pull request da spec
/// ([`sought_ids`]).
pub(crate) fn links_of(conn: &Connection, spec: &str, id: i64, files: &[String], limit: usize) -> Result<Vec<String>> {
    let mut traced = conn.prepare("SELECT 1 FROM lineage_files WHERE path = ?1")?;
    let mut history = conn.prepare(&format!("{BASE_PULL} WHERE c.path = ?1"))?;
    let mut decls = conn.prepare("SELECT name, commits FROM lineage_decls WHERE path = ?1 ORDER BY rowid")?;
    let pulls: BTreeSet<i64> = {
        let mut stmt = conn.prepare("SELECT pr FROM spec_pulls WHERE spec = ?1")?;
        let numbers = stmt.query_map([spec], |row| row.get(0))?;
        numbers.collect::<rusqlite::Result<_>>()?
    };
    let mut out: Vec<String> = Vec::new();
    for (short, changed) in item_commits(conn, spec, id)? {
        if out.len() >= limit {
            break;
        }
        for path in changed {
            let mut found: Vec<String> = Vec::new();
            if traced.query_row([&path], |_| Ok(())).optional()?.is_some() {
                let sought = sought_ids(&mut history, &path, &short, &pulls)?;
                let mut names = decls.query([&path])?;
                while let Some(decl) = names.next()? {
                    let changes: Vec<Value> =
                        decl.get::<_, Option<String>>(1)?.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
                    if changes.iter().filter_map(|change| change.get("id").and_then(Value::as_str)).any(|id| sought.contains(id)) {
                        found.push(format!("{path}:{}", decl.get::<_, String>(0)?));
                    }
                }
            }
            if found.is_empty() {
                found.push(path);
            }
            push_new(&mut out, found);
        }
    }
    // O arquivo que já vem por uma função dele não se repete inteiro.
    let direct: Vec<String> =
        files.iter().filter(|path| !out.iter().any(|link| link.strip_prefix(path.as_str()).is_some_and(|rest| rest.starts_with(':')))).cloned().collect();
    push_new(&mut out, direct);
    out.truncate(limit);
    Ok(out)
}

/// Os commits das ondas do item `id` da spec `spec`, do mais novo ao mais
/// velho, cada um com o começo do hash que a história por função guarda e
/// os arquivos que mudou.
fn item_commits(conn: &Connection, spec: &str, id: i64) -> Result<Vec<(String, Vec<String>)>> {
    let mut commits = conn.prepare("SELECT sha, items, files FROM spec_commits WHERE spec = ?1 ORDER BY rowid DESC")?;
    let rows = commits.query_map([spec], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (sha, items, files) = row?;
        if !serde_json::from_str::<Vec<i64>>(&items).unwrap_or_default().contains(&id) {
            continue;
        }
        let changed: Vec<String> = files.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        out.push((sha.chars().take(SHORT_ID).collect(), changed));
    }
    Ok(out)
}

/// Os arquivos que a busca liga inteiros aos itens `items` — a spec e o
/// código de cada um — só por falta da história por função deles, ou porque
/// a que o mapa guarda venceu: os que os commits das ondas do item mudaram,
/// que o mapa do projeto em `root` tem e cuja história falta ou não vale
/// mais. A história guardada vale pela mesma regra da pergunta da história
/// ([`lineage_is_fresh`]), lida seguindo `moves` mudanças de arquivo: o
/// commit novo da base que mudou o arquivo, como o squash da própria spec,
/// a vence. Do commit mais novo ao mais velho, até `limit` arquivos por
/// item, os que [`links_of`] mostra. Sem a base de onde a história se lê,
/// nenhum: a passada não teria o que ler, e cada busca a tentaria de novo.
///
/// A história do git se lê do mapa só quando algum desses arquivos já tem a
/// história por função guardada.
pub fn untraced(root: &Path, items: &[(&str, &str)], limit: usize, moves: usize) -> std::result::Result<Vec<String>, MapRefusal> {
    let model = model_path(root);
    let db = open_existing(&model)?;
    let linked = linked_in(db.conn(), items, limit).map_err(unreadable)?;
    if linked.iter().all(|(_, stored)| stored.is_none()) {
        return Ok(linked.into_iter().map(|(path, _)| path).collect());
    }
    let mut map = ProjectMap {
        history: history_at(&model)?,
        census_mark: db.mark(CENSUS.name()).map_err(unreadable)?.unwrap_or_default(),
        ..ProjectMap::default()
    };
    for (path, _) in linked.iter().filter(|(_, stored)| stored.is_some()) {
        map.pulls.comments.extend(pull_comments_at(&model, path)?);
    }
    Ok(linked
        .into_iter()
        .filter(|(_, stored)| stored.as_ref().is_none_or(|lineage| !lineage_is_fresh(lineage, &map, moves)))
        .map(|(path, _)| path)
        .collect())
}

/// Os arquivos que os commits das ondas dos itens `items` mudaram e que o
/// mapa tem, cada um uma vez, com a história por função guardada dele — só
/// o que a validade dela confere, sem os commits —, quando há. Até `limit`
/// arquivos por item; nenhum sem a base de onde a história se lê.
fn linked_in(conn: &Connection, items: &[(&str, &str)], limit: usize) -> Result<Vec<(String, Option<FileLineage>)>> {
    let readable = conn
        .query_row("SELECT 1 FROM history_base WHERE base <> '' AND COALESCE(missing, '') = ''", [], |_| Ok(()))
        .optional()?
        .is_some();
    if !readable {
        return Ok(Vec::new());
    }
    let mut item_id = conn.prepare("SELECT id FROM spec_items WHERE spec = ?1 AND code = ?2")?;
    let mut mapped = conn.prepare("SELECT 1 FROM files WHERE path = ?1")?;
    let mut linked: Vec<String> = Vec::new();
    for (spec, code) in items {
        let Some(id) = item_id.query_row([spec, code], |row| row.get::<_, i64>(0)).optional()? else {
            continue;
        };
        let mut shown: Vec<String> = Vec::new();
        for path in item_commits(conn, spec, id)?.into_iter().flat_map(|(_, changed)| changed) {
            if shown.len() >= limit {
                break;
            }
            if shown.contains(&path) {
                continue;
            }
            if !linked.contains(&path) && mapped.query_row([&path], |_| Ok(())).optional()?.is_some() {
                linked.push(path.clone());
            }
            shown.push(path);
        }
    }
    let paths: Vec<&str> = linked.iter().map(String::as_str).collect();
    let heads = lineage_heads(conn, Some(&paths))?;
    Ok(linked
        .iter()
        .map(|path| (path.clone(), heads.iter().find(|head| head.path == *path).cloned()))
        .collect())
}

/// Os commits da história do arquivo `path`, lida por `history`, que dizem o
/// que o commit de onda de hash `short` mudou nele: o próprio, quando a
/// história o tem; senão, os da base cujo pull request é um dos da spec,
/// `pulls`, porque o squash e o rebase dão outro hash ao commit na base. O
/// squash junta as ondas num commit só, e o que outra onda mudou no mesmo
/// arquivo vem junto: é o que a base deixa saber. Sem número na spec, nada.
fn sought_ids(history: &mut Statement, path: &str, short: &str, pulls: &BTreeSet<i64>) -> Result<BTreeSet<String>> {
    let base: Vec<(String, Option<i64>)> =
        history.query_map([path], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    if base.iter().any(|(id, _)| id == short) {
        return Ok(BTreeSet::from([short.to_string()]));
    }
    Ok(base.into_iter().filter(|(_, pr)| pr.is_some_and(|pr| pulls.contains(&pr))).map(|(id, _)| id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::project_map::{DeclChange, DeclLineage, FileLineage, LineageCommit, PullOfCommit};
    use crate::io::project_map as store;
    use serde_json::json;
    use tempfile::{tempdir, TempDir};

    /// As línguas de um projeto com o texto em português e o código em inglês.
    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// Um projeto com um mapa pequeno e a pasta das specs.
    fn project() -> TempDir {
        let dir = tempdir().unwrap();
        store::write_text(dir.path(), r#"{"modules": [{"path": "src/pay.rs", "declarations": [{"name": "pay"}]}]}"#).unwrap();
        dir
    }

    /// Grava as linhas `lines` no fim do arquivo de eventos da spec `spec`.
    fn append(root: &Path, spec: &str, lines: &[Value]) {
        use std::io::Write;
        let dir = root.join(".claude/spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(EVENTS_FILE)).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    /// A decisão de número `id`, com o título e a parte do usuário.
    fn decision(id: u64, title: &str, text: &str) -> Value {
        json!({"id": id, "type": "decision", "title": title, "text": text, "keys": ["k"], "why": "w", "origin": 1})
    }

    /// Os códigos e títulos dos itens que a busca acha.
    fn found(root: &Path, query: &str) -> Vec<(String, String)> {
        sync(root, &languages()).unwrap();
        map_search::search_specs(root, query, &languages(), 5)
            .unwrap()
            .into_iter()
            .map(|item| (item.code, item.title))
            .collect()
    }

    /// A busca por uma palavra da parte do usuário de uma decisão acha a
    /// decisão, com o código, o título e a linha que casou; a mensagem da
    /// conversa, com a mesma palavra, não entra, e a spec fechada entra
    /// como a aberta.
    #[test]
    fn a_decision_of_an_open_or_closed_spec_is_found_and_a_message_is_not() {
        let dir = project();
        let root = dir.path();
        append(root, "aberta", &[
            json!({"id": 1, "type": "message", "author": "user", "text": "o arredondamento do boleto"}),
            decision(2, "Boleto arredonda para cima", "O primeiro parágrafo.\nO boleto arredonda o centavo para cima."),
        ]);
        append(root, "fechada", &[
            decision(1, "Pix confirma na hora", "O pix confirma o pagamento na hora."),
            json!({"id": 2, "type": "state", "phase": "closed"}),
        ]);

        assert_eq!(found(root, "boleto"), [("MSTD-DEC-0001".to_string(), "Boleto arredonda para cima".to_string())]);
        assert_eq!(found(root, "pix"), [("MSTD-DEC-0001".to_string(), "Pix confirma na hora".to_string())]);
        let items = map_search::search_specs(root, "centavo", &languages(), 5).unwrap();
        assert_eq!(items[0].spec, "aberta");
        assert_eq!(items[0].line.as_deref(), Some("O boleto arredonda o centavo para cima."), "the line that matched");
        let db = open_existing(&model_path(root)).unwrap();
        let kinds: Vec<String> =
            db.conn().prepare("SELECT kind FROM spec_items").unwrap().query_map([], |row| row.get(0)).unwrap().map(|k| k.unwrap()).collect();
        assert_eq!(kinds, ["decision", "decision"], "only the agreed items enter");
    }

    /// O item gravado depois aparece na busca seguinte; o removido e a
    /// versão substituída somem, e a versão nova fica com o código do item.
    #[test]
    fn a_written_item_shows_in_the_next_search_and_a_removed_one_disappears() {
        let dir = project();
        let root = dir.path();
        append(root, "obra", &[decision(1, "Boleto arredonda", "O boleto arredonda.")]);
        assert_eq!(found(root, "cartao").len(), 0);

        append(root, "obra", &[
            json!({"id": 2, "type": "message", "author": "user", "text": "e o cartão?"}),
            decision(3, "Cartao parcela", "O cartão parcela em três vezes."),
        ]);
        assert_eq!(found(root, "cartao"), [("MSTD-DEC-0002".to_string(), "Cartao parcela".to_string())]);

        append(root, "obra", &[
            json!({"id": 4, "type": "remove", "targets": [3], "reason": "saiu"}),
            json!({"id": 5, "type": "decision", "replaces": 1, "title": "Boleto trunca", "text": "O boleto trunca o centavo.",
                   "keys": ["k"], "why": "w", "origin": 1}),
        ]);
        assert!(found(root, "cartao").is_empty(), "the removed item is gone");
        assert_eq!(found(root, "boleto"), [("MSTD-DEC-0001".to_string(), "Boleto trunca".to_string())]);
    }

    /// Com o mapa apagado e gravado de novo, o bloco volta vazio e a
    /// resposta seguinte o enche; a spec que sai da pasta sai do bloco.
    #[test]
    fn a_deleted_and_rewritten_map_brings_the_items_back_and_a_gone_spec_leaves() {
        let dir = project();
        let root = dir.path();
        append(root, "obra", &[decision(1, "Boleto arredonda", "O boleto arredonda.")]);
        assert_eq!(found(root, "boleto").len(), 1);

        std::fs::remove_file(model_path(root)).unwrap();
        store::write_text(root, r#"{"modules": []}"#).unwrap();
        assert_eq!(found(root, "boleto").len(), 1, "the block is filled again");

        std::fs::remove_dir_all(root.join(".claude/spec/obra")).unwrap();
        assert!(found(root, "boleto").is_empty(), "the spec that left the folder leaves the block");
    }

    /// Crescer o arquivo só com mensagens não relê a spec: a linha de um
    /// item escrita por baixo, com o tamanho e a hora de antes, não aparece
    /// na busca; o tamanho novo, com uma mensagem, só avança a marca.
    #[test]
    fn a_file_grown_only_by_messages_is_not_read_again() {
        let dir = project();
        let root = dir.path();
        append(root, "obra", &[decision(1, "Boleto arredonda", "O boleto arredonda.")]);
        assert_eq!(found(root, "boleto").len(), 1);
        let path = root.join(".claude/spec/obra").join(EVENTS_FILE);
        let marks = || stored_marks(open_existing(&model_path(root)).unwrap().conn()).unwrap()["obra"];

        let before = std::fs::read_to_string(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, before.replace("Boleto arredonda", "Tarifa arredonda")).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
        assert_eq!(found(root, "boleto").len(), 1, "the same size and time is never read");
        assert!(found(root, "tarifa").is_empty());

        std::fs::write(&path, &before).unwrap();
        append(root, "obra", &[json!({"id": 2, "type": "message", "author": "user", "text": "tarifa?"})]);
        assert_eq!(found(root, "boleto").len(), 1);
        assert_eq!(marks().last_id, 2, "the mark moves on");
    }

    /// Um commit de onda liga as tarefas da onda e o que elas cobrem aos
    /// arquivos dele: pela história por função, quando o arquivo já a tem,
    /// à função que o commit mudou; sem ela, só ao arquivo. O item citado
    /// pela tarefa vale pela versão vigente, e a nota da história é a
    /// decisão.
    #[test]
    fn a_wave_commit_links_to_the_function_through_the_history_and_otherwise_to_the_file() {
        let dir = project();
        let root = dir.path();
        append(root, "obra", &[
            decision(1, "Boleto arredonda", "O boleto arredonda o centavo. Depois conta."),
            json!({"id": 2, "type": "task", "wave": 3, "title": "Arredonda", "text": "Arredonda o boleto.",
                   "files": [{"path": "src/pay.rs"}], "covers": [1], "depends_on": []}),
            json!({"id": 3, "type": "commit", "sha": "abcdef0123456789", "title": "t", "waves": [3],
                   "files": ["src/pay.rs", "src/other.rs"], "repo": "r"}),
        ]);
        store::save_lineage_at(&model_path(root), &FileLineage {
            path: "src/pay.rs".to_string(),
            base: "main".to_string(),
            commits: vec![LineageCommit { id: "abcdef0123".to_string(), at: 1, title: "t".to_string(), pr: None, ..LineageCommit::default() }],
            declarations: vec![DeclLineage {
                name: "pay".to_string(),
                nth: 0,
                commits: vec![DeclChange { id: "abcdef0123".to_string(), form: false }],
                comments: Vec::new(),
            }],
            ..FileLineage::default()
        })
        .unwrap();

        sync(root, &languages()).unwrap();
        let items = map_search::search_specs(root, "centavo", &languages(), 5).unwrap();
        assert_eq!(items[0].code, "MSTD-DEC-0001");
        assert_eq!(items[0].links, ["src/pay.rs:pay", "src/other.rs"], "the function, then the file without a history");
        let db = open_existing(&model_path(root)).unwrap();
        let notes = notes_of(db.conn(), Some(&["abcdef0123"])).unwrap();
        assert_eq!(
            notes["abcdef0123"],
            SpecNote { spec: "obra".to_string(), code: "MSTD-DEC-0001".to_string(), sentence: "O boleto arredonda o centavo.".to_string() }
        );
    }

    /// A spec `obra` com a decisão, a tarefa da onda 3 que a cobre e muda
    /// `src/pay.rs`, e o commit da onda 3 nesse arquivo, seguidos de `more`.
    fn wave_three(root: &Path, more: &[Value]) {
        let mut lines = vec![
            decision(1, "Boleto arredonda", "O boleto arredonda o centavo. Depois conta."),
            json!({"id": 2, "type": "task", "wave": 3, "title": "Arredonda", "text": "Arredonda o boleto.",
                   "files": [{"path": "src/pay.rs"}], "covers": [1], "depends_on": []}),
            json!({"id": 3, "type": "commit", "sha": "abcdef0123456789", "title": "t", "waves": [3],
                   "files": ["src/pay.rs"], "repo": "r"}),
        ];
        lines.extend_from_slice(more);
        append(root, "obra", &lines);
    }

    /// O estado que a rodada grava ao abrir o pull request `number`.
    fn pull_opened(id: u64, number: u64) -> Value {
        json!({"id": id, "type": "state", "phase": "pr_open", "pr": {"number": number, "url": "u"}})
    }

    /// Grava a história da base de `src/pay.rs`: cada commit com o título,
    /// o número tirado dele e as funções que mudou.
    fn pay_history(root: &Path, commits: &[(&str, &str, Option<u32>, &[&str])]) {
        let names: BTreeSet<&str> = commits.iter().flat_map(|(.., changed)| changed.iter().copied()).collect();
        let lineage = FileLineage {
            path: "src/pay.rs".to_string(),
            base: "main".to_string(),
            commits: commits
                .iter()
                .map(|(id, title, pr, _)| LineageCommit { id: (*id).to_string(), at: 1, title: (*title).to_string(), pr: *pr, ..LineageCommit::default() })
                .collect(),
            declarations: names
                .iter()
                .map(|name| DeclLineage {
                    name: (*name).to_string(),
                    nth: 0,
                    commits: commits
                        .iter()
                        .filter(|(.., changed)| changed.contains(name))
                        .map(|(id, ..)| DeclChange { id: (*id).to_string(), form: false })
                        .collect(),
                    comments: Vec::new(),
                })
                .collect(),
            ..FileLineage::default()
        };
        store::save_lineage_at(&model_path(root), &lineage).unwrap();
    }

    /// Os lugares que a busca mostra da decisão da spec `obra`.
    fn decision_links(root: &Path) -> Vec<String> {
        sync(root, &languages()).unwrap();
        let items = map_search::search_specs(root, "centavo", &languages(), 5).unwrap();
        assert_eq!(items[0].code, "MSTD-DEC-0001");
        items[0].links.clone()
    }

    /// A nota da decisão da spec `obra`.
    fn decision_note() -> SpecNote {
        SpecNote { spec: "obra".to_string(), code: "MSTD-DEC-0001".to_string(), sentence: "O boleto arredonda o centavo.".to_string() }
    }

    /// O pull request entrou na base por squash: a base não tem o hash do
    /// commit da onda, e o commit dela diz no título o número do pull
    /// request que a spec abriu. A decisão liga à função que esse commit
    /// mudou, e não ao arquivo inteiro.
    #[test]
    fn a_wave_commit_squashed_into_the_base_links_to_the_function_by_the_pull_request_number() {
        let dir = project();
        let root = dir.path();
        wave_three(root, &[pull_opened(4, 7)]);
        pay_history(root, &[("9999999999", "Arredonda (#7)", Some(7), &["pay"])]);

        assert_eq!(decision_links(root), ["src/pay.rs:pay"], "the function of the squash commit");
    }

    /// O commit do squash sem número no título liga do mesmo jeito pelo
    /// número que o provedor achou para ele.
    #[test]
    fn a_squash_commit_numbered_by_the_provider_links_the_same_way() {
        let dir = project();
        let root = dir.path();
        wave_three(root, &[pull_opened(4, 7)]);
        pay_history(root, &[("9999999999", "Arredonda", None, &["pay"])]);
        store::save_pull_commits_at(&model_path(root), &[PullOfCommit { id: "9999999999".to_string(), pr: 7 }]).unwrap();

        assert_eq!(decision_links(root), ["src/pay.rs:pay"], "the number the provider found");
    }

    /// A linha da história do commit do squash traz a nota da spec cujo pull
    /// request ele trouxe, na pergunta pelos commits e na leitura do mapa
    /// inteiro.
    #[test]
    fn the_history_line_of_a_squash_commit_brings_the_spec_note() {
        let dir = project();
        let root = dir.path();
        wave_three(root, &[pull_opened(4, 7)]);
        pay_history(root, &[("9999999999", "Arredonda (#7)", Some(7), &["pay"])]);
        sync(root, &languages()).unwrap();

        let db = open_existing(&model_path(root)).unwrap();
        let notes = notes_of(db.conn(), Some(&["9999999999"])).unwrap();
        assert_eq!(notes.get("9999999999"), Some(&decision_note()), "the decision comes before the task");
        assert_eq!(store::read(root).unwrap().spec_notes.get("9999999999"), Some(&decision_note()));
    }

    /// Com o hash da onda na história do arquivo, a ligação segue por ele: o
    /// outro commit do mesmo pull request no arquivo não entra.
    #[test]
    fn a_wave_commit_in_the_file_history_links_by_its_hash_and_not_by_the_pull_request() {
        let dir = project();
        let root = dir.path();
        wave_three(root, &[pull_opened(4, 7)]);
        pay_history(root, &[("abcdef0123", "t", None, &["pay"]), ("8888888888", "Estorna (#7)", Some(7), &["refund"])]);

        assert_eq!(decision_links(root), ["src/pay.rs:pay"], "only the function of the wave commit");
    }

    /// A spec sem estado com o número de um pull request liga o commit que a
    /// base não tem ao arquivo inteiro, e a linha da história fica sem nota.
    #[test]
    fn a_spec_without_a_pull_request_number_links_to_the_file() {
        let dir = project();
        let root = dir.path();
        wave_three(root, &[]);
        pay_history(root, &[("9999999999", "Arredonda (#7)", Some(7), &["pay"])]);

        assert_eq!(decision_links(root), ["src/pay.rs"], "the file, as without the number");
        let db = open_existing(&model_path(root)).unwrap();
        assert!(notes_of(db.conn(), Some(&["9999999999"])).unwrap().is_empty());
    }

    /// O pull request aberto depois da última leitura da spec, sem item novo,
    /// faz a spec ser relida: o número entra no mapa, e a ligação o usa.
    #[test]
    fn a_pull_request_opened_after_the_last_read_reads_the_spec_again() {
        let dir = project();
        let root = dir.path();
        wave_three(root, &[]);
        pay_history(root, &[("9999999999", "Arredonda (#7)", Some(7), &["pay"])]);
        assert_eq!(decision_links(root), ["src/pay.rs"]);

        append(root, "obra", &[json!({"id": 4, "type": "message", "author": "user", "text": "abre"}), pull_opened(5, 7)]);
        assert_eq!(decision_links(root), ["src/pay.rs:pay"], "the state with the number is read");
    }
}
