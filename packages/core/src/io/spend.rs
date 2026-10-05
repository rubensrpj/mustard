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
//! O que o Jev gastou não está nas conversas: cada chamada que o usou — a
//! busca por palavra, a do mapa, a montagem da onda, a escolha dos itens —
//! fica gravada na spec do projeto, com os tokens e o custo dele, e entra na
//! linha do dia dela. A chamada feita sem spec onde ficar vai para o arquivo
//! das chamadas soltas, na mesma pasta da máquina ([`record_loose_call`]), e
//! entra na linha do dia do projeto dela do mesmo jeito.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::domain::config::ProjectConfig;
use crate::domain::spend::{day_of, day_of_stamp, day_start, month_of, DayRow, Ledger, Range, Refusal};
use crate::io::fs::lock::{read_shared, LockedFile};
use crate::io::transcript::SpendTally;
use crate::io::workspace::linked_worktree_main;
use crate::platform::error::Error;

/// A variável que muda a pasta do gasto, para quem guarda o cache em outro
/// lugar.
pub const DIR_ENV: &str = "MUSTARD_SPEND_DIR";

/// O arquivo do gasto dentro da pasta da máquina.
const LEDGER: &str = "ledger.json";

/// O arquivo das chamadas ao Jev que não acharam spec onde ficar, dentro da
/// pasta da máquina: uma linha por chamada. Ao contrário do [`LEDGER`], ele
/// não se refaz das conversas, e apagá-lo perde esse gasto.
const LOOSE_CALLS: &str = "jev-calls.ndjson";

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

/// O mês de hoje, `AAAA-MM`, no fuso do gasto.
#[must_use]
pub fn this_month() -> String {
    month_of(&today()).to_string()
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
    /// O projeto se chama pela pasta inteira ([`project_place`]), e não pelo
    /// nome dela: dois projetos de mesmo nome não se somam.
    by_place: bool,
}

impl Projects {
    /// O nome do projeto da pasta `cwd`, ou `None` quando ela não é de um
    /// projeto com `mustard.json`.
    fn name_of(&mut self, cwd: &str) -> Option<String> {
        if let Some(known) = self.by_dir.get(cwd) {
            return known.clone();
        }
        let by_place = self.by_place;
        let found = project_root(Path::new(cwd)).and_then(|root| {
            let name = if by_place { place_of(&root).into_os_string() } else { root.file_name()?.to_os_string() };
            let name = name.to_string_lossy().into_owned();
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

/// A pasta inteira de `folder`, sem atalho nem trecho relativo, que é a mesma
/// venha de onde vier; a pasta que já não existe fica como veio.
fn place_of(folder: &Path) -> PathBuf {
    std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf())
}

/// A pasta inteira do projeto de `root` — qualquer pasta dentro dele ou a
/// cópia de onda dele —, a mesma que a conta das conversas dá a ele, ou
/// `None` quando `root` não é de um projeto com `mustard.json`.
#[must_use]
pub fn project_place(root: &Path) -> Option<PathBuf> {
    project_root(&place_of(root)).map(|found| place_of(&found))
}

/// Se `folder` pode ser a pasta de um projeto. A pasta `.claude` de um
/// projeto, e as de dentro de duas delas, nunca são: uma conversa aberta ali
/// sobe até o projeto, e olhar a própria `.claude` atrás do `mustard.json`
/// derrubaria a conta inteira.
fn may_hold_project(folder: &Path) -> bool {
    let flat = folder.to_string_lossy().replace('\\', "/");
    folder.file_name().is_none_or(|name| name != ".claude") && !flat.contains(".claude/.claude")
}

/// Os caminhos dentro de `dir`, em ordem.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).map(|e| e.path()).collect();
    found.sort();
    found
}

/// Cada conversa do Claude Code em `config_dir`, com a conversa principal da
/// sessão dela quando é a de um agente: as principais de cada projeto do
/// Claude Code primeiro, os agentes depois.
fn transcript_files(config_dir: &Path) -> Vec<(PathBuf, Option<PathBuf>)> {
    let jsonl = |path: &PathBuf| path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl");
    let (mut main, mut agents) = (Vec::new(), Vec::new());
    for folder in entries(&config_dir.join("projects")).into_iter().filter(|path| path.is_dir()) {
        for path in entries(&folder) {
            if jsonl(&path) {
                main.push((path, None));
            } else if path.is_dir() {
                let owner = path.with_extension("jsonl");
                agents.extend(entries(&path.join("subagents")).into_iter().filter(jsonl).map(|file| (file, Some(owner.clone()))));
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

/// Uma linha do arquivo das chamadas soltas: o projeto, o carimbo, os
/// tokens e o custo do Jev.
#[derive(Serialize, Deserialize)]
struct LooseCall {
    project: String,
    at: String,
    #[serde(default)]
    tokens: u64,
    #[serde(default)]
    cost_micro_usd: u64,
}

/// Grava, no arquivo das chamadas soltas da pasta da máquina `dir`, a chamada
/// ao Jev do projeto de `root` que não achou spec onde ficar, com os tokens e
/// o custo dela e o carimbo de agora. Sem isso o gasto dela sumiria da soma
/// do mês e do painel.
///
/// # Errors
///
/// [`Refusal::Io`] quando a trava ou a gravação falham.
pub fn record_loose_call(dir: &Path, root: &Path, tokens: u64, cost_micro_usd: u64) -> Result<(), Refusal> {
    let project = project_name(root).unwrap_or_default();
    append_loose(dir, &LooseCall { project, at: Utc::now().to_rfc3339(), tokens, cost_micro_usd })
}

fn append_loose(dir: &Path, call: &LooseCall) -> Result<(), Refusal> {
    let line = serde_json::to_string(call).map_err(|e| Refusal::Io { detail: e.to_string() })?;
    let io = |e: Error| Refusal::Io { detail: e.to_string() };
    LockedFile::exclusive(&dir.join(LOOSE_CALLS)).map_err(io)?.append_line(&line).map_err(io)
}

/// As chamadas soltas gravadas na pasta da máquina `dir`, cada uma com o
/// projeto dela. A linha que não se lê fica de fora; sem arquivo, nenhuma.
fn loose_calls(dir: &Path) -> Vec<(String, JevCall)> {
    let text = read_shared(&dir.join(LOOSE_CALLS)).unwrap_or_default();
    text.lines()
        .filter_map(|line| serde_json::from_str::<LooseCall>(line).ok())
        .filter_map(|call| {
            let day = day_of_stamp(&call.at)?;
            Some((call.project, JevCall { day, tokens: call.tokens, cost_micro_usd: call.cost_micro_usd }))
        })
        .collect()
}

/// O nome do projeto de `root`, o mesmo que o painel dá: o da pasta do
/// checkout principal, também quando `root` é a cópia de onda dele.
#[must_use]
pub fn project_name(root: &Path) -> Option<String> {
    project_home(root).file_name().map(|name| name.to_string_lossy().into_owned())
}

/// O checkout principal de `root`, ou ele mesmo quando não é cópia de onda.
fn project_home(root: &Path) -> PathBuf {
    linked_worktree_main(root).unwrap_or_else(|| root.to_path_buf())
}

/// Uma chamada ao Jev gravada na spec: o dia dela e o que ele gastou.
struct JevCall {
    day: String,
    tokens: u64,
    cost_micro_usd: u64,
}

/// As chamadas ao Jev gravadas nas specs da pasta `root` nos dias de `range`:
/// toda chamada que levou os tokens ou o custo dele, de qualquer comando. A
/// que falhou antes de ele responder não gastou nada e não entra, e a que não
/// o usa, como a leitura de um item, também não.
fn jev_calls(root: &Path, range: &Range) -> Vec<JevCall> {
    let mut found = Vec::new();
    for (_, log) in crate::io::spec_index::read_specs(root) {
        for event in log.visible().into_iter().filter(|event| event.event_type == "call") {
            let (tokens, cost_micro_usd) = (event.int("tokens"), event.int("cost_micro_usd"));
            if tokens.is_none() && cost_micro_usd.is_none() {
                continue;
            }
            let Some(day) = day_of_stamp(event.at()).filter(|day| range.contains(day)) else { continue };
            found.push(JevCall { day, tokens: tokens.unwrap_or(0), cost_micro_usd: cost_micro_usd.unwrap_or(0) });
        }
    }
    found
}

/// O custo que a linha `line` de uma spec grava, com o dia dela: só o evento
/// `call` que traz `cost_micro_usd`. A linha que não cita os dois nem chega a
/// ser lida como JSON, e são quase todas: uma spec longa passa de dezenas de
/// megabytes, e quem pergunta o gasto do mês não pode relê-la inteira.
fn call_cost(line: &str) -> Option<(String, u64)> {
    if !line.contains("\"type\":\"call\"") || !line.contains("\"cost_micro_usd\"") {
        return None;
    }
    let event: serde_json::Value = serde_json::from_str(line).ok()?;
    if event.get("type")?.as_str()? != "call" {
        return None;
    }
    Some((day_of_stamp(event.get("at")?.as_str()?)?, event.get("cost_micro_usd")?.as_u64()?))
}

/// O que as chamadas ao Jev gravadas nas specs vivas de `root` custaram no mês
/// `month`, em milionésimos de dólar. A spec que não mudou desde o começo do
/// mês não tem chamada dele e nem chega a ser aberta.
fn spec_month_micro_usd(root: &Path, month: &str) -> u64 {
    let floor = day_start(&format!("{month}-01")).map(SystemTime::from);
    crate::io::spec_index::live_spec_files(root)
        .into_iter()
        .filter(|(_, path)| {
            let changed = std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
            floor.is_none_or(|floor| changed.is_none_or(|at| at >= floor))
        })
        .filter_map(|(_, path)| read_shared(&path).ok())
        .map(|text| {
            text.lines()
                .filter_map(call_cost)
                .filter(|(day, _)| month_of(day) == month)
                .fold(0, |sum: u64, (_, cost)| sum.saturating_add(cost))
        })
        .fold(0, u64::saturating_add)
}

/// O que o Jev custou no mês `month` (`AAAA-MM`, no fuso do gasto), em
/// milionésimos de dólar: as chamadas gravadas nas specs do projeto de `root`,
/// de qualquer comando, e, vindo da pasta da máquina em `ledger_dir`, quando
/// há, as chamadas soltas do projeto e o que os outros projetos dela gastaram
/// nos dias que o arquivo do gasto já fechou. As linhas do próprio projeto no
/// arquivo ficam de fora: as specs e as chamadas soltas trazem o mesmo gasto,
/// e até hoje.
#[must_use]
pub fn jev_month_micro_usd(root: &Path, ledger_dir: Option<&Path>, month: &str) -> u64 {
    let home = project_home(root);
    let own = project_name(root);
    let elsewhere = ledger_dir.and_then(|dir| load(dir).ok()).map_or(0, |ledger| {
        ledger
            .rows
            .iter()
            .filter(|row| month_of(&row.day) == month && Some(&row.project) != own.as_ref())
            .fold(0, |sum: u64, row| sum.saturating_add(row.jev_cost_micro_usd))
    });
    let loose = ledger_dir.map_or(0, |dir| {
        loose_calls(dir)
            .into_iter()
            .filter(|(project, call)| Some(project) == own.as_ref() && month_of(&call.day) == month)
            .fold(0, |sum: u64, (_, call)| sum.saturating_add(call.cost_micro_usd))
    });
    spec_month_micro_usd(&home, month).saturating_add(elsewhere).saturating_add(loose)
}

/// As linhas de hoje, o dia aberto: contadas agora, nas conversas de
/// `config_dir` e nas chamadas soltas da pasta da máquina `ledger_dir`, e
/// marcadas como parciais. Hoje nunca vai para o arquivo dos dias fechados; a
/// conta se refaz a cada pedido.
#[must_use]
pub fn count_open(config_dir: &Path, ledger_dir: Option<&Path>, today: &str) -> Vec<DayRow> {
    let range = Range { first: Some(today.to_string()), last: today.to_string() };
    count(config_dir, ledger_dir, &range).into_iter().map(|row| DayRow { partial: true, ..row }).collect()
}

/// A linha de cada dia e projeto de `range`, contada nas conversas de
/// `config_dir` (a pasta de configuração do Claude Code), nas specs dos
/// projetos que elas citam e nas chamadas soltas da pasta da máquina
/// `ledger_dir`, quando há, em ordem de dia e de projeto.
///
/// O arquivo que não mudou desde o começo da faixa não tem linha dela e nem
/// chega a ser aberto.
#[must_use]
pub fn count(config_dir: &Path, ledger_dir: Option<&Path>, range: &Range) -> Vec<DayRow> {
    let (mut rows, mut projects) = conversations(config_dir, range, Projects::default());
    let in_specs = std::mem::take(&mut projects.roots).into_iter().flat_map(|(name, roots)| {
        roots.iter().flat_map(|root| jev_calls(root, range)).map(|call| (name.clone(), call)).collect::<Vec<_>>()
    });
    let loose = ledger_dir.map(loose_calls).unwrap_or_default().into_iter().filter(|(_, call)| range.contains(&call.day));
    for (name, call) in in_specs.chain(loose) {
        let row = rows
            .entry((call.day.clone(), name.clone()))
            .or_insert_with(|| DayRow { day: call.day.clone(), project: name.clone(), ..DayRow::default() });
        row.jev_tokens = row.jev_tokens.saturating_add(call.tokens);
        row.jev_cost_micro_usd = row.jev_cost_micro_usd.saturating_add(call.cost_micro_usd);
    }
    rows.into_values().collect()
}

/// A soma das conversas de `config_dir` nos dias de `range`, sem o Jev: uma
/// linha por dia e projeto, com o projeto chamado como `projects` o chama, e
/// os projetos que elas citam. É a única soma das conversas: o gasto da
/// máquina ([`count`]) e o de um projeto só ([`project_days`]) saem dela.
fn conversations(
    config_dir: &Path,
    range: &Range,
    mut projects: Projects,
) -> (BTreeMap<(String, String), DayRow>, Projects) {
    let floor = range.first.as_deref().and_then(day_start).map(SystemTime::from);
    let mut tally = SpendTally::default();
    for (path, owner) in transcript_files(config_dir) {
        let changed = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
        if floor.is_some_and(|floor| changed.is_some_and(|at| at < floor)) {
            continue;
        }
        let fallback = owner.as_deref().and_then(|owner| owner_project(owner, &mut projects));
        tally_file(&path, range, &mut tally, &mut projects, fallback.as_deref());
    }
    (tally.finish(), projects)
}

/// A linha de cada dia de `range` do projeto de `root`, só das conversas, sem
/// o Jev, em ordem de dia: os tokens, ações e procuras das conversas abertas
/// nele e nas cópias de onda dele, pela mesma soma de [`count`]. O projeto é a
/// pasta inteira ([`project_place`]): outro projeto de mesmo nome, como uma
/// cópia de laboratório, fica fora, ao contrário da página do gasto.
#[must_use]
pub fn project_days(config_dir: &Path, root: &Path, range: &Range) -> Vec<DayRow> {
    let Some(place) = project_place(root) else { return Vec::new() };
    let (key, name) = (place.to_string_lossy(), place.file_name().map(|name| name.to_string_lossy().into_owned()));
    let projects = Projects { by_place: true, ..Projects::default() };
    let rows = conversations(config_dir, range, projects).0.into_values().filter(|row| row.project == key);
    rows.map(|row| DayRow { project: name.clone().unwrap_or_default(), ..row }).collect()
}

/// As conversas do projeto de `root` em `config_dir` mudadas desde `since`:
/// as sessões e os agentes abertos nele e nas cópias de onda dele, com o
/// projeto pela pasta inteira, como em [`project_days`]. Cada conversa é do
/// projeto da primeira linha com uma pasta de projeto; a de agente sem
/// nenhuma herda o da conversa que a chamou.
#[must_use]
pub fn project_conversations(config_dir: &Path, root: &Path, since: SystemTime) -> Vec<PathBuf> {
    let Some(key) = project_place(root).map(|place| place.to_string_lossy().into_owned()) else { return Vec::new() };
    let mut projects = Projects { by_place: true, ..Projects::default() };
    let changed = |path: &PathBuf| std::fs::metadata(path).and_then(|meta| meta.modified()).is_ok_and(|at| at >= since);
    let mut owned = |(path, owner): &(PathBuf, Option<PathBuf>)| {
        let found = owner_project(path, &mut projects).or_else(|| owner_project(owner.as_deref()?, &mut projects));
        found.as_deref() == Some(key.as_str())
    };
    transcript_files(config_dir).into_iter().filter(|file| changed(&file.0) && owned(file)).map(|(path, _)| path).collect()
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

    /// A mesma resposta repetida em várias linhas, e em outro arquivo, conta
    /// os tokens uma vez, os da última linha, e cada uso de ferramenta uma vez;
    /// `grep` no `Bash`, `Grep` e `Explore` são procuras de código, e
    /// `mustard-rt` e `Read` não. Só o projeto com `mustard.json` entra, o dia
    /// é o de -03:00 e o dia fora da faixa, hoje, não entra.
    #[test]
    fn a_repeated_response_counts_once_and_only_projects_with_a_config_and_days_in_range_count() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        let loose = dir.path().join("sem-config");
        fs::create_dir_all(&loose).unwrap();
        let tools = [
            ("Bash", json!({"command": "grep -rn frete src"})),
            ("Bash", json!({"command": "mustard-rt run map search \"frete\""})),
            ("Read", json!({"file_path": "src/a.rs"})),
            ("Grep", json!({"pattern": "x"})),
            ("Agent", json!({"subagent_type": "Explore", "prompt": "x"})),
        ];
        let first = reply("m1", "2026-10-01T15:00:00Z", &root, 100, &tools);
        let last = reply("m1", "2026-10-01T15:00:01Z", &root, 250, &tools);
        let lines = [
            first,
            last.clone(),
            reply("m2", "2026-10-01T16:00:00Z", &root, 40, &[]),
            reply("m3", "2026-10-02T02:30:00Z", &root, 10, &[]),
            reply("m4", "2026-10-02T04:00:00Z", &root, 20, &[]),
            reply("m5", "2026-10-01T15:00:00Z", &loose, 30, &[]),
        ];
        conversation(&config, "p", "s1", &lines);
        conversation(&config, "p", "s2", &[last]);

        let rows = count(&config, None, &range(None, "2026-10-01"));
        assert_eq!(rows.len(), 1, "the loose folder and today stay out: {rows:?}");
        let row = &rows[0];
        assert_eq!((row.day.as_str(), row.project.as_str()), ("2026-10-01", "meu-projeto"));
        assert_eq!(row.tokens, 300, "the last line of m1 once, m2, and 02:30 UTC, which is still October 1st");
        assert_eq!((row.actions, row.code_searches), (5, 3), "grep, Grep and Explore; not the mustard-rt call or the Read");
    }

    /// Toda chamada que gastou o Jev, de qualquer comando — a busca por
    /// palavra, a do mapa, a montagem da onda e a escolha dos itens —, entra
    /// na linha do dia dela com os tokens e o custo; a chamada de outro dia
    /// fora da faixa, a que falhou sem gastar e a que não usa o Jev (a leitura
    /// de um item) ficam fora, e o dia que só tem a chamada da montagem ganha
    /// a linha dele, sem que a leitura crie outra.
    #[test]
    fn every_call_that_billed_the_jev_adds_its_tokens_and_cost_to_its_day() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        conversation(&config, "p", "s1", &[reply("m1", "2026-10-01T15:00:00Z", &root, 10, &[])]);
        let spec = root.join(".claude/spec/uma-spec");
        fs::create_dir_all(&spec).unwrap();
        let call = |id: u64, at: &str, command: &str, extra: Value| {
            let mut event = json!({"v": 1, "id": id, "code": format!("X-CALL-{id:04}"), "at": at, "type": "call",
                "author": "binary", "command": command});
            event.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            event.to_string()
        };
        let billed = |tokens: u64, cost: u64| json!({"filter": "jev", "tokens": tokens, "cost_micro_usd": cost});
        let events = [
            call(1, "2026-10-01T10:00:00-03:00", "word search", billed(1000, 900)),
            call(2, "2026-10-01T11:00:00-03:00", "wave assembly", billed(500, 400)),
            call(3, "2026-10-01T12:00:00-03:00", "wave items", billed(100, 50)),
            call(4, "2026-10-01T13:00:00-03:00", "map search", billed(200, 100)),
            call(5, "2026-10-01T14:00:00-03:00", "word search", json!({"filter": "jev:timeout"})),
            call(6, "2026-10-02T11:00:00-03:00", "wave assembly", billed(700, 600)),
            call(7, "2026-10-03T11:00:00-03:00", "read", json!({"request": "request-1", "item": "X-TASK-0001"})),
            call(8, "2026-10-04T11:00:00-03:00", "word search", billed(9000, 8000)),
        ];
        fs::write(spec.join("spec.ndjson"), events.join("\n") + "\n").unwrap();

        let rows = count(&config, None, &range(None, "2026-10-03"));
        let jev = |row: &DayRow| (row.day.clone(), row.jev_tokens, row.jev_cost_micro_usd);
        let seen: Vec<_> = rows.iter().map(jev).collect();
        assert_eq!(
            seen,
            [("2026-10-01".to_string(), 1800, 1450), ("2026-10-02".to_string(), 700, 600)],
            "the four billed calls of the first day, the assembly alone on the second, and no row for the read"
        );
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

        let rows = count(&config, None, &range(None, "2026-10-01"));
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

    /// O gasto do Jev no mês soma só as chamadas dele: as do mês pedido, de
    /// qualquer comando e de qualquer spec do projeto; as de outro mês, as que
    /// não gastaram, a leitura de um item e o texto que só cita o custo ficam
    /// de fora, a spec que não mudou desde o começo do mês nem se abre, e o
    /// painel de gasto soma o mesmo mês pelo mesmo número. Dos outros projetos
    /// da máquina entra o que o arquivo do gasto já fechou no mês pedido; as
    /// linhas do próprio projeto, que as specs dele já trazem, e as de outro
    /// mês ficam de fora, e um arquivo ilegível vale como ausente. As chamadas
    /// feitas sem spec entram no mês do projeto delas e no painel, cada uma
    /// no seu mês, e a linha que não se lê fica de fora.
    #[test]
    fn the_month_of_jev_spend_counts_only_that_months_billed_calls_and_matches_the_panel() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        conversation(&config, "p", "s1", &[reply("m1", "2026-10-01T15:00:00Z", &root, 10, &[])]);
        let write = |spec: &str, events: &[String]| {
            let folder = root.join(".claude/spec").join(spec);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("spec.ndjson"), events.join("\n") + "\n").unwrap();
            folder.join("spec.ndjson")
        };
        let call = |id: u64, at: &str, command: &str, extra: Value| {
            let mut event = json!({"v": 1, "id": id, "code": format!("X-CALL-{id:04}"), "at": at, "type": "call",
                "author": "binary", "command": command});
            event.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            event.to_string()
        };
        let billed = |cost: u64| json!({"filter": "jev", "tokens": 1000, "cost_micro_usd": cost});
        let note = json!({"v": 1, "id": 9, "code": "X-NOTE-0009", "at": "2026-10-02T10:00:00-03:00", "type": "decision",
            "text": "o campo \"type\":\"call\" traz \"cost_micro_usd\": 777"})
        .to_string();
        write(
            "uma",
            &[
                call(1, "2026-09-30T23:30:00-03:00", "word search", billed(5_000)),
                call(2, "2026-10-01T00:30:00-03:00", "word search", billed(900)),
                call(3, "2026-10-02T11:00:00-03:00", "wave assembly", billed(400)),
                call(4, "2026-10-03T11:00:00-03:00", "word search", json!({"filter": "jev:timeout"})),
                call(5, "2026-10-03T12:00:00-03:00", "read", json!({"item": "X-TASK-0001"})),
                note,
            ],
        );
        write("outra", &[call(1, "2026-10-04T09:00:00-03:00", "wave items", billed(50))]);
        let old = write("velha", &[call(1, "2026-10-05T09:00:00-03:00", "map search", billed(60_000))]);
        let before_october = SystemTime::from(day_start("2026-09-15").unwrap());
        fs::File::options().write(true).open(&old).unwrap().set_modified(before_october).unwrap();

        // O mês de outubro vem sem o fim de setembro, sem o que não gastou e
        // sem a spec que ninguém mexe desde antes do mês.
        let october = jev_month_micro_usd(&root, None, "2026-10");
        assert_eq!(october, 900 + 400 + 50, "{october}");
        assert_eq!(jev_month_micro_usd(&root, None, "2026-09"), 5_000, "the earlier month keeps its own call");
        assert_eq!(jev_month_micro_usd(&root, None, "2026-08"), 0);

        // O painel soma o mesmo mês pelo mesmo número (menos a spec que a
        // leitura leve nem abre).
        let rows = count(&config, None, &range(Some("2026-10-01"), "2026-10-31"));
        let panel: u64 = rows.iter().map(|row| row.jev_cost_micro_usd).sum();
        assert_eq!(panel, october + 60_000, "the panel also reads the file nobody touched");

        // Os outros projetos da máquina gastam do mesmo teto; o próprio
        // projeto no arquivo não conta duas vezes, e o mês passado, só no dele.
        let machine = dir.path().join("spend");
        let row = |day: &str, project: &str, cost: u64| DayRow {
            day: day.to_string(),
            project: project.to_string(),
            jev_cost_micro_usd: cost,
            ..DayRow::default()
        };
        update(&machine, |ledger| {
            ledger.rows = vec![
                row("2026-10-01", "outro", 700),
                row("2026-10-02", "terceiro", 30),
                row("2026-10-02", "meu-projeto", 9_999),
                row("2026-09-30", "outro", 8_888),
            ];
        })
        .unwrap();
        assert_eq!(jev_month_micro_usd(&root, Some(&machine), "2026-10"), october + 730);
        assert_eq!(jev_month_micro_usd(&root, Some(&machine), "2026-09"), 5_000 + 8_888);

        // As chamadas sem spec: a do próprio projeto entra no mês dela e no
        // painel; a de outro projeto entra no painel, e no teto só pelo
        // arquivo do gasto, quando o dia dela fechar.
        let loose = |project: &str, at: &str, cost: u64| {
            let call = LooseCall { project: project.to_string(), at: at.to_string(), tokens: 10, cost_micro_usd: cost };
            append_loose(&machine, &call).unwrap();
        };
        loose("meu-projeto", "2026-10-03T10:00:00-03:00", 2_000);
        loose("meu-projeto", "2026-09-30T23:59:00-03:00", 3_000);
        loose("outro", "2026-10-03T10:00:00-03:00", 4_000);
        let mut file = fs::File::options().append(true).open(machine.join(LOOSE_CALLS)).unwrap();
        std::io::Write::write_all(&mut file, b"{ \"cost_micro_usd\": 50000 }\n").unwrap();
        assert_eq!(jev_month_micro_usd(&root, Some(&machine), "2026-10"), october + 730 + 2_000);
        assert_eq!(jev_month_micro_usd(&root, Some(&machine), "2026-09"), 5_000 + 8_888 + 3_000);
        let rows = count(&config, Some(&machine), &range(Some("2026-10-01"), "2026-10-31"));
        let panel_of = |project: &str| rows.iter().filter(|row| row.project == project).map(|row| row.jev_cost_micro_usd).sum::<u64>();
        assert_eq!((panel_of("meu-projeto"), panel_of("outro")), (october + 60_000 + 2_000, 4_000));

        fs::write(ledger_path(&machine), "{ não é json").unwrap();
        assert_eq!(jev_month_micro_usd(&root, Some(&machine), "2026-10"), october + 2_000, "an unreadable file counts as absent");
    }

    /// O gasto de um projeto só é a linha que a conta da máquina dá a ele,
    /// sem o Jev, em qualquer faixa de dias: o dia que só tem chamada do Jev
    /// não tem conversa, a linha de outro projeto numa conversa dele fica com
    /// o outro, e o agente da cópia apagada fica com o projeto da conversa
    /// que o chamou.
    #[test]
    fn the_spend_of_one_project_is_the_machine_count_of_that_project_without_the_jev() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        let root = project(dir.path(), "meu-projeto");
        let other = project(dir.path(), "outro");
        let gone = dir.path().join("copias/meu-projeto-1/spec/a");
        let tools = [("Grep", json!({"pattern": "x"})), ("Read", json!({"file_path": "a.rs"}))];
        conversation(
            &config,
            "p",
            "s1",
            &[
                reply("m1", "2026-09-30T15:00:00Z", &root, 100, &tools),
                reply("m2", "2026-10-01T15:00:00Z", &root, 40, &[]),
                reply("m3", "2026-10-02T15:00:00Z", &other, 7, &tools),
            ],
        );
        conversation(&config, "o", "s2", &[reply("m4", "2026-10-01T15:00:00Z", &other, 70, &tools)]);
        let agent = config.join("projects/p/s1/subagents/agent-1.jsonl");
        fs::create_dir_all(agent.parent().unwrap()).unwrap();
        fs::write(&agent, reply("m5", "2026-10-01T15:05:00Z", &gone, 90, &tools)).unwrap();
        let spec = root.join(".claude/spec/uma-spec");
        fs::create_dir_all(&spec).unwrap();
        let call = |id: u64, at: &str| {
            json!({"v": 1, "id": id, "code": format!("X-CALL-{id:04}"), "at": at, "type": "call", "author": "binary",
                "command": "word search", "filter": "jev", "tokens": 1000, "cost_micro_usd": 900})
            .to_string()
        };
        let calls = [call(1, "2026-10-01T10:00:00-03:00"), call(2, "2026-10-02T10:00:00-03:00")];
        fs::write(spec.join("spec.ndjson"), calls.join("\n") + "\n").unwrap();

        let ranges = [(None, "2026-10-02"), (Some("2026-10-01"), "2026-10-01"), (Some("2026-10-02"), "2026-10-02")];
        for (first, last) in ranges {
            let range = range(first, last);
            let machine: Vec<DayRow> = count(&config, None, &range)
                .into_iter()
                .filter(|row| row.project == "meu-projeto")
                .map(|row| DayRow { jev_tokens: 0, jev_cost_micro_usd: 0, ..row })
                .filter(|row| row.tokens > 0 || row.actions > 0)
                .collect();
            assert_eq!(project_days(&config, &root, &range), machine, "{first:?} to {last}");
        }
        let days = project_days(&config, &root, &range(None, "2026-10-02"));
        let seen: Vec<_> = days.iter().map(|row| (row.day.as_str(), row.tokens, row.actions, row.code_searches, row.jev_tokens)).collect();
        assert_eq!(seen, [("2026-09-30", 100, 2, 1, 0), ("2026-10-01", 130, 2, 1, 0)], "the agent joins its session's day");
    }
}
