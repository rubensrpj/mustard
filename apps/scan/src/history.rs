//! A história de cada declaração dos arquivos do projeto na branch de
//! partida, montada numa passada só pelo projeto inteiro e gravada no mapa.
//!
//! O git dá a história de todos os arquivos de uma vez: `git log --reverse -p
//! -U0`, do commit mais antigo ao mais novo, sem o arquivo inteiro a cada
//! versão. O rastreador ([`tracker`]) acompanha cada linha por esses commits
//! pelo texto dela, sem espaço nas pontas, seguindo os arquivos renomeados e
//! as linhas que mudam de arquivo no mesmo commit. Da ponta, onde o scan já lê
//! as declarações, sai a lista de cada uma: as linhas dela, e os commits pelos
//! quais cada linha passou. O commit em que a declaração nasceu é o mais
//! antigo entre as linhas dela; a linha que só uma chave ou um `else` escreve
//! não conta, porque se repete pelo arquivo todo.
//!
//! A linha entre declarações não vai a nenhuma. As declarações escritas na
//! mesma linha — a variante de enumeração com os campos ao lado, a estrutura de
//! uma linha só — ocupam as mesmas linhas, e a linha é de todas elas. O commit
//! que só muda espaços, ou que o projeto lista no `.git-blame-ignore-revs`,
//! fica na lista com a marca de só forma.
//!
//! A leitura seguinte não repete a passada: cada lista guarda o commit da ponta
//! em que foi lida, e os arquivos que mudaram partem da lista que já tinham e
//! leem só `<commit lido>..<ponta>`. A passada do começo da história lê no
//! máximo os [`NEWEST_COMMITS`] commits mais novos: a de um projeto de cem mil
//! declarações leva o que leva o git, e o commit muito velho diz pouco da
//! declaração de hoje.

mod diff;
mod tracker;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use anyhow::{anyhow, Result};
use mustard_core::domain::project_map::{
    file_history, CommitFiles, DeclChange, DeclComment, DeclLineage, FileLineage, History, LineageCommit, PullComment,
    CO_CHANGE_MAX_FILES,
};
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::map_lineage;
use mustard_core::io::project_map as store;
use mustard_core::platform::git::GitStream;

use crate::extract::{detect_language, Analyzer, Keep};
use crate::ingest::in_parallel;
use crate::refresh::{self, git};
use crate::routes;
use diff::{hash_of, summary, Patches};
use tracker::{Ln, Tracker, NONE};

/// O cabeçalho de cada commit no `git log`: o hash inteiro, a data, os pais e
/// o título.
const HEADER: &str = "--format=%x00%H %ct %P%x1f%s";

/// Quantos commits, no máximo, a leitura do começo da história lê, dos mais
/// novos: numa história maior, o tempo da leitura cresce com ela e o commit
/// muito velho diz pouco sobre a declaração de hoje. A linha que nenhum deles
/// escreveu fica com o mais antigo dos lidos.
pub(crate) const NEWEST_COMMITS: usize = 5_000;

/// Quantos arquivos, no máximo, uma leitura que só soma o que é novo aceita:
/// passando disso, ler o projeto inteiro custa menos que listar os arquivos.
const SINCE_MAX_FILES: usize = 400;

/// Quantos caracteres de caminhos, no máximo, vão na linha de comando do git
/// de uma leitura que só soma o que é novo.
const SINCE_MAX_PATHS: usize = 24_000;

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
/// grava no mapa em `out`, no lugar da que ele tinha, lendo todos os commits.
/// Uma linha que veio de outro arquivo é seguida nele até `moves` vezes
/// seguidas.
///
/// # Errors
///
/// Sem base ou sem ela no clone, com o mapa ilegível, quando o git não lê a
/// história ou quando a gravação falha.
pub(crate) fn run(root: &Path, out: &Path, file: &str, moves: usize) -> Result<Report> {
    let mut reading = Reading::open(root, out, moves, None)?;
    let files = vec![file.to_string()];
    reading.prepare(&files);
    let pass = reading.pass(&Group { files: files.clone(), plan: Plan::Whole })?;
    let reviews = store::pull_comments_at(out, file).map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
    let lineage = reading
        .trace(&pass, &files, &reviews)
        .pop()
        .ok_or_else(|| anyhow!("the history of {file} was not read"))?;
    store::save_lineage_at(out, &lineage)?;
    Ok(Report { file: file.to_string(), commits: lineage.commits.len(), declarations: lineage.declarations.len() })
}

/// Quantos arquivos vão numa gravação só da leitura do projeto inteiro: cada
/// lote é uma transação, e a busca que chega no meio espera só a gravação
/// dele.
pub(crate) const BATCH: usize = 25;

/// O que a leitura do projeto inteiro fez: quantos arquivos leu e gravou,
/// quantos commits e declarações eles somam, quantos commits leu do git e
/// quantos arquivos o git não deixou ler.
#[derive(Default)]
pub(crate) struct AllReport {
    pub files: usize,
    pub commits: usize,
    pub declarations: usize,
    /// Quantos commits vieram do git nesta leitura: todos os da base na
    /// primeira, só os novos nas seguintes.
    pub read: usize,
    /// A primeira passada só leu os commits mais novos: o que veio antes
    /// deles não está nas listas.
    pub limited: bool,
    pub failed: usize,
    /// Outra leitura do mesmo mapa estava rodando: esta não leu nada.
    pub busy: bool,
}

/// Lê a história de todo arquivo do mapa em `out` que ainda não a tem, ou
/// cuja marca venceu ([`map_lineage::wanted_at`]), e a grava em lotes de
/// `batch` arquivos, cada lote numa transação. Só um processo lê de cada mapa
/// de cada vez: quem chega com outro rodando sai sem ler. O git lê a história
/// numa passada só, do projeto inteiro na primeira vez e só do que veio depois
/// da última leitura nas seguintes. Ao fim de uma passada, vê o que faltou de
/// novo: o mapa pode ter ganhado arquivo, ou commit, enquanto ela lia. Se o git
/// não lê a história, os arquivos ficam de fora do resto desta leitura.
///
/// # Errors
///
/// Com o mapa ilegível ou quando a gravação falha; uma história que o git não
/// lê não é erro, só conta em `failed`.
pub(crate) fn run_all(root: &Path, out: &Path, moves: usize, batch: usize, newest: usize) -> Result<AllReport> {
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
        let mut reading = Reading::open(root, out, moves, Some(newest))?;
        reading.prepare(&wanted);
        for group in reading.groups(&wanted)? {
            let pass = match reading.pass(&group) {
                Ok(pass) => pass,
                Err(_) => {
                    report.failed += group.files.len();
                    left_out.extend(group.files);
                    continue;
                }
            };
            report.read += pass.read;
            report.limited |= pass.limited;
            for files in group.files.chunks(batch.max(1)) {
                let paths: Vec<&str> = files.iter().map(String::as_str).collect();
                let comments = store::pull_comments_for_at(out, &paths)
                    .map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
                let lineages = reading.trace(&pass, files, &comments);
                store::save_lineages_at(out, &lineages)?;
                report.files += lineages.len();
                report.commits += lineages.iter().map(|lineage| lineage.commits.len()).sum::<usize>();
                report.declarations += lineages.iter().map(|lineage| lineage.declarations.len()).sum::<usize>();
            }
        }
    }
    Ok(report)
}

/// O que se sabe de um commit além do que o diff diz: o número do pull request
/// que o trouxe e os arquivos que ele criou e mudou.
struct Meta {
    pr: Option<u32>,
    files: Option<CommitFiles>,
}

/// O que uma passada leu: o rastreador com as linhas de cada arquivo, o que se
/// sabe de cada commit lido, os commits das listas de que a passada partiu,
/// quantos commits vieram do git e se ela parou nos mais novos.
struct Pass {
    tracker: Tracker,
    meta: HashMap<String, Meta>,
    kept: HashMap<String, LineageCommit>,
    read: usize,
    /// A passada leu só os commits mais novos, e o que veio antes não vê.
    limited: bool,
}

/// De onde uma passada parte: do começo da história, ou das listas que já
/// valem, lendo só o que veio depois do commit `from`.
enum Plan {
    Whole,
    Since { from: String, seeds: Vec<FileLineage> },
}

/// Os arquivos que uma passada serve e de onde ela parte.
struct Group {
    files: Vec<String>,
    plan: Plan,
}

/// Os analisadores de declaração já compilados, por língua.
type Analyzers = HashMap<String, Option<Analyzer>>;

/// A leitura da história de vários arquivos da mesma base: o que se prepara
/// uma vez — a base, a história que o mapa guarda, as línguas compiladas e os
/// commits que o projeto manda ignorar — vale para todos os arquivos.
struct Reading<'r> {
    root: &'r Path,
    out: &'r Path,
    base: store::Base,
    /// A história do git que o mapa guarda.
    stored: History,
    /// O número do pull request de cada commit da janela do mapa.
    window: HashMap<String, Option<u32>>,
    moves: usize,
    /// Quantos commits, no máximo, a passada do começo da história lê; sem
    /// limite, lê todos.
    newest: Option<usize>,
    analyzers: Analyzers,
    ignored: Vec<String>,
}

impl<'r> Reading<'r> {
    fn open(root: &'r Path, out: &'r Path, moves: usize, newest: Option<usize>) -> Result<Self> {
        let base = store::base_of(root);
        if base.name.is_empty() || base.tip.is_empty() {
            return Err(anyhow!("the project has no base branch this clone has"));
        }
        let stored = store::history_at(out).map_err(|refusal| anyhow!("{}: {}", out.display(), refusal.reason()))?;
        let window = if stored.base == base.name {
            stored.commits.iter().map(|commit| (commit.id.clone(), commit.pr)).collect()
        } else {
            HashMap::new()
        };
        Ok(Reading { root, out, base, stored, window, moves, newest, analyzers: HashMap::new(), ignored: ignored_revs(root) })
    }

    /// Compila o analisador de cada língua dos arquivos `paths`.
    fn prepare(&mut self, paths: &[String]) {
        for path in paths {
            if let Some(language) = detect_language(Path::new(path)) {
                self.analyzers.entry(language.clone()).or_insert_with(|| Analyzer::declarations_only(&language));
            }
        }
    }

    /// Como ler os arquivos `wanted`: os que têm lista guardada, lida na mesma
    /// ponta, formam um grupo que só soma o que veio depois dela — desde que a
    /// base tenha seguido dali e nenhum arquivo do grupo tenha ganhado o
    /// conteúdo de outro por renomeação —, e os arquivos sem lista que já
    /// existiam na ponta onde alguma leitura parou, assim como os dos grupos
    /// que não se somam, formam o grupo que lê a história desde o começo.
    fn groups(&self, wanted: &[String]) -> Result<Vec<Group>> {
        let paths: Vec<&str> = wanted.iter().map(String::as_str).collect();
        let small = paths.len() <= SINCE_MAX_FILES && paths.iter().map(|path| path.len() + 1).sum::<usize>() <= SINCE_MAX_PATHS;
        let stored = if small {
            map_lineage::stored_at(self.out, &paths).map_err(|refusal| anyhow!("{}: {}", self.out.display(), refusal.reason()))?
        } else {
            Vec::new()
        };
        let mut by_tip: BTreeMap<String, Vec<FileLineage>> = BTreeMap::new();
        for lineage in stored {
            let valid = lineage.base == self.base.name
                && lineage.mark == refresh::FORMAT
                && usize::try_from(lineage.moves) == Ok(self.moves)
                && !lineage.tip.is_empty();
            if valid {
                by_tip.entry(lineage.tip.clone()).or_default().push(lineage);
            }
        }
        // Os grupos cuja ponta ainda está na base, e cujos arquivos não
        // receberam o conteúdo de outro.
        let renamed_into: HashMap<String, HashSet<String>> = by_tip
            .keys()
            .map(|from| (from.clone(), self.renamed_since(from)))
            .collect();
        let mut sinceable: Vec<(String, Vec<FileLineage>)> = Vec::new();
        let mut whole: Vec<String> = Vec::new();
        for (from, seeds) in by_tip {
            let renamed = renamed_into.get(&from);
            let usable = git(self.root, &["merge-base", "--is-ancestor", &from, &self.base.tip]).is_some()
                && renamed.is_some_and(|renamed| !seeds.iter().any(|lineage| renamed.contains(&lineage.path)));
            if usable {
                sinceable.push((from, seeds));
            } else {
                whole.extend(seeds.into_iter().map(|lineage| lineage.path));
            }
        }
        let seeded: HashSet<&str> =
            sinceable.iter().flat_map(|(_, seeds)| seeds.iter().map(|lineage| lineage.path.as_str())).collect();
        let whole_set: HashSet<&str> = whole.iter().map(String::as_str).collect();
        let unseeded: Vec<String> =
            wanted.iter().filter(|path| !seeded.contains(path.as_str()) && !whole_set.contains(path.as_str())).cloned().collect();
        // O arquivo sem lista que já existia na ponta de uma leitura tem
        // passado que a leitura de depois dela não vê: só o que ainda não
        // existia nessa ponta se soma a ela. A mais nova serve primeiro.
        let mut newest_first: Vec<usize> = (0..sinceable.len()).collect();
        newest_first.sort_by_key(|&at| Reverse(self.age_of(&sinceable[at].0)));
        let mut joined: Option<usize> = None;
        if !unseeded.is_empty() {
            for at in newest_first {
                let renamed = &renamed_into[&sinceable[at].0];
                if exists_at(self.root, &sinceable[at].0, &unseeded).is_empty() && !unseeded.iter().any(|path| renamed.contains(path)) {
                    joined = Some(at);
                    break;
                }
            }
        }
        let mut groups: Vec<Group> = Vec::new();
        let mut leftover = whole;
        for (at, (from, seeds)) in sinceable.into_iter().enumerate() {
            let mut files: Vec<String> = seeds.iter().map(|lineage| lineage.path.clone()).collect();
            if joined == Some(at) {
                files.extend(unseeded.iter().cloned());
            }
            groups.push(Group { files, plan: Plan::Since { from, seeds } });
        }
        if joined.is_none() {
            leftover.extend(unseeded);
        }
        if !leftover.is_empty() {
            leftover.sort();
            groups.push(Group { files: leftover, plan: Plan::Whole });
        }
        Ok(groups)
    }

    /// Os arquivos que o `git diff` de `from` até a ponta da base dá por
    /// renomeados: o caminho que ganhou o conteúdo de outro.
    fn renamed_since(&self, from: &str) -> HashSet<String> {
        git(self.root, &["diff", "--name-status", "-M", "--relative", from, &self.base.tip])
            .map(|changes| {
                changes
                    .lines()
                    .filter(|line| line.starts_with('R'))
                    .filter_map(|line| line.rsplit('\t').next().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A data do commit `rev`, para pôr as pontas em ordem.
    fn age_of(&self, rev: &str) -> i64 {
        git(self.root, &["log", "-1", "--format=%ct", rev]).and_then(|at| at.trim().parse().ok()).unwrap_or(0)
    }

    /// A passada que serve a `group`.
    fn pass(&self, group: &Group) -> Result<Pass> {
        match &group.plan {
            Plan::Whole => self.whole(&group.files),
            Plan::Since { from, seeds } => self.since(&group.files, from, seeds),
        }
    }

    /// A passada do começo da história, sobre os arquivos das línguas de
    /// `wanted`: os `newest` commits mais novos que mexem neles, ou todos.
    fn whole(&self, wanted: &[String]) -> Result<Pass> {
        let mut tracker = Tracker::new(self.moves, self.ignored.clone());
        let (meta, limited) = self.stream(&mut tracker, &self.base.tip, &extension_globs(wanted), self.newest)?;
        let read = tracker.commits().len();
        Ok(Pass { tracker, meta, kept: HashMap::new(), read, limited })
    }

    /// A passada que parte das listas `seeds`, lidas na ponta `from`, e lê só
    /// o que veio depois, dos arquivos de `wanted`.
    fn since(&self, wanted: &[String], from: &str, seeds: &[FileLineage]) -> Result<Pass> {
        let mut tracker = Tracker::new(self.moves, self.ignored.clone());
        let kept = self.seed(&mut tracker, seeds, from);
        let already = tracker.commits().len();
        let (meta, _) = self.stream(&mut tracker, &format!("{from}..{}", self.base.tip), wanted, None)?;
        let read = tracker.commits().len() - already;
        Ok(Pass { tracker, meta, kept, read, limited: false })
    }

    /// Põe no rastreador as linhas de cada arquivo de `seeds` como estavam na
    /// ponta `from`, cada uma com os commits da declaração que a continha.
    fn seed(&self, tracker: &mut Tracker, seeds: &[FileLineage], from: &str) -> HashMap<String, LineageCommit> {
        let paths: Vec<String> = seeds.iter().map(|lineage| lineage.path.clone()).collect();
        let texts = blobs_at(self.root, from, &paths);
        let layouts = in_parallel(seeds.iter().collect::<Vec<_>>(), |lineage| {
            texts.get(&lineage.path).map(|text| layout_of(&self.analyzers, &lineage.path, text)).unwrap_or_default()
        });
        let mut kept: HashMap<String, LineageCommit> = HashMap::new();
        for commit in seeds.iter().flat_map(|lineage| &lineage.commits) {
            kept.entry(commit.id.clone()).or_insert_with(|| commit.clone());
        }
        // Os commits das listas entram no rastreador do mais velho ao mais
        // novo, na ordem da história: é o índice que ele dá que desempata os
        // commits da mesma data.
        let order = self.order_before(from);
        let mut ordered: Vec<&LineageCommit> = kept.values().collect();
        ordered.sort_by_key(|commit| (order.get(&commit.id).copied().unwrap_or(0), commit.at, commit.id.clone()));
        let mut known: HashMap<String, u32> =
            ordered.into_iter().map(|commit| (commit.id.clone(), tracker.seed_commit(&commit.id, commit.at, &commit.title))).collect();
        for (lineage, layout) in seeds.iter().zip(layouts) {
            let declared: HashMap<(&str, u32), &DeclLineage> =
                lineage.declarations.iter().map(|decl| ((decl.name.as_str(), decl.nth), decl)).collect();
            let mut nodes: HashMap<Vec<usize>, u32> = HashMap::new();
            let mut lines = Vec::with_capacity(layout.lines.len());
            for (at, text) in layout.lines.iter().enumerate() {
                let line = summary(text.as_bytes());
                let owners = layout.owners_of(at + 1);
                let node = if line.trivial || owners.is_empty() {
                    NONE
                } else if let Some(&node) = nodes.get(owners) {
                    node
                } else {
                    let mut set: Vec<(u32, bool)> = Vec::new();
                    for &owner in owners {
                        let span = &layout.spans[owner];
                        for change in declared.get(&(span.name.as_str(), span.nth)).map_or(&[][..], |decl| decl.commits.as_slice()) {
                            let index = *known.entry(change.id.clone()).or_insert_with(|| tracker.seed_commit(&change.id, 0, ""));
                            if !set.contains(&(index, change.form)) {
                                set.push((index, change.form));
                            }
                        }
                    }
                    let node = if set.is_empty() { NONE } else { tracker.seed_node(set) };
                    nodes.insert(owners.to_vec(), node);
                    node
                };
                lines.push(Ln { hash: line.hash, node });
            }
            tracker.seed_file(&lineage.path, lines);
        }
        kept
    }

    /// A posição de cada commit até `rev` na história, do mais velho ao mais
    /// novo, pelo começo do hash.
    fn order_before(&self, rev: &str) -> HashMap<String, usize> {
        git(self.root, &["rev-list", "--reverse", "--topo-order", rev])
            .map(|list| list.lines().enumerate().map(|(at, sha)| (short(sha).to_string(), at + 1)).collect())
            .unwrap_or_default()
    }

    /// Lê o `git log` de `range` restrito a `pathspec` para dentro do
    /// rastreador, e devolve o que se sabe dos commits lidos — o número do
    /// pull request e os arquivos, que o git dá numa leitura à parte — e se
    /// `limit` cortou a leitura: o commit que passou do limite não foi lido, e
    /// o que a passada não vê fica sem começo.
    fn stream(
        &self,
        tracker: &mut Tracker,
        range: &str,
        pathspec: &[String],
        limit: Option<usize>,
    ) -> Result<(HashMap<String, Meta>, bool)> {
        let already = tracker.commits().len();
        let read = self.read_patches(tracker, range, pathspec, limit)?;
        let limited = limit.is_some_and(|limit| read == limit && self.has_more_than(range, pathspec, limit));
        // Os commits de que se quer o número do pull request e os arquivos
        // são os lidos e os que vieram depois deles.
        let first = tracker.commits().get(already).map(|commit| commit.sha.clone());
        let meta = match (limited, first) {
            (true, Some(first)) => {
                let after = format!("{first}^..{}", self.base.tip);
                let found = self.commit_meta(&after);
                if found.is_empty() { self.commit_meta(range) } else { found }
            }
            _ => self.commit_meta(range),
        };
        Ok((meta, limited))
    }

    /// Se o `git log` de `range` restrito a `pathspec` tem mais de `count`
    /// commits.
    fn has_more_than(&self, range: &str, pathspec: &[String], count: usize) -> bool {
        let wanted = (count + 1).to_string();
        let mut args: Vec<&str> = vec!["rev-list", "--count", "-n", &wanted, range, "--"];
        args.extend(pathspec.iter().map(String::as_str));
        git(self.root, &args).and_then(|found| found.trim().parse::<usize>().ok()).is_some_and(|found| found > count)
    }

    /// Passa cada commit do `git log` de `range`, restrito a `pathspec` e aos
    /// `limit` mais novos, pelo rastreador; devolve quantos leu.
    fn read_patches(&self, tracker: &mut Tracker, range: &str, pathspec: &[String], limit: Option<usize>) -> Result<usize> {
        let newest = limit.map(|limit| limit.to_string());
        let mut args: Vec<&str> = vec![
            "-c",
            "core.quotePath=false",
            "log",
            "--reverse",
            "--topo-order",
            "-M",
            "-p",
            "-U0",
            "--cc",
            "--diff-algorithm=histogram",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-show-signature",
            "--relative",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            HEADER,
        ];
        if let Some(newest) = &newest {
            args.extend(["-n", newest]);
        }
        args.extend([range, "--"]);
        args.extend(pathspec.iter().map(String::as_str));
        let mut stream = GitStream::spawn(self.root, &args, None).map_err(|reason| anyhow!("git log could not read the history: {reason}"))?;
        let mut read = 0;
        let ended = {
            let mut patches = Patches::new(BufReader::with_capacity(1 << 20, &mut stream));
            loop {
                match patches.next_commit() {
                    Ok(Some(commit)) => {
                        tracker.apply(&commit);
                        read += 1;
                    }
                    Ok(None) => break Ok(()),
                    Err(err) => break Err(err),
                }
            }
        };
        let finished = stream.finish();
        ended.map_err(|err| anyhow!("git log could not be read: {err}"))?;
        finished.map_err(|reason| anyhow!("git log could not read the history: {reason}"))?;
        Ok(read)
    }

    /// O número do pull request e os arquivos de cada commit de `range`, pelo
    /// começo do hash. O commit que muda mais de [`CO_CHANGE_MAX_FILES`]
    /// arquivos fica sem eles: ele não conta para "muda junto".
    fn commit_meta(&self, range: &str) -> HashMap<String, Meta> {
        let args = ["log", "--no-show-signature", "--no-renames", "--relative", "--name-status", HEADER, range];
        let Some(text) = git(self.root, &args) else { return HashMap::new() };
        refresh::parse_headers(&text)
            .into_iter()
            .map(|(sha, commit)| {
                let files = (commit.added.len() + commit.changed.len() <= CO_CHANGE_MAX_FILES)
                    .then_some(CommitFiles { added: commit.added, changed: commit.changed });
                (short(&sha).to_string(), Meta { pr: commit.pr, files })
            })
            .collect()
    }

    /// A história de cada arquivo de `files`, na ordem deles, montada sem
    /// gravar; `reviews` são os comentários de revisão presos aos arquivos.
    fn trace(&self, pass: &Pass, files: &[String], reviews: &[PullComment]) -> Vec<FileLineage> {
        let texts = blobs_at(self.root, &self.base.tip, files);
        in_parallel(files.iter().collect::<Vec<_>>(), |file| {
            let mine: Vec<PullComment> = reviews.iter().filter(|comment| comment.path == *file).cloned().collect();
            self.lineage_of(pass, file, texts.get(file).map(String::as_str), &mine)
        })
    }

    /// A história das declarações de `file`, cuja versão na ponta da base é
    /// `text`; `None` quando a base não tem o arquivo.
    fn lineage_of(&self, pass: &Pass, file: &str, text: Option<&str>, reviews: &[PullComment]) -> FileLineage {
        let tracker = &pass.tracker;
        let tip = text.map(|text| layout_of(&self.analyzers, file, text)).unwrap_or_default();
        let nodes = nodes_of(tracker.lines(file), &tip.lines);
        // As linhas de cada declaração: a que a tem entre as mais internas.
        let mut owned: Vec<Vec<usize>> = vec![Vec::new(); tip.spans.len()];
        for line in 1..=tip.lines.len() {
            for &owner in tip.owners_of(line) {
                owned[owner].push(line - 1);
            }
        }
        let mut listed: Vec<Vec<(u32, bool)>> = Vec::with_capacity(owned.len());
        let mut referenced: BTreeSet<u32> = BTreeSet::new();
        for lines in &owned {
            let mut entries: Vec<(u32, bool)> = Vec::new();
            let mut seen: HashSet<u32> = HashSet::new();
            let mut older = false;
            for &line in lines {
                if nodes[line] != NONE {
                    if seen.insert(nodes[line]) {
                        tracker.chain(nodes[line], &mut entries);
                    }
                } else if pass.limited && !summary(tip.lines[line].as_bytes()).trivial {
                    older = true;
                }
            }
            // Numa passada que parou nos commits mais novos, a linha que
            // nenhum deles escreveu é mais velha que todos: fica com o mais
            // antigo dos lidos, o primeiro do rastreador.
            if older {
                entries.push((0, false));
            }
            // O commit é só de forma quando todas as linhas dele na declaração
            // o são, ou quando o projeto manda ignorá-lo.
            let mut only_form: BTreeMap<u32, bool> = BTreeMap::new();
            for (commit, form) in entries {
                *only_form.entry(commit).or_insert(true) &= form;
            }
            let mut list: Vec<(u32, bool)> =
                only_form.into_iter().map(|(commit, form)| (commit, form || tracker.commit(commit).ignored)).collect();
            list.sort_by_key(|&(commit, _)| Reverse((tracker.commit(commit).at, commit)));
            referenced.extend(list.iter().map(|&(commit, _)| commit));
            listed.push(list);
        }
        let mut order: Vec<u32> = referenced.into_iter().collect();
        order.sort_by_key(|&commit| Reverse((tracker.commit(commit).at, commit)));
        let commits: Vec<LineageCommit> = order.iter().map(|&commit| self.commit_of(pass, commit)).collect();

        let mut attached = self.attach(file, reviews, &tip.index(), &commits);
        let declarations: Vec<DeclLineage> = tip
            .spans
            .iter()
            .zip(&listed)
            .enumerate()
            .map(|(at, (span, list))| DeclLineage {
                name: span.name.clone(),
                nth: span.nth,
                commits: list
                    .iter()
                    .map(|&(commit, form)| DeclChange { id: short(&tracker.commit(commit).sha).to_string(), form })
                    .collect(),
                comments: attached.remove(&at).unwrap_or_default(),
            })
            .collect();
        let last_commit = file_history(&self.stored, file)
            .map(|found| found.last_commit)
            .unwrap_or_else(|| commits.first().map(|commit| commit.id.clone()).unwrap_or_default());
        FileLineage {
            path: file.to_string(),
            base: self.base.name.clone(),
            last_commit,
            tip: self.base.tip.clone(),
            mark: refresh::FORMAT.to_string(),
            moves: u32::try_from(self.moves).unwrap_or(u32::MAX),
            comments: u32::try_from(reviews.len()).unwrap_or(u32::MAX),
            commits,
            declarations,
        }
    }

    /// O commit `index` do rastreador como a lista do arquivo o guarda: o
    /// número do pull request vem da janela do mapa e, sem ela, do git.
    fn commit_of(&self, pass: &Pass, index: u32) -> LineageCommit {
        let commit = pass.tracker.commit(index);
        let id = short(&commit.sha).to_string();
        let (meta, kept) = (pass.meta.get(&id), pass.kept.get(&id));
        let pr = match self.window.get(&id) {
            Some(pr) => *pr,
            None => meta.and_then(|meta| meta.pr).or_else(|| kept.and_then(|kept| kept.pr)),
        };
        let files = meta.and_then(|meta| meta.files.clone()).or_else(|| kept.map(|kept| kept.files.clone())).unwrap_or_default();
        LineageCommit { id, at: commit.at, title: commit.title.clone(), pr, files }
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
        &self,
        path: &str,
        reviews: &[PullComment],
        tip: &HashMap<Key, usize>,
        commits: &[LineageCommit],
    ) -> HashMap<usize, Vec<DeclComment>> {
        let mut layouts: HashMap<String, Option<Layout>> = HashMap::new();
        let mut attached: HashMap<usize, Vec<DeclComment>> = HashMap::new();
        for review in reviews {
            let merged = commits.iter().find(|commit| commit.pr == Some(review.number)).map(|commit| commit.id.clone());
            let found = [Some(review.commit.clone()), merged].into_iter().flatten().find_map(|commit| {
                let layout = layouts
                    .entry(commit.clone())
                    .or_insert_with(|| {
                        git(self.root, &["show", "--no-textconv", &format!("{commit}:./{path}")])
                            .map(|text| layout_of(&self.analyzers, path, &text))
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
}

/// O começo do hash, como a história guardada o escreve.
fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

/// O que o git aceita como filtro de caminhos para ler os arquivos de
/// `paths`: as extensões que eles têm, ou o próprio caminho quando não há
/// extensão que sirva.
fn extension_globs(paths: &[String]) -> Vec<String> {
    let mut globs: BTreeSet<String> = BTreeSet::new();
    for path in paths {
        match Path::new(path).extension().and_then(|ext| ext.to_str()).filter(|ext| ext.chars().all(char::is_alphanumeric)) {
            Some(ext) => globs.insert(format!("*.{ext}")),
            None => globs.insert(format!(":(literal){path}")),
        };
    }
    globs.into_iter().collect()
}

/// Dos arquivos `paths`, os que existem na revisão `rev`.
fn exists_at(root: &Path, rev: &str, paths: &[String]) -> Vec<String> {
    if paths.is_empty() {
        return Vec::new();
    }
    let input: String = paths.iter().map(|path| format!("{rev}:./{path}\n")).collect();
    let Ok(mut stream) = GitStream::spawn(root, &["cat-file", "--batch-check"], Some(input.into_bytes())) else {
        return paths.to_vec();
    };
    let mut answers = String::new();
    let read = stream.read_to_string(&mut answers);
    if stream.finish().is_err() || read.is_err() {
        return paths.to_vec();
    }
    paths.iter().zip(answers.lines()).filter(|(_, answer)| !answer.ends_with(" missing")).map(|(path, _)| path.clone()).collect()
}

/// O texto de cada arquivo de `paths` na revisão `rev`, lido numa chamada só ao
/// git; o que a revisão não tem não vem.
fn blobs_at(root: &Path, rev: &str, paths: &[String]) -> HashMap<String, String> {
    let mut texts = HashMap::new();
    if paths.is_empty() {
        return texts;
    }
    let input: String = paths.iter().map(|path| format!("{rev}:./{path}\n")).collect();
    let Ok(mut stream) = GitStream::spawn(root, &["cat-file", "--batch"], Some(input.into_bytes())) else {
        return texts;
    };
    {
        let mut answers = BufReader::new(&mut stream);
        for path in paths {
            let mut head = String::new();
            if !matches!(answers.read_line(&mut head), Ok(read) if read > 0) {
                break;
            }
            let mut words = head.split_whitespace().rev();
            let size = words.next().and_then(|size| size.parse::<usize>().ok());
            let kind = words.next();
            if let (Some(size), Some("blob")) = (size, kind) {
                let mut body = vec![0u8; size + 1];
                if answers.read_exact(&mut body).is_err() {
                    break;
                }
                body.pop();
                texts.insert(path.clone(), String::from_utf8_lossy(&body).into_owned());
            } else if let (Some(size), Some(_)) = (size, kind) {
                // Uma pasta ou um submódulo no lugar do arquivo: o conteúdo
                // não interessa, mas tem de ser lido para chegar ao seguinte.
                let mut skipped = vec![0u8; size + 1];
                if answers.read_exact(&mut skipped).is_err() {
                    break;
                }
            }
        }
    }
    let _ = stream.finish();
    texts
}

/// O nó de cada linha da versão da ponta, `lines`, pelo que o rastreador sabe
/// do arquivo: linha por linha quando as listas são iguais, e senão pelo texto,
/// a ocorrência mais perto de onde a linha devia estar.
fn nodes_of(state: Option<&[Ln]>, lines: &[String]) -> Vec<u32> {
    let hashes: Vec<u64> = lines.iter().map(|line| hash_of(line)).collect();
    let Some(state) = state else { return vec![NONE; lines.len()] };
    if state.len() == hashes.len() && state.iter().zip(&hashes).all(|(line, hash)| line.hash == *hash) {
        return state.iter().map(|line| line.node).collect();
    }
    let mut places: HashMap<u64, Vec<usize>> = HashMap::new();
    for (at, line) in state.iter().enumerate() {
        places.entry(line.hash).or_default().push(at);
    }
    let mut drift = 0isize;
    hashes
        .iter()
        .enumerate()
        .map(|(at, hash)| {
            let Some(found) = places.get(hash) else { return NONE };
            let wanted = at as isize + drift;
            let after = found.partition_point(|&place| (place as isize) < wanted);
            let near = [after.checked_sub(1), Some(after).filter(|&next| next < found.len())]
                .into_iter()
                .flatten()
                .map(|next| found[next])
                .min_by_key(|&place| (place as isize - wanted).abs());
            match near {
                Some(place) => {
                    drift = place as isize - at as isize;
                    state[place].node
                }
                None => NONE,
            }
        })
        .collect()
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
}

/// As declarações de `text`, a versão do arquivo em `path`, pelo analisador da
/// língua do caminho; sem nenhuma quando a língua não é conhecida.
fn layout_of(analyzers: &Analyzers, path: &str, text: &str) -> Layout {
    let analyzer = detect_language(Path::new(path)).and_then(|language| analyzers.get(&language)).and_then(Option::as_ref);
    layout(analyzer, path, text)
}

fn layout(analyzer: Option<&Analyzer>, path: &str, text: &str) -> Layout {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let Some(analyzer) = analyzer else {
        return Layout { lines, ..Layout::default() };
    };
    let keep = Keep { written_text: false, texts_and_routes: false };
    let extracted = analyzer.extract(text, keep, &routes::Project { path, ..routes::Project::default() });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Outra leitura do mesmo mapa em andamento faz a nova sair sem ler
    /// nada, e a que chega depois da primeira acabar lê.
    #[test]
    fn a_reading_of_a_map_being_read_by_another_process_reads_nothing_and_says_it_is_busy() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("grain.db");
        let running = LockedFile::exclusive_if_free(&map_lineage::reading_lock_path(&out)).unwrap().expect("nobody reads this map");

        let busy = run_all(dir.path(), &out, 3, BATCH, NEWEST_COMMITS).unwrap();
        assert!(busy.busy, "the map is being read by the process that holds the lock");
        assert_eq!(busy.files, 0);

        drop(running);
        let free = run_all(dir.path(), &out, 3, BATCH, NEWEST_COMMITS).unwrap();
        assert!(!free.busy, "the lock was released");
    }

    fn state(texts: &[&str]) -> Vec<Ln> {
        texts.iter().enumerate().map(|(at, text)| Ln { hash: hash_of(text), node: u32::try_from(at).unwrap() }).collect()
    }

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_string()).collect()
    }

    #[test]
    fn the_lines_of_the_tip_take_the_nodes_of_the_same_lines_of_the_tracker() {
        let tracker = state(&["a", "b", "c"]);
        assert_eq!(nodes_of(Some(&tracker), &lines(&["a", "  b  ", "c"])), [0, 1, 2], "the same lines, in the same places");
        assert_eq!(nodes_of(None, &lines(&["a"])), [NONE], "no file in the tracker, no node");
    }

    #[test]
    fn a_tip_that_differs_from_the_tracker_takes_the_node_of_the_nearest_line_with_the_same_text() {
        let tracker = state(&["x", "a", "x", "b", "x", "c", "x"]);
        // A ponta perdeu as três primeiras linhas: cada `x` pega o nó do que
        // está mais perto de onde ele devia estar, seguindo o deslocamento da
        // linha anterior, e não o do primeiro `x` do arquivo.
        let tip = lines(&["b", "x", "c", "x"]);
        assert_eq!(nodes_of(Some(&tracker), &tip), [3, 4, 5, 6]);
        assert_eq!(nodes_of(Some(&tracker), &lines(&["novo", "b"])), [NONE, 3], "a line the tracker never saw has no node");
    }

    #[test]
    fn the_files_of_the_reading_become_the_filters_of_the_git_log() {
        let paths: Vec<String> = ["src/a.rs", "src/b.rs", "web/app.tsx", "Makefile", "x/we ird.r[s]"].map(String::from).to_vec();
        assert_eq!(
            extension_globs(&paths),
            ["*.rs", "*.tsx", ":(literal)Makefile", ":(literal)x/we ird.r[s]"],
            "one filter per extension, and the literal path when there is no extension to trust"
        );
    }

    #[test]
    fn the_files_of_a_revision_come_in_one_call_and_the_missing_ones_do_not_come() {
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
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "um\ndois\n").unwrap();
        std::fs::write(root.join("b.txt"), "tres\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "first"]);
        let wanted: Vec<String> = ["a.txt", "nada.txt", "b.txt"].map(String::from).to_vec();
        let texts = blobs_at(root, "HEAD", &wanted);
        assert_eq!(texts.get("a.txt").map(String::as_str), Some("um\ndois\n"));
        assert_eq!(texts.get("b.txt").map(String::as_str), Some("tres\n"), "the file after the missing one is still read");
        assert!(!texts.contains_key("nada.txt"));
        assert_eq!(exists_at(root, "HEAD", &wanted), ["a.txt", "b.txt"]);
    }
}
