//! A história de cada declaração de um arquivo na branch de partida, montada
//! na primeira pergunta sobre ele e gravada no mapa.
//!
//! A passada lê só aquele arquivo: o `git log --follow` da ponta da base para
//! trás, a janela da montagem e o que vem antes dela, com as duas versões
//! inteiras do arquivo no diff de cada commit. Cada versão se lê uma vez, com
//! só a língua dela compilada, e cada declaração vira uma faixa de linhas que
//! inclui a documentação e os enfeites logo acima. Cada linha tirada ou posta
//! vai para a declaração mais interna que a contém, do seu lado; a linha entre
//! declarações não vai a nenhuma.
//!
//! A declaração que some de um lado e nasce do outro no mesmo commit se casa
//! pelo corpo idêntico, depois por pelo menos metade das linhas em comum:
//! primeiro no mesmo arquivo, depois nos outros arquivos do commit. A que veio
//! de outro arquivo segue a história nele, só antes daquele commit e só para
//! ela. O commit que só muda espaços na declaração, ou que o projeto lista no
//! `.git-blame-ignore-revs`, fica na lista com a marca de só forma.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use anyhow::{anyhow, Result};
use mustard_core::domain::project_map::{
    file_history, DeclChange, DeclComment, DeclLineage, FileLineage, LineageCommit, PullComment,
};
use mustard_core::io::project_map as store;

use crate::extract::{detect_language, Analyzer, Keep};
use crate::ingest::in_parallel;
use crate::refresh::{self, git, unquote};
use crate::routes;

/// Contexto grande o bastante para o diff de cada commit trazer o arquivo
/// inteiro, dos dois lados.
const WHOLE_FILE: &str = "-U999999999";

/// O cabeçalho de cada commit no `git log`: o hash inteiro, a data, os pais e
/// o título.
const HEADER: &str = "--format=%x00%H %ct %P%x1f%s";

/// As opções de todo diff lido aqui: o texto como está no commit, sem cor nem
/// filtro do usuário, os caminhos relativos à pasta lida, os prefixos de
/// sempre e o hash inteiro de cada versão.
const PLAIN_DIFF: [&str; 8] = [
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-show-signature",
    "--relative",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--full-index",
];

/// O que a passada leu: o arquivo, quantos commits ficaram na lista dele e
/// quantas declarações ele tem na ponta da base.
pub(crate) struct Report {
    pub file: String,
    pub commits: usize,
    pub declarations: usize,
}

/// Uma declaração dentro de uma versão do arquivo: o nome e a ordem entre as
/// do mesmo nome.
type Key = (String, u32);

/// Monta a história das declarações de `file` na ponta da base de `root` e a
/// grava no mapa em `out`, no lugar da que ele tinha. Uma declaração que veio
/// de outro arquivo é seguida nele até `moves` vezes seguidas.
///
/// # Errors
///
/// Sem base declarada ou sem ela no clone, com o mapa ilegível, quando o git
/// não lê a história do arquivo ou quando a gravação falha.
pub(crate) fn run(root: &Path, out: &Path, file: &str, moves: usize) -> Result<Report> {
    let base = store::base_of(root);
    if base.name.is_empty() || base.tip.is_empty() {
        return Err(anyhow!("the project declares no base branch this clone has"));
    }
    let stored = store::history_at(out).map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
    let reviews = store::pull_comments_at(out, file).map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
    let mut pass = Pass::new(root, moves);
    let tip = match git(root, &["show", "--no-textconv", &format!("{}:./{file}", base.tip)]) {
        Some(text) => pass.layout_of(file, &text),
        None => Layout::default(),
    };
    pass.changes = vec![Vec::new(); tip.spans.len()];
    let start: BTreeMap<Key, usize> = tip.spans.iter().enumerate().map(|(i, span)| (span.key(), i)).collect();
    let newest = if start.is_empty() { None } else { pass.walk(&base.tip, file, start, 0)? };

    // O título e o número do pull request vêm do commit guardado na janela da
    // montagem; o de fora dela, da base inteira lida numa chamada, sem os
    // arquivos, pela mesma regra.
    let window: HashMap<&str, Option<u32>> = if stored.base == base.name {
        stored.commits.iter().map(|commit| (commit.id.as_str(), commit.pr)).collect()
    } else {
        HashMap::new()
    };
    let referenced: BTreeSet<&str> = pass.changes.iter().flatten().map(|change| change.sha.as_str()).collect();
    let numbers: HashMap<String, Option<u32>> = if referenced.iter().any(|sha| !window.contains_key(short(sha))) {
        git(root, &["log", "--no-show-signature", HEADER, &base.tip])
            .map(|text| refresh::parse_headers(&text).into_iter().map(|(sha, commit)| (sha, commit.pr)).collect())
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    let mut commits: Vec<LineageCommit> = referenced
        .iter()
        .map(|sha| {
            let (at, title) = pass.seen.get(*sha).cloned().unwrap_or_default();
            let pr = match window.get(short(sha)) {
                Some(pr) => *pr,
                None => numbers.get(*sha).copied().flatten(),
            };
            LineageCommit { id: short(sha).to_string(), at, title, pr }
        })
        .collect();
    commits.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));

    let mut attached = pass.attach(file, &reviews, &tip.index(), &commits);
    let declarations: Vec<DeclLineage> = tip
        .spans
        .iter()
        .zip(&pass.changes)
        .enumerate()
        .map(|(at, (span, changes))| {
            let mut list: Vec<&Change> = Vec::new();
            for change in changes {
                if !list.iter().any(|kept| kept.sha == change.sha) {
                    list.push(change);
                }
            }
            list.sort_by_key(|change| Reverse(pass.seen.get(&change.sha).map_or(0, |(at, _)| *at)));
            DeclLineage {
                name: span.name.clone(),
                nth: span.nth,
                commits: list.iter().map(|change| DeclChange { id: short(&change.sha).to_string(), form: change.form }).collect(),
                comments: attached.remove(&at).unwrap_or_default(),
            }
        })
        .collect();
    let last_commit = file_history(&stored, file)
        .map(|found| found.last_commit)
        .unwrap_or_else(|| newest.as_deref().map(short).unwrap_or_default().to_string());
    let lineage = FileLineage {
        path: file.to_string(),
        base: base.name,
        last_commit,
        mark: refresh::FORMAT.to_string(),
        moves: u32::try_from(moves).unwrap_or(u32::MAX),
        comments: u32::try_from(reviews.len()).unwrap_or(u32::MAX),
        commits,
        declarations,
    };
    store::save_lineage_at(out, &lineage)?;
    Ok(Report { file: file.to_string(), commits: lineage.commits.len(), declarations: lineage.declarations.len() })
}

/// O começo do hash, como a história guardada o escreve.
fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

/// Um commit que mudou uma declaração, com a marca de só forma.
#[derive(Clone)]
struct Change {
    sha: String,
    form: bool,
}

/// Uma declaração da versão: o nome, a ordem entre as do mesmo nome e as
/// linhas, da primeira à última, a documentação de cima inclusa.
struct Span {
    name: String,
    nth: u32,
    start: usize,
    end: usize,
}

impl Span {
    fn key(&self) -> Key {
        (self.name.clone(), self.nth)
    }
}

/// As declarações de uma versão do arquivo e, de cada linha (a partir de 1),
/// a declaração mais interna que a contém.
#[derive(Default)]
struct Layout {
    spans: Vec<Span>,
    owner: Vec<Option<usize>>,
    lines: Vec<String>,
}

impl Layout {
    fn owner_of(&self, line: usize) -> Option<usize> {
        self.owner.get(line).copied().flatten()
    }

    fn index(&self) -> HashMap<Key, usize> {
        self.spans.iter().enumerate().map(|(i, span)| (span.key(), i)).collect()
    }

    /// As linhas da declaração `i` que dizem alguma coisa: sem os espaços das
    /// pontas, só as que têm letra ou número.
    fn body(&self, i: usize) -> Vec<String> {
        let span = &self.spans[i];
        let from = span.start.saturating_sub(1).min(self.lines.len());
        let to = span.end.min(self.lines.len()).max(from);
        meaningful(self.lines[from..to].iter().map(String::as_str))
    }
}

/// As linhas que dizem alguma coisa, sem os espaços das pontas.
fn meaningful<'l>(lines: impl Iterator<Item = &'l str>) -> Vec<String> {
    lines.map(str::trim).filter(|line| line.chars().any(char::is_alphanumeric)).map(str::to_string).collect()
}

/// Quantas linhas `a` e `b` têm em comum, cada uma contada uma vez por vez
/// que aparece nos dois.
fn in_common(a: &[String], b: &[String]) -> usize {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for line in a {
        *count.entry(line.as_str()).or_default() += 1;
    }
    b.iter()
        .filter(|line| {
            count.get_mut(line.as_str()).is_some_and(|left| {
                let had = *left > 0;
                *left = left.saturating_sub(1);
                had
            })
        })
        .count()
}

/// A candidata que casa com `body`: a de corpo idêntico, senão a com mais
/// linhas em comum, desde que sejam pelo menos metade das linhas da maior das
/// duas. Empate fica com a primeira.
fn best_match(body: &[String], candidates: &[(usize, Vec<String>)]) -> Option<usize> {
    if body.is_empty() {
        return None;
    }
    if let Some((i, _)) = candidates.iter().find(|(_, other)| other.as_slice() == body) {
        return Some(*i);
    }
    let mut best: Option<(usize, usize)> = None;
    for (i, other) in candidates {
        let common = in_common(body, other);
        if common > 0 && 2 * common >= body.len().max(other.len()) && best.is_none_or(|(_, most)| common > most) {
            best = Some((*i, common));
        }
    }
    best.map(|(i, _)| i)
}

/// O texto sem nenhum espaço: o que sobra ao ignorar a forma.
fn squeezed<'l>(lines: impl Iterator<Item = &'l str>) -> String {
    lines.flat_map(str::chars).filter(|c| !c.is_whitespace()).collect()
}

/// Uma versão do arquivo lida do diff: o caminho, que diz a língua, e o
/// texto inteiro.
struct Version {
    path: String,
    text: String,
}

/// Um commit da história do arquivo: as duas versões, as linhas tiradas da
/// antiga e as postas na nova, cada uma com o número dela. Sem trecho, o
/// commit só renomeou ou mudou o modo, e o arquivo segue igual.
struct Step {
    sha: String,
    at: i64,
    title: String,
    path: String,
    old: Option<usize>,
    new: Option<usize>,
    hunks: bool,
    removed: Vec<(usize, String)>,
    added: Vec<(usize, String)>,
}

/// A declaração da ponta que nasceu num commit do arquivo seguido: pode ter
/// vindo de outro arquivo do mesmo commit.
struct Birth {
    ident: usize,
    sha: String,
    path: String,
    body: Vec<String>,
}

/// Uma leitura: os analisadores já compilados, os commits que o projeto
/// manda ignorar e o que se achou de cada declaração da ponta.
struct Pass<'r> {
    root: &'r Path,
    analyzers: HashMap<String, Option<Analyzer>>,
    ignored: Vec<String>,
    /// De cada declaração da ponta, na ordem do arquivo, os commits que a
    /// mudaram.
    changes: Vec<Vec<Change>>,
    /// Cada commit lido, pelo hash inteiro: a data e o título.
    seen: HashMap<String, (i64, String)>,
    /// Quantas vezes seguidas uma declaração é seguida para o arquivo de
    /// onde ela veio.
    moves: usize,
}

impl<'r> Pass<'r> {
    fn new(root: &'r Path, moves: usize) -> Self {
        Pass {
            root,
            analyzers: HashMap::new(),
            ignored: ignored_revs(root),
            changes: Vec::new(),
            seen: HashMap::new(),
            moves,
        }
    }

    /// Os comentários de revisão `reviews`, presos a linhas de `path`, por
    /// declaração da ponta: cada um cai na declaração que continha a linha
    /// no commit comentado e que chega à ponta pela chave de `tip`. O
    /// commit comentado que o clone não tem — o do ramo apagado depois de
    /// um squash — dá lugar ao commit da base, entre os `commits` da lista,
    /// com o número do pull request, cujo arquivo tem as linhas do ramo. O
    /// comentário cuja linha não cai numa declaração, ou cuja declaração
    /// não chegou à ponta, fica sem declaração.
    fn attach(
        &mut self,
        path: &str,
        reviews: &[PullComment],
        tip: &HashMap<Key, usize>,
        commits: &[LineageCommit],
    ) -> HashMap<usize, Vec<DeclComment>> {
        let mut layouts: HashMap<String, Option<Layout>> = HashMap::new();
        let mut attached: HashMap<usize, Vec<DeclComment>> = HashMap::new();
        for review in reviews {
            let merged = commits
                .iter()
                .find(|commit| commit.pr == Some(review.number))
                .and_then(|commit| self.seen.keys().find(|sha| short(sha) == commit.id))
                .cloned();
            let found = [Some(review.commit.clone()), merged].into_iter().flatten().find_map(|commit| {
                let layout = layouts
                    .entry(commit.clone())
                    .or_insert_with(|| {
                        git(self.root, &["show", "--no-textconv", &format!("{commit}:./{path}")])
                            .map(|text| self.layout_of(path, &text))
                    })
                    .as_ref()?;
                let line = usize::try_from(review.line).ok()?;
                let owner = layout.owner_of(line)?;
                tip.get(&layout.spans[owner].key()).copied()
            });
            if let Some(at) = found {
                attached.entry(at).or_default().push(DeclComment {
                    pr: review.number,
                    commit: short(&review.commit).to_string(),
                    body: review.body.clone(),
                });
            }
        }
        attached
    }

    /// O analisador da língua do caminho, compilado uma vez na leitura.
    fn analyzer_for(&mut self, path: &str) -> Option<String> {
        let language = detect_language(Path::new(path))?;
        self.analyzers.entry(language.clone()).or_insert_with(|| Analyzer::declarations_only(&language));
        Some(language)
    }

    fn layout_of(&mut self, path: &str, text: &str) -> Layout {
        let language = self.analyzer_for(path);
        layout(language.and_then(|l| self.analyzers.get(&l)).and_then(Option::as_ref), text)
    }

    /// As declarações de cada versão, lidas em paralelo, uma vez cada.
    fn layouts(&mut self, versions: Vec<Version>) -> Vec<Layout> {
        let languages: Vec<Option<String>> = versions.iter().map(|version| self.analyzer_for(&version.path)).collect();
        let analyzers = &self.analyzers;
        let work: Vec<(Option<String>, Version)> = languages.into_iter().zip(versions).collect();
        in_parallel(work, |(language, version)| {
            layout(language.and_then(|l| analyzers.get(&l)).and_then(Option::as_ref), &version.text)
        })
    }

    /// Segue as declarações de `start`, que estão em `path` na versão de
    /// `rev`, do commit mais novo para o mais antigo, e devolve o hash do
    /// commit mais novo lido.
    fn walk(&mut self, rev: &str, path: &str, start: BTreeMap<Key, usize>, depth: usize) -> Result<Option<String>> {
        let mut args = vec!["log", "--follow", "-M", "-p", WHOLE_FILE, "--diff-algorithm=histogram"];
        args.extend(PLAIN_DIFF);
        args.extend([HEADER, rev, "--", path]);
        let text = git(self.root, &args).ok_or_else(|| anyhow!("git log could not read the history of {path}"))?;
        let (steps, versions) = read_steps(&text);
        let layouts = self.layouts(versions);
        let empty = Layout::default();
        let mut current = start;
        let mut births: Vec<Birth> = Vec::new();
        let newest = steps.first().map(|step| step.sha.clone());
        for step in &steps {
            if current.is_empty() {
                break;
            }
            if !step.hunks {
                continue;
            }
            let new = step.new.map_or(&empty, |v| &layouts[v]);
            let old = step.old.map_or(&empty, |v| &layouts[v]);
            let (new_index, old_index) = (new.index(), old.index());
            let counterpart = counterparts(new, old, &new_index, &old_index);
            let mut added_to: HashMap<usize, Vec<&str>> = HashMap::new();
            for (line, text) in &step.added {
                if let Some(owner) = new.owner_of(*line) {
                    added_to.entry(owner).or_default().push(text);
                }
            }
            let mut removed_from: HashMap<usize, Vec<&str>> = HashMap::new();
            for (line, text) in &step.removed {
                if let Some(owner) = old.owner_of(*line) {
                    removed_from.entry(owner).or_default().push(text);
                }
            }
            let ignored = self.ignored.iter().any(|rev| step.sha.starts_with(rev.as_str()));
            let mut next = BTreeMap::new();
            for (key, ident) in current {
                let Some(&at_new) = new_index.get(&key) else {
                    next.insert(key, ident);
                    continue;
                };
                let at_old = counterpart[at_new];
                let added = added_to.get(&at_new).map_or(&[][..], Vec::as_slice);
                let removed = at_old.and_then(|i| removed_from.get(&i)).map_or(&[][..], Vec::as_slice);
                if !added.is_empty() || !removed.is_empty() {
                    let form = ignored || squeezed(added.iter().copied()) == squeezed(removed.iter().copied());
                    self.changes[ident].push(Change { sha: step.sha.clone(), form });
                    self.seen.entry(step.sha.clone()).or_insert_with(|| (step.at, step.title.clone()));
                }
                match at_old {
                    Some(i) => {
                        next.insert(old.spans[i].key(), ident);
                    }
                    None => births.push(Birth { ident, sha: step.sha.clone(), path: step.path.clone(), body: new.body(at_new) }),
                }
            }
            current = next;
        }
        if depth < self.moves && !births.is_empty() {
            self.follow_moves(births, depth)?;
        }
        Ok(newest)
    }

    /// A declaração nascida num commit que veio de outro arquivo do mesmo
    /// commit segue a história nele, a partir do commit de antes.
    fn follow_moves(&mut self, births: Vec<Birth>, depth: usize) -> Result<()> {
        let shas: BTreeSet<&str> = births.iter().map(|birth| birth.sha.as_str()).collect();
        let mut args = vec!["show", "-U0", "-M", "--diff-filter=a", "--format=%x00%H"];
        args.extend(PLAIN_DIFF);
        args.extend(shas.iter().copied());
        let Some(text) = git(self.root, &args) else {
            return Ok(());
        };
        let mut removed_in: HashMap<&str, Vec<FileDiff>> = HashMap::new();
        for block in text.split('\0').filter(|block| !block.trim().is_empty()) {
            let (sha, diff) = block.split_once('\n').unwrap_or((block, ""));
            removed_in.insert(sha.trim(), read_files(diff));
        }
        let mut moves: Vec<(String, String, Key, usize)> = Vec::new();
        for birth in &births {
            let Some(files) = removed_in.get(birth.sha.as_str()) else {
                continue;
            };
            let mut candidates: Vec<(usize, &FileDiff)> = files
                .iter()
                .filter(|diff| diff.new_path != birth.path && !diff.old_path.is_empty())
                .map(|diff| {
                    let removed = meaningful(diff.removed.iter().map(|(_, text)| text.as_str()));
                    (in_common(&birth.body, &removed), diff)
                })
                .filter(|(common, _)| *common > 0 && 2 * common >= birth.body.len())
                .collect();
            candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.old_path.cmp(&b.1.old_path)));
            for (_, diff) in candidates {
                let before = format!("{}^", birth.sha);
                let Some(old_text) = git(self.root, &["show", "--no-textconv", &format!("{before}:./{}", diff.old_path)]) else {
                    continue;
                };
                let new_text = if diff.new_path.is_empty() {
                    Some(String::new())
                } else {
                    git(self.root, &["show", "--no-textconv", &format!("{}:./{}", birth.sha, diff.new_path)])
                };
                let old = self.layout_of(&diff.old_path, &old_text);
                let new = new_text.map(|text| self.layout_of(&diff.new_path, &text)).unwrap_or_default();
                let new_index = new.index();
                let gone: Vec<(usize, Vec<String>)> = (0..old.spans.len())
                    .filter(|&i| !new_index.contains_key(&old.spans[i].key()))
                    .map(|i| (i, old.body(i)))
                    .collect();
                if let Some(i) = best_match(&birth.body, &gone) {
                    // Mudar de arquivo com o corpo idêntico não muda a
                    // declaração: o commit só a levou de lugar.
                    if gone.iter().any(|(at, body)| *at == i && *body == birth.body) {
                        self.changes[birth.ident].retain(|change| change.sha != birth.sha);
                    }
                    moves.push((before, diff.old_path.clone(), old.spans[i].key(), birth.ident));
                    break;
                }
            }
        }
        for (rev, path, key, ident) in moves {
            self.walk(&rev, &path, BTreeMap::from([(key, ident)]), depth + 1)?;
        }
        Ok(())
    }
}

/// De cada declaração da versão nova, a da versão antiga que ela era: a do
/// mesmo nome e ordem; a que nasceu casa com a que sumiu pelo corpo.
fn counterparts(new: &Layout, old: &Layout, new_index: &HashMap<Key, usize>, old_index: &HashMap<Key, usize>) -> Vec<Option<usize>> {
    let mut out: Vec<Option<usize>> = new.spans.iter().map(|span| old_index.get(&span.key()).copied()).collect();
    let mut gone: Vec<(usize, Vec<String>)> = (0..old.spans.len())
        .filter(|&i| !new_index.contains_key(&old.spans[i].key()))
        .map(|i| (i, old.body(i)))
        .collect();
    for (at, slot) in out.iter_mut().enumerate() {
        if slot.is_some() || gone.is_empty() {
            continue;
        }
        if let Some(i) = best_match(&new.body(at), &gone) {
            *slot = Some(i);
            gone.retain(|(other, _)| *other != i);
        }
    }
    out
}

/// As declarações de `text` pelo analisador, sem nenhuma quando a língua
/// não é conhecida.
fn layout(analyzer: Option<&Analyzer>, text: &str) -> Layout {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let Some(analyzer) = analyzer else {
        return Layout { lines, ..Layout::default() };
    };
    let keep = Keep { written_text: false, texts_and_routes: false };
    let extracted = analyzer.extract(text, keep, &routes::Project::default());
    let mut seen: HashMap<String, u32> = HashMap::new();
    let spans: Vec<Span> = extracted
        .declarations
        .into_iter()
        .zip(extracted.tops)
        .map(|(decl, top)| {
            let nth = seen.entry(decl.name.clone()).or_default();
            let span = Span { nth: *nth, start: top.min(decl.line), end: decl.end_line.max(decl.line), name: decl.name };
            *nth += 1;
            span
        })
        .collect();
    let mut owner = vec![None; lines.len() + 2];
    let mut widest_first: Vec<usize> = (0..spans.len()).collect();
    widest_first.sort_by_key(|&i| Reverse(spans[i].end - spans[i].start));
    for i in widest_first {
        let (start, end) = (spans[i].start.max(1), spans[i].end.min(lines.len() + 1));
        for slot in owner.iter_mut().take(end + 1).skip(start) {
            *slot = Some(i);
        }
    }
    Layout { spans, owner, lines }
}

/// Os commits que o projeto manda ignorar na autoria, um por linha no
/// `.git-blame-ignore-revs`, sem os comentários.
fn ignored_revs(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join(".git-blame-ignore-revs")).unwrap_or_default();
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim().to_ascii_lowercase())
        .filter(|rev| rev.len() >= 7 && rev.chars().all(|c| c.is_ascii_hexdigit()))
        .collect()
}

/// Os commits da saída do `git log --follow -p`, do mais novo para o mais
/// antigo, sem os merges, e as versões do arquivo que eles trazem, cada uma
/// uma vez.
fn read_steps(text: &str) -> (Vec<Step>, Vec<Version>) {
    let mut steps = Vec::new();
    let mut versions: Vec<Version> = Vec::new();
    let mut known: HashMap<String, usize> = HashMap::new();
    for block in text.split('\0').filter(|block| !block.trim().is_empty()) {
        let (header, diff) = block.split_once('\n').unwrap_or((block, ""));
        let (ids, title) = header.split_once('\x1f').unwrap_or((header, ""));
        let mut ids = ids.split_whitespace();
        let (Some(sha), Some(at)) = (ids.next(), ids.next()) else {
            continue;
        };
        if ids.count() > 1 {
            continue;
        }
        let file = read_files(diff).into_iter().next().unwrap_or_default();
        let mut version_of = |oid: &str, path: &str, text: String, side: &str| -> Option<usize> {
            if path.is_empty() {
                return None;
            }
            let id = if oid.is_empty() { format!("{sha}:{side}") } else { oid.to_string() };
            Some(*known.entry(id).or_insert_with(|| {
                versions.push(Version { path: path.to_string(), text });
                versions.len() - 1
            }))
        };
        let (old, new) = if file.hunks {
            (
                version_of(&file.old_oid, &file.old_path, file.old_text, "old"),
                version_of(&file.new_oid, &file.new_path, file.new_text, "new"),
            )
        } else {
            (None, None)
        };
        steps.push(Step {
            sha: sha.to_string(),
            at: at.parse().unwrap_or(0),
            title: title.trim().to_string(),
            path: file.new_path,
            old,
            new,
            hunks: file.hunks,
            removed: file.removed,
            added: file.added,
        });
    }
    (steps, versions)
}

/// Um arquivo de um diff: os caminhos (vazio do lado em que ele não existe),
/// o hash de cada versão, o texto de cada lado que o diff mostra e as linhas
/// tiradas e postas, com o número de cada uma.
#[derive(Default)]
struct FileDiff {
    old_path: String,
    new_path: String,
    old_oid: String,
    new_oid: String,
    old_text: String,
    new_text: String,
    removed: Vec<(usize, String)>,
    added: Vec<(usize, String)>,
    hunks: bool,
}

/// Os arquivos de um diff do git, em ordem.
fn read_files(diff: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut in_hunk = false;
    let (mut old_at, mut new_at) = (0usize, 0usize);
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let mut file = FileDiff::default();
            if let Some((old, new)) = rest.split_once(" b/") {
                file.old_path = old.strip_prefix("a/").unwrap_or(old).to_string();
                file.new_path = new.to_string();
            }
            files.push(file);
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if in_hunk {
            match line.as_bytes().first() {
                Some(b' ') => {
                    push_line(&mut file.old_text, &line[1..]);
                    push_line(&mut file.new_text, &line[1..]);
                    old_at += 1;
                    new_at += 1;
                    continue;
                }
                Some(b'-') => {
                    push_line(&mut file.old_text, &line[1..]);
                    file.removed.push((old_at, line[1..].to_string()));
                    old_at += 1;
                    continue;
                }
                Some(b'+') => {
                    push_line(&mut file.new_text, &line[1..]);
                    file.added.push((new_at, line[1..].to_string()));
                    new_at += 1;
                    continue;
                }
                Some(b'\\') => continue,
                _ => {}
            }
        }
        if let Some(hunk) = line.strip_prefix("@@ -") {
            let mut ranges = hunk.split_whitespace();
            old_at = first_line(ranges.next().unwrap_or(""));
            new_at = first_line(ranges.next().unwrap_or("").trim_start_matches('+'));
            file.hunks = true;
            in_hunk = true;
        } else if in_hunk {
            continue;
        } else if let Some(path) = line.strip_prefix("rename from ") {
            file.old_path = unquote(path);
        } else if let Some(path) = line.strip_prefix("rename to ") {
            file.new_path = unquote(path);
        } else if let Some(ids) = line.strip_prefix("index ") {
            let range = ids.split_whitespace().next().unwrap_or("");
            let (old, new) = range.split_once("..").unwrap_or(("", ""));
            file.old_oid = real_oid(old);
            file.new_oid = real_oid(new);
        } else if let Some(path) = line.strip_prefix("--- ") {
            file.old_path = side_path(path, "a/");
        } else if let Some(path) = line.strip_prefix("+++ ") {
            file.new_path = side_path(path, "b/");
        } else if line.starts_with("new file mode") {
            file.old_path.clear();
        } else if line.starts_with("deleted file mode") {
            file.new_path.clear();
        }
    }
    files
}

fn push_line(text: &mut String, line: &str) {
    text.push_str(line);
    text.push('\n');
}

/// A primeira linha de um lado de um trecho, `12,3` ou `12`: com zero
/// linhas, o número dado é o da linha de antes.
fn first_line(range: &str) -> usize {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    let start: usize = start.parse().unwrap_or(0);
    if count == "0" { start + 1 } else { start }
}

/// O hash de uma versão, vazio quando o lado não existe.
fn real_oid(oid: &str) -> String {
    if oid.chars().all(|c| c == '0') { String::new() } else { oid.to_string() }
}

/// O caminho de um lado do diff, sem o prefixo; vazio quando o lado não
/// existe.
fn side_path(raw: &str, prefix: &str) -> String {
    let path = unquote(raw.trim_end_matches('\t'));
    if path == "/dev/null" {
        return String::new();
    }
    path.strip_prefix(prefix).unwrap_or(&path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_file_diff_gives_both_versions_and_the_numbered_lines() {
        let diff = "diff --git a/src/a.x b/src/b.x\nsimilarity index 80%\nrename from src/a.x\nrename to src/b.x\n\
                    index 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644\n\
                    --- a/src/a.x\n+++ b/src/b.x\n@@ -1,3 +1,3 @@\n um\n-dois\n+Dois\n tres\n";
        let files = read_files(diff);
        assert_eq!(files.len(), 1);
        let file = &files[0];
        assert_eq!((file.old_path.as_str(), file.new_path.as_str()), ("src/a.x", "src/b.x"));
        assert_eq!(file.old_text, "um\ndois\ntres\n");
        assert_eq!(file.new_text, "um\nDois\ntres\n");
        assert_eq!(file.removed, vec![(2, "dois".to_string())]);
        assert_eq!(file.added, vec![(2, "Dois".to_string())]);
        assert_eq!(file.old_oid, "1111111111111111111111111111111111111111");
    }

    #[test]
    fn a_new_file_has_no_old_side_and_an_empty_range_starts_after_its_line() {
        let diff = "diff --git a/n.x b/n.x\nnew file mode 100644\nindex 0000000000..3333333333\n--- /dev/null\n+++ b/n.x\n@@ -0,0 +1,2 @@\n+a\n+b\n";
        let file = &read_files(diff)[0];
        assert!(file.old_path.is_empty());
        assert!(file.old_oid.is_empty());
        assert_eq!(file.added, vec![(1, "a".to_string()), (2, "b".to_string())]);
        assert_eq!(first_line("7,0"), 8);
        assert_eq!(first_line("7"), 7);
    }

    #[test]
    fn half_the_lines_in_common_is_the_floor_of_a_match() {
        let lines = |text: &str| meaningful(text.lines());
        let body = lines("fn a() {\nlet x = 1;\nlet y = 2;\nx + y\n}");
        let near = lines("fn b() {\nlet x = 1;\nlet y = 2;\nx * y\n}");
        let far = lines("fn c() {\nlet z = 3;\nz\n}");
        assert_eq!(best_match(&body, &[(0, far.clone()), (1, near)]), Some(1));
        assert_eq!(best_match(&body, &[(0, far)]), None);
        assert_eq!(squeezed(["a  (b)", "c"].into_iter()), squeezed(["a(b) c"].into_iter()));
    }
}
