//! `project_map` — o mapa do projeto que o scan grava, e as perguntas curtas
//! que se fazem a ele.
//!
//! O scan grava o mapa em `.claude/grain.model.json`: cada arquivo de código
//! com as importações resolvidas (`deps`), os testes que o cobrem (`tests`) e
//! o histórico do git (`history`). Ninguém lê o arquivo inteiro: quem precisa
//! pergunta, e recebe uma resposta curta:
//!
//! - [`examples`]: 2 ou 3 arquivos que servem de exemplo para uma tarefa, com
//!   o motivo de cada um;
//! - [`importers`]: quem importa um arquivo;
//! - [`tests_for`]: que testes cobrem um arquivo;
//! - [`search`]: a busca por conceito, com a mesma preparação de texto e o
//!   mesmo BM25 das lições e das specs;
//! - [`summary`]: o resumo para o início da sessão, até 3 kB;
//! - [`check_skill`]: a conferência de uma skill (caminhos citados e tamanho).
//!
//! O histórico também mora aqui ([`History`]), porque o scan o escreve e as
//! perguntas o leem: um tipo só para os dois lados.
//!
//! Função pura: sem disco e sem relógio. A leitura do arquivo mora em
//! `io::project_map`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::domain::ast::is_test_path;
use crate::domain::search::{query_terms, SearchIndex, TOP};
use crate::domain::spec_events::search_field;
use crate::platform::i18n::{translate, Locale};

/// Um commit que muda mais arquivos do que isto não conta para "muda junto":
/// é formatação, renomeação em massa ou importação, e ligaria tudo a tudo.
pub const CO_CHANGE_MAX_FILES: usize = 30;

/// Quantos commits o mapa guarda, dos mais novos.
pub const MAX_COMMITS: usize = 5000;

/// Quantos arquivos que mudam junto a resposta mostra.
pub const TOGETHER_SHOWN: usize = 5;

/// O tamanho máximo do resumo do início da sessão, em bytes.
pub const SUMMARY_MAX_BYTES: usize = 3 * 1024;

/// O tamanho máximo de uma skill, em linhas.
pub const SKILL_MAX_LINES: usize = 500;

// ---------------------------------------------------------------------------
// Histórico do git
// ---------------------------------------------------------------------------

/// O histórico do git guardado no mapa: os caminhos numa tabela só, em ordem
/// de nome, e os commits do mais antigo para o mais novo, apontando a tabela.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub paths: Vec<String>,
    pub commits: Vec<Commit>,
}

/// Um commit guardado: o começo do hash, a data (segundos desde 1970) e os
/// arquivos que ele criou e os que ele mudou, pelo número na tabela.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Commit {
    pub id: String,
    pub at: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<u32>,
}

/// Um commit com os caminhos por extenso.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawCommit {
    pub id: String,
    pub at: i64,
    pub added: Vec<String>,
    pub changed: Vec<String>,
}

impl RawCommit {
    /// Todos os arquivos do commit, os criados e os mudados.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.added.iter().chain(&self.changed).map(String::as_str)
    }
}

impl History {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    /// Monta o histórico a partir dos commits, do mais antigo para o mais
    /// novo. Ficam só os [`MAX_COMMITS`] mais novos, e a tabela de caminhos
    /// sai em ordem de nome, com os caminhos que os commits guardados citam:
    /// o mesmo histórico dá sempre os mesmos bytes, lido de uma vez ou aos
    /// poucos.
    #[must_use]
    pub fn from_raw(commits: Vec<RawCommit>) -> Self {
        let skip = commits.len().saturating_sub(MAX_COMMITS);
        let commits: Vec<RawCommit> = commits.into_iter().skip(skip).collect();
        let paths: Vec<String> =
            commits.iter().flat_map(RawCommit::files).map(str::to_string).collect::<BTreeSet<_>>().into_iter().collect();
        let index: BTreeMap<&str, u32> =
            paths.iter().enumerate().map(|(i, p)| (p.as_str(), u32::try_from(i).unwrap_or(u32::MAX))).collect();
        let numbers = |list: &[String]| -> Vec<u32> {
            let mut out: Vec<u32> = list.iter().filter_map(|p| index.get(p.as_str()).copied()).collect();
            out.sort_unstable();
            out.dedup();
            out
        };
        let commits = commits
            .iter()
            .map(|c| Commit { id: c.id.clone(), at: c.at, added: numbers(&c.added), changed: numbers(&c.changed) })
            .collect();
        Self { paths, commits }
    }

    /// Os commits com os caminhos por extenso, do mais antigo para o mais
    /// novo.
    #[must_use]
    pub fn raw(&self) -> Vec<RawCommit> {
        let name = |list: &[u32]| -> Vec<String> {
            list.iter().filter_map(|&i| self.paths.get(i as usize)).cloned().collect()
        };
        self.commits
            .iter()
            .map(|c| RawCommit { id: c.id.clone(), at: c.at, added: name(&c.added), changed: name(&c.changed) })
            .collect()
    }

    /// O histórico com `newer` acrescentado no fim.
    #[must_use]
    pub fn extended(&self, newer: Vec<RawCommit>) -> Self {
        let mut all = self.raw();
        all.extend(newer);
        Self::from_raw(all)
    }
}

/// O que o histórico diz de um arquivo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileHistory {
    /// Em quantos commits o arquivo aparece.
    pub commits: u32,
    /// A data do commit mais novo que o mudou.
    pub last_at: i64,
    /// O começo do hash desse commit.
    pub last_commit: String,
    /// Os arquivos que mais mudam junto com ele, com quantas vezes.
    pub together: Vec<(String, u32)>,
}

/// O histórico de um arquivo, ou `None` quando nenhum commit guardado o cita.
#[must_use]
pub fn file_history(history: &History, path: &str) -> Option<FileHistory> {
    let index = u32::try_from(history.paths.binary_search_by(|p| p.as_str().cmp(path)).ok()?).ok()?;
    let mut out = FileHistory::default();
    let mut together: BTreeMap<u32, u32> = BTreeMap::new();
    for commit in &history.commits {
        let touched = commit.added.binary_search(&index).is_ok() || commit.changed.binary_search(&index).is_ok();
        if !touched {
            continue;
        }
        out.commits += 1;
        if commit.at >= out.last_at {
            out.last_at = commit.at;
            out.last_commit.clone_from(&commit.id);
        }
        if commit.added.len() + commit.changed.len() <= CO_CHANGE_MAX_FILES {
            for &other in commit.added.iter().chain(&commit.changed).filter(|&&o| o != index) {
                *together.entry(other).or_insert(0) += 1;
            }
        }
    }
    let mut ranked: Vec<(String, u32)> = together
        .into_iter()
        .filter_map(|(i, n)| history.paths.get(i as usize).map(|p| (p.clone(), n)))
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(TOGETHER_SHOWN);
    out.together = ranked;
    Some(out)
}

/// Para cada arquivo, em quantos commits ele aparece e com quais outros ele
/// muda junto (e quantas vezes). Os commits grandes demais
/// ([`CO_CHANGE_MAX_FILES`]) contam para o número de commits e não para o
/// "muda junto".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryStats {
    pub commits: BTreeMap<String, u32>,
    pub together: BTreeMap<String, BTreeMap<String, u32>>,
}

#[must_use]
pub fn history_stats(history: &History) -> HistoryStats {
    let mut commits: BTreeMap<u32, u32> = BTreeMap::new();
    let mut together: BTreeMap<u32, BTreeMap<u32, u32>> = BTreeMap::new();
    for commit in &history.commits {
        let files: Vec<u32> = commit.added.iter().chain(&commit.changed).copied().collect();
        for &f in &files {
            *commits.entry(f).or_insert(0) += 1;
        }
        if files.len() > CO_CHANGE_MAX_FILES {
            continue;
        }
        for &a in &files {
            for &b in files.iter().filter(|&&b| b != a) {
                *together.entry(a).or_default().entry(b).or_insert(0) += 1;
            }
        }
    }
    let name = |i: u32| history.paths.get(i as usize).cloned().unwrap_or_default();
    HistoryStats {
        commits: commits.into_iter().map(|(i, n)| (name(i), n)).collect(),
        together: together
            .into_iter()
            .map(|(i, others)| (name(i), others.into_iter().map(|(o, n)| (name(o), n)).collect()))
            .collect(),
    }
}

/// `true` quando um teste que muda junto com um arquivo o cobre: pelo menos
/// dois commits juntos, e em pelo menos 30% dos commits do arquivo.
#[must_use]
pub fn covers_by_history(together: u32, commits_of_file: u32) -> bool {
    together >= 2 && u64::from(together) * 10 >= u64::from(commits_of_file) * 3
}

/// A data de um instante, como `2026-09-12` (UTC).
#[must_use]
pub fn date_of(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// O mapa, como as perguntas o leem
// ---------------------------------------------------------------------------

/// A parte do mapa que as perguntas leem. Campos que faltam valem o padrão,
/// e os que sobram são ignorados.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProjectMap {
    pub modules: Vec<MapModule>,
    pub projects: Vec<MapProject>,
    pub languages: Vec<MapLanguage>,
    pub graph: MapGraph,
    pub history: History,
    pub state: MapState,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MapModule {
    pub path: String,
    pub language: String,
    pub loc: usize,
    /// Vazio no arquivo escrito à mão; senão, gerado, de terceiros etc.
    pub file_class: String,
    pub declarations: Vec<MapDecl>,
    /// Os arquivos do projeto que este importa.
    pub deps: Vec<String>,
    /// Os testes que o cobrem.
    pub tests: Vec<String>,
    /// O arquivo traz os próprios testes.
    pub has_tests: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MapDecl {
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MapProject {
    pub name: String,
    pub dir: String,
    pub kind: String,
    pub code_files: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MapLanguage {
    pub language: String,
    pub files: usize,
    pub loc: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MapGraph {
    pub top_fan_in: Vec<MapDegree>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MapDegree {
    pub module: String,
    pub degree: usize,
}

/// De onde o mapa foi lido: o commit da última passada.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MapState {
    pub head: String,
}

// ---------------------------------------------------------------------------
// Recusas
// ---------------------------------------------------------------------------

/// Por que uma pergunta ao mapa não tem resposta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapRefusal {
    /// O projeto ainda não tem mapa.
    MapMissing,
    /// O mapa existe e não se entende.
    MapUnreadable { detail: String },
    /// O arquivo perguntado não está no mapa.
    UnknownFile { file: String },
    /// A pergunta precisa de uma opção que não veio.
    MissingArgument { question: String, flag: String },
    /// A skill não pôde ser lida.
    SkillUnreadable { path: String, detail: String },
    /// A skill cita caminhos que não existem.
    SkillMissingPaths { paths: Vec<String> },
    /// A skill passa do limite de linhas.
    SkillTooLong { lines: usize },
}

impl MapRefusal {
    /// A razão curta e estável da recusa.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::MapMissing => "map-missing",
            Self::MapUnreadable { .. } => "map-unreadable",
            Self::UnknownFile { .. } => "unknown-file",
            Self::MissingArgument { .. } => "missing-argument",
            Self::SkillUnreadable { .. } => "skill-unreadable",
            Self::SkillMissingPaths { .. } => "skill-missing-path",
            Self::SkillTooLong { .. } => "skill-too-long",
        }
    }

    /// A mensagem da recusa no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::MapMissing => fill("map.missing", &[]),
            Self::MapUnreadable { detail } => fill("map.unreadable", &[("{detail}", detail.clone())]),
            Self::UnknownFile { file } => fill("map.unknown_file", &[("{file}", file.clone())]),
            Self::MissingArgument { question, flag } => {
                fill("map.missing_argument", &[("{question}", question.clone()), ("{flag}", flag.clone())])
            }
            Self::SkillUnreadable { path, detail } => {
                fill("map.skill_unreadable", &[("{path}", path.clone()), ("{detail}", detail.clone())])
            }
            Self::SkillMissingPaths { paths } => fill("map.skill_missing_path", &[("{paths}", paths.join(", "))]),
            Self::SkillTooLong { lines } => fill(
                "map.skill_too_long",
                &[("{lines}", lines.to_string()), ("{max}", SKILL_MAX_LINES.to_string())],
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Caminhos
// ---------------------------------------------------------------------------

/// O caminho com barras normais, sem `./` no começo e sem barra no fim.
#[must_use]
pub fn clean_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let path = path.strip_prefix("./").unwrap_or(&path);
    path.trim_end_matches('/').to_string()
}

/// A pasta de um caminho (vazia na raiz).
#[must_use]
pub fn folder_of(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

fn extension_of(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rfind('.').filter(|&i| i > 0).map(|i| &name[i + 1..])
}

impl ProjectMap {
    /// O arquivo do mapa com esse caminho.
    #[must_use]
    pub fn module(&self, path: &str) -> Option<&MapModule> {
        let path = clean_path(path);
        self.modules.iter().find(|m| m.path == path)
    }

    fn known(&self, file: &str) -> Result<&MapModule, MapRefusal> {
        self.module(file).ok_or_else(|| MapRefusal::UnknownFile { file: clean_path(file) })
    }
}

/// `true` para o arquivo de teste e para o escrito por máquina: nenhum dos
/// dois serve de exemplo.
fn is_example_material(m: &MapModule) -> bool {
    m.file_class.is_empty() && !is_test_path(&m.path)
}

// ---------------------------------------------------------------------------
// Quem importa, que teste cobre
// ---------------------------------------------------------------------------

/// Os arquivos que importam `file`, em ordem de nome.
pub fn importers(map: &ProjectMap, file: &str) -> Result<Vec<String>, MapRefusal> {
    let target = map.known(file)?.path.clone();
    let mut out: Vec<String> =
        map.modules.iter().filter(|m| m.deps.contains(&target)).map(|m| m.path.clone()).collect();
    out.sort();
    Ok(out)
}

/// Os testes que cobrem um arquivo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestCoverage {
    /// O arquivo traz os próprios testes.
    pub inline: bool,
    /// Os arquivos de teste que o importam ou que mudam junto com ele.
    pub files: Vec<String>,
}

/// Os testes que cobrem `file`.
pub fn tests_for(map: &ProjectMap, file: &str) -> Result<TestCoverage, MapRefusal> {
    let module = map.known(file)?;
    Ok(TestCoverage { inline: module.has_tests, files: module.tests.clone() })
}

// ---------------------------------------------------------------------------
// Busca por conceito
// ---------------------------------------------------------------------------

/// Um arquivo achado pela busca, com a nota ×1024.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: String,
    pub score: u64,
}

/// O nome quebrado nas palavras dele: `ProcessadorPagamento`,
/// `processador_pagamento` e `processador-pagamento` viram
/// `processador pagamento`.
#[must_use]
pub fn split_identifier(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 8);
    let mut prev: Option<char> = None;
    for c in name.chars() {
        if matches!(c, '_' | '-' | '.' | '/' | ':') {
            out.push(' ');
        } else {
            if c.is_uppercase() && prev.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) {
                out.push(' ');
            }
            out.push(c);
        }
        prev = Some(c);
    }
    out
}

/// O texto de busca de um arquivo: as palavras do caminho e dos nomes que ele
/// declara, pela mesma preparação do campo `search` das specs.
fn search_text(m: &MapModule) -> String {
    let mut text = split_identifier(&m.path);
    for decl in &m.declarations {
        text.push(' ');
        text.push_str(&split_identifier(&decl.name));
    }
    search_field(Some(&text), &[])
}

/// Os arquivos que mais casam com as palavras do pedido, os 5 mais fortes,
/// pelo BM25 de `domain::search`. Arquivo escrito por máquina fica de fora.
#[must_use]
pub fn search(map: &ProjectMap, query: &str) -> Vec<Found> {
    let docs: Vec<(u64, String)> = map
        .modules
        .iter()
        .enumerate()
        .filter(|(_, m)| m.file_class.is_empty())
        .map(|(i, m)| (i as u64, search_text(m)))
        .collect();
    let index = SearchIndex::build(docs.iter().map(|(id, text)| (*id, text.as_str())));
    index
        .top(&query_terms(query), TOP)
        .into_iter()
        .filter_map(|hit| map.modules.get(hit.id as usize).map(|m| Found { path: m.path.clone(), score: hit.score }))
        .collect()
}

/// Quantos achados da busca pesam na escolha da pasta.
const FOLDER_HITS: usize = 50;

/// A pasta que mais casa com as palavras de uma tarefa: a soma das notas dos
/// arquivos achados, pasta a pasta, sobre os achados mais fortes. Teste e
/// arquivo escrito por máquina não contam. `None` quando nada casa.
#[must_use]
pub fn best_folder(map: &ProjectMap, task: &str) -> Option<String> {
    let docs: Vec<(u64, String)> = map
        .modules
        .iter()
        .enumerate()
        .filter(|(_, m)| is_example_material(m))
        .map(|(i, m)| (i as u64, search_text(m)))
        .collect();
    let index = SearchIndex::build(docs.iter().map(|(id, text)| (*id, text.as_str())));
    let mut by_folder: BTreeMap<&str, u64> = BTreeMap::new();
    for hit in index.top(&query_terms(task), FOLDER_HITS) {
        if let Some(m) = map.modules.get(hit.id as usize) {
            *by_folder.entry(folder_of(&m.path)).or_insert(0) += hit.score;
        }
    }
    by_folder.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0))).map(|(f, _)| f.to_string())
}

// ---------------------------------------------------------------------------
// Exemplos para uma tarefa
// ---------------------------------------------------------------------------

/// Um arquivo que serve de exemplo, com o motivo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Example {
    pub path: String,
    pub loc: usize,
    /// As importações principais que ele também tem.
    pub shared_imports: Vec<String>,
    /// Os testes que o cobrem.
    pub tests: Vec<String>,
    pub inline_tests: bool,
    /// A data da última mudança, quando há histórico.
    pub last_change: Option<String>,
    /// Os motivos da escolha, em palavras.
    pub why: Vec<String>,
}

/// A receita tirada do git: um commit que criou um arquivo do mesmo tipo na
/// mesma pasta, e os outros arquivos que ele mudou (registro, teste, texto).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recipe {
    pub commit: String,
    pub date: String,
    pub added: String,
    pub together: Vec<String>,
}

/// A resposta de [`examples`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Examples {
    /// A pasta do alvo da tarefa.
    pub folder: String,
    /// As importações que os arquivos da pasta mais têm (ou as do próprio
    /// alvo, quando ele já existe).
    pub main_imports: Vec<String>,
    /// De 0 a 3 exemplos, o melhor primeiro.
    pub picks: Vec<Example>,
    /// Até 3 receitas do git, a mais nova primeiro.
    pub recipes: Vec<Recipe>,
}

/// Quantos exemplos a resposta dá, no máximo.
const MAX_PICKS: usize = 3;
/// Quantas importações principais contam.
const MAX_MAIN_IMPORTS: usize = 8;
/// Quantos arquivos de uma receita a resposta mostra.
const RECIPE_FILES: usize = 10;

struct Candidate<'a> {
    module: &'a MapModule,
    same_folder: bool,
    shared: Vec<String>,
    last_at: i64,
}

/// As importações principais de uma pasta, tiradas dos arquivos mais
/// parecidos entre si. Cada arquivo vale a soma, sobre as importações dele, de
/// quantos outros arquivos da pasta também as têm; os mais parecidos são o
/// terço de cima (no mínimo dois), e as importações principais são as que pelo
/// menos dois deles têm, das mais comuns para as menos.
fn main_imports_of(files: &[&MapModule]) -> Vec<String> {
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for m in files {
        for d in &m.deps {
            *count.entry(d.as_str()).or_insert(0) += 1;
        }
    }
    let mut ranked: Vec<(usize, &MapModule)> = files
        .iter()
        .filter(|m| !m.deps.is_empty())
        .map(|m| (m.deps.iter().map(|d| count[d.as_str()] - 1).sum(), *m))
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    let core = ranked.len().div_ceil(3).max(2);
    let mut shared: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, m) in ranked.iter().take(core).filter(|(score, _)| *score > 0) {
        for d in &m.deps {
            *shared.entry(d.as_str()).or_insert(0) += 1;
        }
    }
    let mut common: Vec<(&str, usize)> = shared.into_iter().filter(|&(_, n)| n >= 2).collect();
    common.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| count[b.0].cmp(&count[a.0])).then_with(|| a.0.cmp(b.0)));
    common.into_iter().take(MAX_MAIN_IMPORTS).map(|(d, _)| d.to_string()).collect()
}

/// Escolhe 2 ou 3 exemplos para uma tarefa cujo alvo é `target` (um arquivo
/// que existe ou vai existir, ou uma pasta):
///
/// - mesmo lugar e mesmo papel: a mesma pasta do alvo e as mesmas
///   importações principais no grafo, nunca o sufixo do nome;
/// - a receita do git: os últimos 3 commits que criaram um arquivo do mesmo
///   tipo na pasta, com os arquivos mudados junto;
/// - com teste primeiro, e depois o mais recente;
/// - tamanho típico: entre o quartil de baixo e o de cima da pasta, o que
///   deixa de fora o índice de módulo curto, que não mostra como fazer, e o
///   arquivo grande demais, que mistura trabalhos.
///
/// As lições do banco vêm de quem chama, que lê o banco. Sem histórico, valem
/// a pasta, as importações, o teste e o tamanho.
#[must_use]
pub fn examples(map: &ProjectMap, target: &str, lang: Locale) -> Examples {
    let target = clean_path(target);
    let target_module = map.module(&target);
    let is_folder = target_module.is_none()
        && (target.is_empty() || map.modules.iter().any(|m| folder_of(&m.path) == target));
    let folder = if is_folder { target.clone() } else { folder_of(&target).to_string() };
    let last_at = |path: &str| -> i64 { file_history(&map.history, path).map_or(0, |h| h.last_at) };

    // A mesma pasta; com menos de 2, as pastas vizinhas (mesmo pai).
    let usable = |m: &&MapModule| is_example_material(m) && m.path != target;
    let mut pool: Vec<(&MapModule, bool)> =
        map.modules.iter().filter(usable).filter(|m| folder_of(&m.path) == folder).map(|m| (m, true)).collect();
    if pool.len() < 2 {
        let parent = folder_of(&folder);
        let near = |m: &MapModule| {
            let dir = folder_of(&m.path);
            dir != folder && (parent.is_empty() || dir == parent || dir.starts_with(&format!("{parent}/")))
        };
        pool.extend(map.modules.iter().filter(usable).filter(|m| near(m)).map(|m| (m, false)));
    }

    // As importações principais: as do alvo, quando ele existe e importa algo;
    // senão, as dos arquivos mais parecidos da pasta.
    let main_imports: Vec<String> = match target_module {
        Some(m) if !m.deps.is_empty() => m.deps.iter().take(MAX_MAIN_IMPORTS).cloned().collect(),
        _ => {
            let in_folder: Vec<&MapModule> = pool.iter().filter(|(_, same)| *same).map(|(m, _)| *m).collect();
            main_imports_of(&in_folder)
        }
    };

    let mut candidates: Vec<Candidate> = pool
        .into_iter()
        .map(|(module, same_folder)| Candidate {
            module,
            same_folder,
            shared: main_imports.iter().filter(|d| module.deps.contains(d)).cloned().collect(),
            last_at: last_at(&module.path),
        })
        .collect();
    // Com importações principais, só entra quem tem pelo menos uma delas,
    // desde que sobrem dois.
    if !main_imports.is_empty() {
        let sharing = candidates.iter().filter(|c| !c.shared.is_empty()).count();
        if sharing >= 2 {
            candidates.retain(|c| !c.shared.is_empty());
        }
    }
    // Tamanho típico, quando há pelo menos quatro: entre os quartis.
    let typical = candidates.len() >= 4;
    if typical {
        let mut locs: Vec<usize> = candidates.iter().map(|c| c.module.loc).collect();
        locs.sort_unstable();
        let q1 = locs[locs.len() / 4];
        let q3 = locs[(locs.len() * 3).div_ceil(4) - 1];
        candidates.retain(|c| (q1..=q3).contains(&c.module.loc));
    }
    // A mesma pasta antes da vizinha; com teste primeiro; depois o mais
    // recente, mais importações em comum e o nome.
    let tested = |c: &Candidate| !c.module.tests.is_empty() || c.module.has_tests;
    candidates.sort_by(|a, b| {
        b.same_folder
            .cmp(&a.same_folder)
            .then_with(|| tested(b).cmp(&tested(a)))
            .then_with(|| b.last_at.cmp(&a.last_at))
            .then_with(|| b.shared.len().cmp(&a.shared.len()))
            .then_with(|| a.module.path.cmp(&b.module.path))
    });
    let picks = candidates
        .iter()
        .take(MAX_PICKS)
        .map(|c| {
            let mut why = Vec::new();
            let key = if c.same_folder { "map.why.same_folder" } else { "map.why.near_folder" };
            why.push(translate(key, lang).to_string());
            if !main_imports.is_empty() {
                why.push(
                    translate("map.why.imports", lang)
                        .replace("{shared}", &c.shared.len().to_string())
                        .replace("{of}", &main_imports.len().to_string()),
                );
            }
            if !c.module.tests.is_empty() {
                why.push(translate("map.why.tested", lang).replace("{tests}", &c.module.tests.join(", ")));
            }
            if c.module.has_tests {
                why.push(translate("map.why.inline_tests", lang).to_string());
            }
            let last_change = (c.last_at > 0).then(|| date_of(c.last_at));
            if let Some(date) = &last_change {
                why.push(translate("map.why.recent", lang).replace("{date}", date));
            }
            if typical {
                why.push(translate("map.why.size", lang).replace("{loc}", &c.module.loc.to_string()));
            }
            Example {
                path: c.module.path.clone(),
                loc: c.module.loc,
                shared_imports: c.shared.clone(),
                tests: c.module.tests.clone(),
                inline_tests: c.module.has_tests,
                last_change,
                why,
            }
        })
        .collect();

    Examples { recipes: recipes(&map.history, &folder, extension_of(&target)), folder, main_imports, picks }
}

/// A receita do git: os últimos 3 commits que criaram um arquivo do tipo
/// `ext` (qualquer um, sem extensão) na pasta `folder`, e os arquivos que
/// cada um mudou junto.
fn recipes(history: &History, folder: &str, ext: Option<&str>) -> Vec<Recipe> {
    let mut out = Vec::new();
    for commit in history.raw().into_iter().rev() {
        let created = commit.added.iter().find(|p| {
            folder_of(p) == folder && !is_test_path(p) && ext.is_none_or(|e| extension_of(p) == Some(e))
        });
        let Some(created) = created else {
            continue;
        };
        let mut together: Vec<String> = commit.files().filter(|p| *p != created).map(str::to_string).collect();
        together.sort();
        together.truncate(RECIPE_FILES);
        out.push(Recipe { commit: commit.id.clone(), date: date_of(commit.at), added: created.clone(), together });
        if out.len() == MAX_PICKS {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Resumo do início da sessão
// ---------------------------------------------------------------------------

/// Quantos subprojetos o resumo lista.
const SUMMARY_PROJECTS: usize = 12;
/// Quantos arquivos cada linha de arquivos do resumo cita.
const SUMMARY_FILES: usize = 5;
/// O tamanho máximo de uma linha do resumo, em bytes.
const SUMMARY_LINE_MAX: usize = 400;

fn clip(line: String) -> String {
    if line.len() <= SUMMARY_LINE_MAX {
        return line;
    }
    let mut end = SUMMARY_LINE_MAX - 3;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &line[..end])
}

/// O resumo do mapa para o início da sessão: quantos arquivos e de que
/// linguagens, os subprojetos, os arquivos mais importados, os mudados há
/// pouco e como perguntar ao mapa. Nunca passa de [`SUMMARY_MAX_BYTES`]: as
/// linhas que não cabem ficam de fora, e a última (como perguntar) sempre
/// entra.
#[must_use]
pub fn summary(map: &ProjectMap, lang: Locale) -> String {
    let files: usize = map.modules.len();
    let languages: Vec<String> =
        map.languages.iter().take(SUMMARY_FILES).map(|l| format!("{} {}", l.language, l.files)).collect();
    let mut lines: Vec<String> = vec![clip(
        translate("map.summary.head", lang)
            .replace("{files}", &files.to_string())
            .replace("{languages}", &languages.join(", ")),
    )];
    // Um projeto de teste (uma fixture dentro de `tests/`) não é subprojeto.
    let mut projects: Vec<&MapProject> =
        map.projects.iter().filter(|p| p.code_files > 0 && !is_test_path(&format!("{}/x", p.dir))).collect();
    projects.sort_by(|a, b| b.code_files.cmp(&a.code_files).then_with(|| a.dir.cmp(&b.dir)));
    if projects.len() > 1 {
        lines.push(translate("map.summary.projects", lang).to_string());
        for p in projects.iter().take(SUMMARY_PROJECTS) {
            let dir = if p.dir.is_empty() { "." } else { p.dir.as_str() };
            lines.push(clip(
                translate("map.summary.project_line", lang)
                    .replace("{name}", &p.name)
                    .replace("{dir}", dir)
                    .replace("{kind}", &p.kind)
                    .replace("{files}", &p.code_files.to_string()),
            ));
        }
    }
    let hubs: Vec<&str> = map.graph.top_fan_in.iter().take(SUMMARY_FILES).map(|d| d.module.as_str()).collect();
    if !hubs.is_empty() {
        lines.push(clip(translate("map.summary.hubs", lang).replace("{files}", &hubs.join(", "))));
    }
    let known: BTreeSet<&str> = map.modules.iter().map(|m| m.path.as_str()).collect();
    let mut recent: Vec<String> = Vec::new();
    for commit in map.history.raw().iter().rev() {
        for path in commit.files() {
            if known.contains(path) && !recent.iter().any(|r| r == path) {
                recent.push(path.to_string());
            }
        }
        if recent.len() >= SUMMARY_FILES {
            break;
        }
    }
    recent.truncate(SUMMARY_FILES);
    if !recent.is_empty() {
        lines.push(clip(translate("map.summary.recent", lang).replace("{files}", &recent.join(", "))));
    }
    let ask = translate("map.summary.ask", lang).to_string();

    let mut out = String::new();
    for line in lines {
        if out.len() + line.len() + 1 + ask.len() + 1 > SUMMARY_MAX_BYTES {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(&ask);
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Conferência de uma skill
// ---------------------------------------------------------------------------

/// Os caminhos que uma skill cita entre crases: com barra, e com extensão no
/// último pedaço ou com barra no fim. Ficam de fora os modelos (`<nome>`,
/// `{x}`, `*`, `$VAR`), os endereços e o que tem espaço. Um `:linha` no fim
/// sai.
#[must_use]
pub fn cited_paths(text: &str) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for (i, span) in text.split('`').enumerate() {
        if i % 2 == 0 {
            continue;
        }
        let span = span.trim();
        if span.is_empty()
            || span.contains(char::is_whitespace)
            || span.contains("://")
            || span.starts_with('-')
            || span.starts_with('/')
            || span.contains(['<', '>', '{', '}', '*', '$', '|', '(', ')', '[', ']', '…', '"', '\''])
            || !span.contains('/')
        {
            continue;
        }
        let path = match span.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '-') => head,
            _ => span,
        };
        let path = path.trim_end_matches(['.', ',', ';']);
        let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
        if path.ends_with('/') || extension_of(last).is_some() {
            out.insert(path.to_string());
        }
    }
    out.into_iter().collect()
}

/// Confere uma skill: cada caminho citado existe (`exists` responde) e o
/// texto tem no máximo [`SKILL_MAX_LINES`] linhas.
pub fn check_skill(text: &str, exists: impl Fn(&str) -> bool) -> Result<(), MapRefusal> {
    let missing: Vec<String> = cited_paths(text).into_iter().filter(|p| !exists(p)).collect();
    if !missing.is_empty() {
        return Err(MapRefusal::SkillMissingPaths { paths: missing });
    }
    let lines = text.lines().count();
    if lines > SKILL_MAX_LINES {
        return Err(MapRefusal::SkillTooLong { lines });
    }
    Ok(())
}

/// `true` quando algum arquivo do mapa é `cited` ou termina com `/cited`: a
/// skill pode citar só o fim do caminho, como `spec_events/mod.rs`.
#[must_use]
pub fn map_knows(map: &ProjectMap, cited: &str) -> bool {
    let cited = clean_path(cited);
    let tail = format!("/{cited}");
    map.modules.iter().any(|m| {
        m.path == cited || m.path.ends_with(&tail) || folder_of(&m.path) == cited || m.path.starts_with(&format!("{cited}/"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(path: &str, loc: usize, deps: &[&str]) -> MapModule {
        MapModule {
            path: path.to_string(),
            language: "rust".to_string(),
            loc,
            deps: deps.iter().map(|d| (*d).to_string()).collect(),
            ..MapModule::default()
        }
    }

    fn commit(id: &str, at: i64, added: &[&str], changed: &[&str]) -> RawCommit {
        RawCommit {
            id: id.to_string(),
            at,
            added: added.iter().map(|p| (*p).to_string()).collect(),
            changed: changed.iter().map(|p| (*p).to_string()).collect(),
        }
    }

    const DAY: i64 = 86_400;

    /// Uma pasta de comandos parecida com a de verdade: quatro comandos que
    /// importam o mesmo núcleo, um que não importa, um enorme e o teste de um
    /// deles.
    fn command_folder() -> ProjectMap {
        let dir = "apps/rt/src/commands/spec_events";
        let core = "packages/core/src/domain/spec_events.rs";
        let io = "packages/core/src/io/spec_events.rs";
        let mut read = module(&format!("{dir}/read.rs"), 120, &[core, io]);
        read.has_tests = true;
        let mut write = module(&format!("{dir}/write.rs"), 150, &[core, io]);
        write.tests = vec!["apps/rt/tests/spec_events_cli.rs".to_string()];
        let index = module(&format!("{dir}/index.rs"), 90, &[core, io]);
        let pages = module(&format!("{dir}/pages.rs"), 110, &[core]);
        let huge = module(&format!("{dir}/cli.rs"), 2000, &[core, io]);
        let lone = module(&format!("{dir}/mod.rs"), 60, &["packages/core/src/platform/i18n.rs"]);
        let test = module("apps/rt/tests/spec_events_cli.rs", 300, &[]);
        let history = History::from_raw(vec![
            commit("aaaa", DAY, &[&format!("{dir}/pages.rs")], &[]),
            commit("bbbb", 2 * DAY, &[&format!("{dir}/index.rs")], &["apps/rt/tests/run_command_surface.rs"]),
            commit(
                "cccc",
                3 * DAY,
                &[&format!("{dir}/read.rs")],
                &["apps/rt/tests/run_command_surface.rs", "packages/core/src/platform/i18n.rs"],
            ),
            commit("dddd", 4 * DAY, &[], &[&format!("{dir}/write.rs")]),
        ]);
        ProjectMap {
            modules: vec![read, write, index, pages, huge, lone, test],
            history,
            ..ProjectMap::default()
        }
    }

    #[test]
    fn examples_come_from_the_same_folder_with_the_same_imports_tested_and_recent_first() {
        let map = command_folder();
        let got = examples(&map, "apps/rt/src/commands/spec_events/map.rs", Locale::PtBr);
        assert_eq!(got.folder, "apps/rt/src/commands/spec_events");
        let paths: Vec<&str> = got.picks.iter().map(|p| p.path.as_str()).collect();
        assert!((2..=3).contains(&paths.len()), "{paths:?}");
        // Os testados vêm antes, e entre eles o mais recente primeiro.
        assert_eq!(paths[0], "apps/rt/src/commands/spec_events/write.rs", "{paths:?}");
        assert_eq!(paths[1], "apps/rt/src/commands/spec_events/read.rs", "{paths:?}");
        // O enorme passa do quartil de cima e o que não importa o núcleo fica
        // de fora; o teste nunca é exemplo.
        for gone in ["cli.rs", "mod.rs", "spec_events_cli.rs"] {
            assert!(!paths.iter().any(|p| p.ends_with(gone)), "{gone} in {paths:?}");
        }
        for pick in &got.picks {
            assert!(!pick.shared_imports.is_empty(), "{pick:?}");
            assert!(!pick.why.is_empty(), "every pick says why: {pick:?}");
        }
        assert!(got.picks[0].why.iter().any(|w| w.contains("mesma pasta")), "{:?}", got.picks[0].why);
        assert!(got.picks[0].why.iter().any(|w| w.contains("spec_events_cli.rs")), "{:?}", got.picks[0].why);
    }

    /// Uma árvore do tamanho da real: uma pasta de comandos com 24 arquivos que
    /// importam o mesmo núcleo em proporções diferentes, um índice de módulo
    /// curto que importa todos os irmãos, e uma pasta de ganchos com um arquivo
    /// pequeno que casa bem com "run" sozinho.
    fn real_sized_tree() -> ProjectMap {
        let folder = "apps/rt/src/commands/spec";
        let context = "apps/rt/src/shared/context.rs";
        let util = "apps/rt/src/util/mod.rs";
        let sections = "apps/rt/src/commands/spec/spec_sections.rs";
        let names: Vec<String> = (0..24).map(|i| format!("{folder}/cmd_{i:02}.rs")).collect();
        let mut modules = Vec::new();
        for (i, path) in names.iter().enumerate() {
            let mut deps = vec![context];
            if i % 2 == 0 {
                deps.push(util);
            }
            if i % 3 == 0 {
                deps.push(sections);
            }
            let mut m = module(path, 150 + (i * 13) % 200, &deps);
            m.declarations = vec![MapDecl { name: "run".to_string() }, MapDecl { name: format!("Cmd{i}Opts") }];
            m.has_tests = i % 4 == 0;
            modules.push(m);
        }
        let siblings: Vec<&str> = names.iter().map(String::as_str).collect();
        modules.push(module(&format!("{folder}/mod.rs"), 30, &siblings));
        modules.push(module(sections, 400, &[]));
        modules.push(module(context, 900, &[]));
        modules.push(module(util, 300, &[]));
        let mut hook = module("apps/rt/src/hooks/worktree_create.rs", 20, &[context]);
        hook.declarations = vec![MapDecl { name: "run".to_string() }, MapDecl { name: "command".to_string() }];
        modules.push(hook);
        modules.push(module("apps/rt/src/hooks/mod.rs", 10, &["apps/rt/src/hooks/worktree_create.rs"]));
        ProjectMap { modules, ..ProjectMap::default() }
    }

    #[test]
    fn a_task_in_a_real_sized_tree_lands_in_the_command_folder_with_its_main_imports() {
        let map = real_sized_tree();
        let folder = best_folder(&map, "adicionar um comando run").expect("a folder matches");
        assert!(folder.starts_with("apps/rt/src/commands/"), "the folder is summed, not the first hit: {folder}");
        let got = examples(&map, "apps/rt/src/commands/spec/novo.rs", Locale::PtBr);
        assert_eq!(
            got.main_imports.first().map(String::as_str),
            Some("apps/rt/src/shared/context.rs"),
            "the main imports come from the most alike files: {:?}",
            got.main_imports,
        );
        assert!((2..=3).contains(&got.picks.len()), "{:?}", got.picks);
        for pick in &got.picks {
            assert!(!pick.path.ends_with("mod.rs"), "a short module index is no example: {pick:?}");
            assert!(!pick.shared_imports.is_empty(), "{pick:?}");
        }
        assert!(got.picks[0].inline_tests, "tested first: {:?}", got.picks[0]);
    }

    #[test]
    fn the_recipe_lists_the_commits_that_created_a_file_of_the_same_kind_there() {
        let map = command_folder();
        let got = examples(&map, "apps/rt/src/commands/spec_events/map.rs", Locale::EnUs);
        let created: Vec<&str> = got.recipes.iter().map(|r| r.added.as_str()).collect();
        assert_eq!(
            created,
            vec![
                "apps/rt/src/commands/spec_events/read.rs",
                "apps/rt/src/commands/spec_events/index.rs",
                "apps/rt/src/commands/spec_events/pages.rs",
            ],
        );
        assert!(got.recipes[0].together.contains(&"apps/rt/tests/run_command_surface.rs".to_string()));
        assert_eq!(got.recipes[0].date, "1970-01-04");
    }

    #[test]
    fn without_history_the_examples_still_come_by_folder_imports_test_and_size() {
        let mut map = command_folder();
        map.history = History::default();
        let got = examples(&map, "apps/rt/src/commands/spec_events/", Locale::PtBr);
        assert!(got.recipes.is_empty());
        assert!((2..=3).contains(&got.picks.len()), "{:?}", got.picks);
        assert!(got.picks.iter().all(|p| p.last_change.is_none()));
    }

    #[test]
    fn importers_and_tests_answer_from_the_graph() {
        let map = command_folder();
        let who = importers(&map, "packages/core/src/io/spec_events.rs");
        assert_eq!(who, Err(MapRefusal::UnknownFile { file: "packages/core/src/io/spec_events.rs".to_string() }));
        let who = importers(&map, "apps/rt/tests/spec_events_cli.rs").unwrap();
        assert!(who.is_empty());
        let tests = tests_for(&map, "./apps/rt/src/commands/spec_events/write.rs").unwrap();
        assert_eq!(tests.files, vec!["apps/rt/tests/spec_events_cli.rs".to_string()]);
        assert!(!tests.inline);
        assert!(tests_for(&map, "apps/rt/src/commands/spec_events/read.rs").unwrap().inline);
    }

    #[test]
    fn a_portuguese_query_finds_a_portuguese_identifier() {
        let mut pay = module("src/pagamentos/processador_pagamento.rs", 40, &[]);
        pay.declarations = vec![MapDecl { name: "ProcessadorPagamento".to_string() }];
        let other = module("src/usuarios/cadastro.rs", 40, &[]);
        let map = ProjectMap { modules: vec![other, pay], ..ProjectMap::default() };
        let found = search(&map, "processar os pagamentos");
        assert_eq!(found.first().map(|f| f.path.as_str()), Some("src/pagamentos/processador_pagamento.rs"));
    }

    #[test]
    fn the_history_counts_commits_the_last_change_and_what_changes_together() {
        let history = History::from_raw(vec![
            commit("a1", 10, &["a.rs", "a_test.rs"], &[]),
            commit("a2", 20, &[], &["a.rs", "a_test.rs"]),
            commit("a3", 30, &[], &["a.rs", "b.rs"]),
        ]);
        let a = file_history(&history, "a.rs").unwrap();
        assert_eq!(a.commits, 3);
        assert_eq!((a.last_at, a.last_commit.as_str()), (30, "a3"));
        assert_eq!(a.together, vec![("a_test.rs".to_string(), 2), ("b.rs".to_string(), 1)]);
        assert!(file_history(&history, "c.rs").is_none());
        let stats = history_stats(&history);
        assert_eq!(stats.commits["a.rs"], 3);
        assert_eq!(stats.together["a.rs"]["a_test.rs"], 2);
        assert!(covers_by_history(2, 3));
        assert!(!covers_by_history(1, 1));
    }

    #[test]
    fn a_huge_commit_counts_but_links_nothing() {
        let files: Vec<String> = (0..=CO_CHANGE_MAX_FILES).map(|i| format!("f{i}.rs")).collect();
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let history = History::from_raw(vec![commit("big", 1, &[], &refs)]);
        let f0 = file_history(&history, "f0.rs").unwrap();
        assert_eq!(f0.commits, 1);
        assert!(f0.together.is_empty());
    }

    #[test]
    fn the_history_read_at_once_or_in_steps_is_the_same() {
        let all = vec![commit("1", 1, &["x.rs"], &[]), commit("2", 2, &["y.rs"], &["x.rs"]), commit("3", 3, &[], &["y.rs"])];
        let once = History::from_raw(all.clone());
        let steps = History::from_raw(all[..1].to_vec()).extended(all[1..].to_vec());
        assert_eq!(once, steps);
        assert_eq!(once.raw(), all);
    }

    #[test]
    fn the_session_summary_fits_in_three_kilobytes() {
        let modules: Vec<MapModule> =
            (0..5000).map(|i| module(&format!("apps/sub{}/src/{}/file_{i}.rs", i % 40, "x".repeat(60)), 10, &[])).collect();
        let projects: Vec<MapProject> = (0..40)
            .map(|i| MapProject {
                name: format!("sub{i}-{}", "n".repeat(80)),
                dir: format!("apps/sub{i}"),
                kind: "cargo".to_string(),
                code_files: 100 + i,
            })
            .collect();
        let top_fan_in = modules.iter().take(64).map(|m| MapDegree { module: m.path.clone(), degree: 3 }).collect();
        let history = History::from_raw(
            modules.iter().take(200).enumerate().map(|(i, m)| commit(&i.to_string(), i as i64, &[], &[&m.path])).collect(),
        );
        let map = ProjectMap {
            modules,
            projects,
            languages: vec![MapLanguage { language: "rust".to_string(), files: 5000, loc: 50_000 }],
            graph: MapGraph { top_fan_in },
            history,
            state: MapState::default(),
        };
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = summary(&map, lang);
            assert!(text.len() <= SUMMARY_MAX_BYTES, "{} bytes", text.len());
            assert!(text.contains("mustard-rt run map"), "the way to ask always fits: {text}");
            assert!(text.contains("5000"), "{text}");
        }
    }

    #[test]
    fn a_skill_citing_a_path_that_does_not_exist_is_refused() {
        let text = "Veja `apps/rt/src/commands/spec_events/write.rs`, `spec_events/mod.rs:12` e \
                    `apps/rt/src/commands/nao_existe.rs`; o modelo `packages/core/src/domain/<assunto>.rs` \
                    e `plugin/**` não contam, nem `pt-BR/en-US`.";
        assert_eq!(
            cited_paths(text),
            vec![
                "apps/rt/src/commands/nao_existe.rs".to_string(),
                "apps/rt/src/commands/spec_events/write.rs".to_string(),
                "spec_events/mod.rs".to_string(),
            ],
        );
        let map = ProjectMap {
            modules: vec![
                module("apps/rt/src/commands/spec_events/write.rs", 1, &[]),
                module("apps/rt/src/commands/spec_events/mod.rs", 1, &[]),
            ],
            ..ProjectMap::default()
        };
        let refused = check_skill(text, |p| map_knows(&map, p)).unwrap_err();
        assert_eq!(refused, MapRefusal::SkillMissingPaths { paths: vec!["apps/rt/src/commands/nao_existe.rs".to_string()] });
        assert_eq!(refused.reason(), "skill-missing-path");
        assert!(refused.message(Locale::PtBr).contains("nao_existe.rs"));
        assert!(refused.message(Locale::EnUs).contains("nao_existe.rs"));
        let long = "linha\n".repeat(SKILL_MAX_LINES + 1);
        assert_eq!(check_skill(&long, |_| true), Err(MapRefusal::SkillTooLong { lines: SKILL_MAX_LINES + 1 }));
        assert!(check_skill("`apps/rt/src/commands/spec_events/write.rs`", |p| map_knows(&map, p)).is_ok());
    }

    #[test]
    fn identifiers_split_into_their_words() {
        assert_eq!(split_identifier("ProcessadorPagamento"), "Processador Pagamento");
        assert_eq!(split_identifier("processador_pagamento"), "processador pagamento");
        assert_eq!(split_identifier("apps/rt/work-branch.rs"), "apps rt work branch rs");
    }
}
