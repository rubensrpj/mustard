//! `project_map` — o mapa do projeto que o scan grava, e as perguntas curtas
//! que se fazem a ele.
//!
//! O scan grava o mapa em `.claude/grain.db`: cada arquivo de código
//! com as importações resolvidas (`deps`), os testes que o cobrem (`tests`) e
//! o histórico do git (`history`). Ninguém lê o arquivo inteiro: quem precisa
//! pergunta, e recebe uma resposta curta:
//!
//! - [`examples`]: 2 ou 3 arquivos que servem de exemplo para uma tarefa, com
//!   o motivo de cada um;
//! - [`importers`]: quem importa um arquivo;
//! - [`tests_for`]: que testes cobrem um arquivo;
//! - [`declaration`] com [`lines_of`]: o trecho de uma declaração, do começo
//!   ao fim, sem que quem pergunta abra o arquivo;
//! - [`users`]: quem usa uma declaração pelo nome, em que arquivo e linha;
//! - [`search`]: a busca por conceito, com a mesma preparação de texto e o
//!   mesmo BM25 das lições e das specs;
//! - [`summary`]: o resumo do mapa do projeto, até 3 kB;
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
use crate::domain::pattern::Pattern;
pub use crate::domain::search::{Found, FoundText};
use crate::platform::i18n::{Locale, translate};

mod example_material;
use example_material::is_example_material;

/// Um commit que muda mais arquivos do que isto não conta para "muda junto":
/// é formatação, renomeação em massa ou importação, e ligaria tudo a tudo.
pub const CO_CHANGE_MAX_FILES: usize = 30;

/// Quantos commits o mapa guarda, dos mais novos.
pub const MAX_COMMITS: usize = 5000;

/// Quantos arquivos que mudam junto a resposta mostra.
pub const TOGETHER_SHOWN: usize = 5;

/// Quantos títulos de commit do arquivo a resposta mostra, dos mais novos.
pub const TITLES_SHOWN: usize = 3;

/// O tamanho máximo do resumo do mapa, em bytes.
pub const SUMMARY_MAX_BYTES: usize = 3 * 1024;

/// O tamanho máximo de uma skill, em linhas.
pub const SKILL_MAX_LINES: usize = 500;

// ---------------------------------------------------------------------------
// Histórico do git
// ---------------------------------------------------------------------------

/// O histórico do git guardado no mapa: o da branch de partida do projeto,
/// nunca o da branch em que se está. Os caminhos numa tabela só, em ordem de
/// nome, e os commits do mais antigo para o mais novo, apontando a tabela.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    /// A branch de partida de onde os commits vêm; vazia quando o projeto
    /// não declara uma.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub base: String,
    /// Por que não há história, quando o motivo é da configuração ou do
    /// clone, e não da falta de commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<NoHistory>,
    pub paths: Vec<String>,
    pub commits: Vec<Commit>,
}

/// Por que o mapa ficou sem a história do git.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoHistory {
    /// O projeto não diz qual é a branch de partida (`git.flow` no
    /// `mustard.json`).
    NoBase,
    /// A branch de partida declarada não existe no clone, nem a local nem a
    /// do servidor.
    BaseNotFound,
}

/// Um commit guardado: o começo do hash, a data (segundos desde 1970), o
/// título, o número do pull request que o trouxe, quando o git o diz, e os
/// arquivos que ele criou e os que ele mudou, pelo número na tabela.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Commit {
    pub id: String,
    pub at: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u32>,
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
    pub title: String,
    pub pr: Option<u32>,
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
    /// Monta o histórico a partir dos commits, do mais antigo para o mais
    /// novo. Ficam só os [`MAX_COMMITS`] mais novos, e a tabela de caminhos
    /// sai em ordem de nome, com os caminhos que os commits guardados citam:
    /// o mesmo histórico dá sempre os mesmos bytes, lido de uma vez ou aos
    /// poucos.
    #[must_use]
    pub fn from_raw(commits: Vec<RawCommit>) -> Self {
        let skip = commits.len().saturating_sub(MAX_COMMITS);
        let commits: Vec<RawCommit> = commits.into_iter().skip(skip).collect();
        let paths: Vec<String> = commits.iter().flat_map(RawCommit::files).map(str::to_string).collect::<BTreeSet<_>>().into_iter().collect();
        let index: BTreeMap<&str, u32> = paths.iter().enumerate().map(|(i, p)| (p.as_str(), u32::try_from(i).unwrap_or(u32::MAX))).collect();
        let numbers = |list: &[String]| -> Vec<u32> {
            let mut out: Vec<u32> = list.iter().filter_map(|p| index.get(p.as_str()).copied()).collect();
            out.sort_unstable();
            out.dedup();
            out
        };
        let commits = commits
            .iter()
            .map(|c| Commit { id: c.id.clone(), at: c.at, title: c.title.clone(), pr: c.pr, added: numbers(&c.added), changed: numbers(&c.changed) })
            .collect();
        Self { paths, commits, ..Self::default() }
    }

    /// Os commits com os caminhos por extenso, do mais antigo para o mais
    /// novo.
    #[must_use]
    pub fn raw(&self) -> Vec<RawCommit> {
        let name = |list: &[u32]| -> Vec<String> { list.iter().filter_map(|&i| self.paths.get(i as usize)).cloned().collect() };
        self.commits
            .iter()
            .map(|c| RawCommit { id: c.id.clone(), at: c.at, title: c.title.clone(), pr: c.pr, added: name(&c.added), changed: name(&c.changed) })
            .collect()
    }

    /// O histórico com `newer` acrescentado no fim, da mesma branch.
    #[must_use]
    pub fn extended(&self, newer: Vec<RawCommit>) -> Self {
        let mut all = self.raw();
        all.extend(newer);
        Self { base: self.base.clone(), ..Self::from_raw(all) }
    }
}

/// Por que o mapa está sem a história do git, dito para quem pergunta, com o
/// jeito de ter a história; `None` quando o motivo não é da configuração nem
/// do clone.
#[must_use]
pub fn history_note(history: &History, lang: Locale) -> Option<String> {
    let key = match history.missing? {
        NoHistory::NoBase => "map.history.no_base",
        NoHistory::BaseNotFound => "map.history.base_not_found",
    };
    Some(translate(key, lang).replace("{base}", &history.base))
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
    /// Os títulos dos commits mais novos que o citam, até [`TITLES_SHOWN`],
    /// do mais novo para o mais antigo. O commit guardado sem título fica de
    /// fora.
    pub titles: Vec<String>,
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
    let mut ranked: Vec<(String, u32)> = together.into_iter().filter_map(|(i, n)| history.paths.get(i as usize).map(|p| (p.clone(), n))).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(TOGETHER_SHOWN);
    out.together = ranked;
    out.titles = history
        .commits
        .iter()
        .rev()
        .filter(|commit| !commit.title.is_empty())
        .filter(|commit| commit.added.binary_search(&index).is_ok() || commit.changed.binary_search(&index).is_ok())
        .take(TITLES_SHOWN)
        .map(|commit| commit.title.clone())
        .collect();
    Some(out)
}

// ---------------------------------------------------------------------------
// A história de cada declaração
// ---------------------------------------------------------------------------

/// Quantos commits de uma declaração a resposta mostra, dos mais novos,
/// quando o projeto não escreve outro número em `map.historyCommits`.
/// Medido no Mustard, numa função de 26 commits: 5, 10 e 20 dão respostas de 872, 1.256 e 2.048 caracteres.
pub const DECL_COMMITS_SHOWN: usize = 10;

/// Quantas vezes seguidas a história de uma declaração segue para o arquivo
/// de onde ela veio, quando o projeto não escreve outro número em
/// `map.historyMoves`.
/// Medido no Mustard, em três funções (uma movida): 3, 5 e 10 acham os mesmos commits no mesmo tempo, e 5 guarda folga.
pub const MOVES_FOLLOWED: usize = 5;

/// A história das declarações de um arquivo na branch de partida, lida do git
/// na primeira pergunta sobre ele e guardada no mapa: a base, o commit mais
/// novo do arquivo nela quando se leu, a marca do scan que leu, os commits
/// lidos e, de cada declaração, os commits que a mudaram. Nenhum trecho nem
/// código antigo fica guardado.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileLineage {
    pub path: String,
    pub base: String,
    /// O começo do hash do commit mais novo do arquivo na base, quando a
    /// história foi lida.
    pub last_commit: String,
    /// O hash inteiro do commit da ponta da base em que a lista foi lida: a
    /// leitura seguinte parte daí e lê só o que veio depois. Vazio quando a
    /// lista não guarda de onde veio, e então ela se lê inteira de novo.
    pub tip: String,
    /// A marca do scan que leu ([`lineage_is_fresh`]).
    pub mark: String,
    /// Quantas vezes seguidas a leitura podia seguir a declaração para o
    /// arquivo de onde ela veio.
    pub moves: u32,
    /// Quantos comentários de revisão presos ao arquivo o mapa tinha quando a
    /// lista se montou ([`lineage_is_fresh`]).
    pub comments: u32,
    /// Os commits lidos, do mais novo ao mais velho.
    pub commits: Vec<LineageCommit>,
    pub declarations: Vec<DeclLineage>,
}

/// Um commit lido na história de um arquivo: o começo do hash, a data
/// (segundos desde 1970), o título, o número do pull request que o trouxe e
/// os arquivos que ele criou e mudou, o próprio incluído.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineageCommit {
    pub id: String,
    pub at: i64,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u32>,
    /// Vazio no commit que muda mais de [`CO_CHANGE_MAX_FILES`] arquivos:
    /// ele não conta para "muda junto".
    #[serde(skip_serializing_if = "CommitFiles::is_empty")]
    pub files: CommitFiles,
}

/// Os arquivos que um commit criou e os que ele mudou.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitFiles {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<String>,
}

impl CommitFiles {
    /// Nenhum arquivo guardado.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty()
    }
}

/// Uma declaração do arquivo, pelo nome e pela ordem entre as de mesmo nome
/// no arquivo (a primeira é 0), com os commits que a mudaram, do mais novo
/// ao mais velho.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeclLineage {
    pub name: String,
    pub nth: u32,
    pub commits: Vec<DeclChange>,
    /// Os comentários de revisão presos às linhas dela, na versão do commit
    /// comentado.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<DeclComment>,
}

/// Um comentário de revisão preso às linhas de uma declaração: o pull
/// request, o commit comentado e o texto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeclComment {
    pub pr: u32,
    pub commit: String,
    pub body: String,
}

/// Um commit que mudou uma declaração; `form` quando a mudança foi só de
/// forma (espaços, ou um commit que o projeto manda o `git blame` ignorar).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeclChange {
    pub id: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub form: bool,
}

/// A história guardada das declarações de um arquivo ainda vale: é da mesma
/// branch de partida da história do mapa, foi lida pela versão do scan que
/// gravou o censo, seguindo as mesmas `moves` mudanças de arquivo que a
/// pergunta pede agora, e o commit mais novo do arquivo na história do mapa é
/// o que ela registrou — ou o arquivo não tem nenhum lá. O commit novo da
/// base que a montagem soma muda esse commit só nos arquivos que ele tocou;
/// os outros seguem valendo. A base trocada, ou reescrita debaixo do arquivo,
/// pede a leitura de novo.
///
/// Também vence quando o mapa ganhou comentário de revisão preso ao arquivo
/// depois que ela se montou: a pergunta seguinte a refaz, e o comentário
/// novo se liga à declaração dele.
#[must_use]
pub fn lineage_is_fresh(lineage: &FileLineage, map: &ProjectMap, moves: usize) -> bool {
    let comments = map.pulls.comments.iter().filter(|comment| comment.path == lineage.path).count();
    lineage_fresh_in(lineage, &map.history, &map.census_mark, comments, moves)
}

/// A mesma conta de [`lineage_is_fresh`], com as partes do mapa lidas à
/// parte: a história do git, a marca do censo e quantos comentários de
/// revisão presos ao arquivo o mapa tem.
#[must_use]
pub fn lineage_fresh_in(lineage: &FileLineage, history: &History, census_mark: &str, comments: usize, moves: usize) -> bool {
    lineage.base == history.base
        && lineage.mark == census_mark
        && usize::try_from(lineage.moves).is_ok_and(|read| read == moves)
        && usize::try_from(lineage.comments).is_ok_and(|read| read == comments)
        && file_history(history, &lineage.path).is_none_or(|file| file.last_commit == lineage.last_commit)
}

/// Por que a pergunta da história de uma declaração não tem resposta, dito
/// para quem pergunta: o projeto sem branch de partida, ou a que o clone não
/// tem. `None` quando há base.
#[must_use]
pub fn history_missing(history: &History, lang: Locale) -> Option<String> {
    history_note(history, lang).or_else(|| history.base.is_empty().then(|| translate("map.history.no_base", lang).to_string()))
}

/// Os arquivos que declaram `name`, em ordem de caminho: só `file`, quando
/// ele vem. As recusas são as de [`users`].
pub fn declaring_files(map: &ProjectMap, file: Option<&str>, name: &str) -> Result<Vec<String>, MapRefusal> {
    let mut files: Vec<String> = named_in(map, file, name)?.into_iter().map(|(m, _)| m.path.clone()).collect();
    files.dedup();
    Ok(files)
}

/// Um commit da história de uma declaração, como a resposta o mostra.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShownCommit {
    pub id: String,
    pub at: i64,
    /// O título sem o prefixo do tipo (`fix(...)`, `feat:`) e sem o número
    /// do pull request no fim.
    pub title: String,
    pub pr: Option<u32>,
    pub form: bool,
    /// O item combinado de uma spec que a onda deste commit cumpriu, quando
    /// o mapa guarda a spec.
    pub spec: Option<SpecNote>,
}

/// O item de uma spec ligado a um commit da onda que o cumpriu: a spec, o
/// código do item e a primeira frase da parte do usuário, até
/// [`SPEC_SENTENCE_CHARS`] caracteres.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecNote {
    pub spec: String,
    pub code: String,
    pub sentence: String,
}

/// Quantos caracteres da primeira frase de um item de spec a história e a
/// busca mostram.
pub const SPEC_SENTENCE_CHARS: usize = 200;

/// A primeira frase de `text`, até [`SPEC_SENTENCE_CHARS`] caracteres.
#[must_use]
pub fn spec_sentence(text: &str) -> String {
    crate::domain::spec_index::cut(crate::domain::spec_index::first_sentence(text), SPEC_SENTENCE_CHARS)
}

/// A história de uma declaração na base: onde ela mora, quantos commits a
/// mudaram fora os de forma e os mais novos deles, até o número pedido
/// ([`DECL_COMMITS_SHOWN`] sem outro), do mais novo ao mais velho. Sem
/// nenhum, a base ainda não tem commit dela.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclHistory {
    pub file: String,
    pub line: u64,
    pub changes: usize,
    pub commits: Vec<ShownCommit>,
    /// Os pull requests dos commits mostrados, cada um uma vez, na ordem em
    /// que aparecem, com o título que o servidor deu; sem título os que o
    /// mapa ainda não leu.
    pub pulls: Vec<(u32, String)>,
    /// Os comentários de revisão presos às linhas da declaração, cada um com
    /// o pull request e até [`COMMENT_CHARS`] caracteres do texto.
    pub comments: Vec<(u32, String)>,
}

/// Quantos caracteres de um comentário de revisão a resposta da história
/// mostra.
pub const COMMENT_CHARS: usize = 200;

/// Quantos caracteres do primeiro parágrafo da descrição de um pull request
/// a resposta mostra.
pub const DESCRIPTION_CHARS: usize = 600;

/// A história de cada declaração `name` de `file`, pela história guardada
/// do arquivo, com os `shown` commits mais novos de cada uma. As declarações
/// de mesmo nome se casam pela ordem no arquivo. As recusas são as de
/// [`users`].
pub fn decl_history(map: &ProjectMap, file: &str, name: &str, shown: usize) -> Result<Vec<DeclHistory>, MapRefusal> {
    let found = named_in(map, Some(file), name)?;
    let lineage = map.lineage.iter().find(|lineage| found.first().is_some_and(|(m, _)| m.path == lineage.path));
    let commits: BTreeMap<&str, &LineageCommit> = lineage.map(|l| l.commits.iter().map(|c| (c.id.as_str(), c)).collect()).unwrap_or_default();
    // O número que o provedor deu ao commit sem número no título.
    let asked: BTreeMap<&str, u32> = map.pulls.commits.iter().filter(|c| c.pr > 0).map(|c| (c.id.as_str(), c.pr)).collect();
    let titles: BTreeMap<u32, &str> = map.pulls.texts.iter().map(|t| (t.number, t.title.as_str())).collect();
    Ok(found
        .iter()
        .enumerate()
        .map(|(nth, (module, decl))| {
            let declared = lineage.and_then(|l| l.declarations.iter().find(|d| d.name == decl.name && d.nth as usize == nth));
            let changes: &[DeclChange] = declared.map_or(&[], |d| d.commits.as_slice());
            let shown_commits: Vec<ShownCommit> = changes
                .iter()
                .take(shown)
                .map(|change| {
                    let commit = commits.get(change.id.as_str());
                    ShownCommit {
                        id: change.id.clone(),
                        at: commit.map_or(0, |c| c.at),
                        title: commit.map(|c| clean_title(&c.title)).unwrap_or_default(),
                        pr: commit.and_then(|c| c.pr).or_else(|| asked.get(change.id.as_str()).copied()),
                        form: change.form,
                        spec: map.spec_notes.get(change.id.as_str()).cloned(),
                    }
                })
                .collect();
            let mut pulls: Vec<(u32, String)> = Vec::new();
            for number in shown_commits.iter().filter_map(|c| c.pr) {
                if !pulls.iter().any(|(seen, _)| *seen == number) {
                    pulls.push((number, titles.get(&number).map(|t| t.trim().to_string()).unwrap_or_default()));
                }
            }
            let comments =
                declared.map(|d| d.comments.iter().map(|c| (c.pr, crate::domain::spec_index::cut(c.body.trim(), COMMENT_CHARS))).collect()).unwrap_or_default();
            DeclHistory {
                file: module.path.clone(),
                line: decl.line,
                changes: changes.iter().filter(|c| !c.form).count(),
                commits: shown_commits,
                pulls,
                comments,
            }
        })
        .collect())
}

/// O título e o primeiro parágrafo da descrição do pull request `number`,
/// até [`DESCRIPTION_CHARS`] caracteres; `None` quando o mapa ainda não tem
/// o texto dele ou quando o provedor não achou o número.
#[must_use]
pub fn pull_description(map: &ProjectMap, number: u32) -> Option<(String, String)> {
    let text = map.pulls.texts.iter().find(|text| text.number == number && !text.title.trim().is_empty())?;
    let body = text.body.replace("\r\n", "\n");
    let paragraph = body.trim().split("\n\n").next().unwrap_or_default().trim();
    Some((text.title.trim().to_string(), crate::domain::spec_index::cut(paragraph, DESCRIPTION_CHARS)))
}

/// Uma linha da história de uma declaração: a data, o começo do hash, o
/// título e o número do pull request, com a marca do commit só de forma e,
/// no commit de uma onda, o item da spec que ela cumpriu.
#[must_use]
pub fn history_line(commit: &ShownCommit, lang: Locale) -> String {
    let mut line = format!("{} {} {}", date_of(commit.at), commit.id, commit.title);
    if let Some(pr) = commit.pr {
        line.push_str(" #");
        line.push_str(&pr.to_string());
    }
    if commit.form {
        line.push(' ');
        line.push_str(translate("map.history.form", lang));
    }
    if let Some(note) = &commit.spec {
        line.push_str(" — ");
        line.push_str(&translate("map.history.spec", lang).replace("{spec}", &note.spec).replace("{code}", &note.code).replace("{sentence}", &note.sentence));
    }
    line
}

/// O título de um commit sem o prefixo do tipo — `fix(escopo): `, `feat!: `
/// — e sem o número do pull request escrito no fim, `(#12)`.
#[must_use]
pub fn clean_title(title: &str) -> String {
    let mut title = title.trim();
    if let Some((kind, rest)) = title.split_once(": ") {
        let head = kind.strip_suffix('!').unwrap_or(kind);
        let (word, scope) = head.split_once('(').map_or((head, None), |(word, scope)| (word, Some(scope)));
        let scoped = scope.is_none_or(|scope| scope.ends_with(')') && !scope[..scope.len() - 1].contains(['(', ')']));
        if !word.is_empty() && word.chars().all(|c| c.is_ascii_alphabetic()) && scoped {
            title = rest.trim_start();
        }
    }
    if let Some((before, number)) = title.strip_suffix(')').and_then(|t| t.rsplit_once("(#"))
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
    {
        title = before.trim_end();
    }
    title.to_string()
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
        together: together.into_iter().map(|(i, others)| (name(i), others.into_iter().map(|(o, n)| (name(o), n)).collect())).collect(),
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectMap {
    pub modules: Vec<MapModule>,
    pub resources: Vec<crate::domain::knowledge::resources::File>,
    pub projects: Vec<MapProject>,
    pub languages: Vec<MapLanguage>,
    pub graph: MapGraph,
    pub history: History,
    pub state: MapState,
    /// A camada da arquitetura de cada pasta, como o scan a grava.
    pub skeleton: Vec<MapSkeleton>,
    /// A história das declarações dos arquivos que alguém já perguntou, que
    /// o mapa guarda à parte do que a montagem grava.
    pub lineage: Vec<FileLineage>,
    /// A marca da versão do scan que gravou o censo; vazia no mapa escrito
    /// à mão.
    #[serde(skip)]
    pub census_mark: String,
    /// O que o servidor disse dos pull requests da base, guardado à parte
    /// do que a montagem grava.
    #[serde(skip)]
    pub pulls: Pulls,
    /// O item de spec de cada commit de onda da história guardada, e do
    /// commit da base que o pull request de uma spec trouxe por squash ou
    /// rebase, pelo começo do hash, como o bloco das specs o liga.
    #[serde(skip)]
    pub spec_notes: BTreeMap<String, SpecNote>,
}

/// O que o mapa guarda do servidor sobre os pull requests da base, lido uma
/// vez depois que o commit entra nela: o texto de cada um, os comentários de
/// revisão presos a linhas e o número do pull request de cada commit que não
/// o diz no título.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pulls {
    pub texts: Vec<PullText>,
    pub comments: Vec<PullComment>,
    pub commits: Vec<PullOfCommit>,
}

/// O texto de um pull request: o título, a descrição, a marca de versão com
/// que o servidor o deu (vazia no provedor que não a dá) e o commit mais novo
/// da base que o citava quando ele foi lido.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullText {
    pub number: u32,
    pub title: String,
    pub body: String,
    pub etag: String,
    pub through: String,
}

/// Um comentário de revisão preso a uma linha: o pull request, o commit
/// comentado (o hash inteiro), o arquivo e a linha nessa versão, e o texto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullComment {
    pub number: u32,
    pub commit: String,
    pub path: String,
    pub line: u64,
    pub body: String,
}

/// O pull request de um commit da base que não diz o número no título, como
/// o provedor respondeu: 0 quando ele não achou nenhum.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullOfCommit {
    pub id: String,
    pub pr: u32,
}

/// A última linha do trecho de teste que cobre o arquivo inteiro, o do arquivo
/// que um módulo declara como teste: o trecho começa na linha 1 e termina aqui,
/// além de qualquer arquivo. Ele diz o que é do teste, e não onde os testes
/// começam.
pub const WHOLE_FILE_END: u64 = u32::MAX as u64;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapModule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<serde_json::Value>,
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
    /// As linhas, da primeira à última, de cada trecho de teste escrito
    /// dentro do arquivo, como o scan o reconhece. O arquivo que um módulo
    /// declara como teste traz um trecho que vai da linha 1 a
    /// [`WHOLE_FILE_END`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub test_lines: Vec<(u64, u64)>,
    /// As rotas do servidor registradas no arquivo, com as chamadas da tela
    /// que alcançam cada uma.
    pub routes: Vec<MapRoute>,
    /// As medidas de qualidade do arquivo, como o scan as mede.
    #[serde(skip_serializing_if = "Quality::is_empty")]
    pub quality: Quality,
}

// ---------------------------------------------------------------------------
// A qualidade de cada arquivo
// ---------------------------------------------------------------------------

/// As medidas de qualidade de um arquivo escrito à mão, fora os de teste: o
/// tamanho e o de cada função, as importações, as linhas repetidas em outro
/// arquivo e a participação num ciclo de importações. Só informam: o corte
/// de "grande" e de "repetido" é relativo ao próprio projeto
/// ([`QualityCuts`]), e nenhuma medida recusa trabalho.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Quality {
    /// As linhas não vazias do arquivo, fora os trechos de teste escritos
    /// dentro dele.
    pub size: usize,
    /// Quantas importações o arquivo escreve, fora as do trecho de teste.
    pub imports: usize,
    /// As linhas que caem numa janela de linhas seguidas igual à de outro
    /// arquivo do projeto ([`REPEATED_WINDOW`]).
    #[serde(skip_serializing_if = "is_zero")]
    pub repeated: usize,
    /// O arquivo está num ciclo de importações.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cycle: bool,
    /// Cada função fora dos trechos de teste: a linha em que começa e as
    /// linhas não vazias dela.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<(u64, usize)>,
}

impl Quality {
    /// Nenhuma medida: o arquivo que o scan não mede, ou o mapa escrito à
    /// mão.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Quantas linhas seguidas, iguais em dois arquivos, contam como repetição.
pub const REPEATED_WINDOW: usize = 10;

/// A parte de cima dos arquivos do projeto, em porcentagem, que conta como
/// "grande" ou "repetido". O corte sai da posição entre os arquivos do
/// próprio projeto, nunca de um teto fixo em linhas.
pub const QUALITY_TOP_PERCENT: usize = 5;

/// Os cortes de "grande" e de "repetido" de um projeto: o valor do primeiro
/// arquivo logo abaixo dos [`QUALITY_TOP_PERCENT`] de cima, entre os escritos
/// à mão que não são teste, pelo caminho ou por um módulo que o declare.
/// Passa do corte só quem tem mais do que ele: o
/// empate com o resto nunca conta, e o projeto com menos de 20 arquivos não
/// tem nenhum acima.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityCuts {
    pub size: usize,
    pub repeated: usize,
}

impl QualityCuts {
    /// Os cortes medidos nos arquivos `modules`.
    #[must_use]
    pub fn of(modules: &[MapModule]) -> Self {
        let measured: Vec<&Quality> = modules.iter().filter(|m| is_example_material(m)).map(|m| &m.quality).collect();
        Self { size: top_cut(measured.iter().map(|q| q.size).collect()), repeated: top_cut(measured.iter().map(|q| q.repeated).collect()) }
    }

    /// O arquivo passa do corte de tamanho.
    #[must_use]
    pub fn large(&self, m: &MapModule) -> bool {
        m.quality.size > self.size
    }

    /// O arquivo passa do corte de repetição.
    #[must_use]
    pub fn repeated(&self, m: &MapModule) -> bool {
        m.quality.repeated > self.repeated
    }
}

/// O valor logo abaixo dos [`QUALITY_TOP_PERCENT`] de cima de `values`; o
/// maior possível quando a parte de cima não tem nenhum arquivo.
fn top_cut(mut values: Vec<usize>) -> usize {
    let top = values.len() * QUALITY_TOP_PERCENT / 100;
    if top == 0 {
        return usize::MAX;
    }
    values.sort_unstable_by(|a, b| b.cmp(a));
    values.get(top).copied().unwrap_or(0)
}

/// Uma rota do servidor como as perguntas a leem: o método HTTP, o caminho
/// padronizado, o nome da função que a atende e a linha dela, e as chamadas
/// da tela que a alcançam, cada uma como um uso, provado ou suspeito
/// ([`UseSite`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapRoute {
    pub method: String,
    pub path: String,
    pub handler: String,
    pub line: u64,
    pub called_by: Vec<UseSite>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapDecl {
    /// O tipo da declaração, como o scan o grava: `function`, `struct`…
    pub kind: String,
    pub name: String,
    /// A linha em que a declaração começa.
    pub line: u64,
    /// A linha em que a declaração termina. `0` num mapa antigo, sem o
    /// campo, ou quando o scan não resolveu — nesses casos a leitura
    /// obrigatória de uma tarefa cita só o nome, sem linha.
    pub end_line: u64,
    /// O comentário de documentação escrito em cima da declaração, sem as
    /// marcas de comentário. Vazio quando não há um ali.
    pub doc: String,
    /// A assinatura da declaração, sem o corpo. Vazia quando o scan não
    /// gravou uma.
    pub signature: String,
    /// Cada uso da declaração no projeto: o arquivo, a linha e a declaração
    /// de onde parte a chamada, provado ou suspeito. Vazio num mapa antigo,
    /// sem o campo.
    pub used_by: Vec<UseSite>,
    /// Quantas chamadas pelo nome da declaração ficaram sem ligação porque o
    /// nome é comum demais: cada uma podia alcançar mais declarações que o
    /// teto do scan.
    pub common_calls: usize,
    /// Os donos da declaração, do mais interno para o mais externo: as
    /// declarações do mesmo arquivo cuja faixa contém a dela e, depois, o
    /// tipo escrito fora dela (o do bloco `impl` do Rust, o receptor do
    /// método do Go). Só os nomes.
    pub owner: Vec<String>,
    /// O contrato que a declaração cumpre por onde foi escrita: o traço de
    /// `impl Traço for Tipo`. Só os nomes.
    pub contract: Vec<String>,
    /// Num tipo, as declarações que o têm como dono mais interno, os métodos
    /// primeiro.
    pub members: Vec<DeclAt>,
    /// Num método, o método de mesmo nome do contrato que ele cumpre.
    pub implements: Vec<DeclAt>,
    /// Num método de contrato, os métodos que o cumprem.
    pub implemented_by: Vec<DeclAt>,
}

/// Uma declaração apontada por uma ligação do mapa: o arquivo, a linha em que
/// ela começa e o nome dela. O scan a grava num texto só,
/// `arquivo:linha:nome`, como grava o uso.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclAt {
    pub file: String,
    pub line: usize,
    pub name: String,
}

impl Serialize for DeclAt {
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        out.collect_str(&format_args!("{}:{}:{}", self.file, self.line, self.name))
    }
}

impl<'de> Deserialize<'de> for DeclAt {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let text = String::deserialize(input)?;
        let wrong = || D::Error::custom(format!("a declaration reads `file:line:name`, not `{text}`"));
        let (head, name) = text.rsplit_once(':').ok_or_else(wrong)?;
        let (file, line) = head.rsplit_once(':').ok_or_else(wrong)?;
        Ok(Self { file: file.to_string(), line: line.parse().map_err(|_| wrong())?, name: name.to_string() })
    }
}

/// Um uso de uma declaração: o arquivo em que a chamada está escrita, a linha
/// e a declaração de onde ela parte (vazia quando a chamada fica fora de toda
/// declaração). É a ligação com nome: quem chama quem, e onde.
///
/// A ligação é provada quando `candidates` fica vazia: a chamada só alcança
/// esta declaração. Na suspeita, `candidates` traz cada declaração que a
/// chamada pode alcançar, esta inclusa, e só quem lê o código com o tipo de
/// cada valor decide qual.
///
/// O scan grava o uso provado num texto só, `arquivo:linha:quem`
/// (`arquivo:linha` quando a chamada fica fora de toda declaração), o mesmo
/// `arquivo:linha` que um compilador imprime; o suspeito, como
/// `{"at": "arquivo:linha:quem", "candidates": ["arquivo:linha:nome", …]}`.
/// O mapa antigo, só com textos, se lê inteiro como provado. O tipo mora
/// aqui, e o scan o reexporta: quem grava o mapa e quem responde a partir
/// dele leem o mesmo texto do mesmo jeito.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct UseSite {
    pub file: String,
    pub line: usize,
    pub from: String,
    /// Vazia na ligação provada; na suspeita, as declarações que a chamada
    /// pode alcançar.
    pub candidates: Vec<DeclAt>,
}

impl UseSite {
    /// A chamada só alcança a declaração que guarda este uso.
    #[must_use]
    pub fn is_proven(&self) -> bool {
        self.candidates.is_empty()
    }

    /// O lugar do uso como o scan o grava: `arquivo:linha:quem`, ou
    /// `arquivo:linha` fora de toda declaração.
    #[must_use]
    pub fn place(&self) -> String {
        if self.from.is_empty() { format!("{}:{}", self.file, self.line) } else { format!("{}:{}:{}", self.file, self.line, self.from) }
    }

    /// O uso lido de `arquivo:linha[:quem]`, sem candidatas. `None` quando o
    /// texto não tem essa forma.
    fn parse_place(text: &str) -> Option<Self> {
        let (head, tail) = text.rsplit_once(':')?;
        // `arquivo:linha` ou `arquivo:linha:quem`: quem diz qual dos dois é o
        // número da linha, que é sempre a última parte que é um número.
        Some(match tail.parse() {
            Ok(line) => Self { file: head.to_string(), line, ..Self::default() },
            Err(_) => {
                let (file, line) = head.rsplit_once(':')?;
                Self { file: file.to_string(), line: line.parse().ok()?, from: tail.to_string(), ..Self::default() }
            }
        })
    }
}

/// O uso suspeito como o scan o grava: o lugar e as candidatas.
#[derive(Serialize, Deserialize)]
struct SuspectUse {
    at: String,
    candidates: Vec<DeclAt>,
}

impl Serialize for UseSite {
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        if self.is_proven() { out.collect_str(&self.place()) } else { SuspectUse { at: self.place(), candidates: self.candidates.clone() }.serialize(out) }
    }
}

impl<'de> Deserialize<'de> for UseSite {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        /// As duas formas gravadas: o texto do uso provado e o objeto do
        /// suspeito.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Proven(String),
            Suspect(SuspectUse),
        }
        let (text, candidates) = match Written::deserialize(input)? {
            Written::Proven(text) => (text, Vec::new()),
            Written::Suspect(SuspectUse { at, candidates }) => (at, candidates),
        };
        let place = Self::parse_place(&text).ok_or_else(|| D::Error::custom(format!("a use reads `file:line[:from]`, not `{text}`")))?;
        Ok(Self { candidates, ..place })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapProject {
    pub name: String,
    pub dir: String,
    pub kind: String,
    pub code_files: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapLanguage {
    pub language: String,
    pub files: usize,
    pub loc: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapGraph {
    pub top_fan_in: Vec<MapDegree>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapDegree {
    pub module: String,
    pub degree: usize,
}

/// A camada da arquitetura (`L0`, `L1`, `L2`) que o scan dá a uma pasta.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSkeleton {
    pub dir: String,
    pub role: String,
}

/// De onde o mapa foi lido: o commit da última passada.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Blocos que a pergunta lê voltaram vazios numa troca de formato, e o
    /// scan ainda não os encheu de novo: `blocks` traz o nome de cada um.
    MapUnfilled { blocks: Vec<String> },
    /// O arquivo perguntado não está no mapa.
    UnknownFile { file: String },
    /// A pergunta precisa de uma opção que não veio.
    MissingArgument { question: String, flag: String },
    /// A skill cita caminhos que não existem.
    SkillMissingPaths { paths: Vec<String> },
    /// A skill passa do limite de linhas.
    SkillTooLong { lines: usize },
    /// O arquivo está no mapa e não tem uma declaração com esse nome; sem
    /// arquivo, o mapa inteiro não tem.
    UnknownDeclaration { file: Option<String>, name: String },
    /// O arquivo está no mapa e não pôde ser lido do disco.
    FileUnreadable { file: String, detail: String },
    /// Numa cópia de trabalho, a declaração mudou depois do mapa, que tem a
    /// do projeto a partir da linha `line`: o trecho dela na cópia não se
    /// acha pelas linhas do mapa. `copy` traz o começo e o fim da faixa que
    /// ela ocupa na cópia, quando o casamento das linhas a acha, para quem
    /// lê o arquivo por linhas; sem ela, a recusa só diz a linha do mapa.
    ChangedInCopy { file: String, name: String, line: u64, copy: Option<(u64, u64)> },
    /// A história das declarações do arquivo não pôde ser lida do git.
    HistoryUnreadable { file: String, detail: String },
}

impl MapRefusal {
    /// A razão curta e estável da recusa.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::MapMissing => "map-missing",
            Self::MapUnreadable { .. } => "map-unreadable",
            Self::MapUnfilled { .. } => "map-unfilled",
            Self::UnknownFile { .. } => "unknown-file",
            Self::MissingArgument { .. } => "missing-argument",
            Self::SkillMissingPaths { .. } => "skill-missing-path",
            Self::SkillTooLong { .. } => "skill-too-long",
            Self::UnknownDeclaration { .. } => "unknown-declaration",
            Self::FileUnreadable { .. } => "file-unreadable",
            Self::ChangedInCopy { .. } => "changed-in-copy",
            Self::HistoryUnreadable { .. } => "history-unreadable",
        }
    }

    /// A mensagem da recusa no idioma pedido.
    #[must_use]
    pub fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, String)]| slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value));
        match self {
            Self::MapMissing => fill("map.missing", &[]),
            Self::MapUnreadable { detail } => fill("map.unreadable", &[("{detail}", detail.clone())]),
            Self::MapUnfilled { blocks } => {
                let blocks: Vec<String> = blocks.iter().map(|block| format!("`{block}`")).collect();
                fill("map.unfilled", &[("{blocks}", blocks.join(", "))])
            }
            Self::UnknownFile { file } => fill("map.unknown_file", &[("{file}", file.clone())]),
            Self::MissingArgument { question, flag } => fill("map.missing_argument", &[("{question}", question.clone()), ("{flag}", flag.clone())]),
            Self::SkillMissingPaths { paths } => fill("map.skill_missing_path", &[("{paths}", paths.join(", "))]),
            Self::SkillTooLong { lines } => fill("map.skill_too_long", &[("{lines}", lines.to_string()), ("{max}", SKILL_MAX_LINES.to_string())]),
            Self::UnknownDeclaration { file: Some(file), name } => fill("map.unknown_declaration", &[("{file}", file.clone()), ("{name}", name.clone())]),
            Self::UnknownDeclaration { file: None, name } => fill("map.unknown_name", &[("{name}", name.clone())]),
            Self::FileUnreadable { file, detail } => fill("map.file_unreadable", &[("{file}", file.clone()), ("{detail}", detail.clone())]),
            Self::ChangedInCopy { file, name, line, copy: None } => {
                fill("map.changed_in_copy", &[("{file}", file.clone()), ("{name}", name.clone()), ("{line}", line.to_string())])
            }
            Self::ChangedInCopy { file, name, copy: Some((first, last)), .. } => fill(
                "map.changed_in_copy_range",
                &[("{file}", file.clone()), ("{name}", name.clone()), ("{first}", first.to_string()), ("{last}", last.to_string())],
            ),
            Self::HistoryUnreadable { file, detail } => fill("map.history_unreadable", &[("{file}", file.clone()), ("{detail}", detail.clone())]),
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

    /// Onde o mapa declara `name`: o caminho e a linha de cada declaração com
    /// esse nome, em ordem de caminho e de linha.
    #[must_use]
    pub fn declared(&self, name: &str) -> Vec<(String, u64)> {
        let mut out: Vec<(String, u64)> =
            self.modules.iter().flat_map(|m| m.declarations.iter().filter(|d| d.name == name).map(|d| (m.path.clone(), d.line))).collect();
        out.sort();
        out.dedup();
        out
    }

    fn known(&self, file: &str) -> Result<&MapModule, MapRefusal> {
        self.module(file).ok_or_else(|| MapRefusal::UnknownFile { file: clean_path(file) })
    }
}

// ---------------------------------------------------------------------------
// Quem importa, que teste cobre
// ---------------------------------------------------------------------------

/// Os arquivos que importam `file`, em ordem de nome.
pub fn importers(map: &ProjectMap, file: &str) -> Result<Vec<String>, MapRefusal> {
    let target = map.known(file)?.path.clone();
    let mut out: Vec<String> = map.modules.iter().filter(|m| m.deps.contains(&target)).map(|m| m.path.clone()).collect();
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
// O trecho de uma declaração
// ---------------------------------------------------------------------------

/// Onde uma declaração mora: o arquivo e as linhas dela, mais o que o mapa já
/// guarda dela. É o que quem pergunta precisa para ter o trecho sem abrir o
/// arquivo atrás dele.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclPlace {
    pub file: String,
    pub kind: String,
    pub name: String,
    pub line: u64,
    /// A última linha da declaração. Quando o mapa não a tem, a primeira: o
    /// trecho é então a linha em que ela começa.
    pub end_line: u64,
    pub doc: String,
    pub signature: String,
}

/// Onde a declaração `name` de `file` começa e termina. Recusa
/// [`MapRefusal::UnknownFile`] quando o arquivo não está no mapa e
/// [`MapRefusal::UnknownDeclaration`] quando ele está e a declaração não.
/// Havendo mais de uma com o mesmo nome no arquivo, vale a primeira.
pub fn declaration(map: &ProjectMap, file: &str, name: &str) -> Result<DeclPlace, MapRefusal> {
    let module = map.known(file)?;
    let name = name.trim();
    let found = module
        .declarations
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| MapRefusal::UnknownDeclaration { file: Some(module.path.clone()), name: name.to_string() })?;
    Ok(DeclPlace {
        file: module.path.clone(),
        kind: found.kind.clone(),
        name: found.name.clone(),
        line: found.line,
        end_line: found.end_line.max(found.line),
        doc: found.doc.clone(),
        signature: found.signature.clone(),
    })
}

// ---------------------------------------------------------------------------
// As partes de um arquivo
// ---------------------------------------------------------------------------

/// Os tipos de declaração que são dado de outra — o campo, o membro de enum,
/// a propriedade e o parâmetro escrito no cabeçalho do tipo. Moram dentro das
/// linhas da dona e ficam fora das partes.
const MEMBER_KINDS: &[&str] = &["field", "enum_member", "property", "parameter"];

/// Uma parte de um arquivo: uma declaração, com o tipo, o nome e as linhas de
/// começo e de fim.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FilePart {
    pub kind: String,
    pub name: String,
    pub line: u64,
    /// A última linha; a primeira, quando o mapa não a tem.
    pub end_line: u64,
}

/// As partes de um arquivo, para quem vai ler só um trecho dele: as
/// declarações fora dos testes escritos dentro do arquivo, em ordem de linha,
/// e a linha em que os testes começam, quando há.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileParts {
    pub file: String,
    pub parts: Vec<FilePart>,
    pub tests_line: Option<u64>,
}

/// As partes de `file`: cada declaração que não é dado de outra
/// ([`MEMBER_KINDS`]) e que começa fora dos trechos de teste, e a primeira
/// linha do primeiro trecho de teste. O arquivo que é todo de teste
/// ([`WHOLE_FILE_END`]) lista as partes dele como o arquivo de teste que o
/// nome diz, e não tem linha em que os testes começam. Recusa
/// [`MapRefusal::UnknownFile`] quando o arquivo não está no mapa.
pub fn parts(map: &ProjectMap, file: &str) -> Result<FileParts, MapRefusal> {
    let module = map.known(file)?;
    let blocks: Vec<(u64, u64)> = module.test_lines.iter().copied().filter(|&(_, to)| to != WHOLE_FILE_END).collect();
    let in_tests = |line: u64| blocks.iter().any(|&(from, to)| from <= line && line <= to);
    let mut parts: Vec<FilePart> = module
        .declarations
        .iter()
        .filter(|d| !MEMBER_KINDS.contains(&d.kind.as_str()) && !in_tests(d.line))
        .map(|d| FilePart { kind: d.kind.clone(), name: d.name.clone(), line: d.line, end_line: d.end_line.max(d.line) })
        .collect();
    parts.sort_by_key(|part| part.line);
    let tests_line = blocks.iter().map(|&(from, _)| from).min();
    Ok(FileParts { file: module.path.clone(), parts, tests_line })
}

/// Uma declaração com o nome perguntado, onde ela mora e quem a usa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclUsers {
    pub file: String,
    pub kind: String,
    pub name: String,
    pub line: u64,
    pub end_line: u64,
    pub used_by: Vec<UseSite>,
    /// Quantas chamadas pelo nome ficaram sem ligação por ele ser comum
    /// demais ([`MapDecl::common_calls`]).
    pub common_calls: usize,
    /// As rotas do servidor que a declaração atende, com as chamadas da tela
    /// que alcançam cada uma.
    pub routes: Vec<MapRoute>,
}

/// Cada declaração chamada `name`, com os usos de cada uma e as rotas que ela
/// atende — as do arquivo dela com o nome e a linha dela —, em ordem de
/// caminho e de linha. Com `file`, só as desse arquivo. Recusa
/// [`MapRefusal::UnknownFile`] quando o arquivo não está no mapa e
/// [`MapRefusal::UnknownDeclaration`] quando nenhuma declaração tem o nome:
/// no arquivo pedido, que a recusa cita, ou no mapa inteiro, e a recusa diz
/// que o mapa não tem a declaração.
pub fn users(map: &ProjectMap, file: Option<&str>, name: &str) -> Result<Vec<DeclUsers>, MapRefusal> {
    Ok(named_in(map, file, name)?
        .into_iter()
        .map(|(m, d)| DeclUsers {
            file: m.path.clone(),
            kind: d.kind.clone(),
            name: d.name.clone(),
            line: d.line,
            end_line: d.end_line.max(d.line),
            used_by: d.used_by.clone(),
            common_calls: d.common_calls,
            routes: m.routes.iter().filter(|r| r.handler == d.name && r.line == d.line).cloned().collect(),
        })
        .collect())
}

/// Cada declaração chamada `name`, com o arquivo dela, em ordem de caminho e
/// de linha; com `file`, só as desse arquivo. As recusas são as de [`users`].
fn named_in<'m>(map: &'m ProjectMap, file: Option<&str>, name: &str) -> Result<Vec<(&'m MapModule, &'m MapDecl)>, MapRefusal> {
    let name = name.trim();
    let modules: Vec<&MapModule> = match file {
        Some(file) => vec![map.known(file)?],
        None => map.modules.iter().collect(),
    };
    let mut found: Vec<(&MapModule, &MapDecl)> = modules.iter().flat_map(|m| m.declarations.iter().filter(|d| d.name == name).map(move |d| (*m, d))).collect();
    if found.is_empty() {
        let file = file.and(modules.first()).map(|m| m.path.clone());
        return Err(MapRefusal::UnknownDeclaration { file, name: name.to_string() });
    }
    found.sort_by(|a, b| (&a.0.path, a.1.line).cmp(&(&b.0.path, b.1.line)));
    Ok(found)
}

/// O trecho de `text` da linha `line` à linha `end_line`, contadas a partir de
/// 1. Sem nenhuma dessas linhas, o trecho é vazio.
#[must_use]
pub fn lines_of(text: &str, line: u64, end_line: u64) -> String {
    let first = line.max(1) as usize;
    let last = end_line.max(line) as usize;
    text.lines().skip(first - 1).take(last + 1 - first).collect::<Vec<_>>().join("\n")
}

/// As declarações de fora de um arquivo — as que nenhuma outra contém —,
/// dadas a primeira e a última linha de cada uma, na ordem em que foram
/// escritas. A última linha menor que a primeira vale como a primeira. Uma
/// contém a outra quando começa na mesma linha ou antes e termina na mesma
/// ou depois; entre duas de linhas iguais, a escrita antes contém a outra.
/// Devolve as posições delas, da que começa mais acima para a de mais abaixo:
/// a primeira é a que começa mais acima e, entre as que começam na mesma
/// linha, a de mais linhas. O scan e o índice de busca leem as declarações
/// de fora por esta regra só.
#[must_use]
pub fn outer_declarations(lines: &[(usize, usize)]) -> Vec<usize> {
    let span = |at: usize| (lines[at].0, lines[at].1.max(lines[at].0));
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by_key(|&at| {
        let (first, last) = span(at);
        (first, std::cmp::Reverse(last))
    });
    let mut outer = Vec::new();
    let mut reach: Option<usize> = None;
    for at in order {
        let last = span(at).1;
        if reach.is_none_or(|reach| last > reach) {
            outer.push(at);
            reach = Some(reach.map_or(last, |reach| reach.max(last)));
        }
    }
    outer
}

// ---------------------------------------------------------------------------
// Busca por conceito
// ---------------------------------------------------------------------------

/// Um item de spec achado pela busca: a spec, o código, o título, a linha
/// da parte do usuário que mais casa com a pergunta (nenhuma quando só o
/// título ou a parte do agente casou) e os lugares ligados a ele — a função,
/// como `arquivo:nome`, que um commit da onda dele mudou, ou o arquivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundItem {
    pub spec: String,
    pub code: String,
    pub title: String,
    pub line: Option<String>,
    pub links: Vec<String>,
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

/// A receita tirada do git para um arquivo: a soma dos commits da janela do
/// mapa que fizeram o mesmo trabalho — criar um arquivo do mesmo tipo na
/// mesma pasta, ou mudar o arquivo —, com o que eles fizeram junto. Só entra
/// o que passa de metade dos commits contados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub of: RecipeOf,
    /// Quantos commits se contaram.
    pub commits: u32,
    /// Cada arquivo que mudou junto em mais da metade deles, com em quantos
    /// — o registro que o arquivo novo pede, o teste, o texto —, do mais
    /// frequente para o menos, até [`RECIPE_FILES`].
    pub together: Vec<(String, u32)>,
    /// Em quantos deles um teste novo nasceu junto, quando passa de metade.
    pub tests: Option<u32>,
}

/// O trabalho que os commits de uma receita fizeram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeOf {
    /// Criaram um arquivo do mesmo tipo na mesma pasta, como `pasta/*.rs`.
    Created(String),
    /// Mudaram o arquivo.
    Changed(String),
}

/// Quantos commits uma receita precisa contar: com menos, não há receita.
pub const RECIPE_MIN_COMMITS: u32 = 3;

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
    /// Por que não há receita do git: o mapa está sem a história da branch
    /// de partida.
    pub no_history: Option<String>,
}

/// Quantos exemplos a resposta dá, no máximo.
const MAX_PICKS: usize = 3;
/// Quantas importações principais contam.
const MAX_MAIN_IMPORTS: usize = 8;
/// Quantos arquivos de uma receita a resposta mostra.
pub const RECIPE_FILES: usize = 10;

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
    let mut ranked: Vec<(usize, &MapModule)> =
        files.iter().filter(|m| !m.deps.is_empty()).map(|m| (m.deps.iter().map(|d| count[d.as_str()] - 1).sum(), *m)).collect();
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
/// - com teste primeiro, e depois o mais recente;
/// - tamanho típico: entre o quartil de baixo e o de cima da pasta, o que
///   deixa de fora o índice de módulo curto, que não mostra como fazer, e o
///   arquivo grande demais, que mistura trabalhos.
///
/// As lições do banco vêm de quem chama, que lê o banco. Sem histórico, valem
/// a pasta, as importações, o teste e o tamanho.
///
/// Só é exemplo o arquivo cujas importações seguem as regras fortes do
/// padrão do projeto ([`crate::domain::pattern::learn`]), aprendido do mesmo
/// mapa: o que importa contra uma delas ensinaria o caminho errado. O que
/// passa do corte de tamanho ou de repetição do projeto ([`QualityCuts`])
/// também fica de fora.
#[must_use]
pub fn examples(map: &ProjectMap, target: &str, lang: Locale) -> Examples {
    examples_following(map, target, lang, &crate::domain::pattern::learn(map))
}

/// [`examples`] com o padrão `pattern` já aprendido do mapa: quem pede os
/// exemplos de várias tarefas aprende o padrão uma vez só.
#[must_use]
pub fn examples_following(map: &ProjectMap, target: &str, lang: Locale, pattern: &Pattern) -> Examples {
    let target = clean_path(target);
    let target_module = map.module(&target);
    let is_folder = target_module.is_none() && (target.is_empty() || map.modules.iter().any(|m| folder_of(&m.path) == target));
    let folder = if is_folder { target.clone() } else { folder_of(&target).to_string() };
    let last_at = |path: &str| -> i64 { file_history(&map.history, path).map_or(0, |h| h.last_at) };

    // A mesma pasta; com menos de 2, as pastas vizinhas (mesmo pai).
    // Nem o arquivo que passa do corte de tamanho ou de repetição do projeto
    // ([`QualityCuts`]): ele ensinaria a crescer ou a copiar.
    let cuts = QualityCuts::of(&map.modules);
    let usable = |m: &&MapModule| is_example_material(m) && m.path != target && pattern.follows(m) && !cuts.large(m) && !cuts.repeated(m);
    let mut pool: Vec<(&MapModule, bool)> = map.modules.iter().filter(usable).filter(|m| folder_of(&m.path) == folder).map(|m| (m, true)).collect();
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
                why.push(translate("map.why.imports", lang).replace("{shared}", &c.shared.len().to_string()).replace("{of}", &main_imports.len().to_string()));
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

    Examples { no_history: history_note(&map.history, lang), folder, main_imports, picks }
}

/// A receita de criar o arquivo `target`, que ainda não existe: a soma dos
/// commits da janela que criaram um arquivo do mesmo tipo (a mesma extensão;
/// qualquer um, sem extensão) na mesma pasta, fora os de teste. O caminho
/// que termina em `/` é a pasta, e vale qualquer tipo. O arquivo criado não
/// entra no "muda junto": cada commit cria o seu.
#[must_use]
pub fn recipe_for_new(history: &History, target: &str) -> Option<Recipe> {
    let folder = folder_of(target);
    let ext = if target.ends_with('/') { None } else { extension_of(target) };
    let same_kind: Vec<bool> =
        history.paths.iter().map(|p| folder_of(p) == folder && !is_test_path(p) && ext.is_none_or(|e| extension_of(p) == Some(e))).collect();
    let kind = |i: u32| same_kind.get(i as usize).copied().unwrap_or(false);
    let mut sum = RecipeSum::default();
    for commit in history.commits.iter().filter(|c| c.added.len() + c.changed.len() <= CO_CHANGE_MAX_FILES) {
        if commit.added.iter().any(|&i| kind(i)) {
            sum.count(history, commit, |i| commit.added.contains(&i) && kind(i));
        }
    }
    let shown = match (folder, ext) {
        ("", Some(e)) => format!("*.{e}"),
        ("", None) => "*".to_string(),
        (dir, Some(e)) => format!("{dir}/*.{e}"),
        (dir, None) => format!("{dir}/*"),
    };
    sum.recipe(RecipeOf::Created(shown))
}

/// A receita de mudar o arquivo `path`: a soma dos commits da janela que o
/// mudaram, com os outros arquivos de cada um.
#[must_use]
pub fn recipe_for_existing(history: &History, path: &str) -> Option<Recipe> {
    let index = u32::try_from(history.paths.binary_search_by(|p| p.as_str().cmp(path)).ok()?).ok()?;
    let mut sum = RecipeSum::default();
    for commit in history.commits.iter().filter(|c| c.added.len() + c.changed.len() <= CO_CHANGE_MAX_FILES) {
        if commit.added.binary_search(&index).is_ok() || commit.changed.binary_search(&index).is_ok() {
            sum.count(history, commit, |i| i == index);
        }
    }
    sum.recipe(RecipeOf::Changed(path.to_string()))
}

/// A receita de mudar o arquivo da história `lineage`, lida do git além da
/// janela do mapa: a mesma soma de [`recipe_for_existing`], dos commits que
/// guardam os arquivos que mudaram junto.
#[must_use]
pub fn recipe_from_lineage(lineage: &FileLineage) -> Option<Recipe> {
    let mut sum = RecipeSum::default();
    for commit in lineage.commits.iter().filter(|c| !c.files.is_empty()) {
        sum.commits += 1;
        if commit.files.added.iter().any(|p| is_test_path(p)) {
            sum.tests += 1;
        }
        let others: BTreeSet<&str> = commit.files.added.iter().chain(&commit.files.changed).map(String::as_str).collect();
        for other in others.into_iter().filter(|p| *p != lineage.path) {
            *sum.together.entry(other.to_string()).or_insert(0) += 1;
        }
    }
    sum.recipe(RecipeOf::Changed(lineage.path.clone()))
}

/// A soma dos commits de uma receita.
#[derive(Default)]
struct RecipeSum {
    commits: u32,
    tests: u32,
    together: BTreeMap<String, u32>,
}

impl RecipeSum {
    /// Conta `commit`, com cada arquivo dele que `own` não diz ser o da
    /// própria receita.
    fn count(&mut self, history: &History, commit: &Commit, own: impl Fn(u32) -> bool) {
        let path = |i: u32| history.paths.get(i as usize).map(String::as_str);
        self.commits += 1;
        if commit.added.iter().filter_map(|&i| path(i)).any(is_test_path) {
            self.tests += 1;
        }
        let others: BTreeSet<u32> = commit.added.iter().chain(&commit.changed).copied().filter(|&i| !own(i)).collect();
        for other in others.into_iter().filter_map(path) {
            *self.together.entry(other.to_string()).or_insert(0) += 1;
        }
    }

    /// A receita da soma: com menos de [`RECIPE_MIN_COMMITS`] commits, ou sem
    /// nada que passe de metade deles, nenhuma.
    fn recipe(self, of: RecipeOf) -> Option<Recipe> {
        if self.commits < RECIPE_MIN_COMMITS {
            return None;
        }
        let half = |n: u32| u64::from(n) * 2 > u64::from(self.commits);
        let mut together: Vec<(String, u32)> = self.together.into_iter().filter(|(_, n)| half(*n)).collect();
        together.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        together.truncate(RECIPE_FILES);
        let tests = half(self.tests).then_some(self.tests);
        (!together.is_empty() || tests.is_some()).then_some(Recipe { of, commits: self.commits, together, tests })
    }
}

// ---------------------------------------------------------------------------
// Resumo do mapa
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

/// O resumo do mapa: quantos arquivos e de que
/// linguagens, os subprojetos, os arquivos mais importados, os mudados há
/// pouco e como perguntar ao mapa. Nunca passa de [`SUMMARY_MAX_BYTES`]: as
/// linhas que não cabem ficam de fora, e a última (como perguntar) sempre
/// entra.
#[must_use]
pub fn summary(map: &ProjectMap, lang: Locale) -> String {
    let files: usize = map.modules.len();
    let languages: Vec<String> = map.languages.iter().take(SUMMARY_FILES).map(|l| format!("{} {}", l.language, l.files)).collect();
    let mut lines: Vec<String> =
        vec![clip(translate("map.summary.head", lang).replace("{files}", &files.to_string()).replace("{languages}", &languages.join(", ")))];
    // Um projeto de teste (uma fixture dentro de `tests/`) não é subprojeto.
    let mut projects: Vec<&MapProject> = map.projects.iter().filter(|p| p.code_files > 0 && !is_test_path(&format!("{}/x", p.dir))).collect();
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
    if let Some(note) = history_note(&map.history, lang) {
        lines.push(clip(note));
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
/// `{x}`, `*`, `$VAR`), os endereços — também os com esquema antes da
/// primeira barra, como `package:flutter/material.dart` — e o que tem
/// espaço. Um `:linha` no fim sai.
#[must_use]
pub fn cited_paths(text: &str) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for (i, span) in text.split('`').enumerate() {
        if i % 2 == 0 {
            continue;
        }
        if let Some(path) = span_path(span) {
            out.insert(path.to_string());
        }
    }
    out.into_iter().collect()
}

/// O caminho que um trecho sem espaço cita, pela regra de [`cited_paths`]:
/// sem o `:linha` do fim e sem a pontuação final; `None` quando o trecho
/// não é caminho de arquivo nem de pasta.
fn span_path(span: &str) -> Option<&str> {
    let span = span.trim();
    if span.is_empty()
        || span.contains(char::is_whitespace)
        || span.contains("://")
        || span.starts_with('-')
        || span.starts_with('/')
        || span.contains(['<', '>', '{', '}', '*', '$', '|', '(', ')', '[', ']', '…', '"', '\''])
        || !span.contains('/')
        || span.split('/').next().is_some_and(|head| head.contains(':'))
    {
        return None;
    }
    let path = match span.rsplit_once(':') {
        Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '-') => head,
        _ => span,
    };
    let path = path.trim_end_matches(['.', ',', ';']);
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    (path.ends_with('/') || extension_of(last).is_some()).then_some(path)
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

/// `true` quando algum arquivo do mapa é `cited`, termina com `/cited` ou
/// fica dentro de uma pasta que é ou termina com `cited`: a skill, e a lição,
/// pode citar só o fim do caminho, como `spec_events/mod.rs` ou
/// `domain/model/`.
#[must_use]
pub fn map_knows(map: &ProjectMap, cited: &str) -> bool {
    let cited = clean_path(cited);
    let tail = format!("/{cited}");
    let inside = format!("/{cited}/");
    map.modules.iter().any(|m| {
        m.path == cited || m.path.ends_with(&tail) || folder_of(&m.path) == cited || m.path.starts_with(&format!("{cited}/")) || m.path.contains(&inside)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(path: &str, loc: usize, deps: &[&str]) -> MapModule {
        MapModule { path: path.to_string(), language: "rust".to_string(), loc, deps: deps.iter().map(|d| (*d).to_string()).collect(), ..MapModule::default() }
    }

    fn decl(name: &str) -> MapDecl {
        MapDecl { name: name.to_string(), ..MapDecl::default() }
    }

    fn commit(id: &str, at: i64, added: &[&str], changed: &[&str]) -> RawCommit {
        RawCommit {
            id: id.to_string(),
            at,
            title: String::new(),
            pr: None,
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
            commit("cccc", 3 * DAY, &[&format!("{dir}/read.rs")], &["apps/rt/tests/run_command_surface.rs", "packages/core/src/platform/i18n.rs"]),
            commit("dddd", 4 * DAY, &[], &[&format!("{dir}/write.rs")]),
        ]);
        ProjectMap { modules: vec![read, write, index, pages, huge, lone, test], history, ..ProjectMap::default() }
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

    /// Cinco controllers que importam cinco services, em 24 importações, e o
    /// `service4`, o único testado e o mais recente da pasta, que importa o
    /// `controller4` contra a regra forte.
    fn services_against_one_rule() -> ProjectMap {
        let named = |role: &str, n: usize| format!("src/{role}/{role}{n}.{role}.ts");
        let mut modules = Vec::new();
        for c in 0..5 {
            let deps: Vec<String> = (0..5).map(|s| named("service", s)).take(if c == 4 { 4 } else { 5 }).collect();
            modules.push(MapModule { path: named("controller", c), language: "typescript".to_string(), loc: 50, deps, ..MapModule::default() });
        }
        for s in 0..5 {
            let against = s == 4;
            modules.push(MapModule {
                path: named("service", s),
                language: "typescript".to_string(),
                loc: 50,
                deps: if against { vec![named("controller", 4)] } else { Vec::new() },
                has_tests: against,
                declarations: vec![decl(&format!("Service{s}"))],
                ..MapModule::default()
            });
        }
        let history = History::from_raw(vec![commit("aaaa", DAY, &[], &[&named("service", 4)])]);
        ProjectMap { modules, history, ..ProjectMap::default() }
    }

    #[test]
    fn a_file_that_imports_against_a_strong_rule_is_never_an_example() {
        let map = services_against_one_rule();
        assert_eq!(crate::domain::pattern::learn(&map).strong.len(), 1, "controller imports service is a strong rule");
        let got = examples(&map, "src/service/service9.service.ts", Locale::PtBr);
        let paths: Vec<&str> = got.picks.iter().map(|p| p.path.as_str()).collect();
        assert!(!paths.is_empty(), "the services that follow the rule are still examples");
        assert!(!paths.contains(&"src/service/service4.service.ts"), "{paths:?}");
        // Sem a regra, o mesmo arquivo seria o primeiro: testado e recente.
        let loose = examples_following(&map, "src/service/service9.service.ts", Locale::PtBr, &Pattern::default());
        assert_eq!(loose.picks[0].path, "src/service/service4.service.ts");
    }

    #[test]
    fn the_map_summary_carries_no_pattern() {
        let map = services_against_one_rule();
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = summary(&map, lang);
            for key in ["prompt.pattern.head", "prompt.pattern.rule", "prompt.pattern.info", "prompt.pattern.example"] {
                let fixed = translate(key, lang).split('{').next().unwrap_or_default();
                assert!(!text.contains(fixed), "{key} in the map summary: {text}");
            }
            assert!(!text.contains("importa service") && !text.contains("imports service"), "{text}");
        }
        for template in [include_str!("../../templates/agents/pt-BR/wave.md"), include_str!("../../templates/agents/en-US/wave.md")] {
            assert!(!template.contains("importa service") && !template.contains("regra:") && !template.contains("rule:"), "{template}");
        }
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
            m.declarations = vec![decl("run"), decl(&format!("Cmd{i}Opts"))];
            m.has_tests = i % 4 == 0;
            modules.push(m);
        }
        let siblings: Vec<&str> = names.iter().map(String::as_str).collect();
        modules.push(module(&format!("{folder}/mod.rs"), 30, &siblings));
        modules.push(module(sections, 400, &[]));
        modules.push(module(context, 900, &[]));
        modules.push(module(util, 300, &[]));
        let mut hook = module("apps/rt/src/hooks/worktree_create.rs", 20, &[context]);
        hook.declarations = vec![decl("run"), decl("command")];
        modules.push(hook);
        modules.push(module("apps/rt/src/hooks/mod.rs", 10, &["apps/rt/src/hooks/worktree_create.rs"]));
        ProjectMap { modules, ..ProjectMap::default() }
    }

    #[test]
    fn a_new_file_in_a_real_sized_tree_gets_the_main_imports_of_its_command_folder() {
        let map = real_sized_tree();
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
    fn the_recipe_sums_every_commit_that_created_a_file_of_the_same_kind_there() {
        let map = command_folder();
        let recipe = recipe_for_new(&map.history, "apps/rt/src/commands/spec_events/map.rs").expect("three commits created a command there");
        assert_eq!(recipe.of, RecipeOf::Created("apps/rt/src/commands/spec_events/*.rs".to_string()));
        assert_eq!(recipe.commits, 3);
        // O texto mudou junto em só 1 dos 3: fica de fora.
        assert_eq!(recipe.together, vec![("apps/rt/tests/run_command_surface.rs".to_string(), 2)]);
        assert_eq!(recipe.tests, None);
    }

    /// Dez commits que criaram um comando em `apps/rt/src/commands`: nove
    /// registram o comando no índice, sete criam o teste dele (quando
    /// `tested`), três mudam o texto, e um arquivo de teste criado na própria
    /// pasta não conta como comando. Ao lado, commits que não criam comando.
    fn ten_commands(tested: bool) -> History {
        let dir = "apps/rt/src/commands";
        let mut commits = Vec::new();
        for n in 0..10_i64 {
            let created = format!("{dir}/cmd_{n}.rs");
            let test = format!("apps/rt/tests/cmd_{n}.rs");
            let mut added = vec![created.as_str()];
            if tested && n < 7 {
                added.push(test.as_str());
            }
            let mut changed = Vec::new();
            if n < 9 {
                changed.push("apps/rt/src/commands/mod.rs");
            }
            if n < 3 {
                changed.push("packages/core/src/platform/i18n.rs");
            }
            commits.push(commit(&format!("c{n}"), n * DAY, &added, &changed));
            commits.push(commit(&format!("o{n}"), n * DAY + 1, &[], &["README.md"]));
        }
        commits.push(commit("t0", 20 * DAY, &[&format!("{dir}/cmd_test.rs")], &["apps/rt/src/commands/mod.rs"]));
        History::from_raw(commits)
    }

    #[test]
    fn a_task_creating_a_command_gets_the_registry_and_the_test_it_usually_brings() {
        let recipe = recipe_for_new(&ten_commands(true), "apps/rt/src/commands/novo.rs").expect("ten commands were created");
        assert_eq!(recipe.of, RecipeOf::Created("apps/rt/src/commands/*.rs".to_string()));
        assert_eq!(recipe.commits, 10, "every commit of the window is summed, not the last three");
        assert_eq!(recipe.together, vec![("apps/rt/src/commands/mod.rs".to_string(), 9)]);
        assert_eq!(recipe.tests, Some(7));
    }

    #[test]
    fn a_project_without_tests_gets_no_test_line_in_the_recipe() {
        let recipe = recipe_for_new(&ten_commands(false), "apps/rt/src/commands/novo.rs").expect("ten commands were created");
        assert_eq!(recipe.tests, None);
        assert_eq!(recipe.together, vec![("apps/rt/src/commands/mod.rs".to_string(), 9)]);
    }

    #[test]
    fn fewer_than_three_commits_or_no_history_give_no_recipe() {
        let two = History::from_raw(vec![commit("a", DAY, &["src/a.rs"], &["src/mod.rs"]), commit("b", 2 * DAY, &["src/b.rs"], &["src/mod.rs"])]);
        assert_eq!(recipe_for_new(&two, "src/c.rs"), None);
        assert_eq!(recipe_for_new(&History::default(), "src/c.rs"), None);
        assert_eq!(recipe_for_existing(&History::default(), "src/a.rs"), None);
        let three = History::from_raw(vec![
            commit("a", DAY, &["src/a.rs"], &["src/mod.rs"]),
            commit("b", 2 * DAY, &["src/b.rs"], &["src/mod.rs"]),
            commit("c", 3 * DAY, &["src/d.rs"], &["src/mod.rs"]),
        ]);
        assert_eq!(recipe_for_new(&three, "src/c.rs").map(|r| r.commits), Some(3));
    }

    #[test]
    fn the_recipe_of_an_existing_file_sums_the_commits_that_changed_it_and_skips_the_huge_ones() {
        let wide: Vec<String> = (0..=CO_CHANGE_MAX_FILES).map(|n| format!("src/other_{n}.rs")).collect();
        let mut wide_changed: Vec<&str> = wide.iter().map(String::as_str).collect();
        wide_changed.push("src/pay.rs");
        let history = History::from_raw(vec![
            commit("a", DAY, &["src/pay.rs"], &["src/mod.rs"]),
            commit("b", 2 * DAY, &[], &["src/pay.rs", "tests/pay.rs"]),
            commit("c", 3 * DAY, &[], &["src/pay.rs", "tests/pay.rs"]),
            commit("d", 4 * DAY, &[], &["src/pay.rs", "tests/pay.rs", "src/mod.rs"]),
            commit("e", 5 * DAY, &[], &wide_changed),
        ]);
        let recipe = recipe_for_existing(&history, "src/pay.rs").expect("four commits changed it");
        assert_eq!(recipe.of, RecipeOf::Changed("src/pay.rs".to_string()));
        assert_eq!(recipe.commits, 4, "the commit over the co-change ceiling does not count");
        assert_eq!(recipe.together, vec![("tests/pay.rs".to_string(), 3)]);
    }

    #[test]
    fn the_recipe_of_a_history_read_beyond_the_window_comes_from_its_commits_files() {
        let with = |id: &str, added: &[&str], changed: &[&str]| LineageCommit {
            id: id.to_string(),
            files: CommitFiles { added: added.iter().map(|p| (*p).to_string()).collect(), changed: changed.iter().map(|p| (*p).to_string()).collect() },
            ..LineageCommit::default()
        };
        let lineage = FileLineage {
            path: "src/old.rs".to_string(),
            commits: vec![
                with("a", &["src/old.rs", "tests/old.rs"], &["src/mod.rs"]),
                with("b", &[], &["src/old.rs", "src/mod.rs"]),
                with("c", &["tests/more.rs"], &["src/old.rs", "src/mod.rs"]),
                with("d", &[], &[]),
            ],
            ..FileLineage::default()
        };
        let recipe = recipe_from_lineage(&lineage).expect("three commits keep their files");
        assert_eq!(recipe.commits, 3, "the commit without files does not count");
        assert_eq!(recipe.together, vec![("src/mod.rs".to_string(), 3)]);
        assert_eq!(recipe.tests, Some(2));
    }

    /// Vinte e cinco arquivos medidos: uma pasta de pedidos com cinco que
    /// importam o mesmo núcleo, entre eles o maior do projeto e o mais
    /// repetido, os dois testados e os mais recentes; e vinte outros, pequenos.
    fn orders_with_quality() -> ProjectMap {
        let core = "src/core.rs";
        let mut modules = Vec::new();
        for name in ["big", "copied", "a", "b", "c"] {
            let mut m = module(&format!("src/orders/{name}.rs"), 100, &[core]);
            m.quality = Quality { size: 100, imports: 1, ..Quality::default() };
            m.has_tests = matches!(name, "big" | "copied");
            modules.push(m);
        }
        modules[0].quality.size = 5000;
        modules[1].quality.repeated = 400;
        for n in 0..20 {
            let mut m = module(&format!("src/other/f{n}.rs"), 100, &[]);
            m.quality = Quality { size: 100 + n, ..Quality::default() };
            modules.push(m);
        }
        let history = History::from_raw(vec![commit("aaaa", DAY, &[], &["src/orders/big.rs", "src/orders/copied.rs"])]);
        ProjectMap { modules, history, ..ProjectMap::default() }
    }

    #[test]
    fn an_example_above_the_size_or_the_repetition_cut_does_not_enter() {
        let map = orders_with_quality();
        let cuts = QualityCuts::of(&map.modules);
        assert!(cuts.large(&map.modules[0]), "the largest file passes the cut: {cuts:?}");
        assert!(cuts.repeated(&map.modules[1]), "the most repeated file passes the cut: {cuts:?}");
        let got = examples(&map, "src/orders/new.rs", Locale::PtBr);
        let paths: Vec<&str> = got.picks.iter().map(|p| p.path.as_str()).collect();
        assert!(!paths.is_empty());
        assert!(!paths.contains(&"src/orders/big.rs") && !paths.contains(&"src/orders/copied.rs"), "{paths:?}");
        // Sem as medidas, os dois seriam os primeiros: testados e recentes.
        let mut plain = map.clone();
        for m in &mut plain.modules {
            m.quality = Quality::default();
        }
        let loose = examples(&plain, "src/orders/new.rs", Locale::PtBr);
        let first: Vec<&str> = loose.picks.iter().take(2).map(|p| p.path.as_str()).collect();
        assert_eq!(first, ["src/orders/big.rs", "src/orders/copied.rs"]);
    }

    #[test]
    fn the_cut_is_relative_to_the_project_and_a_tie_with_the_rest_never_passes() {
        let sized = |sizes: &[usize]| -> Vec<MapModule> {
            sizes
                .iter()
                .enumerate()
                .map(|(n, &size)| MapModule { path: format!("src/f{n}.rs"), quality: Quality { size, ..Quality::default() }, ..MapModule::default() })
                .collect()
        };
        // Com menos de 20 arquivos, os 5% de cima não têm nenhum.
        let few = sized(&[10, 20, 3000]);
        assert!(!QualityCuts::of(&few).large(&few[2]));
        // Quarenta arquivos: os 2 maiores passam, o terceiro não.
        let many: Vec<usize> = (1..=40).map(|n| n * 10).collect();
        let forty = sized(&many);
        let cuts = QualityCuts::of(&forty);
        assert_eq!(forty.iter().filter(|m| cuts.large(m)).count(), 2, "{cuts:?}");
        // Todos iguais: ninguém passa.
        let same = sized(&[50; 30]);
        let cuts = QualityCuts::of(&same);
        assert!(same.iter().all(|m| !cuts.large(m)), "{cuts:?}");
    }

    #[test]
    fn without_history_the_examples_still_come_by_folder_imports_test_and_size() {
        let mut map = command_folder();
        map.history = History::default();
        let got = examples(&map, "apps/rt/src/commands/spec_events/", Locale::PtBr);
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
    fn the_map_summary_fits_in_three_kilobytes() {
        let modules: Vec<MapModule> = (0..5000).map(|i| module(&format!("apps/sub{}/src/{}/file_{i}.rs", i % 40, "x".repeat(60)), 10, &[])).collect();
        let projects: Vec<MapProject> = (0..40)
            .map(|i| MapProject { name: format!("sub{i}-{}", "n".repeat(80)), dir: format!("apps/sub{i}"), kind: "cargo".to_string(), code_files: 100 + i })
            .collect();
        let top_fan_in = modules.iter().take(64).map(|m| MapDegree { module: m.path.clone(), degree: 3 }).collect();
        let history = History::from_raw(modules.iter().take(200).enumerate().map(|(i, m)| commit(&i.to_string(), i as i64, &[], &[&m.path])).collect());
        let map = ProjectMap {
            modules,
            projects,
            languages: vec![MapLanguage { language: "rust".to_string(), files: 5000, loc: 50_000 }],
            graph: MapGraph { top_fan_in },
            history,
            state: MapState::default(),
            skeleton: Vec::new(),
            ..ProjectMap::default()
        };
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = summary(&map, lang);
            assert!(text.len() <= SUMMARY_MAX_BYTES, "{} bytes", text.len());
            assert!(text.contains("mustard-rt run map"), "the way to ask always fits: {text}");
            assert!(text.contains("5000"), "{text}");
        }
    }

    /// O convite do resumo, que diz como perguntar ao mapa, cita a pergunta da
    /// história de uma declaração e o resumo de um arquivo nos dois idiomas,
    /// ao lado das outras.
    #[test]
    fn the_map_summary_invites_the_history_question() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = summary(&ProjectMap::default(), lang);
            let ask = text.lines().last().unwrap_or_default();
            assert!(ask.contains("`history --name <"), "{lang:?}: {ask}");
            assert!(ask.contains("`summary --file <"), "{lang:?}: {ask}");
            assert!(ask.contains("`users --name <") && ask.contains("`search \""), "{lang:?}: {ask}");
            assert!(!ask.contains("--query"), "{lang:?}: {ask}");
        }
    }

    #[test]
    fn a_skill_citing_a_path_that_does_not_exist_is_refused() {
        let text = "Veja `apps/rt/src/commands/spec_events/write.rs`, `spec_events/mod.rs:12` e \
                    `apps/rt/src/commands/nao_existe.rs`; o modelo `packages/core/src/domain/<assunto>.rs` \
                    e `plugin/**` não contam, nem `pt-BR/en-US`, nem o endereço `package:flutter/material.dart`.";
        assert_eq!(
            cited_paths(text),
            vec!["apps/rt/src/commands/nao_existe.rs".to_string(), "apps/rt/src/commands/spec_events/write.rs".to_string(), "spec_events/mod.rs".to_string(),],
        );
        let map = ProjectMap {
            modules: vec![module("apps/rt/src/commands/spec_events/write.rs", 1, &[]), module("apps/rt/src/commands/spec_events/mod.rs", 1, &[])],
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

    /// A pasta citada só pelo fim é achada quando algum arquivo do mapa mora
    /// numa pasta que termina com ela; a pasta que nenhum arquivo tem
    /// continua recusada, e um pedaço do nome da pasta não conta.
    #[test]
    fn a_folder_cited_by_its_tail_is_known_only_when_a_file_lives_under_it() {
        let map = ProjectMap { modules: vec![module("packages/core/src/domain/model/contract.rs", 1, &[])], ..ProjectMap::default() };
        assert!(map_knows(&map, "domain/model/"));
        assert!(map_knows(&map, "src/domain/"));
        assert!(!map_knows(&map, "domain/modelo/"), "a pasta que nenhum arquivo tem");
        assert!(!map_knows(&map, "main/model/"), "um pedaço do nome da pasta não conta");
        let refused = check_skill("Veja `domain/modelo/`.", |p| map_knows(&map, p)).unwrap_err();
        assert_eq!(refused, MapRefusal::SkillMissingPaths { paths: vec!["domain/modelo/".to_string()] });
    }

    #[test]
    fn the_map_keeps_the_kind_and_the_line_of_each_declaration() {
        let map: ProjectMap = serde_json::from_str(
            r#"{"modules":[
                {"path":"src/b.rs","declarations":[{"kind":"function","name":"run","line":103}]},
                {"path":"src/a.rs","declarations":[{"kind":"struct","name":"run","line":7,"supertypes":[]},{"name":"old"}]}
            ]}"#,
        )
        .unwrap();
        let decl = &map.modules[0].declarations[0];
        assert_eq!((decl.kind.as_str(), decl.name.as_str(), decl.line), ("function", "run", 103));
        assert_eq!(map.declared("run"), vec![("src/a.rs".to_string(), 7), ("src/b.rs".to_string(), 103)]);
        assert_eq!(map.declared("old"), vec![("src/a.rs".to_string(), 0)], "an old map without the line still reads");
        assert!(map.declared("nada").is_empty());
    }

    /// A declaração de fora é a que nenhuma outra contém: o método dentro do
    /// tipo sai, as duas que dividem uma linha ficam, e das duas de linhas
    /// iguais fica a escrita antes. A primeira é a que começa mais acima.
    #[test]
    fn the_outer_declarations_are_the_ones_no_other_holds() {
        // tipo 1-9 com método 3-5; função 9-12 que divide a linha 9; duas de
        // uma linha só, a 14; e uma sem a última linha, na 20.
        let lines = [(1, 9), (3, 5), (9, 12), (14, 14), (14, 14), (20, 0)];
        assert_eq!(outer_declarations(&lines), vec![0, 2, 3, 5]);
        // Escritas fora de ordem, a primeira devolvida é a de mais acima; das
        // que começam na mesma linha, a de mais linhas.
        assert_eq!(outer_declarations(&[(5, 6), (2, 2), (2, 8)]), vec![2]);
        assert!(outer_declarations(&[]).is_empty());
    }

    /// As partes de um arquivo são as declarações fora dos testes, em ordem
    /// de linha, sem os campos, os membros de enum nem os parâmetros do
    /// cabeçalho do tipo, com a linha em que os testes começam. O arquivo
    /// fora do mapa é recusado.
    #[test]
    fn the_parts_of_a_file_leave_out_the_tests_and_the_members() {
        let map: ProjectMap = serde_json::from_str(
            r#"{"modules":[{"path":"src/a.rs","test_lines":[[40,60]],"declarations":[
                {"kind":"method","name":"run","line":12,"end_line":20},
                {"kind":"struct","name":"Alpha","line":3,"end_line":10},
                {"kind":"parameter","name":"width","line":3,"end_line":3},
                {"kind":"field","name":"size","line":4,"end_line":4},
                {"kind":"enum_member","name":"Red","line":30,"end_line":30},
                {"kind":"function","name":"tail","line":25},
                {"kind":"function","name":"a_test","line":45,"end_line":50}
            ]}]}"#,
        )
        .unwrap();
        let found = parts(&map, "./src/a.rs").unwrap();
        assert_eq!(found.file, "src/a.rs");
        let seen: Vec<(&str, &str, u64, u64)> = found.parts.iter().map(|p| (p.kind.as_str(), p.name.as_str(), p.line, p.end_line)).collect();
        assert_eq!(seen, [("struct", "Alpha", 3, 10), ("method", "run", 12, 20), ("function", "tail", 25, 25)]);
        assert_eq!(found.tests_line, Some(40));
        assert!(matches!(parts(&map, "src/b.rs"), Err(MapRefusal::UnknownFile { .. })));
    }

    /// O arquivo que um módulo declara como teste traz o trecho do arquivo
    /// inteiro, e as partes dele seguem na lista: sem elas a leitura inteira
    /// recusada não diria onde ler. O trecho de teste escrito dentro dele
    /// segue dizendo onde os testes começam.
    #[test]
    fn the_parts_of_a_file_that_is_all_test_are_listed() {
        let whole = format!("[[1,{WHOLE_FILE_END}],[40,60]]");
        let map: ProjectMap = serde_json::from_str(&format!(
            r#"{{"modules":[{{"path":"src/a/helpers.rs","test_lines":{whole},"declarations":[
                {{"kind":"function","name":"searched","line":3,"end_line":9}},
                {{"kind":"function","name":"a_test","line":45,"end_line":50}}
            ]}}]}}"#
        ))
        .unwrap();
        let found = parts(&map, "src/a/helpers.rs").unwrap();
        let seen: Vec<(&str, u64)> = found.parts.iter().map(|p| (p.name.as_str(), p.line)).collect();
        assert_eq!(seen, [("searched", 3)]);
        assert_eq!(found.tests_line, Some(40));
    }
}
