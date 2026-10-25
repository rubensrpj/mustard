//! As cópias de obra que sobraram no disco, na limpeza (`mustard-rt run
//! clean`).
//!
//! Cada obra prepara cópias do projeto para as ondas e a revisão, com a
//! compilação dentro delas. O fechamento e o descarte as apagam; a limpeza
//! recolhe o que ficou de obras que já acabaram: fechadas, com o pull request
//! aberto, entregues, descartadas ou que não existem mais. A cópia de obra
//! aberta fica, com o motivo no relatório. A regra de idade da limpeza não
//! vale aqui: a obra que acabou não volta a usar a cópia.
//!
//! Os lugares olhados são dois, e nada fora deles sai:
//!
//! - a pasta das cópias do projeto ([`copies_dir`]), com a pasta nova de cada
//!   obra (`<spec>/<vaga>`) e as antigas (`<spec>-<n>` e
//!   `<spec>-final-review`);
//! - o lugar antigo dentro do projeto, `.claude/worktrees/mustard-<spec>-<n>`.
//!   Pasta dali sem o prefixo não é do Mustard e nem entra na lista.
//!
//! A pasta das cópias de página dos descartes ([`DISCARD_COPIES`](crate::commands::flow::discard::DISCARD_COPIES)) mora na
//! pasta das cópias, mas não é cópia de obra e não entra na lista delas. Dela
//! sai só a cópia de um descarte que passou do prazo, pela mesma varredura do
//! descarte ([`sweep_old_discard_copies`]), numa linha própria do relatório;
//! a de um descarte recém-feito fica. Do mesmo jeito, a pasta das cópias que
//! já saíram da obra (`.leaving`) não entra na lista: o que sobrou
//! nela, porque apagar depois de soltar a trava não terminou, sai numa linha
//! própria.
//!
//! Pasta de nome oculto na pasta das cópias — a `.claude` que um programa
//! rodando ali deixa, por exemplo — não é cópia de obra: nome de spec nunca
//! começa por ponto, e ela nem entra na lista.
//!
//! As branches das obras que acabaram ou não existem mais também saem, e as
//! que o aviso do início da sessão conta como mergeadas, pela mesma leitura
//! do git local dele: só a branch que o projeto registrou como obra, nunca
//! uma base de integração ou protegida, a branch atual, nem a que tem commit
//! que a base não tem ([`sweep_branches`]).
//!
//! A pasta principal e a compilação dela nunca entram. Link não é seguido.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::io::spec_index::DISCARDED_DIR;
use mustard_core::io::wave_prompt::{copies_dir, shown};
use serde::Serialize;

use mustard_core::platform::i18n::{Locale, translate};

use super::scratch_gc::{ErrorRecord, human_bytes, tree_bytes};
use crate::commands::event::work_branch::branch_exists;
use crate::commands::flow::discard::sweep_old_discard_copies;
use crate::commands::git_delete::{NotAUnit, delete_local_branch, not_a_unit};
use crate::shared::branch_state::local_states;
use crate::shared::work_kind::BaseFlow;
use crate::commands::flow::round::{Removal, landed_kept, leaving_place, remove_single_copy, remove_spec_copies};

/// O prefixo das cópias antigas dentro do projeto.
const OLD_PREFIX: &str = "mustard-";

/// O sufixo da cópia antiga do revisor final.
const FINAL_REVIEW_SUFFIX: &str = "-final-review";

/// As fases de obra que acabou: a cópia dela sai.
const FINISHED: [&str; 4] = ["closed", "pr_open", "delivered", "discarded"];

/// A fase em que as ondas rodam, e a única em que a cópia de obra existe para
/// ser trabalhada.
const RUNNING: &str = "running";

/// O motivo, no relatório, da cópia de uma obra que ficou em execução depois
/// de a branch dela sair do repositório. A obra em execução segura a cópia
/// dela, e sem a branch ninguém retoma essa obra: a cópia era peso parado,
/// e a limpeza passava ao lado dela por anos.
const RUNNING_WITHOUT_BRANCH: &str = "spec running without its branch";

/// O motivo, no relatório, da cópia de página de descarte que sai.
const OLD_DISCARD_COPY: &str = "discard page copy older than a day";

/// O motivo, no relatório, da cópia que já tinha saído da obra, com o código
/// guardado e o git sem ela, e ficou no disco porque apagá-la, depois de
/// soltar a trava, não terminou.
const LEFT_ON_DISK: &str = "copy already out of its work, left on disk";

/// O motivo, no relatório, do retrato do mapa da base de uma obra que acabou
/// ou que não existe mais. São dezenas de megabytes por spec, e ninguém os
/// apagava: numa máquina de verdade um deles chegou a 89 MB.
const OLD_MAP: &str = "validation base map of a finished spec";

/// Uma cópia de obra achada, com a obra dela e o motivo de sair ou ficar.
#[derive(Debug, Serialize)]
pub(crate) struct CopyRecord {
    pub path: String,
    pub spec: String,
    /// `"spec <fase>"` ou `"spec missing"` para a que sai; `"spec still
    /// open: <fase>"` ou o que impediu de ler a obra, para a que fica. A
    /// cópia de página de descarte que passou do prazo, sem obra, sai com
    /// [`OLD_DISCARD_COPY`].
    pub reason: String,
    /// A pasta a tirar, fora do JSON.
    #[serde(skip)]
    dir: PathBuf,
    /// A pasta de todas as vagas da obra, e não uma cópia só.
    #[serde(skip)]
    whole: bool,
    /// A obra acabou ou não existe mais: a cópia sai.
    #[serde(skip)]
    leaves: bool,
}

/// O código que a limpeza guardou antes de apagar uma cópia: de onde saiu, a
/// ref que o guarda no repositório principal, os arquivos e o comando que o
/// traz de volta.
#[derive(Debug, Serialize)]
pub(crate) struct KeptRecord {
    pub copy: String,
    #[serde(rename = "ref")]
    pub refname: String,
    pub files: Vec<String>,
    pub restore: String,
}

/// A parte das cópias de obra no relatório da limpeza.
#[derive(Debug, Serialize)]
pub(crate) struct CopiesReport {
    /// O projeto cujas cópias foram olhadas.
    pub root: String,
    pub candidates: Vec<CopyRecord>,
    pub kept: Vec<CopyRecord>,
    pub removed: Vec<String>,
    pub errors: Vec<ErrorRecord>,
    /// O código que as cópias tinham além do commit, guardado antes de saírem.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub code_kept: Vec<KeptRecord>,
    /// As refs guardadas de spec entregue cujo código a base já tem: saem com
    /// `apply`, e sem ele só são listadas.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept_landed: Vec<String>,
    /// As branches de obra que acabou, não existe mais ou já entrou na base
    /// por merge, que saem com `apply`; sem ele, só são listadas.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<BranchRecord>,
    /// As que sairiam e ficam, com o motivo: sem o registro de obra no
    /// projeto, base de integração ou protegida, a branch atual, ou com commit
    /// fora da base.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches_kept: Vec<BranchRecord>,
    /// As branches que saíram, com `apply`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches_removed: Vec<String>,
}

/// Uma branch de obra que acabou ou não existe mais, com o motivo de sair ou
/// ficar.
#[derive(Debug, Serialize)]
pub(crate) struct BranchRecord {
    pub branch: String,
    pub spec: String,
    pub reason: String,
}

/// As cópias de obra do projeto em que `start` está: lista as que saem e as
/// que ficam e, com `apply`, tira as que saem. As cópias de página de
/// descarte que passaram do prazo entram entre as que saem, cada uma numa
/// linha. `None` fora de projeto git.
pub(crate) fn clean(start: &Path, apply: bool) -> Option<CopiesReport> {
    let root = crate::commands::spec_events::project(start).root;
    if !root.join(".git").exists() {
        return None;
    }
    let (candidates, kept): (Vec<CopyRecord>, Vec<CopyRecord>) = found(&root).into_iter().partition(|record| record.leaves);
    let mut report = CopiesReport {
        root: shown(&root),
        candidates,
        kept,
        removed: Vec::new(),
        errors: Vec::new(),
        code_kept: Vec::new(),
        kept_landed: Vec::new(),
        branches: Vec::new(),
        branches_kept: Vec::new(),
        branches_removed: Vec::new(),
    };
    if apply {
        remove(&root, &mut report);
    }
    sweep_branches(&root, apply, &mut report);
    report.kept_landed = kept_specs(&root).iter().flat_map(|spec| landed_kept(&root, spec, apply)).collect();
    let discard = sweep_old_discard_copies(&root, apply).into_iter().map(|swept| (swept, OLD_DISCARD_COPY));
    let left_on_disk = sweep_left_on_disk(&root, apply).into_iter().map(|swept| (swept, LEFT_ON_DISK));
    let maps = sweep_old_maps(&root, apply).into_iter().map(|swept| (swept, OLD_MAP));
    for ((dir, error), reason) in discard.chain(left_on_disk).chain(maps) {
        let path = shown(&dir);
        if apply {
            match error {
                None => report.removed.push(path.clone()),
                Some(error) => report.errors.push(ErrorRecord { path: path.clone(), error }),
            }
        }
        let reason = reason.to_string();
        report.candidates.push(CopyRecord { path, spec: String::new(), reason, dir, whole: false, leaves: true });
    }
    report.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    report.removed.sort();
    Some(report)
}

/// A branch que a obra perdeu: o nome dela quando a obra está em execução e a
/// branch já não existe no repositório, e `None` em qualquer outro caso.
///
/// É uma regra com dois leitores, e por isso vive num lugar só: a limpeza tira
/// do caminho a cópia de uma obra assim ([`fate`]), e a retomada diz ao
/// usuário que a obra não tem mais como ser retomada. Antes de `running` a
/// obra não tem cópia nem trabalho na branch, e uma branch ainda não cortada
/// não é uma branch perdida.
pub(crate) fn lost_branch(root: &Path, state: &State) -> Option<String> {
    let branch = state.branch.clone().filter(|_| state.phase == Some(RUNNING))?;
    (!branch_exists(root, &branch)).then_some(branch)
}

/// Uma cópia de obra que já não serve a ninguém, com o tamanho que ela ocupa
/// no disco: a obra dela acabou, não existe mais, ou ficou em execução sem a
/// branch ([`fate`] diz qual é o caso em `reason`).
#[derive(Debug, Serialize)]
pub(crate) struct Orphan {
    pub path: String,
    pub spec: String,
    pub reason: String,
    pub bytes: u64,
}

/// As cópias de obra que a limpeza tiraria do projeto em que `start` está, em
/// ordem de caminho, cada uma medida. Nada sai do disco aqui: quem apaga é
/// `mustard-rt run clean --apply`, e esta é a lista que a retomada e o
/// fechamento mostram para o usuário saber que ela tem o que tirar.
///
/// Só as sobras são medidas. Num projeto arrumado não há nenhuma, e a função
/// não toca em disco além de ler as fases das obras; a passada por uma árvore
/// de gigabytes só acontece quando existe o peso parado que a medida vai
/// nomear.
pub(crate) fn orphans(start: &Path) -> Vec<Orphan> {
    let root = crate::commands::spec_events::project(start).root;
    if !root.join(".git").exists() {
        return Vec::new();
    }
    found(&root)
        .into_iter()
        .filter(|record| record.leaves)
        .map(|record| Orphan { bytes: tree_bytes(&record.dir), path: record.path, spec: record.spec, reason: record.reason })
        .collect()
}

/// O aviso das sobras, com o total e cada cópia pelo caminho e pelo tamanho,
/// ou `None` quando não há nenhuma. É o mesmo texto na retomada e no
/// fechamento: os dois mostram a mesma lista, e uma segunda frase seria uma
/// segunda verdade.
pub(crate) fn orphans_hint(orphans: &[Orphan], lang: Locale) -> Option<String> {
    if orphans.is_empty() {
        return None;
    }
    let total = orphans.iter().fold(0u64, |sum, orphan| sum.saturating_add(orphan.bytes));
    let list: Vec<String> = orphans.iter().map(|orphan| format!("`{}` ({})", orphan.path, human_bytes(orphan.bytes))).collect();
    Some(translate("copies.left", lang).replace("{total}", &human_bytes(total)).replace("{list}", &list.join(", ")))
}

/// Tira as candidatas: a pasta nova de cada obra por [`remove_spec_copies`],
/// que prende a trava das cópias e a do passo do git; as cópias antigas por
/// [`remove_single_copy`], com as mesmas travas presas, nesta ordem. Cada cópia guarda antes o
/// código que tem além do commit: a que não pôde guardar não sai, e o motivo
/// vai entre os erros; o que ficou guardado vai nomeado em `code_kept`.
fn remove(root: &Path, report: &mut CopiesReport) {
    let (whole, single): (Vec<&CopyRecord>, Vec<&CopyRecord>) = report.candidates.iter().partition(|record| record.whole);
    let mut removed = Vec::new();
    let mut errors = Vec::new();
    let mut code_kept = Vec::new();
    let mut absorb = |record: &CopyRecord, outcome: Removal, only_path: bool| {
        if outcome.left.is_empty() && outcome.unkept.is_empty() {
            removed.push(record.path.clone());
        }
        errors.extend(
            outcome.left.into_iter().chain(outcome.unkept).map(|(path, error)| ErrorRecord { path: if only_path { record.path.clone() } else { path }, error }),
        );
        code_kept.extend(outcome.kept.iter().map(|kept| KeptRecord {
            copy: kept.copy.clone(),
            refname: kept.refname.clone(),
            files: kept.files.clone(),
            restore: format!("git cherry-pick --no-commit {}", kept.refname),
        }));
    };
    for record in whole {
        absorb(record, remove_spec_copies(root, &record.spec, None), false);
    }
    let mut lock_error = None;
    if !single.is_empty() {
        match crate::commands::git_settle::copies_lock(root).and_then(|copies| Ok((copies, crate::commands::git_settle::git_step_lock(root)?))) {
            Ok(_held) => {
                for record in single {
                    absorb(record, remove_single_copy(root, &record.dir, &record.spec), true);
                }
            }
            Err(error) => lock_error = Some(ErrorRecord { path: shown(root), error }),
        }
    }
    errors.extend(lock_error);
    removed.sort();
    report.removed = removed;
    report.errors = errors;
    report.code_kept = code_kept;
}

/// As branches locais de obra que saem, pela leitura do git local que o
/// aviso do início da sessão usa ([`local_states`]): a da obra que acabou —
/// fase fechada, entregue, com o pull request aberto ou descartada — ou que
/// não está mais no disco, e a que o aviso conta como mergeada. Sai só a que o
/// projeto registrou como obra e não é base de integração nem protegida
/// ([`not_a_unit`], a mesma conferência do descarte), que não é a branch atual
/// do checkout principal e cujas refs a base declarada já tem, todas. A que
/// sairia e fica vai com o motivo; a sem registro e com commit fora da base
/// não é do projeto, e nem entra. Com `apply`, cada uma sai com a trava do
/// passo do git presa ([`delete_local_branch`]), e a recusa do git, como a da
/// branch em uso por outra cópia, fica entre os erros.
fn sweep_branches(root: &Path, apply: bool, report: &mut CopiesReport) {
    let config = mustard_core::ProjectConfig::load(root);
    let flow = BaseFlow::of_at(&config.git, root);
    let current = mustard_core::platform::git_branches::current_branch(root);
    let mut leaving = Vec::new();
    for state in local_states(root, &flow).into_iter().filter(|state| state.local) {
        let Some(spec) = flow.slug_of(&state.branch) else { continue };
        let why = match ended(root, &spec) {
            Some(reason) => reason,
            None if state.state.is_awaiting_prune() => "merged into the base".to_string(),
            None => continue,
        };
        let kept = match not_a_unit(root, &state.branch, &flow, &config) {
            Some(NotAUnit::NoRecord) if !state.ancestry => continue,
            Some(not) => Some(not.reason()),
            None if current.as_deref() == Some(state.branch.as_str()) => Some("current branch"),
            None if flow.bases().is_empty() => Some("no declared base"),
            None if !state.ancestry => Some("commits outside the base"),
            None => None,
        };
        let record = |reason: String| BranchRecord { branch: state.branch.clone(), spec: spec.clone(), reason };
        match kept {
            Some(kept) => report.branches_kept.push(record(format!("{why}; {kept}"))),
            None => leaving.push(record(why)),
        }
    }
    if apply && !leaving.is_empty() {
        match crate::commands::git_settle::git_step_lock(root) {
            Ok(_held) => {
                for record in &leaving {
                    match delete_local_branch(root, &record.branch) {
                        Ok(()) => report.branches_removed.push(record.branch.clone()),
                        Err(error) => report.errors.push(ErrorRecord { path: format!("refs/heads/{}", record.branch), error }),
                    }
                }
            }
            Err(error) => report.errors.push(ErrorRecord { path: shown(root), error }),
        }
    }
    report.branches = leaving;
}

/// As specs que têm ref guardada (`refs/mustard/kept/<spec>/…`), sem repetir.
fn kept_specs(root: &Path) -> std::collections::BTreeSet<String> {
    let out = std::process::Command::new("git").args(["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]).current_dir(root).output();
    let listed = out.map(|out| String::from_utf8_lossy(&out.stdout).into_owned()).unwrap_or_default();
    listed.lines().filter_map(|refname| refname.strip_prefix("refs/mustard/kept/")?.split_once('/').map(|(spec, _)| spec.to_string())).collect()
}

/// Cada pasta que ficou no lugar das cópias que saem ([`leaving_place`]):
/// cópia que já saiu da obra, com o código guardado e o git sem ela, e que o
/// apagar depois da trava não terminou. Com `apply`, cada uma sai, e o texto
/// ao lado dela diz por que não saiu; sem `apply`, nada sai. Link não é
/// seguido.
fn sweep_left_on_disk(root: &Path, apply: bool) -> Vec<(PathBuf, Option<String>)> {
    let mut left = folders(&leaving_place(root));
    left.sort();
    left.into_iter()
        .map(|dir| {
            let error = apply.then(|| mustard_core::io::fs::remove_dir_all(&dir).err().map(|e| e.to_string())).flatten();
            (dir, error)
        })
        .collect()
}

/// Os retratos do mapa da base que já não servem a ninguém: o da obra que
/// acabou e o da que não existe mais ([`fate`]), nos dois lugares — a pasta de
/// fora do projeto, onde eles moram agora, e a pasta da spec, onde moravam. Com
/// `apply`, cada um sai, e o texto ao lado dele diz por que não saiu; sem
/// `apply`, nada sai.
///
/// O da obra aberta fica: a análise de declarações sem uso ainda vai compará-lo
/// com o mapa de agora.
fn sweep_old_maps(root: &Path, apply: bool) -> Vec<(PathBuf, Option<String>)> {
    use mustard_core::io::wave_prompt::{OLD_VALIDATION_MAP, validation_base_map};

    let outside = validation_base_map(root, "x");
    let mut found: Vec<PathBuf> = std::fs::read_dir(outside.parent().unwrap_or(root))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|kind| kind == "json") && path.is_file())
                .filter(|path| path.file_stem().is_some_and(|spec| fate(root, &spec.to_string_lossy()).0))
                .collect()
        })
        .unwrap_or_default();
    let specs = store::spec_file(root, "x").ok().and_then(|path| path.parent().and_then(Path::parent).map(Path::to_path_buf));
    found.extend(
        folders(&specs.unwrap_or_else(|| root.to_path_buf()))
            .into_iter()
            .filter(|dir| dir.file_name().is_some_and(|spec| fate(root, &spec.to_string_lossy()).0))
            .map(|dir| dir.join(OLD_VALIDATION_MAP))
            .filter(|path| path.is_file()),
    );
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let error = apply.then(|| std::fs::remove_file(&path).err().map(|e| e.to_string())).flatten();
            (path, error)
        })
        .collect()
}

/// Toda cópia de obra dos dois lugares, em ordem de caminho, cada uma com o
/// motivo de sair ou ficar. Pasta de nome oculto não é cópia de obra e não
/// entra: a das cópias de página dos descartes ([`DISCARD_COPIES`](crate::commands::flow::discard::DISCARD_COPIES)), a das
/// cópias que já saíram da obra (`.leaving`) e qualquer outra.
fn found(root: &Path) -> Vec<CopyRecord> {
    let mut records = Vec::new();
    for dir in folders(&copies_dir(root)) {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        let whole = !dir.join(".git").exists();
        let spec = if whole { Some(name) } else { old_spec(&name) };
        records.push(record(root, dir, spec, whole));
    }
    for dir in folders(&root.join(".claude").join("worktrees")) {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let Some(rest) = name.strip_prefix(OLD_PREFIX) else {
            continue;
        };
        if dir.join(".git").is_file() {
            let spec = old_spec(rest);
            records.push(record(root, dir, spec, false));
        }
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    records
}

/// A cópia `dir`, da obra `spec` quando o nome a diz, com o motivo.
fn record(root: &Path, dir: PathBuf, spec: Option<String>, whole: bool) -> CopyRecord {
    let (leaves, reason) = match &spec {
        Some(spec) => fate(root, spec),
        None => (false, "not a copy of a work".to_string()),
    };
    CopyRecord { path: shown(&dir), spec: spec.unwrap_or_default(), reason, dir, whole, leaves }
}

/// As pastas de dentro de `place`, sem seguir link.
fn folders(place: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(place)
        .map(|entries| entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

/// A obra do nome antigo de cópia: `<spec>-final-review` ou `<spec>-<n>`.
fn old_spec(name: &str) -> Option<String> {
    if let Some(spec) = name.strip_suffix(FINAL_REVIEW_SUFFIX) {
        return Some(spec.to_string()).filter(|spec| !spec.is_empty());
    }
    let (spec, wave) = name.rsplit_once('-')?;
    (!spec.is_empty() && !wave.is_empty() && wave.bytes().all(|b| b.is_ascii_digit())).then(|| spec.to_string())
}

/// Se a cópia da obra `spec` sai, e por quê: sai a da obra que acabou, a da
/// que não existe mais — nem entre as specs, nem entre as descartadas — e a da
/// que ficou em execução sem a branch dela ([`RUNNING_WITHOUT_BRANCH`]); fica
/// a da obra aberta e a da obra que não se pôde ler.
fn fate(root: &Path, spec: &str) -> (bool, String) {
    match work(root, spec) {
        Work::Ended(reason) => (true, reason),
        Work::Open(state) => match lost_branch(root, &state) {
            Some(branch) => (true, format!("{RUNNING_WITHOUT_BRANCH}: {branch}")),
            None => (false, format!("spec still open: {}", state.phase.unwrap_or("unknown"))),
        },
        Work::Unread(reason) => (false, reason.to_string()),
    }
}

/// O motivo da obra `spec` que acabou ou não existe mais; `None` da obra
/// aberta e da que não se lê.
fn ended(root: &Path, spec: &str) -> Option<String> {
    match work(root, spec) {
        Work::Ended(reason) => Some(reason),
        Work::Open(_) | Work::Unread(_) => None,
    }
}

/// O que se lê da obra de uma cópia ou de uma branch.
enum Work {
    /// Acabou — `"spec <fase>"` — ou não existe mais — `"spec discarded"`,
    /// `"spec missing"`.
    Ended(String),
    /// Segue aberta, com o estado dela.
    Open(State),
    /// O nome não serve para spec ou o arquivo não se lê.
    Unread(&'static str),
}

/// A obra `spec`: acabada ou inexistente ([`FINISHED`], a descartada e a que
/// não está mais no disco), aberta, ou ilegível.
fn work(root: &Path, spec: &str) -> Work {
    let Ok(file) = store::spec_file(root, spec) else {
        return Work::Unread("not a spec name");
    };
    match store::read(&file) {
        Ok(Some(log)) => {
            let state = State::from_log(&log);
            match state.phase.filter(|phase| FINISHED.contains(phase)) {
                Some(phase) => Work::Ended(format!("spec {phase}")),
                None => Work::Open(state),
            }
        }
        Ok(None) => {
            let specs = file.parent().and_then(Path::parent).unwrap_or(root);
            let reason = if specs.join(DISCARDED_DIR).join(spec).is_dir() { "spec discarded" } else { "spec missing" };
            Work::Ended(reason.to_string())
        }
        Err(_) => Work::Unread("spec unreadable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::flow::discard::DISCARD_COPIES;
    use crate::commands::spec_events::write::record_open;
    use mustard_core::io::wave_prompt::slot_path;
    use serde_json::json;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Um projeto git com o que a pasta principal compilou em `target`, que
    /// o git ignora.
    fn project(root: &Path) {
        git(root, &["init", "-q", "."]);
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n.claude/\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "semente"]);
        std::fs::create_dir_all(root.join("target").join("debug")).unwrap();
        std::fs::write(root.join("target").join("debug").join("mustard"), "compilado").unwrap();
        crate::commands::flow::round::copies_leave_with_the_test(root);
    }

    /// A obra `spec` levada, passo a passo do fluxo, até a fase `phase`, com a
    /// branch dela cortada no repositório, como o fluxo a corta ao abrir: a
    /// obra em execução sem a branch é outro caso, e tem teste próprio.
    fn work(root: &Path, spec: &str, phase: &str) {
        const STEPS: &[&str] = &["survey", "plan", "approved", "running", "closed"];
        let path = store::spec_file(root, spec).expect("spec file");
        std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
        git(root, &["branch", &format!("feature/{spec}")]);
        for step in STEPS {
            let mut fields = json!({"phase": step, "author": "binary"});
            if *step == "survey" {
                fields["branch"] = json!(format!("feature/{spec}"));
                fields["base"] = json!("dev");
            }
            if *step == "approved" {
                fields["witness"] = json!({"question": "Aprovar?", "answer": "Aprovar"});
            }
            store::write(&path, "state", fields.as_object().cloned().expect("an object"), &[]).expect("state");
            if *step == phase {
                return;
            }
        }
    }

    /// Uma cópia registrada no git em `dir`, com o que ela compilou.
    fn copy(root: &Path, dir: &Path) {
        git(root, &["worktree", "add", "-q", "--detach", &dir.to_string_lossy()]);
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("target").join("compilado"), "x").unwrap();
    }

    fn registered(root: &Path) -> String {
        let out = std::process::Command::new("git").args(["worktree", "list", "--porcelain"]).current_dir(root).output().expect("git");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn paths(records: &[CopyRecord]) -> Vec<String> {
        records.iter().map(|record| record.path.clone()).collect()
    }

    /// A limpeza lista as cópias de obra fechada, descartada e que não existe
    /// mais, nos dois lugares e no desenho novo e no antigo, e deixa a da
    /// obra aberta, com o motivo. Sem a opção de apagar, nada sai; com ela,
    /// saem as listadas — pasta e registro do git — e a da obra aberta fica.
    /// A pasta principal e o que ela compilou ficam sempre.
    #[test]
    fn clean_removes_the_copies_of_finished_or_missing_works_and_keeps_the_open_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        work(root, "fechada", "closed");
        work(root, "aberta", "running");
        work(root, "velha", "closed");
        assert_eq!(record_open(root, "descartada", "feature/descartada", "dev"), Ok(true));
        let copies = copies_dir(root);
        let closed = [slot_path(root, "fechada", 0), slot_path(root, "fechada", 1)];
        let open = slot_path(root, "aberta", 0);
        let missing = slot_path(root, "sumiu", 0);
        let old_here = copies.join("velha-3");
        let old_inside = root.join(".claude").join("worktrees").join("mustard-velha-7");
        let old_open = copies.join("aberta-final-review");
        let theirs = root.join(".claude").join("worktrees").join("outra-coisa");
        for slot in closed.iter().chain([&open, &missing, &old_here, &old_inside, &old_open, &theirs]) {
            copy(root, slot);
        }
        let discarded = slot_path(root, "descartada", 0);
        copy(root, &discarded);
        let code = crate::commands::flow::discard::discard_for(
            &crate::commands::flow::discard::DiscardOpts {
                root: root.to_path_buf(),
                spec: Some("descartada".into()),
                remote: false,
                delete: false,
                confirm: None,
            },
            None,
        )["token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        // O descarte já tira a cópia; uma nova, deixada depois, é a sobra.
        crate::commands::flow::discard::discard_for(
            &crate::commands::flow::discard::DiscardOpts {
                root: root.to_path_buf(),
                spec: Some("descartada".into()),
                remote: false,
                delete: false,
                confirm: Some(code),
            },
            None,
        );
        assert!(!discarded.exists(), "o descarte tirou a cópia dele");
        copy(root, &discarded);

        let leaves = |spec: &str| shown(&copies.join(spec));
        let listed = clean(root, false).expect("um projeto git");
        assert_eq!(
            paths(&listed.candidates),
            vec![shown(&old_inside), leaves("descartada"), leaves("fechada"), leaves("sumiu"), shown(&old_here)]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>(),
            "{listed:?}"
        );
        assert_eq!(paths(&listed.kept), {
            let mut kept = vec![leaves("aberta"), shown(&old_open)];
            kept.sort();
            kept
        });
        assert!(listed.kept.iter().all(|record| record.reason == "spec still open: running"), "{listed:?}");
        assert!(listed.removed.is_empty(), "{listed:?}");
        assert!(closed.iter().all(|slot| slot.join("target").is_dir()), "sem a opção de apagar, nada sai");
        assert!(old_inside.join(".git").is_file() && missing.is_dir() && discarded.is_dir());

        let applied = clean(root, true).expect("um projeto git");
        assert!(applied.errors.is_empty(), "{applied:?}");
        assert_eq!(applied.removed, paths(&listed.candidates), "{applied:?}");
        for gone in closed.iter().chain([&missing, &old_here, &old_inside, &discarded]) {
            assert!(!gone.exists(), "{} saiu", gone.display());
        }
        for spec in ["fechada", "sumiu", "descartada"] {
            assert!(!copies.join(spec).exists(), "a pasta das cópias de {spec} saiu");
        }
        let still = registered(root);
        for gone in closed.iter().chain([&missing, &old_here, &old_inside, &discarded]) {
            assert!(!still.contains(&shown(gone)), "{still}");
        }
        assert!(open.join("target").join("compilado").is_file(), "a cópia da obra aberta fica");
        assert!(old_open.join(".git").is_file(), "a cópia antiga da obra aberta fica");
        assert!(theirs.join(".git").is_file(), "a pasta sem o prefixo não é do Mustard e fica");
        let built = root.join("target").join("debug").join("mustard");
        assert_eq!(std::fs::read_to_string(built).unwrap(), "compilado", "a compilação principal fica");
        assert!(root.join("mustard.json").is_file() && root.join(".git").is_dir(), "a pasta principal fica");
    }

    /// A obra que ficou em execução depois de a branch dela sair do
    /// repositório não segura mais a cópia: ela entra entre as que saem, com
    /// o motivo que nomeia a branch que faltou, e a lista de sobras a traz
    /// medida, com o aviso que a retomada e o fechamento mostram. A obra em
    /// execução com a branch no lugar segue segurando a dela.
    #[test]
    fn a_work_running_without_its_branch_stops_holding_its_copy() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        work(root, "viva", "running");
        work(root, "orfa", "running");
        let alive = slot_path(root, "viva", 0);
        let lost = slot_path(root, "orfa", 0);
        copy(root, &alive);
        copy(root, &lost);
        git(root, &["branch", "-D", "feature/orfa"]);

        let listed = clean(root, false).expect("um projeto git");
        let folder = |spec: &str| shown(&copies_dir(root).join(spec));
        assert_eq!(paths(&listed.kept), vec![folder("viva")], "a obra com a branch no lugar fica: {listed:?}");
        assert_eq!(paths(&listed.candidates), vec![folder("orfa")], "a obra sem a branch sai: {listed:?}");
        assert_eq!(listed.candidates[0].reason, "spec running without its branch: feature/orfa", "{listed:?}");

        let left = orphans(root);
        assert_eq!(left.iter().map(|one| one.path.clone()).collect::<Vec<_>>(), vec![folder("orfa")], "{left:?}");
        assert!(left[0].bytes > 0, "a sobra vai medida: {left:?}");
        let hint = orphans_hint(&left, Locale::PtBr).expect("o aviso das sobras");
        assert!(hint.contains(&folder("orfa")) && hint.contains("run clean --apply"), "{hint}");
        assert!(orphans_hint(&[], Locale::PtBr).is_none(), "sem sobra, sem aviso");
        assert!(lost.join("target").is_dir() && alive.join("target").is_dir(), "listar não apaga nada");
    }

    /// O retrato do mapa da base de uma obra que acabou sai na limpeza, nos
    /// dois lugares: a pasta de fora do projeto, onde ele mora agora, e a
    /// pasta da spec, onde ele morava. O da obra aberta fica.
    ///
    /// Em campo (21/10/2026) três desses retratos somavam cerca de 200 MB na
    /// máquina do usuário, e um deles tinha 89 MB.
    #[test]
    fn the_validation_base_map_of_a_finished_spec_leaves_in_the_clean() {
        use mustard_core::io::wave_prompt::{OLD_VALIDATION_MAP, validation_base_map};

        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        work(root, "fechada", "closed");
        work(root, "aberta", "running");
        let outside = |spec: &str| validation_base_map(root, spec);
        std::fs::create_dir_all(outside("fechada").parent().expect("a pasta dos mapas")).unwrap();
        for spec in ["fechada", "aberta"] {
            std::fs::write(outside(spec), "{}").unwrap();
            let inside = store::spec_file(root, spec).expect("a spec").parent().expect("a pasta").join(OLD_VALIDATION_MAP);
            std::fs::write(&inside, "{}").unwrap();
        }
        let inside = |spec: &str| store::spec_file(root, spec).expect("a spec").parent().expect("a pasta").join(OLD_VALIDATION_MAP);

        let listed = clean(root, false).expect("um projeto git");
        let gone: Vec<String> = listed.candidates.iter().filter(|one| one.reason == OLD_MAP).map(|one| one.path.clone()).collect();
        assert_eq!(gone, vec![shown(&inside("fechada")), shown(&outside("fechada"))].into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>(), "{listed:?}");
        assert!(outside("fechada").is_file() && inside("fechada").is_file(), "sem a opção de apagar, nada sai");

        let applied = clean(root, true).expect("um projeto git");
        assert!(applied.errors.is_empty(), "{applied:?}");
        assert!(!outside("fechada").exists() && !inside("fechada").exists(), "o mapa da obra fechada saiu");
        assert!(outside("aberta").is_file() && inside("aberta").is_file(), "o da obra aberta fica");
    }

    /// Fora de projeto git, a limpeza não olha cópia de obra nenhuma.
    #[test]
    fn outside_a_git_project_clean_leaves_the_copies_alone() {
        let dir = tempdir().unwrap();
        assert!(clean(dir.path(), true).is_none());
    }

    /// O nome antigo de cópia diz a obra; o que não segue o desenho não diz.
    #[test]
    fn the_old_copy_name_tells_the_work() {
        assert_eq!(old_spec("obra-de-exemplo-12").as_deref(), Some("obra-de-exemplo"));
        assert_eq!(old_spec("x-final-review").as_deref(), Some("x"));
        assert_eq!(old_spec("sem-numero"), None);
        assert_eq!(old_spec("-3"), None);
    }

    /// Abre a obra `spec` e a descarta apagando a pasta dela, pelo caminho de
    /// quem usa: a prévia e o sim com o código. Devolve a pasta da cópia da
    /// página que o descarte deixou na pasta das cópias do projeto.
    fn deleted(root: &Path, spec: &str) -> PathBuf {
        use crate::commands::flow::discard::{DiscardOpts, discard_for};
        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        assert!(mustard_core::io::spec_index::rebuild(root).is_ok());
        let opts = |confirm: Option<String>| DiscardOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), remote: false, delete: true, confirm };
        let code = discard_for(&opts(None), None)["token"].as_str().unwrap_or_default().to_string();
        let done = discard_for(&opts(Some(code)), None);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert!(done.get("copy").is_none(), "new discards do not generate a database");
        // Seed a historical cache explicitly: cleanup compatibility must
        // not rely on restoring the retired automatic publication path.
        let folder = copies_dir(root).join(DISCARD_COPIES).join(spec);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("old-snapshot.json"), "{}").unwrap();
        folder
    }

    /// Toda linha do relatório, das que saem e das que ficam.
    fn every_record(report: &CopiesReport) -> impl Iterator<Item = &CopyRecord> {
        report.candidates.iter().chain(&report.kept)
    }

    /// A cópia da página de um descarte recém-feito fica depois da limpeza
    /// com a opção de apagar, com os lotes dela, e nem aparece no relatório.
    #[test]
    fn a_fresh_discard_page_copy_stays_after_clean_with_apply() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        let fresh = deleted(root, "apagada");
        let batches = std::fs::read_dir(&fresh).unwrap().count();
        assert!(batches > 0, "{fresh:?}: the discard left no batch");

        let applied = clean(root, true).expect("um projeto git");
        assert!(fresh.is_dir(), "{fresh:?}: the fresh discard page copy left: {applied:?}");
        assert_eq!(std::fs::read_dir(&fresh).unwrap().count(), batches, "{applied:?}");
        assert!(applied.removed.is_empty() && applied.errors.is_empty(), "{applied:?}");
        assert!(every_record(&applied).all(|record| !record.path.contains(DISCARD_COPIES)), "{applied:?}");
    }

    /// A pasta das cópias de página dos descartes não entra no relatório
    /// como cópia de obra: nem como obra que não existe mais, nem como nome
    /// que não é de spec, nem entre as que ficam.
    #[test]
    fn the_report_does_not_list_the_discard_copies_folder_as_a_work() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        deleted(root, "apagada");
        let place = shown(&copies_dir(root).join(DISCARD_COPIES));

        let listed = clean(root, false).expect("um projeto git");
        assert!(every_record(&listed).all(|record| record.spec != DISCARD_COPIES && record.path != place), "{listed:?}");
        assert!(listed.candidates.is_empty() && listed.kept.is_empty(), "{listed:?}");
    }

    /// A cópia da página de um descarte que não muda há um dia e um minuto
    /// ganha uma linha própria no relatório, com o motivo em palavras, e sai
    /// só com a opção de apagar; a de um dia menos um minuto fica sempre.
    #[test]
    fn a_discard_page_copy_older_than_a_day_leaves_at_clean_with_apply() {
        use crate::commands::flow::discard::DISCARD_COPY_KEPT;
        use crate::commands::maint::scratch_gc::set_mtime;
        use std::time::{Duration, SystemTime};
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        let old = deleted(root, "velha");
        let recent = deleted(root, "recente");
        let now = SystemTime::now();
        set_mtime(&old, now - DISCARD_COPY_KEPT - Duration::from_secs(60));
        set_mtime(&recent, now - DISCARD_COPY_KEPT + Duration::from_secs(60));

        let listed = clean(root, false).expect("um projeto git");
        let lines: Vec<(&str, &str, &str)> = listed.candidates.iter().map(|r| (r.path.as_str(), r.spec.as_str(), r.reason.as_str())).collect();
        assert_eq!(lines, vec![(shown(&old).as_str(), "", OLD_DISCARD_COPY)], "{listed:?}");
        assert!(listed.kept.is_empty() && listed.removed.is_empty(), "{listed:?}");
        assert!(old.is_dir(), "{old:?}: without the option to remove, nothing leaves");

        let applied = clean(root, true).expect("um projeto git");
        assert!(applied.errors.is_empty(), "{applied:?}");
        assert_eq!(applied.removed, vec![shown(&old)], "{applied:?}");
        assert!(!old.exists(), "{old:?}: the copy older than a day stayed");
        assert!(recent.is_dir(), "{recent:?}: the copy younger than a day left");
    }
    /// A limpeza guarda, sob uma ref do repositório principal, o código que
    /// cada cópia tem além do commit antes de tirá-la — a de obra fechada e a
    /// do lugar antigo —, e o relatório nomeia a ref e o comando para trazê-lo
    /// de volta. A cópia cujo código o git não deixa guardar não sai, e o
    /// motivo vai entre os erros.
    #[test]
    fn clean_keeps_the_code_a_copy_holds_beyond_the_commit_before_removing_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        work(root, "fechada", "closed");
        work(root, "velha", "closed");
        let slot = slot_path(root, "fechada", 0);
        let old = root.join(".claude").join("worktrees").join("mustard-velha-7");
        for path in [&slot, &old] {
            copy(root, path);
            std::fs::write(path.join("depois.txt"), format!("código de {}", crate::shared::paths::canonical(&path.to_string_lossy()))).unwrap();
        }

        let applied = clean(root, true).expect("um projeto git");
        assert!(applied.errors.is_empty(), "{applied:?}");
        assert!(!slot.exists() && !old.exists(), "{applied:?}");
        assert_eq!(applied.code_kept.len(), 2, "{applied:?}");
        for kept in &applied.code_kept {
            assert_eq!(kept.files, vec!["depois.txt".to_string()], "{kept:?}");
            assert_eq!(kept.restore, format!("git cherry-pick --no-commit {}", kept.refname), "{kept:?}");
            let shown_copy = crate::shared::paths::canonical(&kept.copy);
            let out = std::process::Command::new("git").args(["show", &format!("{}:depois.txt", kept.refname)]).current_dir(root).output().expect("git");
            assert_eq!(String::from_utf8_lossy(&out.stdout), format!("código de {shown_copy}"), "{kept:?}");
        }

        // O git não cria ref dentro de outra: sem lugar para guardar, a cópia fica.
        work(root, "outra", "closed");
        let stuck = slot_path(root, "outra", 0);
        copy(root, &stuck);
        std::fs::write(stuck.join("depois.txt"), "sem lugar").unwrap();
        let head = std::process::Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().unwrap();
        git(root, &["update-ref", "refs/mustard/kept/outra", String::from_utf8_lossy(&head.stdout).trim()]);
        let again = clean(root, true).expect("um projeto git");
        assert!(stuck.join("depois.txt").is_file(), "a cópia sem onde guardar fica: {again:?}");
        assert!(again.removed.is_empty(), "{again:?}");
        assert_eq!(again.errors.len(), 1, "{again:?}");
        assert!(again.errors[0].path.contains("outra"), "{again:?}");
    }

    /// A limpeza tira a ref guardada de spec entregue cujo código a base já
    /// tem, conferido pelo git; a que guarda código que a base não tem fica,
    /// e sem a opção de apagar a ref só é listada.
    #[test]
    fn clean_drops_the_kept_ref_of_a_delivered_spec_whose_code_the_base_has() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        git(root, &["branch", "-M", "dev"]);
        work(root, "entregue", "closed");
        let path = store::spec_file(root, "entregue").unwrap();
        store::write(&path, "state", json!({"phase": "delivered", "author": "binary"}).as_object().cloned().unwrap(), &[]).unwrap();
        let text = |args: &[&str]| String::from_utf8_lossy(&std::process::Command::new("git").args(args).current_dir(root).output().unwrap().stdout).trim().to_string();
        let seed = text(&["rev-parse", "HEAD"]);
        let kept = |name: &str, file: &str, body: &str| {
            git(root, &["checkout", "-q", "--detach", &seed]);
            std::fs::write(root.join(file), body).unwrap();
            git(root, &["add", file]);
            git(root, &["commit", "-q", "-m", "guardado"]);
            let commit = text(&["rev-parse", "HEAD"]);
            git(root, &["checkout", "-q", "dev"]);
            let refname = format!("refs/mustard/kept/entregue/{name}");
            git(root, &["update-ref", &refname, &commit]);
            refname
        };
        let landed = kept("1-3-aaaa", "entrou.txt", "na base\n");
        let lost = kept("2-4-bbbb", "fora.txt", "só na ref\n");
        std::fs::write(root.join("entrou.txt"), "na base\n").unwrap();
        git(root, &["add", "entrou.txt"]);
        git(root, &["commit", "-q", "-m", "entrou"]);

        let listed = clean(root, false).expect("um projeto git");
        assert_eq!(listed.kept_landed, vec![landed.clone()], "{listed:?}");
        assert_eq!(text(&["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]).lines().count(), 2, "nada sai sem apagar");
        let applied = clean(root, true).expect("um projeto git");
        assert_eq!(applied.kept_landed, vec![landed], "{applied:?}");
        assert_eq!(text(&["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]), lost);
    }
}
