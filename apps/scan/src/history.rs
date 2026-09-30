//! A história de cada declaração de um arquivo na branch de partida, montada
//! na primeira pergunta sobre ele e gravada no mapa.
//!
//! A passada lê só aquele arquivo: o `git log --follow` da ponta da base para
//! trás, a janela da montagem e o que vem antes dela, com as duas versões
//! inteiras do arquivo no diff de cada commit. Cada versão se lê uma vez, com
//! só a língua dela compilada, e cada declaração vira uma faixa de linhas que
//! inclui a documentação e os enfeites logo acima. Cada linha tirada ou posta
//! vai para a declaração mais interna que a contém, do seu lado; a linha entre
//! declarações não vai a nenhuma. As declarações escritas na mesma linha —
//! a variante de enumeração com os campos ao lado, a estrutura de uma linha
//! só — ocupam as mesmas linhas, e a linha é de todas elas.
//!
//! A declaração que some de um lado e nasce do outro no mesmo commit se casa
//! pelo corpo idêntico, depois por pelo menos metade das linhas em comum:
//! primeiro no mesmo arquivo, depois nos outros arquivos do commit. A que veio
//! de outro arquivo segue a história nele, só antes daquele commit e só para
//! ela. O commit que só muda espaços na declaração, ou que o projeto lista no
//! `.git-blame-ignore-revs`, fica na lista com a marca de só forma.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Result};
use mustard_core::domain::project_map::{
    file_history, CommitFiles, DeclChange, DeclComment, DeclLineage, FileLineage, History, LineageCommit, PullComment,
    CO_CHANGE_MAX_FILES,
};
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::map_lineage;
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
/// Sem base ou sem ela no clone, com o mapa ilegível, quando o git não lê a
/// história do arquivo ou quando a gravação falha.
pub(crate) fn run(root: &Path, out: &Path, file: &str, moves: usize) -> Result<Report> {
    let base = store::base_of(root);
    if base.name.is_empty() || base.tip.is_empty() {
        return Err(anyhow!("the project has no base branch this clone has"));
    }
    let stored = store::history_at(out).map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
    let reviews = store::pull_comments_at(out, file).map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
    let shared = Shared::default();
    let lineage = Reader::new(root, &base, &stored, moves, &shared).trace(&reviews, file)?;
    store::save_lineage_at(out, &lineage)?;
    Ok(Report { file: file.to_string(), commits: lineage.commits.len(), declarations: lineage.declarations.len() })
}

/// Quantos arquivos vão numa gravação só da leitura do projeto inteiro: cada
/// lote é uma transação, e a busca que chega no meio espera só a gravação
/// dele.
pub(crate) const BATCH: usize = 25;

/// O que a leitura do projeto inteiro fez: quantos arquivos leu e gravou,
/// quantos commits e declarações eles somam, e quantos o git não deixou ler.
#[derive(Default)]
pub(crate) struct AllReport {
    pub files: usize,
    pub commits: usize,
    pub declarations: usize,
    pub failed: usize,
    /// Outra leitura do mesmo mapa estava rodando: esta não leu nada.
    pub busy: bool,
}

/// Lê a história de todo arquivo do mapa em `out` que ainda não a tem, ou
/// cuja marca venceu ([`map_lineage::wanted_at`]), e a grava em lotes de
/// `batch` arquivos, cada lote numa transação. Só um processo lê de cada mapa
/// de cada vez: quem chega com outro rodando sai sem ler. Vários arquivos se
/// leem ao mesmo tempo dentro de cada lote ([`workers`]). Ao fim de uma
/// passada, vê o que faltou de novo: o mapa pode ter ganhado arquivo, ou
/// commit, enquanto ela lia. O arquivo que o git não lê fica de fora do resto
/// desta leitura.
///
/// # Errors
///
/// Com o mapa ilegível ou quando a gravação falha; uma história de arquivo
/// que o git não lê não é erro, só conta em `failed`.
pub(crate) fn run_all(root: &Path, out: &Path, moves: usize, batch: usize) -> Result<AllReport> {
    let mut report = AllReport::default();
    let Some(_alone) = LockedFile::exclusive_if_free(&map_lineage::reading_lock_path(out))? else {
        report.busy = true;
        return Ok(report);
    };
    let mut left_out: BTreeSet<String> = BTreeSet::new();
    loop {
        let base = store::base_of(root);
        if base.name.is_empty() || base.tip.is_empty() {
            break;
        }
        let wanted: Vec<String> = map_lineage::wanted_at(out, moves)
            .map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?
            .into_iter()
            .filter(|path| !left_out.contains(path))
            .collect();
        if wanted.is_empty() {
            break;
        }
        let stored = store::history_at(out).map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
        let shared = Shared::default();
        let mut readers: Vec<Reader> = (0..workers().min(wanted.len())).map(|_| Reader::new(root, &base, &stored, moves, &shared)).collect();
        for files in wanted.chunks(batch.max(1)) {
            let paths: Vec<&str> = files.iter().map(String::as_str).collect();
            let comments = store::pull_comments_for_at(out, &paths)
                .map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
            let mut lineages: Vec<FileLineage> = Vec::with_capacity(files.len());
            for (file, traced) in files.iter().zip(trace_all(&mut readers, files, &comments)) {
                match traced {
                    Ok(lineage) => lineages.push(lineage),
                    Err(_) => {
                        report.failed += 1;
                        left_out.insert(file.clone());
                    }
                }
            }
            store::save_lineages_at(out, &lineages)?;
            report.files += lineages.len();
            report.commits += lineages.iter().map(|lineage| lineage.commits.len()).sum::<usize>();
            report.declarations += lineages.iter().map(|lineage| lineage.declarations.len()).sum::<usize>();
        }
    }
    Ok(report)
}

/// Quantos arquivos se leem ao mesmo tempo: cada um já divide a análise das
/// versões entre os núcleos, então poucos bastam para manter todos ocupados
/// enquanto os outros esperam o git.
fn workers() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| (cores.get() / 2).clamp(1, 4))
}

/// A história de cada arquivo de `files`, na ordem deles: cada leitor de
/// `readers` lê um arquivo por vez, e os arquivos vão para o primeiro que
/// ficar livre. Os comentários de revisão de cada arquivo saem de `comments`.
fn trace_all(readers: &mut [Reader], files: &[String], comments: &[PullComment]) -> Vec<Result<FileLineage>> {
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, Result<FileLineage>)>> = Mutex::new(Vec::with_capacity(files.len()));
    std::thread::scope(|scope| {
        for reader in readers.iter_mut() {
            let (next, done) = (&next, &done);
            scope.spawn(move || loop {
                let at = next.fetch_add(1, Ordering::Relaxed);
                let Some(file) = files.get(at) else { break };
                let reviews: Vec<PullComment> = comments.iter().filter(|comment| comment.path == *file).cloned().collect();
                let traced = reader.trace(&reviews, file);
                done.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((at, traced));
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    done.sort_by_key(|(at, _)| *at);
    done.into_iter().map(|(_, traced)| traced).collect()
}

/// A chave de um texto já analisado: o caminho, o tamanho e dois resumos dele.
type TextKey = (String, usize, u64, u64);

/// O que os leitores de uma mesma passada repartem: o que um deles lê ou
/// analisa serve aos outros, porque a mesma versão de um arquivo e o mesmo
/// commit voltam na leitura de vários arquivos.
#[derive(Default)]
pub(crate) struct Shared {
    /// O número do pull request de cada commit da base inteira, lido do git
    /// na primeira vez que um arquivo cita commit de fora da janela do mapa,
    /// por quem chegar primeiro.
    numbers: OnceLock<HashMap<String, Option<u32>>>,
    /// As declarações de cada versão de arquivo já analisadas, pelo caminho e
    /// pelo texto. Ler a árvore da versão custa mais que todo o resto da
    /// leitura, e a mesma versão volta na ponta do arquivo, na história dele
    /// e na de cada arquivo de onde uma declaração veio.
    parsed: Mutex<HashMap<TextKey, Arc<Vec<Span>>>>,
    /// O que cada commit tirou de arquivo que já existia, como o git o
    /// mostra: o commit que muitos arquivos citam como nascimento (a
    /// importação inicial, uma mudança de pasta) é lido uma vez só.
    removals: Mutex<HashMap<String, Arc<String>>>,
}

/// A leitura da história de vários arquivos da mesma base: o que se prepara
/// uma vez — as línguas compiladas, os commits que o projeto manda ignorar e
/// o que a passada reparte ([`Shared`]) — vale para todos os arquivos.
pub(crate) struct Reader<'r> {
    root: &'r Path,
    base: &'r store::Base,
    /// A história do git que o mapa guarda.
    stored: &'r History,
    moves: usize,
    analyzers: HashMap<String, Option<Analyzer>>,
    ignored: Vec<String>,
    shared: &'r Shared,
}

impl<'r> Reader<'r> {
    pub(crate) fn new(
        root: &'r Path,
        base: &'r store::Base,
        stored: &'r History,
        moves: usize,
        shared: &'r Shared,
    ) -> Self {
        Reader { root, base, stored, moves, analyzers: HashMap::new(), ignored: ignored_revs(root), shared }
    }

    /// A história das declarações de `file` na ponta da base, montada sem
    /// gravar; `reviews` são os comentários de revisão presos ao arquivo.
    ///
    /// # Errors
    ///
    /// Quando o git não lê a história do arquivo.
    pub(crate) fn trace(&mut self, reviews: &[PullComment], file: &str) -> Result<FileLineage> {
        let (root, base, stored, moves) = (self.root, self.base, self.stored, self.moves);
        let mut pass = Pass::new(root, moves, &mut self.analyzers, &self.ignored, self.shared);
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
        if referenced.iter().any(|sha| !window.contains_key(short(sha))) {
            self.shared.numbers.get_or_init(|| {
                git(root, &["log", "--no-show-signature", HEADER, &base.tip])
                    .map(|text| refresh::parse_headers(&text).into_iter().map(|(sha, commit)| (sha, commit.pr)).collect())
                    .unwrap_or_default()
            });
        }
        let numbers = self.shared.numbers.get();
        // Os arquivos que cada commit criou e mudou, para a receita do arquivo
        // além da janela da montagem.
        let mut files = files_of(root, &referenced.iter().copied().collect::<Vec<_>>());
        let mut commits: Vec<LineageCommit> = referenced
            .iter()
            .map(|sha| {
                let (at, title) = pass.seen.get(*sha).cloned().unwrap_or_default();
                let pr = match window.get(short(sha)) {
                    Some(pr) => *pr,
                    None => numbers.and_then(|all| all.get(*sha)).copied().flatten(),
                };
                let files = files.remove(*sha).unwrap_or_default();
                LineageCommit { id: short(sha).to_string(), at, title, pr, files }
            })
            .collect();
        commits.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));

        let mut attached = pass.attach(file, reviews, &tip.index(), &commits);
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
        let last_commit = file_history(stored, file)
            .map(|found| found.last_commit)
            .unwrap_or_else(|| newest.as_deref().map(short).unwrap_or_default().to_string());
        Ok(FileLineage {
            path: file.to_string(),
            base: base.name.clone(),
            last_commit,
            mark: refresh::FORMAT.to_string(),
            moves: u32::try_from(moves).unwrap_or(u32::MAX),
            comments: u32::try_from(reviews.len()).unwrap_or(u32::MAX),
            commits,
            declarations,
        })
    }
}

/// Quantos commits vão numa chamada só ao git que lê os arquivos de cada um.
const FILES_BATCH: usize = 200;

/// Os arquivos que cada commit de `shas` criou e mudou, pelo hash inteiro,
/// lidos do git em lotes, com os caminhos como a história da montagem os
/// guarda. O commit que muda mais de [`CO_CHANGE_MAX_FILES`] arquivos fica
/// sem eles: ele não conta para "muda junto".
fn files_of(root: &Path, shas: &[&str]) -> HashMap<String, CommitFiles> {
    let mut out = HashMap::new();
    for batch in shas.chunks(FILES_BATCH) {
        let mut args = vec!["log", "--no-walk=unsorted", "--no-show-signature", "--no-renames", "--relative", "--name-status", HEADER];
        args.extend(batch);
        let Some(text) = git(root, &args) else { continue };
        for (sha, commit) in refresh::parse_headers(&text) {
            if commit.added.len() + commit.changed.len() <= CO_CHANGE_MAX_FILES {
                out.insert(sha, CommitFiles { added: commit.added, changed: commit.changed });
            }
        }
    }
    out
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

impl Shared {
    /// As declarações da versão `text` do arquivo `path`: as já analisadas
    /// nesta passada, ou as que `read` analisa agora e fica guardando. A chave
    /// leva o caminho, o tamanho e dois resumos do texto.
    fn spans_of(&self, path: &str, text: &str, read: impl FnOnce() -> Vec<Span>) -> Arc<Vec<Span>> {
        use std::hash::{Hash, Hasher};
        let (mut first, mut second) = (std::collections::hash_map::DefaultHasher::new(), std::collections::hash_map::DefaultHasher::new());
        text.hash(&mut first);
        (path, text, 1u8).hash(&mut second);
        let key = (path.to_string(), text.len(), first.finish(), second.finish());
        let known = self.parsed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key).cloned();
        if let Some(spans) = known {
            return spans;
        }
        let spans = Arc::new(read());
        self.parsed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key, Arc::clone(&spans));
        spans
    }
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
    spans: Arc<Vec<Span>>,
    /// De cada linha, as declarações mais internas que a contêm: mais de uma
    /// só quando várias ocupam exatamente as mesmas linhas.
    owners: Vec<Vec<usize>>,
    lines: Vec<String>,
}

impl Layout {
    /// A declaração mais interna da linha; entre as que ocupam as mesmas
    /// linhas, a última do arquivo.
    fn owner_of(&self, line: usize) -> Option<usize> {
        self.owners_of(line).last().copied()
    }

    /// Todas as declarações mais internas da linha, na ordem do arquivo.
    fn owners_of(&self, line: usize) -> &[usize] {
        self.owners.get(line).map_or(&[], Vec::as_slice)
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
    /// Os analisadores, compilados uma vez para todos os arquivos da leitura.
    analyzers: &'r mut HashMap<String, Option<Analyzer>>,
    ignored: &'r [String],
    shared: &'r Shared,
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
    fn new(
        root: &'r Path,
        moves: usize,
        analyzers: &'r mut HashMap<String, Option<Analyzer>>,
        ignored: &'r [String],
        shared: &'r Shared,
    ) -> Self {
        Pass {
            root,
            analyzers,
            ignored,
            shared,
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
        layout(language.and_then(|l| self.analyzers.get(&l)).and_then(Option::as_ref), path, text, self.shared)
    }

    /// As declarações de cada versão, lidas em paralelo, uma vez cada.
    fn layouts(&mut self, versions: Vec<Version>) -> Vec<Layout> {
        let languages: Vec<Option<String>> = versions.iter().map(|version| self.analyzer_for(&version.path)).collect();
        let (analyzers, shared) = (&*self.analyzers, self.shared);
        let work: Vec<(Option<String>, Version)> = languages.into_iter().zip(versions).collect();
        in_parallel(work, |(language, version)| {
            layout(language.and_then(|l| analyzers.get(&l)).and_then(Option::as_ref), &version.path, &version.text, shared)
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
            let new_index = new.index();
            let counterpart = counterparts(new, old, &carried_lines(step, new.lines.len(), old.lines.len()));
            let mut added_to: HashMap<usize, Vec<&str>> = HashMap::new();
            for (line, text) in &step.added {
                for &owner in new.owners_of(*line) {
                    added_to.entry(owner).or_default().push(text);
                }
            }
            let mut removed_from: HashMap<usize, Vec<&str>> = HashMap::new();
            for (line, text) in &step.removed {
                for &owner in old.owners_of(*line) {
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
                // A declaração que a versão de antes não tinha nasceu neste
                // commit, mesmo que o diff dê a linha dela por igual à de
                // outra coisa (o parâmetro de uma função que virou campo).
                if !added.is_empty() || !removed.is_empty() || at_old.is_none() {
                    let touched = !added.is_empty() || !removed.is_empty();
                    let form = ignored || (touched && squeezed(added.iter().copied()) == squeezed(removed.iter().copied()));
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
        let diffs = self.removals(&shas);
        // Por commit, os arquivos de onde algo saiu. Cada arquivo é lido e
        // analisado uma vez, na primeira vez que uma declaração nascida no
        // commit o procura, e serve a todas as outras do mesmo commit.
        let mut removed_in: HashMap<&str, Vec<Source>> = HashMap::new();
        for sha in &shas {
            if let Some(diff) = diffs.get(*sha) {
                removed_in.insert(sha, read_files(diff).into_iter().map(Source::new).collect());
            }
        }
        let mut moves: Vec<(String, String, Key, usize)> = Vec::new();
        for birth in &births {
            let Some(sources) = removed_in.get_mut(birth.sha.as_str()) else {
                continue;
            };
            let mut candidates: Vec<(usize, usize)> = sources
                .iter()
                .enumerate()
                .filter(|(_, source)| source.diff.new_path != birth.path && !source.diff.old_path.is_empty())
                .map(|(at, source)| (in_common(&birth.body, &source.removed), at))
                .filter(|(common, _)| *common > 0 && 2 * common >= birth.body.len())
                .collect();
            candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| sources[a.1].diff.old_path.cmp(&sources[b.1].diff.old_path)));
            for (_, at) in candidates {
                let before = format!("{}^", birth.sha);
                let source = &mut sources[at];
                if source.loaded.is_none() {
                    source.loaded = Some(self.load_source(&before, &birth.sha, &source.diff));
                }
                let Some(Some(loaded)) = source.loaded.as_ref() else {
                    continue;
                };
                if let Some(i) = best_match(&birth.body, &loaded.gone) {
                    // Mudar de arquivo com o corpo idêntico não muda a
                    // declaração: o commit só a levou de lugar.
                    if loaded.gone.iter().any(|(other, body)| *other == i && *body == birth.body) {
                        self.changes[birth.ident].retain(|change| change.sha != birth.sha);
                    }
                    moves.push((before, source.diff.old_path.clone(), loaded.old.spans[i].key(), birth.ident));
                    break;
                }
            }
        }
        // Uma leitura por arquivo de origem serve a todas as declarações que
        // vieram dele; duas que tenham a mesma chave nele leem em separado.
        let mut starts: BTreeMap<(String, String), Vec<BTreeMap<Key, usize>>> = BTreeMap::new();
        for (rev, path, key, ident) in moves {
            let lists = starts.entry((rev, path)).or_default();
            match lists.iter_mut().find(|list| !list.contains_key(&key)) {
                Some(list) => {
                    list.insert(key, ident);
                }
                None => lists.push(BTreeMap::from([(key, ident)])),
            }
        }
        for ((rev, path), lists) in starts {
            for start in lists {
                self.walk(&rev, &path, start, depth + 1)?;
            }
        }
        Ok(())
    }

    /// O que cada commit de `shas` tirou de arquivo que já existia, como o
    /// `git show` o mostra: o que a passada já leu não volta ao git.
    fn removals(&self, shas: &BTreeSet<&str>) -> HashMap<String, Arc<String>> {
        let known = || self.shared.removals.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let missing: Vec<&str> = {
            let known = known();
            shas.iter().copied().filter(|sha| !known.contains_key(*sha)).collect()
        };
        if !missing.is_empty() {
            let mut args = vec!["show", "-U0", "-M", "--diff-filter=a", "--format=%x00%H"];
            args.extend(PLAIN_DIFF);
            args.extend(missing.iter().copied());
            if let Some(text) = git(self.root, &args) {
                let mut known = known();
                for block in text.split('\0').filter(|block| !block.trim().is_empty()) {
                    let (sha, diff) = block.split_once('\n').unwrap_or((block, ""));
                    known.insert(sha.trim().to_string(), Arc::new(diff.to_string()));
                }
            }
        }
        let known = known();
        shas.iter().filter_map(|sha| known.get(*sha).map(|diff| ((*sha).to_string(), Arc::clone(diff)))).collect()
    }

    /// O arquivo de onde o commit `sha` tirou linhas, lido no commit de antes
    /// (`before`): as declarações dele e as que o commit deixou de ter no
    /// arquivo para onde foi. `None` quando o git não o lê.
    fn load_source(&mut self, before: &str, sha: &str, diff: &FileDiff) -> Option<Loaded> {
        let old_text = git(self.root, &["show", "--no-textconv", &format!("{before}:./{}", diff.old_path)])?;
        let new_text = if diff.new_path.is_empty() {
            Some(String::new())
        } else {
            git(self.root, &["show", "--no-textconv", &format!("{sha}:./{}", diff.new_path)])
        };
        let old = self.layout_of(&diff.old_path, &old_text);
        let new = new_text.map(|text| self.layout_of(&diff.new_path, &text)).unwrap_or_default();
        let new_index = new.index();
        let gone: Vec<(usize, Vec<String>)> = (0..old.spans.len())
            .filter(|&i| !new_index.contains_key(&old.spans[i].key()))
            .map(|i| (i, old.body(i)))
            .collect();
        Some(Loaded { old, gone })
    }
}

/// Um arquivo de onde um commit tirou linhas: o que o diff dele diz, as linhas
/// tiradas que dizem alguma coisa e, depois de lido, o que se sabe dele.
struct Source {
    diff: FileDiff,
    removed: Vec<String>,
    /// `None` enquanto ninguém o procurou; `Some(None)` quando o git não o lê.
    loaded: Option<Option<Loaded>>,
}

impl Source {
    fn new(diff: FileDiff) -> Self {
        let removed = meaningful(diff.removed.iter().map(|(_, text)| text.as_str()));
        Source { diff, removed, loaded: None }
    }
}

/// As declarações do arquivo de onde as linhas saíram, no commit de antes, e
/// as que o commit deixou de ter no arquivo novo, cada uma com as linhas do
/// corpo.
struct Loaded {
    old: Layout,
    gone: Vec<(usize, Vec<String>)>,
}

/// De cada linha da versão nova (a partir de 1), a linha da antiga de que ela
/// veio: as que o commit não pôs seguem, na ordem, as que ele não tirou. A
/// linha que o commit pôs não veio de nenhuma.
fn carried_lines(step: &Step, new_len: usize, old_len: usize) -> Vec<Option<usize>> {
    let put: HashSet<usize> = step.added.iter().map(|(line, _)| *line).collect();
    let taken: HashSet<usize> = step.removed.iter().map(|(line, _)| *line).collect();
    let mut before = (1..=old_len).filter(|line| !taken.contains(line));
    let mut out: Vec<Option<usize>> = vec![None; new_len + 1];
    for line in (1..=new_len).filter(|line| !put.contains(line)) {
        out[line] = before.next();
    }
    out
}

/// De cada declaração da versão nova, a da versão antiga que ela era. Entre as
/// de nome que se repete, a ordem do nome não diz qual é qual, porque uma
/// declaração posta ou tirada acima muda a ordem das de baixo: cada uma fica
/// com a que tinha as suas linhas na versão antiga, a que tem mais delas
/// primeiro. As outras, e as que não dividem linha com nenhuma, ficam com a de
/// mesmo nome e ordem que sobrou; a que nasceu casa com a que sumiu pelo corpo.
fn counterparts(new: &Layout, old: &Layout, carried: &[Option<usize>]) -> Vec<Option<usize>> {
    let mut out: Vec<Option<usize>> = vec![None; new.spans.len()];
    let mut taken = vec![false; old.spans.len()];

    let mut old_by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (j, span) in old.spans.iter().enumerate() {
        old_by_name.entry(span.name.as_str()).or_default().push(j);
    }
    let mut new_count: HashMap<&str, usize> = HashMap::new();
    for span in new.spans.iter() {
        *new_count.entry(span.name.as_str()).or_default() += 1;
    }
    let mut shared: Vec<(usize, usize, usize)> = Vec::new();
    for (at, span) in new.spans.iter().enumerate() {
        let same = old_by_name.get(span.name.as_str()).map_or(&[][..], Vec::as_slice);
        if same.is_empty() || (same.len() == 1 && new_count[span.name.as_str()] == 1) {
            continue;
        }
        let mut lines_in: HashMap<usize, usize> = HashMap::new();
        for line in span.start.max(1)..=span.end.min(new.lines.len()) {
            let Some(Some(before)) = carried.get(line) else { continue };
            for &j in same.iter().filter(|&&j| old.spans[j].start <= *before && *before <= old.spans[j].end) {
                *lines_in.entry(j).or_default() += 1;
            }
        }
        shared.extend(lines_in.into_iter().map(|(j, lines)| (lines, at, j)));
    }
    shared.sort_by_key(|&(lines, at, j)| (Reverse(lines), old.spans[j].nth.abs_diff(new.spans[at].nth), at, j));
    for (_, at, j) in shared {
        if out[at].is_none() && !taken[j] {
            out[at] = Some(j);
            taken[j] = true;
        }
    }

    let old_index = old.index();
    for (at, span) in new.spans.iter().enumerate() {
        if out[at].is_some() {
            continue;
        }
        if let Some(&j) = old_index.get(&span.key()).filter(|&&j| !taken[j]) {
            out[at] = Some(j);
            taken[j] = true;
        }
    }

    let mut gone: Vec<(usize, Vec<String>)> = (0..old.spans.len()).filter(|&j| !taken[j]).map(|j| (j, old.body(j))).collect();
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

/// As declarações de `text`, a versão do arquivo em `path`, pelo
/// analisador, sem nenhuma quando a língua não é conhecida.
fn layout(analyzer: Option<&Analyzer>, path: &str, text: &str, shared: &Shared) -> Layout {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let Some(analyzer) = analyzer else {
        return Layout { lines, ..Layout::default() };
    };
    let spans = shared.spans_of(path, text, || {
        let keep = Keep { written_text: false, texts_and_routes: false };
        let extracted = analyzer.extract(text, keep, &routes::Project { path, ..routes::Project::default() });
        let mut seen: HashMap<String, u32> = HashMap::new();
        extracted
            .declarations
            .into_iter()
            .zip(extracted.tops)
            .map(|(decl, top)| {
                let nth = seen.entry(decl.name.clone()).or_default();
                let span = Span { nth: *nth, start: top.min(decl.line), end: decl.end_line.max(decl.line), name: decl.name };
                *nth += 1;
                span
            })
            .collect()
    });
    let mut owners: Vec<Vec<usize>> = vec![Vec::new(); lines.len() + 2];
    let mut widths: Vec<usize> = vec![usize::MAX; lines.len() + 2];
    for (i, span) in spans.iter().enumerate() {
        let (start, end) = (span.start.max(1), span.end.min(lines.len() + 1));
        let width = span.end - span.start;
        for (slot, narrowest) in owners.iter_mut().zip(widths.iter_mut()).take(end + 1).skip(start) {
            let same_lines = slot.first().is_some_and(|&first| (spans[first].start, spans[first].end) == (span.start, span.end));
            if width < *narrowest || (width == *narrowest && !same_lines) {
                *narrowest = width;
                slot.clear();
            }
            if width == *narrowest {
                slot.push(i);
            }
        }
    }
    Layout { spans, owners, lines }
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

    /// A versão que a passada já analisou não volta ao analisador: vale para o
    /// mesmo caminho e o mesmo texto, e um texto ou um caminho diferente lê
    /// de novo.
    #[test]
    fn a_version_of_a_file_already_analyzed_in_the_reading_is_not_analyzed_again() {
        let shared = Shared::default();
        let mut analyzed = 0;
        let mut read = |path: &str, text: &str| {
            shared
                .spans_of(path, text, || {
                    analyzed += 1;
                    vec![Span { name: "a".to_string(), nth: 0, start: 1, end: 2 }]
                })
                .len()
        };
        assert_eq!(read("src/a.x", "fn a() {}"), 1);
        assert_eq!(read("src/a.x", "fn a() {}"), 1);
        read("src/a.x", "fn a() { 1 }");
        read("src/b.x", "fn a() {}");
        assert_eq!(analyzed, 3, "only the first reading of a path and text is analyzed; a new text or a new path is analyzed too");
    }

    /// O que um commit tirou de arquivo é lido do git uma vez só na passada:
    /// apagado o repositório, a segunda pergunta ainda tem a resposta.
    #[test]
    fn what_a_commit_removed_from_files_is_read_from_git_once_in_the_reading() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let run = |args: &[&str]| {
            let done = std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("run git");
            assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
            String::from_utf8_lossy(&done.stdout).trim().to_string()
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "um\ndois\ntres\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "first"]);
        std::fs::write(root.join("a.txt"), "um\nquatro\ntres\n").unwrap();
        run(&["commit", "-q", "-am", "second"]);
        let sha = run(&["rev-parse", "HEAD"]);

        let (shared, mut analyzers) = (Shared::default(), HashMap::new());
        let pass = Pass::new(root, 1, &mut analyzers, &[], &shared);
        let shas: BTreeSet<&str> = BTreeSet::from([sha.as_str()]);
        let first = pass.removals(&shas);
        assert!(first[&sha].contains("-dois"), "the commit took the line `dois` out of `a.txt`: {}", first[&sha]);

        std::fs::remove_dir_all(root.join(".git")).unwrap();
        let second = pass.removals(&shas);
        assert_eq!(second[&sha], first[&sha], "the repository is gone, so the answer came from what the reading kept");
    }

    /// Outra leitura do mesmo mapa em andamento faz a nova sair sem ler
    /// nada, e a que chega depois da primeira acabar lê.
    #[test]
    fn a_reading_of_a_map_being_read_by_another_process_reads_nothing_and_says_it_is_busy() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("grain.db");
        let running = LockedFile::exclusive_if_free(&map_lineage::reading_lock_path(&out)).unwrap().expect("nobody reads this map");

        let busy = run_all(dir.path(), &out, 3, BATCH).unwrap();
        assert!(busy.busy, "the map is being read by the process that holds the lock");
        assert_eq!(busy.files, 0);

        drop(running);
        let free = run_all(dir.path(), &out, 3, BATCH).unwrap();
        assert!(!free.busy, "the lock was released");
    }
}
