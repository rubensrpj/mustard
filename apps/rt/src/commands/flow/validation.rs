//! Execution measurements use the spec's existing event log. Presentation
//! reads these records; it never reruns validation to obtain a duration.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Refusal, SpecLog};
use mustard_core::domain::spec_state::{PhaseWriter, State};
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

impl Drop for Stage<'_> {
    fn drop(&mut self) {
        let draft = json!({"phase":self.phase,"stage":self.name,"result":self.result,
            "ms":self.started.elapsed().as_millis() as u64,"scope":if self.inclusive {"inclusive"}else{"leaf"},
            "version":mustard_core::harness_version(),
            "attempt":std::process::id(),"author":"binary"});
        if let Err(error) = record(self.root, self.spec, "stage_run", draft.as_object().cloned().unwrap_or_default(), PhaseWriter::Binary) {
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
    for event in log
        .block(mustard_core::domain::spec_events::BlockQuery::Block(mustard_core::domain::spec_events::Block::Criteria))
        .into_iter()
        .filter(|e| e.event_type == "criterion")
    {
        digest.update(&serde_json::to_vec(&event.fields).ok()?);
    }
    let mut env: Vec<_> = std::env::vars_os()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            !matches!(name.as_ref(), "_" | "SHLVL" | "PWD" | "OLDPWD") && !name.starts_with("CLAUDE_") && !name.contains("SESSION")
        })
        .collect();
    env.sort();
    for (name, value) in env {
        digest.update(name.to_str()?.as_bytes());
        digest.update(&[0]);
        digest.update(value.to_str()?.as_bytes());
        digest.update(&[0]);
    }
    Some(digest.hex_digest())
}

/// Os arquivos que o git lista no projeto: os rastreados e os novos que ele
/// não ignora. Sem git que responda, nenhuma lista.
fn listed_files(root: &Path) -> Option<Vec<String>> {
    let files = git::run(root, &["ls-files", "--cached", "--others", "--exclude-standard", "-z"]).out()?;
    Some(files.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect())
}

/// A obra contra a qual se lê a saída de um comando que caiu: os arquivos que
/// ela mudou e, de cada onda, os que a onda declarou ou entregou. Um arquivo é
/// da obra quando o git diz que a branch o mudou, e não porque uma onda o
/// listou: a lista da onda pode vir incompleta.
#[derive(Debug)]
pub(super) struct Work {
    /// Os arquivos que a obra mudou.
    changed: BTreeSet<String>,
    /// Cada onda, na ordem pedida, com os arquivos que declarou ou entregou.
    waves: Vec<(u64, Vec<String>)>,
}

impl Work {
    /// A obra como o git e a spec a contam: os arquivos que a branch mudou
    /// desde a base gravada no `state` da spec (`git diff --name-only
    /// <base>...HEAD`, com a base local e, sem ela, a do `origin`) e, de cada
    /// onda de `waves`, os que as tarefas dela declaram mais os que as
    /// entregas dela listam. Sem base, ou sem git que responda, nenhum arquivo
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
        Self { changed: branch_files(root, log), waves }
    }

    /// Junta o que a onda `wave` mudou e a branch ainda não leva — a volta que
    /// a rodada juntou no disco, antes do commit —: os arquivos `files` contam
    /// como mudados pela obra e como da onda.
    pub(super) fn pending(&mut self, wave: u64, files: &[String]) {
        let at = match self.waves.iter().position(|(n, _)| *n == wave) {
            Some(at) => at,
            None => {
                self.waves.push((wave, Vec::new()));
                self.waves.len() - 1
            }
        };
        for file in files {
            push_path(&mut self.waves[at].1, file);
            if let Some(path) = project_path(file) {
                self.changed.insert(path);
            }
        }
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
    let Some(base) = State::from_log(log).base else {
        return BTreeSet::new();
    };
    let resolves = git::run(root, &["rev-parse", "--verify", "-q", &format!("{base}^{{commit}}")]).ok;
    let base = if resolves { base } else { format!("origin/{base}") };
    let range = format!("{base}...HEAD");
    let listed = git::run(root, &["diff", "--name-only", "-z", "--no-renames", "--relative", &range]).out().unwrap_or_default();
    listed.split('\0').filter_map(project_path).collect()
}

/// O que a saída de um comando que caiu cita, lida contra a obra.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Cited {
    /// As ondas que declararam ou entregaram um arquivo que a obra mudou e que
    /// a saída cita, na ordem da obra.
    pub waves: Vec<u64>,
    /// Os arquivos do projeto que a saída cita e que a obra não mudou, na
    /// ordem em que aparecem na saída.
    pub outside: Vec<String>,
    /// A saída não cita arquivo nenhum do projeto, nem da obra nem de fora.
    pub none: bool,
}

/// A leitura única da saída `output` de um comando que caiu, para a rodada e
/// para o fechamento: as ondas da obra `work` cujo arquivo mudado a saída
/// cita, os arquivos que o git lista no projeto e que a saída cita sem a obra
/// os ter mudado, e se ela não cita arquivo nenhum.
pub(super) fn cited(root: &Path, output: &str, work: &Work) -> Cited {
    cited_among(output, work, &listed_files(root).unwrap_or_default())
}

/// [`cited`] com a lista de arquivos do projeto já lida em `listed`.
fn cited_among(output: &str, work: &Work, listed: &[String]) -> Cited {
    let changed: HashSet<&str> = work.changed.iter().map(String::as_str).collect();
    let known: HashSet<&str> = changed.iter().copied().chain(listed.iter().map(String::as_str)).collect();
    let found = paths_in(output, &known);
    let waves = work
        .waves
        .iter()
        .filter(|(_, files)| files.iter().any(|file| changed.contains(file.as_str()) && found.contains(&file.as_str())))
        .map(|(wave, _)| *wave)
        .collect();
    let outside = found.iter().filter(|file| !changed.contains(*file)).map(|file| (*file).to_string()).collect();
    Cited { waves, outside, none: found.is_empty() }
}

/// Os arquivos de `known` que a saída `output` cita pelo caminho, na ordem em
/// que aparecem, sem repetir. Cada trecho da saída feito de caracteres de
/// caminho (letras, números e `_-./\@+~`) cita o arquivo mais longo de
/// `known` que é o trecho inteiro ou o fim dele depois de uma `/`: o caminho
/// absoluto, o `./` na frente e o caminho dentro da cópia de uma onda citam o
/// arquivo relativo à raiz, e `lib/src/b.rs` não cita `src/b.rs`. O ponto
/// colado ao fim do caminho, o da frase, fica de fora. O caminho relativo que
/// uma ferramenta escreveu dentro de um pacote ([`tool_lines`]) se lê a partir
/// da pasta do pacote, e só dali: o `src/a.ts` de `packages/web` não cita o
/// `src/a.ts` da raiz nem o de outro pacote.
fn paths_in<'a>(output: &str, known: &HashSet<&'a str>) -> Vec<&'a str> {
    let mut found: Vec<&'a str> = Vec::new();
    for (dir, text) in tool_lines(output) {
        for word in text.split(|c: char| !(c.is_alphanumeric() || "_-./\\@+~".contains(c))) {
            let word = word.replace('\\', "/");
            let word = word.trim_end_matches('.');
            let file = if dir.is_empty() || word.starts_with('/') {
                longest_known_tail(word, known)
            } else {
                from_dir(dir, word).and_then(|path| known.get(path.as_str()).copied())
            };
            if let Some(file) = file.filter(|file| !found.contains(file)) {
                found.push(file);
            }
        }
    }
    found
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

/// Cada linha da saída `output`, com a pasta, relativa à raiz, de onde rodou a
/// ferramenta que a escreveu (vazia para a raiz) e o texto que ela escreveu.
/// O `pnpm -r` anuncia o script de cada pacote com a pasta dele e o nome do
/// script (`packages/web lint$ tsc --noEmit`) e põe a mesma frente em cada
/// linha que o script escreve (`packages/web lint: src/a.ts(1,10): …`); o
/// script roda dentro do pacote, e o caminho relativo da linha é de lá. Só a
/// frente anunciada conta, e a linha sem ela é da raiz. A pasta longa que o
/// pnpm encurta (`...nome-do-pacote`) não leva a arquivo nenhum.
pub(super) fn tool_lines(output: &str) -> Vec<(&str, &str)> {
    let announced: Vec<(&str, &str)> = output
        .lines()
        .filter_map(|line| line.split_once("$ "))
        .filter_map(|(head, _)| head.rsplit_once(' ').filter(|(dir, stage)| !dir.trim().is_empty() && !stage.is_empty()).map(|(dir, _)| (head, dir)))
        .collect();
    output
        .lines()
        .map(|line| {
            announced
                .iter()
                .find_map(|(head, dir)| line.strip_prefix(head).and_then(|rest| rest.strip_prefix(": ")).map(|text| (*dir, text)))
                .unwrap_or(("", line))
        })
        .collect()
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

pub(super) fn record_run(root: &Path, spec: &str, phase: &str, stage: &str, command: &str, out: &ProofRun) -> Result<(), Refusal> {
    record_run_fields(root, spec, phase, stage, command, out, Map::new())
}

pub(super) fn record_criterion(root: &Path, spec: &str, criterion: u64, waves: &[u64], command: &str, out: &ProofRun) -> Result<(), Refusal> {
    let fields = serde_json::Map::from_iter([("criterion".into(), json!(criterion)), ("waves".into(), json!(waves))]);
    record_run_fields(root, spec, "delivery", "criterion-proof", command, out, fields)
}

#[allow(clippy::too_many_arguments)]
fn record_run_fields(root: &Path, spec: &str, phase: &str, stage: &str, command: &str, out: &ProofRun, mut draft: Map<String, Value>) -> Result<(), Refusal> {
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
    record(root, spec, "stage_run", draft, PhaseWriter::Binary).map(|_| ())
}

/// Review elapsed wall time includes the host's dispatch and waiting time;
/// it is observational and must not be added to machine stage durations.
pub(super) fn review_elapsed(root: &Path, spec: &str, log: &mustard_core::domain::spec_events::SpecLog) -> Result<(), Refusal> {
    let verdict = log.visible().into_iter().rev().find(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&Value::Bool(true)));
    let Some(verdict) = verdict else {
        return Ok(());
    };
    let key = format!("review-verdict-{}", verdict.id);
    if log.events.iter().any(|e| e.event_type == "stage_run" && e.str_field("fingerprint") == Some(&key)) {
        return Ok(());
    }
    let Some(sent) = log.visible().into_iter().rev().find(|e| e.event_type == "send" && e.str_field("role") == Some("review") && e.id < verdict.id) else {
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};

    use super::{Cited, PNPM_RECURSIVE_LINT, Work, cited_among, paths_in};

    /// Com o mesmo `src/a.ts` na raiz, fora da obra, e nos dois pacotes, cada
    /// um mudado por uma onda, a saída do tsc rodado dentro de `packages/web`
    /// cita só o arquivo dele: só a onda desse pacote reabre, e nada de fora.
    #[test]
    fn a_path_written_inside_a_workspace_package_cites_the_file_of_that_package() {
        let (web, api) = ("packages/web/src/a.ts".to_string(), "packages/api/src/a.ts".to_string());
        let work = Work { changed: BTreeSet::from([web.clone(), api.clone()]), waves: vec![(1, vec![api.clone()]), (2, vec![web.clone()])] };
        let listed = ["src/a.ts".to_string(), web, api];
        let output = PNPM_RECURSIVE_LINT.replace("{root}", "/repo");
        assert_eq!(cited_among(&output, &work, &listed), Cited { waves: vec![2], outside: vec![], none: false });
    }

    /// Cada caminho da saída cita o arquivo do projeto que ele nomeia também
    /// com `./` na frente, com a `\` do Windows, com a posição entre
    /// parênteses ou com o ponto da frase colado ao fim, e o arquivo citado
    /// duas vezes entra uma vez só.
    #[test]
    fn a_failure_output_cites_a_project_file_in_every_spelling_of_its_path() {
        let known: HashSet<&str> = ["src/b.rs", "web/x.ts"].into_iter().collect();
        let output = "./src/b.rs(2,1): erro\nem C:\\repo\\web\\x.ts.\nde novo src/b.rs:2";
        assert_eq!(paths_in(output, &known), vec!["src/b.rs", "web/x.ts"]);
    }
}
