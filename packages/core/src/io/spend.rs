//! O gasto de cada dia no disco: o arquivo da máquina e a leitura das conversas.
//!
//! O arquivo do gasto ([`Ledger`]) mora na pasta [`machine_dir`], fora de
//! qualquer projeto, ao lado do cache do Mustard: ele junta todos os projetos
//! da máquina. Recontar é apagá-lo e deixar o comando refazê-lo.
//!
//! A conta lê todo `projects/*/**/*.jsonl` da pasta de configuração do Claude
//! Code — a conversa de cada sessão e a de cada agente dela —, e só soma o dia
//! de projeto que tem `mustard.json`. O nome do projeto é o da pasta, achada
//! pelo `cwd` gravado na conversa: a pasta do próprio projeto, uma pasta de
//! dentro dele ou a cópia de onda dele. A conversa de agente que trabalhou
//! numa cópia já apagada herda o projeto da conversa que o chamou.
//!
//! As buscas do Mustard que o gancho faz sozinho não estão nas conversas: a
//! chamada `word search` fica gravada na spec do projeto, com os tokens e o
//! custo do Jev, e entra na linha do dia dela. A busca por comando (`Bash` com
//! `mustard-rt run map search`) está nas conversas e, quando o filtro a julga,
//! também grava a chamada na spec: as duas contam a mesma busca, e ela entra
//! uma vez só, com os tokens e o custo da chamada.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};

use crate::domain::config::ProjectConfig;
use crate::domain::spec_events::calls_command;
use crate::domain::spend::{day_of, day_of_stamp, day_start, paired_searches, DayRow, Ledger, Range, Refusal};
use crate::io::fs::lock::{read_shared, LockedFile};
use crate::io::transcript::{MapSearch, SpendTally};
use crate::io::workspace::linked_worktree_main;
use crate::platform::error::Error;

/// A variável que muda a pasta do gasto, para quem guarda o cache em outro
/// lugar.
pub const DIR_ENV: &str = "MUSTARD_SPEND_DIR";

/// O arquivo do gasto dentro da pasta da máquina.
const LEDGER: &str = "ledger.json";

/// O comando gravado na spec pela busca que o gancho faz sozinho.
const WORD_SEARCH: &str = "word search";

/// A pasta do gasto na máquina: `MUSTARD_SPEND_DIR`, ou `spend` ao lado das
/// cópias de onda, em `~/.cache/mustard/`. `None` quando a pasta pessoal não
/// se acha.
#[must_use]
pub fn machine_dir() -> Option<PathBuf> {
    std::env::var_os(DIR_ENV)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::platform::harness::home_dir().map(|home| home.join(".cache").join("mustard").join("spend")))
}

/// O caminho do arquivo do gasto na pasta `dir`.
#[must_use]
pub fn ledger_path(dir: &Path) -> PathBuf {
    dir.join(LEDGER)
}

/// Hoje, no fuso do gasto.
#[must_use]
pub fn today() -> String {
    day_of(Utc::now())
}

/// O arquivo do gasto da pasta `dir`. Sem arquivo, o vazio: nada foi contado
/// ainda.
///
/// # Errors
///
/// [`Refusal::UnreadableLedger`] quando o arquivo existe e não se lê como o
/// arquivo do gasto; [`Refusal::Io`] quando o disco falha.
pub fn load(dir: &Path) -> Result<Ledger, Refusal> {
    let path = ledger_path(dir);
    match read_shared(&path) {
        Ok(text) => parse(&path, &text),
        Err(Error::NotFound(_)) => Ok(Ledger::default()),
        Err(e) => Err(Refusal::Io { detail: e.to_string() }),
    }
}

/// O texto do arquivo do gasto como [`Ledger`]; o arquivo vazio é o vazio.
fn parse(path: &Path, text: &str) -> Result<Ledger, Refusal> {
    if text.trim().is_empty() {
        return Ok(Ledger::default());
    }
    serde_json::from_str(text)
        .map_err(|e| Refusal::UnreadableLedger { path: path.display().to_string(), detail: e.to_string() })
}

/// Lê o arquivo do gasto, deixa `change` mexer nele e o grava de volta, tudo
/// com a trava do arquivo presa: dois comandos ao mesmo tempo nunca se
/// sobrepõem. O arquivo que não existe nasce vazio.
///
/// # Errors
///
/// As recusas de [`load`], e [`Refusal::Io`] quando a trava ou a gravação
/// falham.
pub fn update<R>(dir: &Path, change: impl FnOnce(&mut Ledger) -> R) -> Result<R, Refusal> {
    let path = ledger_path(dir);
    let io = |e: Error| Refusal::Io { detail: e.to_string() };
    let mut file = LockedFile::exclusive(&path).map_err(io)?;
    let text = file.read_to_string().map_err(io)?;
    let mut ledger = parse(&path, &text)?;
    let out = change(&mut ledger);
    let json = serde_json::to_string_pretty(&ledger).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    file.replace(json.as_bytes()).map_err(io)?;
    Ok(out)
}

/// Os projetos que as conversas citam, por nome da pasta, com a pasta de cada
/// um. O `cwd` de uma linha vira o nome de um projeto uma vez só.
#[derive(Default)]
struct Projects {
    by_dir: HashMap<String, Option<String>>,
    roots: BTreeMap<String, BTreeSet<PathBuf>>,
}

impl Projects {
    /// O nome do projeto da pasta `cwd`, ou `None` quando ela não é de um
    /// projeto com `mustard.json`.
    fn name_of(&mut self, cwd: &str) -> Option<String> {
        if let Some(known) = self.by_dir.get(cwd) {
            return known.clone();
        }
        let found = project_root(Path::new(cwd)).and_then(|root| {
            let name = root.file_name()?.to_string_lossy().into_owned();
            Some((name, root))
        });
        let name = found.map(|(name, root)| {
            self.roots.entry(name.clone()).or_default().insert(root);
            name
        });
        self.by_dir.insert(cwd.to_string(), name.clone());
        name
    }
}

/// A pasta do projeto de `dir`: a mais próxima, subindo, que tem
/// `mustard.json`; ou o checkout principal, quando `dir` é a cópia de onda
/// dele, que não carrega o arquivo.
fn project_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|folder| may_hold_project(folder) && ProjectConfig::exists(folder))
        .map(Path::to_path_buf)
        .or_else(|| linked_worktree_main(dir).filter(|main| may_hold_project(main) && ProjectConfig::exists(main)))
}

/// Se `folder` pode ser a pasta de um projeto. A pasta `.claude` de um
/// projeto, e as de dentro de duas delas, nunca são: uma conversa aberta ali
/// sobe até o projeto, e olhar a própria `.claude` atrás do `mustard.json`
/// derrubaria a conta inteira.
fn may_hold_project(folder: &Path) -> bool {
    let flat = folder.to_string_lossy().replace('\\', "/");
    folder.file_name().is_none_or(|name| name != ".claude") && !flat.contains(".claude/.claude")
}

/// Cada conversa do Claude Code em `config_dir`, com a conversa principal da
/// sessão dela quando é a de um agente: as principais de cada projeto do
/// Claude Code primeiro, os agentes depois.
fn transcript_files(config_dir: &Path) -> Vec<(PathBuf, Option<PathBuf>)> {
    let mut main = Vec::new();
    let mut agents = Vec::new();
    let mut folders: Vec<PathBuf> = std::fs::read_dir(config_dir.join("projects"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    for folder in folders {
        let mut entries: Vec<PathBuf> =
            std::fs::read_dir(&folder).into_iter().flatten().filter_map(Result::ok).map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                main.push((path, None));
            } else if path.is_dir() {
                let owner = path.with_extension("jsonl");
                let mut found: Vec<PathBuf> = std::fs::read_dir(path.join("subagents"))
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|file| file.is_file() && file.extension().is_some_and(|ext| ext == "jsonl"))
                    .collect();
                found.sort();
                agents.extend(found.into_iter().map(|file| (file, Some(owner.clone()))));
            }
        }
    }
    main.extend(agents);
    main
}

/// Quantas linhas do começo da conversa principal a procura do projeto de um
/// agente lê.
const OWNER_LINES: usize = 400;

/// O projeto da conversa principal `owner`: o da primeira linha com uma pasta
/// de projeto.
fn owner_project(owner: &Path, projects: &mut Projects) -> Option<String> {
    let file = std::fs::File::open(owner).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = Vec::new();
    for _ in 0..OWNER_LINES {
        buf.clear();
        if reader.read_until(b'\n', &mut buf).ok()? == 0 {
            return None;
        }
        let text = String::from_utf8_lossy(&buf);
        let Some(at) = text.find("\"cwd\":\"") else { continue };
        let rest = &text[at + "\"cwd\":\"".len()..];
        let Some(end) = rest.find('"') else { continue };
        if let Some(name) = projects.name_of(&rest[..end].replace("\\\\", "\\")) {
            return Some(name);
        }
    }
    None
}

/// Soma as linhas do arquivo de conversa `path` em `tally`.
fn tally_file(path: &Path, range: &Range, tally: &mut SpendTally, projects: &mut Projects, fallback: Option<&str>) {
    let Ok(file) = std::fs::File::open(path) else { return };
    let mut reader = std::io::BufReader::new(file);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buf);
        let mut project_of =
            |cwd: Option<&str>| cwd.and_then(|cwd| projects.name_of(cwd)).or_else(|| fallback.map(str::to_string));
        tally.add_line(&text, range, &mut project_of);
    }
}

/// Uma chamada `word search` gravada na spec: quando a busca acabou, quanto
/// durou, o dia dela e o que o Jev gastou.
struct WordSearch {
    at: DateTime<Utc>,
    ms: u64,
    day: String,
    tokens: u64,
    cost_micro_usd: u64,
    empty: bool,
}

/// As chamadas `word search` gravadas nas specs da pasta `root` nos dias de
/// `range`.
fn word_searches(root: &Path, range: &Range) -> Vec<WordSearch> {
    let mut found = Vec::new();
    for (_, log) in crate::io::spec_index::read_specs(root) {
        for event in log.visible() {
            if !calls_command(event, WORD_SEARCH) {
                continue;
            }
            let Some(day) = day_of_stamp(event.at()).filter(|day| range.contains(day)) else { continue };
            let Ok(at) = DateTime::parse_from_rfc3339(event.at()) else { continue };
            found.push(WordSearch {
                at: at.with_timezone(&Utc),
                ms: event.int("ms").unwrap_or(0),
                day,
                tokens: event.int("tokens").unwrap_or(0),
                cost_micro_usd: event.int("cost_micro_usd").unwrap_or(0),
                empty: event.int("returned") == Some(0),
            });
        }
    }
    found
}

/// Soma, nas linhas de `rows`, as chamadas `word search` do projeto `name`:
/// quantas foram, os tokens e o custo do Jev e quantas voltaram vazias. Cada
/// chamada que a busca por comando gravou (`commands`, os usos de `Bash` do
/// projeto) já foi contada como o uso de ferramenta dela: o uso sai da conta,
/// e a busca fica uma só.
fn add_word_searches(rows: &mut BTreeMap<(String, String), DayRow>, name: &str, found: &[WordSearch], commands: &[&MapSearch]) {
    for search in found {
        let row = rows
            .entry((search.day.clone(), name.to_string()))
            .or_insert_with(|| DayRow { day: search.day.clone(), project: name.to_string(), ..DayRow::default() });
        row.mustard_searches = row.mustard_searches.saturating_add(1);
        row.jev_tokens = row.jev_tokens.saturating_add(search.tokens);
        row.jev_cost_micro_usd = row.jev_cost_micro_usd.saturating_add(search.cost_micro_usd);
        row.empty_searches = row.empty_searches.saturating_add(u64::from(search.empty));
    }
    let started: Vec<DateTime<Utc>> = commands.iter().map(|command| command.at).collect();
    let recorded: Vec<(DateTime<Utc>, u64)> = found.iter().map(|search| (search.at, search.ms)).collect();
    for at in paired_searches(&started, &recorded) {
        if let Some(row) = rows.get_mut(&(commands[at].day.clone(), name.to_string())) {
            row.mustard_searches = row.mustard_searches.saturating_sub(1);
        }
    }
}

/// As linhas de hoje, o dia aberto: contadas agora, nas conversas de
/// `config_dir`, e marcadas como parciais. Hoje nunca vai para o arquivo dos
/// dias fechados; a conta se refaz a cada pedido.
#[must_use]
pub fn count_open(config_dir: &Path, today: &str) -> Vec<DayRow> {
    let range = Range { first: Some(today.to_string()), last: today.to_string() };
    count(config_dir, &range).into_iter().map(|row| DayRow { partial: true, ..row }).collect()
}

/// A linha de cada dia e projeto de `range`, contada nas conversas de
/// `config_dir` (a pasta de configuração do Claude Code) e nas specs dos
/// projetos que elas citam, em ordem de dia e de projeto.
///
/// O arquivo que não mudou desde o começo da faixa não tem linha dela e nem
/// chega a ser aberto.
#[must_use]
pub fn count(config_dir: &Path, range: &Range) -> Vec<DayRow> {
    let floor = range.first.as_deref().and_then(day_start).map(SystemTime::from);
    let mut projects = Projects::default();
    let mut tally = SpendTally::default();
    for (path, owner) in transcript_files(config_dir) {
        let changed = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
        if floor.is_some_and(|floor| changed.is_some_and(|at| at < floor)) {
            continue;
        }
        let fallback = owner.as_deref().and_then(|owner| owner_project(owner, &mut projects));
        tally_file(&path, range, &mut tally, &mut projects, fallback.as_deref());
    }
    let spent = tally.finish();
    let mut rows = spent.rows;
    for (name, roots) in std::mem::take(&mut projects.roots) {
        let found: Vec<WordSearch> = roots.iter().flat_map(|root| word_searches(root, range)).collect();
        let commands: Vec<&MapSearch> = spent.map_searches.iter().filter(|search| search.project == name).collect();
        add_word_searches(&mut rows, &name, &found, &commands);
    }
    rows.into_values().collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::*;

    /// Um projeto com `mustard.json` na pasta `name` de `base`.
    fn project(base: &Path, name: &str) -> PathBuf {
        let root = base.join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("mustard.json"), "{}").unwrap();
        root
    }

    /// Uma resposta do modelo com os tokens e os usos de ferramenta de
    /// `tools` (nome e argumentos), gravada em `cwd` no carimbo `at`.
    fn reply(id: &str, at: &str, cwd: &Path, tokens: u64, tools: &[(&str, Value)]) -> String {
        let content: Vec<Value> = tools
            .iter()
            .enumerate()
            .map(|(n, (name, input))| json!({"type": "tool_use", "id": format!("{id}-{n}"), "name": name, "input": input}))
            .collect();
        json!({"timestamp": at, "cwd": cwd.to_string_lossy(), "message": {
            "id": id, "model": "m", "usage": {"input_tokens": tokens, "output_tokens": 0}, "content": content}})
        .to_string()
    }

    /// Grava as `lines` na conversa `session` do projeto do Claude Code
    /// `folder`, dentro de `config`.
    fn conversation(config: &Path, folder: &str, session: &str, lines: &[String]) -> PathBuf {
        let dir = config.join("projects").join(folder);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{session}.jsonl"));
        fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    fn range(first: Option<&str>, last: &str) -> Range {
        Range { first: first.map(str::to_string), last: last.to_string() }
    }

    /// Uma conversa falsa com a mesma resposta repetida em várias linhas conta
    /// os tokens uma vez, os da última linha, e cada uso de ferramenta uma
    /// vez; a resposta que reaparece noutro arquivo também.
    #[test]
    fn a_repeated_response_counts_its_tokens_once() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        let first = reply("m1", "2026-10-01T15:00:00Z", &root, 100, &[("Read", json!({"file_path": "a"}))]);
        let last = reply("m1", "2026-10-01T15:00:01Z", &root, 250, &[("Read", json!({"file_path": "a"}))]);
        let other = reply("m2", "2026-10-01T16:00:00Z", &root, 40, &[]);
        conversation(&config, "p", "s1", &[first.clone(), last.clone(), other]);
        conversation(&config, "p", "s2", &[last]);

        let rows = count(&config, &range(None, "2026-10-01"));
        assert_eq!(rows.len(), 1, "{rows:?}");
        let row = &rows[0];
        assert_eq!((row.day.as_str(), row.project.as_str()), ("2026-10-01", "meu-projeto"));
        assert_eq!(row.tokens, 290, "the repeated response counts the last line once, plus the other response");
        assert_eq!((row.actions, row.file_reads, row.code_searches), (1, 1, 0));
    }

    /// `Bash` com `grep` conta como procura, com `mustard-rt` não conta e vira
    /// busca do Mustard, e `Read` vai para as leituras e não para as procuras.
    #[test]
    fn grep_in_bash_is_a_search_and_mustard_rt_is_not() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        let tools = [
            ("Bash", json!({"command": "grep -rn frete src"})),
            ("Bash", json!({"command": "mustard-rt run map search \"frete\""})),
            ("Bash", json!({"command": "mustard-rt run read request-1"})),
            ("Read", json!({"file_path": "src/a.rs"})),
            ("Grep", json!({"pattern": "x"})),
            ("Agent", json!({"subagent_type": "Explore", "prompt": "x"})),
            ("Edit", json!({"file_path": "src/a.rs"})),
        ];
        conversation(&config, "p", "s1", &[reply("m1", "2026-10-01T15:00:00Z", &root, 10, &tools)]);

        let rows = count(&config, &range(None, "2026-10-01"));
        let row = &rows[0];
        assert_eq!(row.actions, 7);
        assert_eq!(row.code_searches, 3, "grep, Grep and Explore, and not the mustard-rt calls or the Read");
        assert_eq!(row.file_reads, 1, "the Read has its own column");
        assert_eq!(row.mustard_searches, 1, "only the map search command");
    }

    /// Só o projeto com `mustard.json` entra, o dia é o de -03:00, e o dia de
    /// hoje, fora da faixa, não entra.
    #[test]
    fn only_projects_with_a_config_and_days_in_range_count() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "com-config");
        let loose = dir.path().join("sem-config");
        fs::create_dir_all(&loose).unwrap();
        let lines = [
            reply("a", "2026-10-02T02:30:00Z", &root, 10, &[]),
            reply("b", "2026-10-02T04:00:00Z", &root, 20, &[]),
            reply("c", "2026-10-01T15:00:00Z", &loose, 30, &[]),
        ];
        conversation(&config, "p", "s1", &lines);

        let rows = count(&config, &range(None, "2026-10-01"));
        let seen: Vec<(&str, &str, u64)> = rows.iter().map(|r| (r.day.as_str(), r.project.as_str(), r.tokens)).collect();
        assert_eq!(seen, [("2026-10-01", "com-config", 10)], "02:30 UTC is still October 1st; the loose folder and today stay out");
    }

    /// O `word search` gravado na spec do projeto entra na linha do dia dele,
    /// com os tokens e o custo do Jev, e a que voltou com zero é contada como
    /// vazia.
    #[test]
    fn a_word_search_with_zero_returned_counts_as_empty() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        conversation(&config, "p", "s1", &[reply("m1", "2026-10-01T15:00:00Z", &root, 10, &[])]);
        let spec = root.join(".claude/spec/uma-spec");
        fs::create_dir_all(&spec).unwrap();
        let call = |id: u64, at: &str, returned: u64, tokens: u64, cost: u64| {
            json!({"v": 1, "id": id, "code": format!("X-CALL-{id:04}"), "at": at, "type": "call", "author": "binary",
                "command": "word search", "returned": returned, "tokens": tokens, "cost_micro_usd": cost})
            .to_string()
        };
        let other = json!({"v": 1, "id": 4, "at": "2026-10-01T10:00:00-03:00", "type": "call", "author": "binary",
            "command": "map search", "returned": 0, "tokens": 5, "cost_micro_usd": 5})
        .to_string();
        let events = [
            call(1, "2026-10-01T10:00:00-03:00", 2, 1000, 900),
            call(2, "2026-10-01T11:00:00-03:00", 0, 500, 400),
            call(3, "2026-10-02T11:00:00-03:00", 0, 700, 600),
            other,
        ];
        fs::write(spec.join("spec.ndjson"), events.join("\n") + "\n").unwrap();

        let rows = count(&config, &range(None, "2026-10-01"));
        let row = &rows[0];
        assert_eq!(row.mustard_searches, 2, "two word searches that day; the other command and the next day stay out");
        assert_eq!(row.empty_searches, 1, "the one that returned zero");
        assert_eq!((row.jev_tokens, row.jev_cost_micro_usd), (1500, 1300));
    }

    /// A busca por comando e o evento `word search` que ela grava contam a
    /// mesma busca, uma vez, com os tokens e o custo do evento; a busca por
    /// comando sem evento (a cravada) e o evento sem comando (o do gancho)
    /// contam uma cada.
    #[test]
    fn a_search_by_command_and_its_recorded_event_count_once() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        let search = [("Bash", json!({"command": "mustard-rt run map search \"frete\""}))];
        conversation(
            &config,
            "p",
            "s1",
            &[
                reply("m1", "2026-10-01T15:00:00Z", &root, 10, &search),
                reply("m2", "2026-10-01T19:30:00Z", &root, 10, &search),
            ],
        );
        let spec = root.join(".claude/spec/uma-spec");
        fs::create_dir_all(&spec).unwrap();
        let call = |id: u64, at: &str, ms: u64, returned: u64, tokens: u64, cost: u64| {
            json!({"v": 1, "id": id, "code": format!("X-CALL-{id:04}"), "at": at, "type": "call", "author": "binary",
                "command": "word search", "ms": ms, "returned": returned, "tokens": tokens, "cost_micro_usd": cost})
            .to_string()
        };
        let events = [
            call(1, "2026-10-01T12:00:02-03:00", 1500, 2, 1000, 900),
            call(2, "2026-10-01T15:00:00-03:00", 900, 0, 500, 400),
        ];
        fs::write(spec.join("spec.ndjson"), events.join("\n") + "\n").unwrap();

        let rows = count(&config, &range(None, "2026-10-01"));
        let row = &rows[0];
        assert_eq!(row.actions, 2);
        assert_eq!(row.mustard_searches, 3, "the command with its event once, the command alone, and the event alone");
        assert_eq!(row.empty_searches, 1, "the event that returned zero");
        assert_eq!((row.jev_tokens, row.jev_cost_micro_usd), (1500, 1300), "the tokens and the cost come from the events");
    }

    /// A conversa de um agente que trabalhou numa cópia já apagada fica com o
    /// projeto da conversa que o chamou.
    #[test]
    fn an_agent_in_a_deleted_copy_keeps_the_project_of_its_session() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        let gone = dir.path().join("copias/meu-projeto-1/spec/a");
        conversation(&config, "p", "s1", &[reply("m1", "2026-10-01T15:00:00Z", &root, 10, &[])]);
        let agents = config.join("projects/p/s1/subagents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(agents.join("agent-1.jsonl"), reply("m9", "2026-10-01T15:05:00Z", &gone, 90, &[])).unwrap();

        let rows = count(&config, &range(None, "2026-10-01"));
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((rows[0].project.as_str(), rows[0].tokens), ("meu-projeto", 100));
    }

    /// O arquivo do gasto: sem arquivo é o vazio, o que se grava volta igual, e
    /// um arquivo que não se lê é recusado sem ser apagado.
    #[test]
    fn the_ledger_round_trips_and_an_unreadable_one_is_refused_untouched() {
        let dir = tempdir().unwrap();
        let folder = dir.path().join("spend");
        assert_eq!(load(&folder), Ok(Ledger::default()));
        update(&folder, |ledger| {
            ledger.url = Some("https://exemplo".into());
            ledger.counted_through = Some("2026-10-01".into());
        })
        .unwrap();
        let saved = load(&folder).unwrap();
        assert_eq!(saved.url.as_deref(), Some("https://exemplo"));
        fs::write(ledger_path(&folder), "{ não é json").unwrap();
        let refused = load(&folder).unwrap_err();
        assert_eq!(refused.reason(), "unreadable-ledger");
        assert_eq!(update(&folder, |_| ()).unwrap_err().reason(), "unreadable-ledger");
        assert_eq!(fs::read_to_string(ledger_path(&folder)).unwrap(), "{ não é json", "the refusal leaves the file alone");
    }
}
