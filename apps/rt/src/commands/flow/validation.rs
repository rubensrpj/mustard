//! Execution measurements use the spec's existing event log. Presentation
//! reads these records; it never reruns validation to obtain a duration.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery, Refusal, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, State, answered_request};
use mustard_core::domain::wave_prompt::delivered_request_proofs;
use mustard_core::platform::git;
use serde_json::{Map, Value, json};

use crate::commands::review::qa_run::ProofRun;
use crate::commands::spec_events::write::record;

/// A timer defaults to failure so early returns are measured too. Inclusive
/// stages describe wall time; readers must not add them to their child stages.
pub(crate) struct Stage<'a> {
    root: &'a Path,
    spec: &'a str,
    phase: &'a str,
    name: &'a str,
    started: std::time::Instant,
    result: &'static str,
    inclusive: bool,
}

impl<'a> Stage<'a> {
    pub(crate) fn new(root: &'a Path, spec: &'a str, phase: &'a str, stage: &'a str, inclusive: bool) -> Self {
        Self { root, spec, phase, name: stage, started: std::time::Instant::now(), result: "fail", inclusive }
    }
    pub(crate) fn passed(&mut self) {
        self.result = "pass";
    }
    pub(crate) fn skipped(&mut self) {
        self.result = "skipped";
    }
}

/// A fase das etapas da rodada. O tempo delas é observação: ninguém decide
/// por ele, e ele mora no arquivo de operações da spec, que só recebe a linha,
/// em vez de regravar a spec a cada etapa. A etapa do fechamento fica na spec:
/// a máquina que falha depois do recibo da validação final o desfaz.
const ROUND_PHASE: &str = "delivery";

impl Drop for Stage<'_> {
    fn drop(&mut self) {
        let draft = json!({"phase":self.phase,"stage":self.name,"result":self.result,
            "ms":self.started.elapsed().as_millis() as u64,"scope":if self.inclusive {"inclusive"}else{"leaf"},
            "version":mustard_core::harness_version(),
            "attempt":std::process::id(),"author":"binary"});
        let draft = draft.as_object().cloned().unwrap_or_default();
        if self.phase == ROUND_PHASE {
            // Como toda observação, a que não fica não falha para quem mede.
            // A etapa não recebe a sessão da entrada: vale a do processo, a
            // mesma que a entrada `run` da rodada leu.
            let session = crate::shared::spec_state::session_from_env();
            let _ = crate::commands::spec_events::conversation::observe(self.root, self.spec, session.as_deref(), "stage_run", vec![draft]);
            return;
        }
        if let Err(error) = record(self.root, self.spec, "stage_run", draft, PhaseWriter::Binary) {
            eprintln!("[Mustard] stage measurement was not recorded: {}", error.reason());
        }
    }
}

/// Content and execution inputs, rather than just HEAD. Failure to read any
/// input disables reuse; it never turns an incomplete digest into a receipt.
pub(super) fn fingerprint(root: &Path, log: &SpecLog) -> Option<String> {
    use mustard_core::io::sha256::Sha256;
    let mut names = listed_files(root)?;
    // Confirmed local inputs are often ignored by Git, including .env files.
    // Their contents enter only the digest, never the event or diagnostic.
    let config_inputs = mustard_core::ProjectConfig::load(root);
    for name in config_inputs.local_files.iter().flatten() {
        if !mustard_core::io::wave_prompt::local_file_inside(name) {
            return None;
        }
        names.push(name.clone());
    }
    names.sort_unstable();
    names.dedup();
    let mut digest = Sha256::new();
    let config = root.join("mustard.json");
    match std::fs::read(config) {
        Ok(bytes) => digest.update(&bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => digest.update(b"no-config"),
        Err(_) => return None,
    }
    digest.update(b"mustard-final-validation-v2\0");
    for name in names {
        // The log and its projection change while validating. They are state,
        // not code inputs; criteria and config enter the digest separately.
        if name.starts_with(".claude/spec/")
            || name.starts_with(".claude/mustard/operations/")
            || name.starts_with(".claude/project/")
            || name.starts_with(".claude/pending/")
            || name.starts_with(".claude/grain.db")
            || name.starts_with(".claude/mustard/pages/")
            || name.starts_with(".claude/mustard/publications/")
            || name.starts_with(".claude/judgements/")
        {
            continue;
        }
        digest.update(name.as_bytes());
        digest.update(&[0]);
        let path = root.join(name);
        match std::fs::symlink_metadata(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => digest.update(b"deleted\0"),
            Err(_) => return None,
            Ok(meta) if meta.file_type().is_symlink() => {
                digest.update(std::fs::read_link(&path).ok()?.to_str()?.as_bytes());
                // Content can change without changing the link's spelling.
                // A broken/external/directory link cannot prove reuse safely.
                let resolved = path.canonicalize().ok()?;
                if !resolved.starts_with(root.canonicalize().ok()?) || !resolved.is_file() {
                    return None;
                }
                digest.update(&std::fs::read(resolved).ok()?);
            }
            Ok(meta) if meta.is_file() => {
                digest.update(&std::fs::read(path).ok()?);
            }
            _ => return None,
        }
        digest.update(&[0]);
    }
    for event in log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter(|e| e.event_type == "criterion") {
        digest.update(&serde_json::to_vec(&event.fields).ok()?);
    }
    // As provas que a máquina roda entram também: a de um pedido que a
    // tarefa de um achado entregou sem mudar arquivo nenhum chega à lista sem
    // mudar o código nem os critérios, e o recibo de antes dela não vale.
    let declared = config_inputs.commands();
    let proofs = proof_list(log, declared.test.as_deref());
    for (id, code, command) in &proofs {
        digest.update(&serde_json::to_vec(&(id, code, command)).ok()?);
    }
    let commands: Vec<&str> = [&declared.build, &declared.lint, &declared.test]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .chain(proofs.iter().map(|(_, _, command)| command.as_str()))
        .collect();
    for (name, value) in cited_env(std::env::vars_os(), &commands) {
        digest.update(name.to_str()?.as_bytes());
        digest.update(&[0]);
        digest.update(value.to_str()?.as_bytes());
        digest.update(&[0]);
    }
    Some(digest.hex_digest())
}

/// As variáveis de `env` que a impressão da validação leva, em ordem: o
/// `PATH`, que escolhe os programas, e as que um dos `commands` cita pelo
/// nome, como `$TOKEN` ou `TOKEN=1 cargo test`. A variável sem relação com o
/// que a máquina roda não a faz rodar de novo.
fn cited_env(env: impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)>, commands: &[&str]) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let cites = |command: &str, name: &str| {
        command.match_indices(name).any(|(at, _)| {
            !command[..at].chars().next_back().is_some_and(word) && !command[at + name.len()..].chars().next().is_some_and(word)
        })
    };
    let mut kept: Vec<_> = env
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            !name.is_empty() && (name == "PATH" || commands.iter().any(|command| cites(command, &name)))
        })
        .collect();
    kept.sort();
    kept
}

/// As provas que a máquina do fechamento roda, cada uma com o número e o
/// código do item e o comando: a de cada critério vigente da spec, na ordem
/// em que a leitura os dá, e depois a de cada pedido que uma tarefa entregue
/// cobre e cujo relato a nomeia — o pedido nascido de um achado —, pela mesma
/// seleção da formação do lote ([`delivered_request_proofs`]), com o comando
/// de teste do projeto (`test`). As duas rodam nas mesmas cópias e caem na
/// mesma recusa, e a lista entra na impressão da validação ([`fingerprint`]).
pub(super) fn proof_list(log: &SpecLog, test: Option<&str>) -> Vec<(u64, String, String)> {
    let codes = log.codes();
    let code = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let criteria = log.block(BlockQuery::Block(Block::Criteria)).into_iter().filter(|e| e.event_type == "criterion").filter_map(|e| {
        let proof = e.str_field("proof")?.trim().to_string();
        Some((e.id, code(e.id), proof))
    });
    let requests = delivered_request_proofs(log, test).into_iter().map(|(id, proof)| (id, code(id), proof));
    criteria.chain(requests).collect()
}

/// Os arquivos que o git lista no projeto: os rastreados e os novos que ele
/// não ignora. Sem git que responda, nenhuma lista.
fn listed_files(root: &Path) -> Option<Vec<String>> {
    let files = git::run(root, &["ls-files", "--cached", "--others", "--exclude-standard", "-z"]).out()?;
    Some(files.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect())
}

/// A obra contra a qual se lê a saída de um comando que caiu: os arquivos que
/// ela mudou, de cada onda, os que a onda declarou ou entregou, e as ondas de
/// cada commit que a rodada gravou. Um arquivo é da obra quando o git diz que
/// a branch o mudou, e não porque uma onda o listou: a lista da onda pode vir
/// incompleta.
#[derive(Debug)]
pub(super) struct Work {
    /// Os arquivos que a obra mudou.
    changed: BTreeSet<String>,
    /// Cada onda, na ordem pedida, com os arquivos que declarou ou entregou.
    waves: Vec<(u64, Vec<String>)>,
    /// As ondas de cada commit da obra, pelo código dele: o evento `commit`
    /// liga o commit às ondas que escreveram nele, sem as que entraram nele
    /// sem mudança nenhuma.
    commits: BTreeMap<String, Vec<u64>>,
}

impl Work {
    /// A obra como o git e a spec a contam: os arquivos que a branch mudou
    /// desde a base gravada no `state` da spec (`git diff --name-only
    /// <base>...HEAD`, com a base local e, sem ela, a do `origin`) e, de cada
    /// onda de `waves`, os que as tarefas dela declaram mais os que as
    /// entregas dela listam, e as ondas de cada commit gravado que mudaram
    /// arquivo nele — a onda que o commit fecha sem ter mudado nada não
    /// escreveu linha dele. Sem base, ou sem git que responda, nenhum arquivo
    /// conta como mudado pela obra.
    pub(super) fn of(root: &Path, log: &SpecLog, waves: &[u64]) -> Self {
        let waves = waves
            .iter()
            .map(|wave| {
                let delivered = log
                    .events
                    .iter()
                    .filter(|e| e.event_type == "delivered" && e.wave() == Some(*wave))
                    .flat_map(|e| e.fields.get("files").and_then(Value::as_array).into_iter().flatten())
                    .filter_map(Value::as_str)
                    .map(str::to_string);
                let mut files: Vec<String> = Vec::new();
                for file in mustard_core::domain::wave_prompt::wave_files(log, *wave).into_iter().chain(delivered) {
                    push_path(&mut files, &file);
                }
                (*wave, files)
            })
            .collect();
        let commits = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "commit")
            .filter_map(|e| {
                let unchanged = e.ints("unchanged");
                let wrote = e.ints("waves").into_iter().filter(|wave| !unchanged.contains(wave)).collect();
                e.str_field("sha").map(|sha| (sha.to_string(), wrote))
            })
            .collect();
        Self { changed: branch_files(root, log), waves, commits }
    }
}

/// O caminho `file` relativo à raiz, com `/`, sem `./` na frente nem bordas
/// em branco; o vazio não é caminho.
fn project_path(file: &str) -> Option<String> {
    let path = file.trim().replace('\\', "/");
    let path = path.trim_start_matches("./");
    (!path.is_empty()).then(|| path.to_string())
}

/// Põe o caminho de `file` em `files`, sem repetir.
fn push_path(files: &mut Vec<String>, file: &str) {
    if let Some(path) = project_path(file).filter(|path| !files.contains(path)) {
        files.push(path);
    }
}

/// Os arquivos que a branch do checkout em `root` mudou desde a base gravada
/// no `state` da spec: o que [`Work::of`] conta como obra.
fn branch_files(root: &Path, log: &SpecLog) -> BTreeSet<String> {
    let Some(base) = base_ref(root, log) else {
        return BTreeSet::new();
    };
    let range = format!("{base}...HEAD");
    let listed = git::run(root, &["diff", "--name-only", "-z", "--no-renames", "--relative", &range]).out().unwrap_or_default();
    listed.split('\0').filter_map(project_path).collect()
}

/// A base gravada no `state` da spec como o git do checkout em `root` a
/// acha: a branch local e, sem ela, a do `origin`. `None` sem base gravada.
fn base_ref(root: &Path, log: &SpecLog) -> Option<String> {
    let base = State::from_log(log).base?;
    let resolves = git::run(root, &["rev-parse", "--verify", "-q", &format!("{base}^{{commit}}")]).ok;
    Some(if resolves { base } else { format!("origin/{base}") })
}

/// O commit base da obra: onde a branch do checkout em `root` se separou da
/// base gravada no `state` da spec, o mesmo ponto de que [`Work::of`] conta
/// os arquivos mudados. É nele que o fechamento roda a prova de cada critério
/// no código de antes da obra. `None` sem base gravada, ou quando o git não
/// acha a base nem o ponto em que a branch saiu dela.
pub(super) fn base_commit(root: &Path, log: &SpecLog) -> Option<String> {
    let base = base_ref(root, log)?;
    let sha = git::run(root, &["merge-base", &base, "HEAD"]).out()?;
    Some(sha.trim().to_string()).filter(|sha| !sha.is_empty())
}

/// O que a saída de um comando que caiu cita, lida contra a obra.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Cited {
    /// As ondas que declararam ou entregaram um arquivo que a obra mudou e que
    /// a saída cita, na ordem da obra. Com a linha citada, só as do commit
    /// que mudou a linha por último, quando ele é da obra.
    pub waves: Vec<u64>,
    /// Os arquivos que a obra mudou e que a saída cita, na ordem em que
    /// aparecem nela. Vazio quando nada do que a saída cita é da obra, e é
    /// essa a pergunta que separa o erro da obra do que já estava na base:
    /// nenhuma onda citada não quer dizer nenhum arquivo da obra citado.
    pub mine: Vec<String>,
    /// Os arquivos do projeto que a saída cita e que a obra não mudou, na
    /// ordem em que aparecem na saída.
    pub outside: Vec<String>,
    /// A saída não cita arquivo nenhum do projeto, nem da obra nem de fora.
    pub none: bool,
}

/// A leitura única da saída `output` de um comando que caiu, para a rodada e
/// para o fechamento: as ondas da obra `work` cujo arquivo mudado a saída
/// cita, os arquivos que o git lista no projeto e que a saída cita sem a obra
/// os ter mudado, e se ela não cita arquivo nenhum. A linha citada vai ao
/// `git blame` ([`line_commits`]): o arquivo de várias ondas implica só a que
/// escreveu a linha.
pub(super) fn cited(root: &Path, output: &str, work: &Work) -> Cited {
    let listed = listed_files(root).unwrap_or_default();
    let packages = Packages::read(root, listed.iter().chain(&work.changed).map(String::as_str));
    cited_among(output, work, &listed, &packages, &|file, lines| line_commits(root, file, lines))
}

/// [`cited`] com a lista de arquivos do projeto já lida em `listed`, os
/// pacotes dele em `packages` e o `blame` que diz os commits que mudaram por
/// último as linhas citadas de um arquivo. Para cada arquivo da obra citado,
/// as ondas implicadas são as dos commits da obra que o `blame` dá às linhas
/// citadas dele; sem linha citada, ou sem commit da obra nas linhas, as que
/// declararam ou entregaram o arquivo.
fn cited_among(output: &str, work: &Work, listed: &[String], packages: &Packages, blame: &dyn Fn(&str, &[u32]) -> Vec<String>) -> Cited {
    let changed: HashSet<&str> = work.changed.iter().map(String::as_str).collect();
    let known: HashSet<&str> = changed.iter().copied().chain(listed.iter().map(String::as_str)).collect();
    let citations = citations(output, &known, packages);
    let found = distinct(&citations);
    let mut implicated: BTreeSet<u64> = BTreeSet::new();
    for file in found.iter().copied().filter(|file| changed.contains(file)) {
        let mut lines: Vec<u32> = citations.iter().filter(|(cited, _)| *cited == file).filter_map(|(_, line)| *line).collect();
        lines.sort_unstable();
        lines.dedup();
        let owners: BTreeSet<u64> = if lines.is_empty() {
            BTreeSet::new()
        } else {
            blame(file, &lines).iter().filter_map(|sha| work.commits.get(sha)).flatten().copied().collect()
        };
        if owners.is_empty() {
            implicated.extend(work.waves.iter().filter(|(_, files)| files.iter().any(|own| own == file)).map(|(wave, _)| *wave));
        } else {
            implicated.extend(owners);
        }
    }
    let waves = work.waves.iter().map(|(wave, _)| *wave).filter(|wave| implicated.contains(wave)).collect();
    let mine = found.iter().filter(|file| changed.contains(*file)).map(|file| (*file).to_string()).collect();
    let outside = found.iter().filter(|file| !changed.contains(*file)).map(|file| (*file).to_string()).collect();
    Cited { waves, mine, outside, none: found.is_empty() }
}

/// Cada citação de um arquivo de `known` na saída `output`, na ordem em que
/// aparece, com a linha colada logo depois do caminho ([`line_after`]),
/// quando vem. Cada trecho da saída feito de caracteres de
/// caminho (letras, números e `_-./\@+~`) cita o arquivo mais longo de
/// `known` que é o trecho inteiro ou o fim dele depois de uma `/`: o caminho
/// absoluto, o `./` na frente e o caminho dentro da cópia de uma onda citam o
/// arquivo relativo à raiz, e `lib/src/b.rs` não cita `src/b.rs`. O ponto
/// colado ao fim do caminho, o da frase, fica de fora. O caminho relativo que
/// uma ferramenta escreveu dentro de um dos `packages` ([`tool_lines`]) se lê
/// a partir da pasta do pacote, e só dali: o `src/a.ts` de `packages/web` não
/// cita o `src/a.ts` da raiz nem o de outro pacote. O caminho relativo escrito
/// num pacote que não se sabe qual é não cita arquivo nenhum; o absoluto, o
/// do Windows inclusive (o `C:` fica de fora do trecho), cita.
fn citations<'a>(output: &str, known: &HashSet<&'a str>, packages: &Packages) -> Vec<(&'a str, Option<u32>)> {
    let mut found: Vec<(&'a str, Option<u32>)> = Vec::new();
    for (dir, text) in tool_lines(output, packages) {
        for raw in text.split(|c: char| !(c.is_alphanumeric() || "_-./\\@+~".contains(c))) {
            // O fim do trecho no texto, para ler o que vem colado depois dele.
            let end = raw.as_ptr() as usize - text.as_ptr() as usize + raw.len();
            let word = raw.replace('\\', "/");
            let word = word.trim_end_matches('.');
            let file = match dir {
                Some("") => longest_known_tail(word, known),
                _ if word.starts_with('/') => longest_known_tail(word, known),
                Some(dir) => from_dir(dir, word).and_then(|path| known.get(path.as_str()).copied()),
                None => None,
            };
            if let Some(file) = file {
                found.push((file, line_after(&text[end..])));
            }
        }
    }
    found
}

/// Os arquivos de `citations`, na ordem, sem repetir.
fn distinct<'a>(citations: &[(&'a str, Option<u32>)]) -> Vec<&'a str> {
    let mut found: Vec<&'a str> = Vec::new();
    for (file, _) in citations {
        if !found.contains(file) {
            found.push(file);
        }
    }
    found
}

/// A linha que a ferramenta cola logo depois do caminho: `:12` em
/// `src/a.rs:12:5` e `(12` em `src/a.ts(12,5)`. `None` sem número ali.
fn line_after(rest: &str) -> Option<u32> {
    let digits = rest.strip_prefix(':').or_else(|| rest.strip_prefix('('))?;
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    digits[..end].parse().ok().filter(|line| *line > 0)
}

/// Os commits que mudaram por último as linhas `lines` do arquivo `file`, na
/// cópia de trabalho de `root`, pelo `git blame`. A linha ainda não comitada
/// vem com o código de zeros, que não é commit da obra. Vazio quando o git
/// não responde, como para a linha que o arquivo não tem.
fn line_commits(root: &Path, file: &str, lines: &[u32]) -> Vec<String> {
    let ranges: Vec<String> = lines.iter().map(|line| format!("{line},{line}")).collect();
    let mut args: Vec<&str> = vec!["blame", "--porcelain"];
    for range in &ranges {
        args.extend(["-L", range.as_str()]);
    }
    args.extend(["--", file]);
    let out = git::run(root, &args).out().unwrap_or_default();
    out.lines()
        .filter_map(|line| {
            let (sha, rest) = line.split_once(' ')?;
            (sha.len() >= 40 && sha.chars().all(|c| c.is_ascii_hexdigit()) && rest.starts_with(|c: char| c.is_ascii_digit())).then(|| sha.to_string())
        })
        .collect()
}

/// O arquivo mais longo de `known` que é o caminho `word` inteiro ou o fim
/// dele depois de uma `/`.
fn longest_known_tail<'a>(word: &str, known: &HashSet<&'a str>) -> Option<&'a str> {
    let mut rest = word;
    while !rest.is_empty() {
        if let Some(file) = known.get(rest) {
            return Some(*file);
        }
        rest = rest.split_once('/')?.1;
    }
    None
}

/// O caminho relativo `word`, escrito por uma ferramenta que rodou na pasta
/// `dir`, relativo à raiz e com `/`; `None` quando ele sai da raiz.
fn from_dir(dir: &str, word: &str) -> Option<String> {
    let dir = dir.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in dir.split('/').chain(word.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Os pacotes do projeto que as ferramentas de monorepo do JavaScript (pnpm,
/// npm, turbo e yarn) dizem ter rodado: o que o `package.json` de cada um
/// declara ([`Package`]), mais a pasta absoluta da raiz, que essas
/// ferramentas escrevem na frente da pasta do pacote.
#[derive(Debug, Default)]
pub(super) struct Packages {
    /// A pasta absoluta da raiz, com `/`, como foi dada e como o sistema a
    /// resolve.
    roots: Vec<String>,
    /// Cada pacote, um por pasta.
    found: Vec<Package>,
}

/// Um pacote do projeto, como o `package.json` dele o declara.
#[derive(Debug)]
struct Package {
    /// A pasta do `package.json`, relativa à raiz, com `/` e vazia para a raiz.
    dir: String,
    /// O nome que ele declara.
    name: Option<String>,
    /// Ele declara nome e versão, e o npm anuncia o script dele com os dois
    /// (`> @mono/web@1.0.0 lint`); sem um deles, o npm o anuncia só com o
    /// script (`> lint`).
    identified: bool,
}

impl Package {
    /// O pacote da pasta `dir` cujo `package.json` traz `manifest`.
    fn declared(dir: &str, manifest: &Value) -> Self {
        let field = |key: &str| manifest.get(key).and_then(Value::as_str).filter(|value| !value.is_empty());
        Self { dir: dir.to_string(), name: manifest.get("name").and_then(Value::as_str).map(str::to_string), identified: field("name").and(field("version")).is_some() }
    }
}

impl Packages {
    /// Os pacotes do projeto em `root`, entre os arquivos que o git lista.
    pub(super) fn of(root: &Path) -> Self {
        Self::read(root, listed_files(root).unwrap_or_default().iter().map(String::as_str))
    }

    /// Os pacotes entre os arquivos `files` do projeto em `root`: cada
    /// `package.json` que se lê, uma vez por pasta.
    fn read<'a>(root: &Path, files: impl IntoIterator<Item = &'a str>) -> Self {
        let mut found: Vec<Package> = Vec::new();
        for file in files {
            let Some(dir) = file.strip_suffix("package.json").filter(|dir| dir.is_empty() || dir.ends_with('/')) else {
                continue;
            };
            let dir = dir.trim_end_matches('/');
            if found.iter().any(|seen| seen.dir == dir) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(root.join(file)) else {
                continue;
            };
            found.push(Package::declared(dir, &serde_json::from_str::<Value>(&text).unwrap_or_default()));
        }
        let mut roots = vec![slashed(&root.to_string_lossy())];
        if let Some(real) = root.canonicalize().ok().map(|real| slashed(&real.to_string_lossy())).filter(|real| !roots.contains(real)) {
            roots.push(real);
        }
        Self { roots, found }
    }

    /// A pasta do único pacote de que `matches`, dadas a pasta e o nome, diz
    /// sim; com dois, nenhuma, para não ligar a linha ao pacote errado.
    fn only(&self, matches: impl Fn(&str, Option<&str>) -> bool) -> Option<&str> {
        let mut found = self.found.iter().filter(|package| matches(&package.dir, package.name.as_deref()));
        let package = found.next()?;
        found.next().is_none().then_some(package.dir.as_str())
    }

    /// A pasta do único pacote que declara o nome `name`.
    fn named(&self, name: &str) -> Option<&str> {
        self.only(|_, declared| declared == Some(name))
    }

    /// A pasta do único pacote que o `pnpm -r` escreve como `prefix`
    /// ([`pnpm_prefix`]).
    fn shortened(&self, prefix: &str) -> Option<&str> {
        self.only(|dir, _| pnpm_prefix(dir) == prefix)
    }

    /// A pasta do pacote que fica na pasta absoluta `place`, a raiz inclusive:
    /// a pasta da raiz mais a do pacote, inteira. A pasta que só termina como
    /// a de um pacote não é a dele.
    fn at(&self, place: &str) -> Option<&str> {
        let place = slashed(place);
        let dir = self.roots.iter().find_map(|root| if place == *root { Some("") } else { place.strip_prefix(root.as_str())?.strip_prefix('/') })?;
        self.found.iter().map(|package| package.dir.as_str()).find(|seen| *seen == dir)
    }

    /// A pasta do pacote cujo script o anúncio `banner` abriu e cuja falha o
    /// rodapé seguinte diz, com a pasta absoluta `footer` quando ele a escreve:
    /// o pacote que fica na pasta do rodapé e na do anúncio, as duas a mesma,
    /// e que declara o nome que o anúncio escreve. O anúncio sem nome
    /// (`> lint`), que o npm escreve no pacote sem nome ou sem versão, só fecha
    /// com o rodapé de um pacote assim. O rodapé de outro pacote, a pasta que
    /// não é de pacote e a falta de pasta não dão pacote nenhum.
    fn closing(&self, banner: &Banner, footer: Option<&str>) -> Option<&str> {
        let mut dir = None;
        for place in [banner.place, footer].into_iter().flatten() {
            let found = self.at(place)?;
            if dir.is_some_and(|seen| seen != found) {
                return None;
            }
            dir = Some(found);
        }
        let package = self.found.iter().find(|package| Some(package.dir.as_str()) == dir)?;
        let announced = match banner.name {
            Some(name) => package.name.as_deref() == Some(name).filter(|name| !name.is_empty()),
            None => !package.identified,
        };
        announced.then_some(package.dir.as_str())
    }

    /// A pasta do pacote e o texto da linha `line` que o turbo
    /// (`@mono/web:lint: `) ou o `yarn workspaces foreach -v`
    /// (`[@mono/web]: `) escreveu com o nome do pacote na frente; só o nome
    /// de um pacote do projeto conta.
    fn prefixed<'l>(&self, line: &'l str) -> Option<(&str, &'l str)> {
        let yarn = line.strip_prefix('[').and_then(|rest| rest.split_once("]:")).map(|(name, text)| (name, text.strip_prefix(' ').unwrap_or(text)));
        let turbo = line.split_once(':').and_then(|(name, rest)| {
            let (task, text) = rest.split_once(": ").or_else(|| Some((rest.strip_suffix(':')?, "")))?;
            (!task.is_empty() && !task.contains(char::is_whitespace)).then_some((name, text))
        });
        yarn.into_iter().chain(turbo).find_map(|(name, text)| self.named(name).map(|dir| (dir, text)))
    }
}

/// O caminho `path` com `/` no lugar de `\`, sem bordas em branco nem `/` no
/// fim.
fn slashed(path: &str) -> String {
    path.trim().replace('\\', "/").trim_end_matches('/').to_string()
}

/// O caminho `path`, com `/` ou com `\`, é absoluto: começa por `/` ou pela
/// letra de um disco do Windows (`C:\`, `C:/`).
fn absolute(path: &str) -> bool {
    let mut start = path.chars();
    path.starts_with('/') || (start.next().is_some_and(|disk| disk.is_ascii_alphabetic()) && start.next() == Some(':') && matches!(start.next(), Some('/' | '\\')))
}

/// O anúncio com que o npm e o pnpm abrem o script de um pacote, em duas
/// linhas: `> @mono/web@1.0.0 lint`, que o pnpm segue da pasta absoluta do
/// pacote, e `> tsc --noEmit -p .`, o comando.
struct Banner<'a> {
    /// O nome do pacote, vazio no pacote sem nome; `None` quando o anúncio
    /// não o escreve, como o npm no pacote sem versão (`> lint`).
    name: Option<&'a str>,
    /// A pasta absoluta onde o script rodou, que só o pnpm escreve.
    place: Option<&'a str>,
}

/// O anúncio que a linha `line` faz, seguida da linha `next`: a primeira diz
/// o pacote e o script (`> @mono/web@1.0.0 lint`, `> @mono/web@1.0.0 lint
/// all` no script de nome com espaço, `> @mono/web@next lint
/// /repo/packages/web`, `> @ lint /repo` no pacote sem nome, `> lint` no npm
/// sem versão), e a segunda, o comando (`> tsc`). Só o fim absoluto da
/// primeira é a pasta. A linha `> ` que não vem seguida de outra não anuncia
/// nada.
fn banner<'a>(line: &'a str, next: Option<&str>) -> Option<Banner<'a>> {
    next?.strip_prefix("> ")?;
    let mut words = line.strip_prefix("> ")?.splitn(3, ' ');
    let first = words.next()?;
    if words.next().is_none() {
        return Some(Banner { name: None, place: None });
    }
    let (name, _) = first.rsplit_once('@')?;
    Some(Banner { name: Some(name), place: words.next().filter(|place| absolute(place)) })
}

/// O rodapé com que o npm e o pnpm dizem que o script de um pacote caiu.
struct Footer<'a> {
    /// A pasta absoluta do pacote, quando o rodapé a escreve.
    place: Option<&'a str>,
}

/// O rodapé que a linha `line`, depois da linha `previous`, faz: `npm error
/// path <pasta>` do npm; o erro do pnpm (`ERR_PNPM_…` ou `ELIFECYCLE`), com
/// a pasta na linha de antes (`<pasta>:`), que ele só escreve quando o pacote
/// não é a pasta de onde rodou.
fn footer<'a>(line: &'a str, previous: Option<&'a str>) -> Option<Footer<'a>> {
    if let Some(place) = line.strip_prefix("npm error path ") {
        return Some(Footer { place: Some(place) });
    }
    (line.contains("ERR_PNPM") || line.contains("ELIFECYCLE")).then(|| Footer { place: previous.and_then(|previous| previous.strip_suffix(':')) })
}

/// A pasta `dir` de um pacote, relativa à raiz, como o `pnpm -r` a escreve na
/// frente das linhas: a de mais de 40 caracteres vira os 37 do fim, a partir
/// da primeira `/` que sobra neles, se houver, com `...` na frente.
fn pnpm_prefix(dir: &str) -> String {
    let dir = if dir.is_empty() { "." } else { dir };
    let length = dir.chars().count();
    if length <= 40 {
        return dir.to_string();
    }
    let tail: String = dir.chars().skip(length - 37).collect();
    match tail.find('/') {
        Some(at) if at > 0 => format!("...{}", &tail[at..]),
        _ => format!("...{tail}"),
    }
}

/// Cada linha da saída `output`, com a pasta, relativa à raiz, de onde rodou a
/// ferramenta que a escreveu — vazia para a raiz e `None` para um dos
/// `packages` que não se sabe qual é — e o texto que ela escreveu. O script
/// de um pacote roda dentro dele, e o caminho relativo da linha é de lá. Cada
/// ferramenta de monorepo diz o pacote do seu jeito:
/// - o `pnpm -r` anuncia o script de cada pacote com a pasta dele e o nome do
///   script (`packages/web lint$ tsc --noEmit`) e põe a mesma frente em cada
///   linha que o script escreve (`packages/web lint: src/a.ts(1,10): …`); a
///   pasta de mais de 40 caracteres ele encurta (`...-for-the-billing-service`),
///   e ela é a do único pacote que se encurta assim;
/// - o turbo põe o nome do pacote e a tarefa na frente de cada linha
///   (`@mono/web:lint: `), e o `yarn workspaces foreach -v`, o nome entre
///   colchetes (`[@mono/web]: `);
/// - o pnpm de um pacote só (`--filter`, `-C`) e o npm (`--workspaces`)
///   anunciam o script em duas linhas ([`banner`]) e não põem frente nas
///   linhas dele. O bloco do anúncio vale só até o rodapé de falha do mesmo
///   pacote ([`footer`], [`Packages::closing`]); o bloco que acaba no
///   próximo anúncio ou no fim da saída, sem esse rodapé, pode ter a saída
///   de outra ferramenta no fim, e as linhas dele ficam sem pacote. No
///   projeto sem `package.json`, nenhum script de pacote se anuncia, e as
///   linhas de anúncio e de rodapé são da raiz como as outras.
///
/// A linha sem nada disso, fora de bloco, é da raiz.
pub(super) fn tool_lines<'a>(output: &'a str, packages: &'a Packages) -> Vec<(Option<&'a str>, &'a str)> {
    let lines: Vec<&str> = output.lines().collect();
    let announced: Vec<(&str, Option<&str>)> = lines
        .iter()
        .filter_map(|line| line.split_once("$ "))
        .filter_map(|(head, _)| head.rsplit_once(' ').filter(|(dir, stage)| !dir.trim().is_empty() && !stage.is_empty()).map(|(dir, _)| (head, dir)))
        .map(|(head, dir)| (head, if dir.starts_with("...") { packages.shortened(dir) } else { Some(dir) }))
        .collect();
    let mut out: Vec<(Option<&str>, &str)> = Vec::with_capacity(lines.len());
    // O anúncio do bloco aberto e as linhas dele em `out`, sem pacote até o
    // rodapé dizer qual é.
    let mut open: Option<(Banner, Vec<usize>)> = None;
    for (at, line) in lines.iter().copied().enumerate() {
        let recursive = announced.iter().find_map(|(head, dir)| line.strip_prefix(head).and_then(|rest| rest.strip_prefix(": ")).map(|text| (*dir, text)));
        if let Some(found) = recursive.or_else(|| packages.prefixed(line).map(|(dir, text)| (Some(dir), text))) {
            out.push(found);
            continue;
        }
        if !packages.found.is_empty()
            && let Some(found) = banner(line, lines.get(at + 1).copied())
        {
            open = Some((found, vec![out.len()]));
            out.push((None, line));
            continue;
        }
        let Some((block, mut held)) = open.take() else {
            out.push((Some(""), line));
            continue;
        };
        let previous = at.checked_sub(1).and_then(|before| lines.get(before)).copied();
        if let Some(end) = footer(line, previous) {
            let dir = packages.closing(&block, end.place);
            for index in held {
                out[index].0 = dir;
            }
            out.push((Some(""), line));
        } else {
            held.push(out.len());
            out.push((None, line));
            open = Some((block, held));
        }
    }
    out
}

pub(crate) fn reusable(root: &Path, log: &mustard_core::domain::spec_events::SpecLog) -> bool {
    let Some(key) = fingerprint(root, log) else {
        return false;
    };
    log.events
        .iter()
        .rev()
        .find(|e| e.event_type == "stage_run" && e.str_field("stage") == Some("final-validation"))
        .is_some_and(|e| e.str_field("result") == Some("pass") && e.str_field("fingerprint") == Some(&key))
}

pub(super) fn receipt(root: &Path, spec: &str, key: &str) -> Result<(), Refusal> {
    let draft = json!({"phase":"final", "stage":"final-validation", "result":"pass", "ms":0,
        "fingerprint":key, "scope":"receipt", "version":mustard_core::harness_version(), "author":"binary"});
    let Some(draft) = draft.as_object().cloned() else {
        return Err(Refusal::NotAnObject { detail: String::new() });
    };
    record(root, spec, "stage_run", draft, PhaseWriter::Binary).map(|_| ())
}

/// Grava a execução `out` da etapa `stage` e devolve o evento gravado: é ele
/// que a resposta cita para quem quer ler a saída inteira.
pub(super) fn record_run(root: &Path, spec: &str, phase: &str, stage: &str, command: &str, out: &ProofRun) -> Result<mustard_core::io::spec_events::Written, Refusal> {
    record_run_fields(root, spec, phase, stage, command, out, Map::new())
}

/// [`record_run`] com os campos de `draft` a mais na execução gravada.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_run_fields(
    root: &Path,
    spec: &str,
    phase: &str,
    stage: &str,
    command: &str,
    out: &ProofRun,
    mut draft: Map<String, Value>,
) -> Result<mustard_core::io::spec_events::Written, Refusal> {
    draft.insert("phase".into(), json!(phase));
    draft.insert("stage".into(), json!(stage));
    draft.insert("command".into(), json!(crate::commands::review::qa_run::canonical_test_command(command)));
    draft.insert("result".into(), json!(out.result));
    draft.insert("exit".into(), json!(out.exit));
    draft.insert("ms".into(), json!(out.ms));
    draft.insert("scope".into(), json!(if out.reused { "receipt" } else { "leaf" }));
    draft.insert("reused".into(), json!(out.reused));
    draft.insert("version".into(), json!(mustard_core::harness_version()));
    draft.insert("attempt".into(), json!(std::process::id()));
    draft.insert("author".into(), Value::String("binary".into()));
    if !out.output.is_empty() {
        draft.insert("output".into(), json!(out.output));
    }
    record(root, spec, "stage_run", draft, PhaseWriter::Binary).map(|recorded| recorded.written)
}

/// O tempo de relógio da revisão final inclui o despacho e a espera do
/// hospedeiro; é observação, e não soma às durações das etapas da máquina.
/// Ele conta do pedido que o veredito responde ([`answered_request`]), na
/// versão que o gravou: a versão que o lançamento do revisor grava depois
/// deixaria a espera do despacho de fora.
pub(super) fn review_elapsed(root: &Path, spec: &str, log: &mustard_core::domain::spec_events::SpecLog) -> Result<(), Refusal> {
    let verdict = log.visible().into_iter().rev().find(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&Value::Bool(true)));
    let Some(verdict) = verdict else {
        return Ok(());
    };
    let key = format!("review-verdict-{}", verdict.id);
    if log.events.iter().any(|e| e.event_type == "stage_run" && e.str_field("fingerprint") == Some(&key)) {
        return Ok(());
    }
    let Some(sent) = answered_request(log, verdict.id).and_then(|asked| log.get(asked)) else {
        return Ok(());
    };
    let Some(ms) = chrono::DateTime::parse_from_rfc3339(sent.at())
        .ok()
        .zip(chrono::DateTime::parse_from_rfc3339(verdict.at()).ok())
        .and_then(|(from, to)| u64::try_from((to - from).num_milliseconds()).ok())
    else {
        return Ok(());
    };
    let draft = json!({"phase":"final","stage":"independent-review-elapsed","result":if verdict.str_field("result")==Some("approved"){"pass"}else{"fail"},
        "ms":ms,"scope":"inclusive","attempt":sent.id,"version":mustard_core::harness_version(),"fingerprint":key,"author":"binary"});
    record(root, spec, "stage_run", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary).map(|_| ())
}

/// A saída de `pnpm -r lint` (pnpm 10.18, tsc 7.0 com `noUnusedLocals`) num
/// monorepo de dois pacotes, `packages/web` e `packages/api`, com o mesmo
/// `src/a.ts`: o tsc do `packages/web` caiu primeiro e o pnpm parou o do
/// `packages/api` antes de ele escrever o aviso. `{root}` é a raiz.
#[cfg(test)]
pub(super) const PNPM_RECURSIVE_LINT: &str = "Scope: 2 of 3 workspace projects
packages/api lint$ tsc --noEmit -p .
packages/web lint$ tsc --noEmit -p .
packages/web lint: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
packages/web lint: Failed
{root}/packages/web:
\u{2009}ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL\u{2009} @mono/web@1.0.0 lint: `tsc --noEmit -p .`
Exit status 1
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
";

/// A saída real de cada ferramenta de monorepo do JavaScript que roda o
/// script `lint` (`tsc --noEmit -p .`, tsc 7.0 com `noUnusedLocals`) num
/// monorepo de dois pacotes, `packages/web` (`@mono/web`) e `packages/api`
/// (`@mono/api`), com o mesmo `src/a.ts`, e as pastas dos pacotes cujo
/// `src/a.ts` ela cita, na ordem em que aparecem: pnpm 10.18, npm 11.19,
/// turbo 2.11 sobre o npm e yarn 4.18, com o stderr na ordem em que saiu
/// (`2>&1`). `{root}` é a raiz, e o caminho do npm que o turbo mostra foi
/// encurtado.
#[cfg(test)]
pub(super) const MONOREPO_LINT: [(&str, &str, &[&str]); 5] = [
    (
        "pnpm --filter @mono/web lint",
        "
> @mono/web@1.0.0 lint {root}/packages/web
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
{root}/packages/web:
\u{2009}ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL\u{2009} @mono/web@1.0.0 lint: `tsc --noEmit -p .`
Exit status 1
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
",
        &["packages/web"],
    ),
    (
        "pnpm -C packages/web lint",
        "
> @mono/web@1.0.0 lint {root}/packages/web
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
\u{2009}ELIFECYCLE\u{2009} Command failed with exit code 1.
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
",
        &["packages/web"],
    ),
    (
        "npm run lint --workspaces",
        "
> @mono/api@1.0.0 lint
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
npm error Lifecycle script `lint` failed with error:
npm error code 1
npm error path {root}/packages/api
npm error workspace @mono/api@1.0.0
npm error location {root}/packages/api
npm error command failed
npm error command sh -c tsc --noEmit -p .


> @mono/web@1.0.0 lint
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
npm error Lifecycle script `lint` failed with error:
npm error code 1
npm error path {root}/packages/web
npm error workspace @mono/web@1.0.0
npm error location {root}/packages/web
npm error command failed
npm error command sh -c tsc --noEmit -p .
",
        &["packages/api", "packages/web"],
    ),
    (
        "turbo run lint --continue",
        " WARNING  No locally installed `turbo` found in your repository. Using globally installed version (2.11.8). Using a specified version in your repository will result in more predictable behavior.

   \u{2022} turbo 2.11.8
   \u{2022} Packages in scope: @mono/api, @mono/web
   \u{2022} Running lint in 2 packages
   \u{2022} Remote caching disabled

@mono/web:lint: cache miss, executing 78e710077785ff2b
@mono/api:lint: cache miss, executing 6196c948bbf18084
@mono/web:lint:\x20
@mono/web:lint: > @mono/web@1.0.0 lint
@mono/web:lint: > tsc --noEmit -p .
@mono/web:lint:\x20
@mono/api:lint:\x20
@mono/api:lint: > @mono/api@1.0.0 lint
@mono/api:lint: > tsc --noEmit -p .
@mono/api:lint:\x20
@mono/web:lint: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
@mono/api:lint: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
@mono/web:lint: npm error Lifecycle script `lint` failed with error:
@mono/api:lint: npm error Lifecycle script `lint` failed with error:
@mono/web:lint: npm error code 1
@mono/api:lint: npm error code 1
@mono/web:lint: npm error path {root}/packages/web
@mono/api:lint: npm error path {root}/packages/api
@mono/api:lint: npm error workspace @mono/api@1.0.0
@mono/web:lint: npm error workspace @mono/web@1.0.0
@mono/api:lint: npm error location {root}/packages/api
@mono/web:lint: npm error location {root}/packages/web
@mono/api:lint: npm error command failed
@mono/web:lint: npm error command failed
@mono/api:lint: npm error command sh -c tsc --noEmit -p .
@mono/web:lint: npm error command sh -c tsc --noEmit -p .
@mono/web#lint:  WARNING  command finished with error, but continuing...
@mono/api#lint:  WARNING  command finished with error, but continuing...
@mono/web#lint:  ERROR  command ({root}/packages/web) /usr/bin/npm run lint exited (1)
@mono/api#lint:  ERROR  command ({root}/packages/api) /usr/bin/npm run lint exited (1)

 Tasks:    0 successful, 2 total
Cached:    0 cached, 2 total
  Time:    1.203s\x20
Failed:    @mono/api#lint, @mono/web#lint

 ERROR  run failed: command  exited (1)
",
        &["packages/web", "packages/api"],
    ),
    (
        "yarn workspaces foreach -A -pv run lint",
        "[@mono/web]: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
[@mono/api]: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
The command failed in workspace @mono/api@workspace:packages/api with exit code 1
The command failed in workspace @mono/web@workspace:packages/web with exit code 1
Failed with errors in 0s 448ms
",
        &["packages/web", "packages/api"],
    ),
];

/// A saída real do npm 11.19 e do pnpm 10.18 (tsc 7.0 com `noUnusedLocals`)
/// num monorepo com o mesmo `src/a.ts` na raiz (`mono`), em `packages/web`
/// (`@mono/web`) e em `packages/api` (`@mono/api`), em que o anúncio do
/// script de um pacote não vale até o fim da saída, e as pastas dos pacotes
/// cujo `src/a.ts` ela cita. O `package.json` de `packages/web` sem versão
/// faz o npm anunciar o script só com `> lint`; o rodapé do npm, no stderr,
/// fecha o bloco e chega depois dele, com o `2>&1` e pelo executor dos
/// comandos, que lê as duas saídas num cano só. Quando o pacote passa e a
/// ferramenta da raiz cai depois dele, não há rodapé de pacote nenhum.
/// `{root}` é a raiz.
#[cfg(test)]
pub(super) const PACKAGE_BLOCKS: [(&str, &str, &[&str]); 4] = [
    (
        "npm run lint --workspaces 2>&1, sem versão em packages/web",
        "
> @mono/api@1.0.0 lint
> tsc --noEmit -p .


> lint
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
npm error Lifecycle script `lint` failed with error:
npm error code 1
npm error path {root}/packages/web
npm error workspace @mono/web
npm error location {root}/packages/web
npm error command failed
npm error command sh -c tsc --noEmit -p .
",
        &["packages/web"],
    ),
    (
        "npm run lint --workspaces && tsc --noEmit -p .",
        "
> @mono/api@1.0.0 lint
> tsc --noEmit -p .


> @mono/web@1.0.0 lint
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
",
        &[],
    ),
    (
        "pnpm --filter @mono/web lint && tsc --noEmit -p .",
        "
> @mono/web@1.0.0 lint {root}/packages/web
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
",
        &[],
    ),
    (
        "npm run lint, com npm run lint --workspaces && tsc --noEmit -p . na raiz",
        "
> mono@1.0.0 lint
> npm run lint --workspaces && tsc --noEmit -p .


> @mono/api@1.0.0 lint
> tsc --noEmit -p .


> @mono/web@1.0.0 lint
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
",
        &[],
    ),
];

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};

    use serde_json::{Value, json};

    use super::{Cited, MONOREPO_LINT, PACKAGE_BLOCKS, PNPM_RECURSIVE_LINT, Package, Packages, Work, citations, cited, cited_among, distinct};

    /// Os arquivos que a saída cita, na ordem, sem repetir.
    fn paths_in<'a>(output: &str, known: &HashSet<&'a str>, packages: &Packages) -> Vec<&'a str> {
        distinct(&citations(output, known, packages))
    }

    /// A linha colada ao caminho vem com a citação, no jeito do compilador
    /// (`arquivo:linha:coluna`) e no do `tsc` (`arquivo(linha,coluna)`); o
    /// caminho sem número colado vem sem linha.
    #[test]
    fn a_cited_line_comes_with_the_path_in_both_tool_formats() {
        let known: HashSet<&str> = ["src/a.rs", "src/b.ts", "src/c.rs"].into_iter().collect();
        let output = "error: x\n  --> src/a.rs:12:5\nsrc/b.ts(3,10): error TS6133\nwarning in src/c.rs: 7 problems\n";
        assert_eq!(citations(output, &known, &Packages::default()), [("src/a.rs", Some(12)), ("src/b.ts", Some(3)), ("src/c.rs", None)]);
    }

    /// A saída real de `pnpm -r --no-bail lint` (pnpm 10.18, tsc 7.0 com
    /// `noUnusedLocals`) num monorepo de quatro pacotes com o mesmo
    /// `src/a.ts`: `packages/a-very-long-package-name-for-the-billing-service`,
    /// que o pnpm encurta; `packages/billing-service`, de mesmo fim de nome; e
    /// `packages/x-very-long-package-name-for-the-payment-service` e
    /// `packages/y-very-long-package-name-for-the-payment-service`, que o pnpm
    /// encurta para a mesma frente. `{root}` é a raiz.
    const PNPM_LONG_FOLDERS_LINT: &str = "Scope: 4 of 5 workspace projects
...-package-name-for-the-billing-service lint$ tsc --noEmit -p .
packages/billing-service lint$ tsc --noEmit -p .
...-package-name-for-the-payment-service lint$ tsc --noEmit -p .
...-package-name-for-the-payment-service lint$ tsc --noEmit -p .
...-package-name-for-the-billing-service lint: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
packages/billing-service lint: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
...-package-name-for-the-billing-service lint: Failed
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
packages/billing-service lint: Failed
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
...-package-name-for-the-payment-service lint: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
...-package-name-for-the-payment-service lint: Failed
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
...-package-name-for-the-payment-service lint: src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
...-package-name-for-the-payment-service lint: Failed
\u{2009}ERR_PNPM_RECURSIVE_FAIL\u{2009}\x20


Summary: 4 fails, 0 passes

{root}/packages/a-very-long-package-name-for-the-billing-service:
\u{2009}ERROR\u{2009} @mono/a-very-long-package-name-for-the-billing-service@1.0.0 lint: `tsc --noEmit -p .`
Exit status 1

{root}/packages/billing-service:
\u{2009}ERROR\u{2009} @mono/billing-service@1.0.0 lint: `tsc --noEmit -p .`
Exit status 1

{root}/packages/x-very-long-package-name-for-the-payment-service:
\u{2009}ERROR\u{2009} @mono/x-very-long-package-name-for-the-payment-service@1.0.0 lint: `tsc --noEmit -p .`
Exit status 1

{root}/packages/y-very-long-package-name-for-the-payment-service:
\u{2009}ERROR\u{2009} @mono/y-very-long-package-name-for-the-payment-service@1.0.0 lint: `tsc --noEmit -p .`
Exit status 1
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
";

    /// Os pacotes de um projeto na pasta absoluta `root`: a pasta de cada um
    /// e o que o `package.json` dele traz.
    fn project(root: &str, found: &[(&str, Value)]) -> Packages {
        Packages { roots: vec![root.to_string()], found: found.iter().map(|(dir, manifest)| Package::declared(dir, manifest)).collect() }
    }

    /// Os pacotes de um projeto em `/repo`: a raiz, `mono`, e os das pastas
    /// `dirs`, cada um com o nome `@mono/` mais o fim da pasta, todos na
    /// versão `1.0.0`.
    fn packages(dirs: &[&str]) -> Packages {
        let found: Vec<(&str, Value)> = std::iter::once(("", json!({"name": "mono", "version": "1.0.0"})))
            .chain(dirs.iter().map(|dir| (*dir, json!({"name": format!("@mono/{}", dir.rsplit('/').next().unwrap_or(dir)), "version": "1.0.0"}))))
            .collect();
        project("/repo", &found)
    }

    /// Com o mesmo `src/a.ts` na raiz, fora da obra, e nos dois pacotes, cada
    /// um mudado por uma onda, a saída do tsc rodado dentro de `packages/web`
    /// cita só o arquivo dele: só a onda desse pacote reabre, e nada de fora.
    #[test]
    fn a_path_written_inside_a_workspace_package_cites_the_file_of_that_package() {
        let (web, api) = ("packages/web/src/a.ts".to_string(), "packages/api/src/a.ts".to_string());
        let work =
            Work { changed: BTreeSet::from([web.clone(), api.clone()]), waves: vec![(1, vec![api.clone()]), (2, vec![web.clone()])], commits: Default::default() };
        let listed = ["src/a.ts".to_string(), web.clone(), api];
        let output = PNPM_RECURSIVE_LINT.replace("{root}", "/repo");
        let expected = Cited { waves: vec![2], mine: vec![web], outside: vec![], none: false };
        assert_eq!(cited_among(&output, &work, &listed, &Packages::default(), &|_, _| Vec::new()), expected);
    }

    /// A pasta de mais de 40 caracteres que o `pnpm -r` encurta leva ao único
    /// pacote que se encurta assim, e não ao de mesmo fim de nome; a frente
    /// que dois pacotes encurtados dividem não leva a nenhum deles, nem à raiz.
    #[test]
    fn a_package_folder_that_pnpm_shortens_cites_only_the_package_shortened_that_way() {
        let dirs = [
            "packages/a-very-long-package-name-for-the-billing-service",
            "packages/billing-service",
            "packages/x-very-long-package-name-for-the-payment-service",
            "packages/y-very-long-package-name-for-the-payment-service",
        ];
        let files: Vec<String> = std::iter::once("src/a.ts".to_string()).chain(dirs.iter().map(|dir| format!("{dir}/src/a.ts"))).collect();
        let known: HashSet<&str> = files.iter().map(String::as_str).collect();
        let output = PNPM_LONG_FOLDERS_LINT.replace("{root}", "/repo");
        assert_eq!(paths_in(&output, &known, &packages(&dirs)), [files[1].as_str(), files[2].as_str()]);
    }

    /// Na saída real do pnpm de um pacote só, do npm, do turbo e do yarn, o
    /// `src/a.ts` que o tsc escreveu dentro de um pacote cita o arquivo desse
    /// pacote, e não o da raiz nem o do outro pacote, também com as pastas
    /// absolutas que essas ferramentas escrevem no Windows (`C:\repo\…`).
    #[test]
    fn each_monorepo_tool_cites_the_file_of_the_package_its_script_ran_in() {
        let known: HashSet<&str> = ["src/a.ts", "packages/web/src/a.ts", "packages/api/src/a.ts"].into_iter().collect();
        for (root, separator) in [("/repo", "/"), ("C:\\repo", "\\")] {
            let packages = project(
                &root.replace('\\', "/"),
                &[
                    ("", json!({"name": "mono", "version": "1.0.0"})),
                    ("packages/web", json!({"name": "@mono/web", "version": "1.0.0"})),
                    ("packages/api", json!({"name": "@mono/api", "version": "1.0.0"})),
                ],
            );
            for (tool, output, dirs) in MONOREPO_LINT {
                let output = ["web", "api"].into_iter().fold(output.to_string(), |text, name| {
                    text.replace(&format!("{{root}}/packages/{name}"), &[root, "packages", name].join(separator))
                });
                let expected: Vec<String> = dirs.iter().map(|dir| format!("{dir}/src/a.ts")).collect();
                assert_eq!(paths_in(&output.replace("{root}", root), &known, &packages), expected, "{tool} em {root}");
            }
        }
    }

    /// Cada caminho da saída cita o arquivo do projeto que ele nomeia também
    /// com `./` na frente, com a `\` do Windows, com a posição entre
    /// parênteses ou com o ponto da frase colado ao fim, e o arquivo citado
    /// duas vezes entra uma vez só. No bloco de um pacote que não se sabe qual
    /// é, o caminho absoluto do Windows cita, e o relativo não.
    #[test]
    fn a_failure_output_cites_a_project_file_in_every_spelling_of_its_path() {
        let known: HashSet<&str> = ["src/b.rs", "web/x.ts"].into_iter().collect();
        let output = "./src/b.rs(2,1): erro\nem C:\\repo\\web\\x.ts.\nde novo src/b.rs:2";
        assert_eq!(paths_in(output, &known, &Packages::default()), vec!["src/b.rs", "web/x.ts"]);
        let unknown_package = "> lint\n> tsc\n\nC:\\repo\\web\\x.ts(1,10): erro\nsrc/b.rs(2,1): erro";
        assert_eq!(paths_in(unknown_package, &known, &packages(&[])), vec!["web/x.ts"], "num pacote que não se sabe qual é, só o absoluto cita");
    }

    /// No projeto sem `package.json`, nenhum script de pacote se anuncia: as
    /// linhas `> ` seguidas e o rodapé de falha do pnpm são da raiz como as
    /// outras, e o caminho relativo entre eles cita o arquivo da raiz.
    #[test]
    fn without_a_package_json_the_banner_and_footer_lines_belong_to_the_root() {
        let known: HashSet<&str> = ["src/a.ts", "App/Calc.cs"].into_iter().collect();
        let output = "> lint\n> tsc --noEmit -p .\n\nsrc/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.\n\
            App/Calc.cs(8,14): warning IDE0051: Private member 'Calc.LonelyTotal' is unused\n\u{2009}ELIFECYCLE\u{2009} Command failed with exit code 1.\n";
        assert_eq!(paths_in(output, &known, &Packages::default()), ["src/a.ts", "App/Calc.cs"]);
    }

    /// O anúncio do script de um pacote vale só até o rodapé de falha dele: o
    /// `> lint` do npm sem versão fecha o bloco do pacote anterior, e o
    /// `src/a.ts` de `packages/web` cita o arquivo de lá, e não o de
    /// `packages/api`. O bloco sem esse rodapé, como o do pacote que passou
    /// antes de a ferramenta da raiz cair, não cita o arquivo do pacote nem o
    /// da raiz.
    #[test]
    fn a_package_block_holds_only_until_the_failure_footer_of_that_package() {
        let known: HashSet<&str> = ["src/a.ts", "packages/web/src/a.ts", "packages/api/src/a.ts"].into_iter().collect();
        let packages = project(
            "/repo",
            &[
                ("", json!({"name": "mono", "version": "1.0.0"})),
                ("packages/web", json!({"name": "@mono/web"})),
                ("packages/api", json!({"name": "@mono/api", "version": "1.0.0"})),
            ],
        );
        for (tool, output, dirs) in PACKAGE_BLOCKS {
            let expected: Vec<String> = dirs.iter().map(|dir| format!("{dir}/src/a.ts")).collect();
            assert_eq!(paths_in(&output.replace("{root}", "/repo"), &known, &packages), expected, "{tool}");
        }
    }

    /// O anúncio é a linha do pacote e do script seguida da linha do comando,
    /// seja qual for a versão do pacote e o nome do script: a saída real do
    /// `npm run lint --workspaces 2>&1` com `packages/web` na versão `next`, a
    /// do `npm run "lint all" -w packages/web 2>&1` e a do `pnpm -C
    /// packages/web lint` com o script `tsc`, de uma palavra só, citam o
    /// `src/a.ts` de `packages/web`. A linha `> tsc`, sem outra `> ` depois,
    /// não anuncia nada, e o fim do anúncio que não é pasta absoluta é parte
    /// do nome do script.
    #[test]
    fn a_banner_is_the_package_line_followed_by_the_command_line() {
        const NPM_SCRIPT_WITH_SPACE: &str = "
> @mono/web@1.0.0 lint all
> tsc

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
npm error Lifecycle script `lint all` failed with error:
npm error code 1
npm error path {root}/packages/web
npm error workspace @mono/web@1.0.0
npm error location {root}/packages/web
npm error command failed
npm error command sh -c tsc
";
        const NPM_NEXT_VERSION: &str = "
> @mono/api@1.0.0 lint
> tsc


> @mono/web@next lint
> tsc

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
npm error Lifecycle script `lint` failed with error:
npm error code 1
npm error path {root}/packages/web
npm error workspace @mono/web@next
npm error location {root}/packages/web
npm error command failed
npm error command sh -c tsc
";
        const PNPM_ONE_WORD_COMMAND: &str = "
> @mono/web@next lint {root}/packages/web
> tsc

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
\u{2009}ELIFECYCLE\u{2009} Command failed with exit code 1.
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
";
        let known: HashSet<&str> = ["src/a.ts", "packages/web/src/a.ts", "packages/api/src/a.ts"].into_iter().collect();
        for output in [NPM_NEXT_VERSION, NPM_SCRIPT_WITH_SPACE, PNPM_ONE_WORD_COMMAND] {
            assert_eq!(paths_in(&output.replace("{root}", "/repo"), &known, &packages(&["packages/web", "packages/api"])), ["packages/web/src/a.ts"], "{output}");
        }
    }

    /// O rodapé de outro pacote não fecha o bloco do anúncio: o script da raiz
    /// que roda o tsc nela e depois `pnpm --filter … exec tsc` em
    /// `packages/api`, que cai sem anúncio, deixa as linhas da raiz e do
    /// pacote sem pacote. Na saída real do npm, o nome do anúncio (`mono`) não
    /// é o do pacote do rodapé; na mesma saída com a raiz sem versão, que o
    /// npm anuncia só com `> lint`, o pacote do rodapé declara nome e versão e
    /// não seria anunciado assim; na do pnpm, sem nome nenhum, a pasta do
    /// anúncio (a raiz) não é a do rodapé.
    #[test]
    fn a_footer_of_another_package_leaves_the_block_without_package() {
        const NPM_ROOT_SCRIPT: &str = "
> mono@1.0.0 lint
> tsc --noEmit -p . ; pnpm --filter @mono/api exec tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
src/b.ts(1,10): error TS6133: 'lonelyCount' is declared but its value is never read.
undefined
{root}/packages/api:
\u{2009}ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL\u{2009} Command failed with exit code 1: tsc --noEmit -p .
";
        const PNPM_UNNAMED_ROOT_SCRIPT: &str = "
> @ lint {root}
> tsc --noEmit -p . ; pnpm --filter ./packages/api exec tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
src/b.ts(1,10): error TS6133: 'lonelyCount' is declared but its value is never read.
undefined
{root}/packages/api:
\u{2009}ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL\u{2009} Command failed with exit code 1: tsc --noEmit -p .
\u{2009}ELIFECYCLE\u{2009} Command failed with exit code 1.
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
";
        let known: HashSet<&str> = ["src/a.ts", "packages/api/src/a.ts", "packages/api/src/b.ts"].into_iter().collect();
        let api = ("packages/api", json!({"name": "@mono/api", "version": "1.0.0"}));
        let named = project("/repo", &[("", json!({"name": "mono", "version": "1.0.0"})), api.clone()]);
        assert_eq!(paths_in(&NPM_ROOT_SCRIPT.replace("{root}", "/repo"), &known, &named), Vec::<&str>::new(), "npm");
        let unversioned = project("/repo", &[("", json!({"name": "mono"})), api]);
        let bare = NPM_ROOT_SCRIPT.replace("> mono@1.0.0 lint", "> lint").replace("{root}", "/repo");
        assert_eq!(paths_in(&bare, &known, &unversioned), Vec::<&str>::new(), "npm, raiz sem versão");
        let unnamed = project("/repo", &[("", json!({})), ("packages/api", json!({}))]);
        assert_eq!(paths_in(&PNPM_UNNAMED_ROOT_SCRIPT.replace("{root}", "/repo"), &known, &unnamed), Vec::<&str>::new(), "pnpm");
    }

    /// A raiz sem nome numa pasta de mesmo nome que um pacote (`/home/dev/web`,
    /// com o pacote `web`) é a raiz: na saída real do `pnpm lint` dela, o
    /// `src/a.ts` cita o arquivo da raiz, e não o do pacote.
    #[test]
    fn a_root_folder_named_like_a_package_is_still_the_root() {
        const PNPM_ROOT_SCRIPT: &str = "
> @ lint {root}
> tsc --noEmit -p .

src/a.ts(1,10): error TS6133: 'lonelyTotal' is declared but its value is never read.
\u{2009}ELIFECYCLE\u{2009} Command failed with exit code 1.
\u{2009}WARN\u{2009}  Local package.json exists, but node_modules missing, did you mean to install?
";
        let known: HashSet<&str> = ["src/a.ts", "web/src/a.ts"].into_iter().collect();
        let packages = project("/home/dev/web", &[("", json!({})), ("web", json!({}))]);
        assert_eq!(paths_in(&PNPM_ROOT_SCRIPT.replace("{root}", "/home/dev/web"), &known, &packages), ["src/a.ts"]);
    }

    /// A leitura da rodada e do fechamento acha os pacotes pelo
    /// `package.json` de cada um no projeto: com a saída real do npm sem
    /// versão em `packages/web`, só a onda que mudou o `src/a.ts` de lá reabre.
    /// O `package.json` que a obra mudou, e que o git também lista, conta uma
    /// vez só: na saída real do turbo, que liga cada linha ao pacote pelo
    /// nome, as duas ondas reabrem, e o `src/a.ts` da raiz fica de fora. Com a
    /// raiz dada por um atalho, a pasta que a ferramenta escreve é a de
    /// verdade, e a leitura é a mesma.
    #[test]
    fn the_failure_reading_finds_the_packages_by_their_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = &dir.path().join("repo");
        let work = npm_monorepo(root);
        #[cfg(unix)]
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root, &link).unwrap();
        for ((tool, output, _), waves) in [(PACKAGE_BLOCKS[0], vec![2]), (MONOREPO_LINT[3], vec![1, 2])] {
            let written = |place: &std::path::Path| output.replace("{root}", &place.to_string_lossy());
            let mine = cited(root, &written(root), &work).mine;
            let expected = Cited { waves, mine, outside: vec![], none: false };
            assert_eq!(cited(root, &written(root), &work), expected, "{tool}");
            #[cfg(unix)]
            assert_eq!(cited(&link, &written(&root.canonicalize().unwrap()), &work), expected, "{tool}, pela pasta de verdade");
        }
    }

    /// O `npm run lint --workspaces` declarado sem `2>&1` escreve o anúncio e
    /// o erro do tsc de cada pacote no stdout e o rodapé `npm error path` no
    /// stderr. Rodado pelo executor dos comandos, como a rodada e o fechamento
    /// rodam o lint, o rodapé chega depois do bloco do pacote, e o `src/a.ts`
    /// cita o arquivo de `packages/web`: só a onda que o mudou reabre.
    #[test]
    fn a_package_failure_split_across_both_streams_cites_the_package_file_through_the_executor() {
        let dir = tempfile::tempdir().unwrap();
        let root = &dir.path().join("repo");
        let work = npm_monorepo(root);
        let output = PACKAGE_BLOCKS[0].1.replace("{root}", &root.to_string_lossy());
        let footer = output.find("npm error ").expect("o rodapé do npm");
        let streams = dir.path().join("streams");
        std::fs::create_dir_all(&streams).unwrap();
        std::fs::write(streams.join("stdout"), &output[..footer]).unwrap();
        std::fs::write(streams.join("stderr"), &output[footer..]).unwrap();
        let run = crate::commands::review::qa_run::run_server_command("cat stdout; cat stderr >&2; exit 1", &streams);
        assert_eq!(run.result, "fail", "{}", run.output);
        let mine = cited(root, &run.output, &work).mine;
        assert!(mine.iter().any(|file| file.ends_with("packages/web/src/a.ts")), "{mine:?}");
        assert_eq!(cited(root, &run.output, &work), Cited { waves: vec![2], mine, outside: vec![], none: false }, "{}", run.output);
    }

    /// O monorepo do npm em `root`, sob git, com o mesmo `src/a.ts` na raiz e
    /// nos pacotes `packages/web`, sem versão, e `packages/api`, e a obra em
    /// que a onda 1 mudou o `src/a.ts` de `packages/api` e a onda 2 o de
    /// `packages/web` e o `package.json` dele.
    fn npm_monorepo(root: &std::path::Path) -> Work {
        let manifests = [
            ("", json!({"name": "mono", "version": "1.0.0", "private": true, "workspaces": ["packages/*"]})),
            ("packages/web", json!({"name": "@mono/web", "private": true})),
            ("packages/api", json!({"name": "@mono/api", "version": "1.0.0", "private": true})),
        ];
        for (folder, manifest) in manifests {
            std::fs::create_dir_all(root.join(folder).join("src")).unwrap();
            std::fs::write(root.join(folder).join("package.json"), manifest.to_string()).unwrap();
            std::fs::write(root.join(folder).join("src/a.ts"), "function lonelyTotal(): number {\n  return 2;\n}\n").unwrap();
        }
        assert!(std::process::Command::new("git").args(["init", "-q"]).current_dir(root).status().unwrap().success());
        let (web, api, manifest) = ("packages/web/src/a.ts".to_string(), "packages/api/src/a.ts".to_string(), "packages/web/package.json".to_string());
        Work {
            changed: BTreeSet::from([web.clone(), api.clone(), manifest.clone()]),
            waves: vec![(1, vec![api]), (2, vec![web, manifest])],
            commits: Default::default(),
        }
    }

    /// A impressão leva o `PATH` e só as variáveis que um comando cita pelo
    /// nome: a que nenhum comando cita fica de fora, e o pedaço de outro nome
    /// não conta como citação.
    #[test]
    fn the_fingerprint_keeps_only_the_path_and_the_variables_a_command_names() {
        let env = ["PATH", "TOKEN", "TOKEN_OLD", "EDITOR", "DB_URL"].map(|name| (name.into(), "v".into()));
        let kept = super::cited_env(env.into_iter(), &["TOKEN=1 cargo test", "psql ${DB_URL}"]);
        let names: Vec<String> = kept.into_iter().map(|(name, _)| name.to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["DB_URL", "PATH", "TOKEN"]);
    }

    /// O tempo de cada etapa da rodada vai para o arquivo de operações da
    /// spec, e a spec fica como estava. A etapa do fechamento continua na spec:
    /// a máquina que falha depois do recibo da validação final o desfaz.
    #[test]
    fn round_stage_timings_go_to_the_operations_file_and_leave_the_spec_as_it_was() {
        use mustard_core::io::spec_events as store;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        crate::shared::spec_state::stand_on_spec_branch(root, "etapas");
        crate::commands::spec_events::write::record_open(root, "etapas", "feature/etapas", "dev").expect("open");
        let path = store::spec_file(root, "etapas").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        for stage in ["lock-wait", "join", "commit"] {
            let mut timing = super::Stage::new(root, "etapas", "delivery", stage, false);
            timing.passed();
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "the round's timings leave the spec as it was");
        let observed = crate::commands::spec_events::conversation::recorded_operations(root, "etapas");
        let stages: Vec<(&str, &str)> = observed
            .events
            .iter()
            .filter(|e| e.event_type == "stage_run")
            .filter_map(|e| e.str_field("stage").zip(e.str_field("result")))
            .collect();
        assert_eq!(stages, [("lock-wait", "pass"), ("join", "pass"), ("commit", "pass")]);

        drop(super::Stage::new(root, "etapas", "final", "machine", true));
        let log = store::read(&path).unwrap().unwrap();
        let machine = log.events.iter().find(|e| e.event_type == "stage_run" && e.str_field("stage") == Some("machine"));
        assert_eq!(machine.and_then(|e| e.str_field("result")), Some("fail"), "the close's stage stays in the spec");
    }
}
