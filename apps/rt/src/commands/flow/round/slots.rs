//! As vagas da obra: a cópia fixa de cada vaga, fora da pasta do projeto,
//! que passa de uma onda para a seguinte com a compilação dentro dela. Aqui
//! mora quem prepara a vaga para a onda que sai (nova, reaproveitada ou
//! zerada), quem leva a ela os arquivos locais do projeto e os submódulos, e
//! quem apaga as cópias da obra quando ela fecha ou é descartada.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::domain::wave_prompt::{Reuse, WaveCopy};
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::wave_prompt::{
    copies_dir, final_copy_path, is_slot_of, local_file_inside, recorded_copy, shown, slot_path, spec_copies_dir,
};
use mustard_core::platform::git;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

use super::queue::{max_parallel, open_review, open_sends, orphaned_waves};
use crate::commands::git_settle::{enter_unit_branch, submodule_holding, submodules_of};
use crate::commands::wave::wave_overlap_check::wave_graph;

/// As vagas presas da spec `spec`, lida em `log`, cada uma pelo caminho como
/// o envio a grava: a cópia gravada em cada envio aberto de onda — a órfã
/// inclusive, que a rodada reenvia na mesma vaga — e, com a revisão final
/// aberta, a vaga que o envio dela gravou. A vaga da revisão é a que o
/// fechamento preparou, e continua dela até o veredito, mesmo que outra onda
/// saia e comite depois. O envio de revisão antigo, sem a cópia gravada, cai
/// na vaga da última onda ([`final_copy_path`]). É a conta única da vaga
/// ocupada: o despacho não entrega nenhuma delas a outra onda, e a busca dos
/// processos presos não encerra o que roda nelas.
pub(crate) fn held_slots(root: &Path, spec: &str, log: &SpecLog) -> BTreeSet<String> {
    let mut held: BTreeSet<String> = open_sends(log)
        .keys()
        .filter_map(|wave| recorded_copy(log, *wave))
        .map(|copy| copy.path)
        .filter(|copy| is_slot_of(root, spec, copy))
        .collect();
    if let Some(review) = open_review(log) {
        let recorded = log.get(review).and_then(|sent| sent.str_field("copy")).map(str::to_string);
        held.insert(recorded.unwrap_or_else(|| shown(&final_copy_path(root, spec, log))));
    }
    held
}

/// As cópias das ondas `waves`, que saem agora, cada uma numa vaga livre da
/// spec `spec`: a vaga que não está presa ([`held_slots`]) por um envio
/// aberto nem pela revisão final aberta. A vaga é uma cópia fixa, com a
/// compilação dentro dela, que passa de uma onda para a seguinte: só o que o
/// git mudou muda de data, e a compilação refaz só isso.
///
/// A onda que sai de novo e cujo último envio, sem entrega depois, gravou uma
/// vaga hoje livre — a replanejada — volta a essa vaga, e a cópia com
/// mudança fica como está ([`ensure_copy`]). Toda outra vaga é zerada no
/// commit atual ([`reset_slot`]). A vaga traz cada submódulo que as tarefas
/// da onda tocam. A onda cuja cópia não pôde ser preparada não sai, e o
/// aviso diz por quê; a onda sem vaga livre também não sai, e fica para a
/// rodada seguinte. Roda com a trava do passo do git que o despacho já
/// prendeu (`_held`): duas rodadas ao mesmo tempo não pegam a mesma vaga.
///
/// A obra de até 3 pontos (`solo`, de
/// [`crate::commands::flow::plan::is_solo_work`]) não cria cópia nenhuma: o
/// orquestrador faz a onda no checkout principal, na própria janela, e nada
/// aqui teria onde compilar.
pub(super) fn open_copies(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    _held: &LockedFile,
    waves: &[u64],
    solo: bool,
    lang: Locale,
) -> (BTreeMap<u64, WaveCopy>, Vec<Value>) {
    if solo {
        return (BTreeMap::new(), Vec::new());
    }
    // A cópia da onda órfã — em andamento sem o processo que a mandou — volta
    // ao commit atual sozinha, nesta rodada, sem esperar o reenvio pedir
    // isso: a onda falhou no meio do trabalho, e o que ela deixou para trás
    // não é uma retomada em curso. A vaga continua dela até o reenvio.
    for wave in orphaned_waves(log).keys() {
        super::commit::clean_orphan_copy(root, log, *wave);
    }
    let held = held_slots(root, spec, log);
    let mut free: Vec<PathBuf> =
        (0..max_parallel(root)).map(|slot| slot_path(root, spec, slot)).filter(|slot| !held.contains(&shown(slot))).collect();

    // A onda que volta à vaga que o último envio dela gravou pega essa vaga
    // antes de as outras escolherem.
    let sends = log.last_by_wave("send");
    let delivered = log.last_by_wave("delivered");
    let mut chosen: BTreeMap<u64, (PathBuf, bool)> = BTreeMap::new();
    for wave in waves {
        let Some(sent) = sends.get(wave) else { continue };
        if delivered.get(wave).is_some_and(|id| id > sent) {
            continue;
        }
        let Some(copy) = log.get(*sent).and_then(|event| event.str_field("copy")) else { continue };
        if let Some(at) = free.iter().position(|slot| shown(slot) == copy) {
            chosen.insert(*wave, (free.remove(at), true));
        }
    }
    for wave in waves {
        if !chosen.contains_key(wave) && !free.is_empty() {
            chosen.insert(*wave, (free.remove(0), false));
        }
    }

    let mut copies = BTreeMap::new();
    let mut warnings = Vec::new();
    let failed = |wave: u64, detail: String| {
        let hint = translate("round.copy_failed", lang).replace("{wave}", &wave.to_string()).replace("{detail}", &detail);
        json!({ "reason": "copy-not-created", "wave": wave, "hint": hint })
    };
    let head = git::run(root, &["rev-parse", "HEAD"]).result();
    let subs = submodules_of(root);
    let files = if subs.is_empty() { BTreeMap::new() } else { wave_graph(log).files };
    let unit = State::from_log(log).branch.unwrap_or_default();
    for wave in waves.iter().copied() {
        let Some((path, own)) = chosen.remove(&wave) else { continue };
        let touched: BTreeSet<&str> = files
            .get(&wave)
            .into_iter()
            .flatten()
            .filter_map(|file| submodule_holding(&subs, file).map(|(sub, _)| sub))
            .collect();
        let made = match &head {
            Err(detail) => Err(detail.clone()),
            Ok(head) => if own { ensure_copy(root, &path, head) } else { reset_slot(root, &path, head) }
                .and_then(|prepared| {
                    touched.iter().try_for_each(|sub| copy_submodule(root, &path, sub, &unit)).map(|()| prepared)
                }),
        };
        match made {
            Ok(prepared) => {
                let copy = shown(&path);
                for file in prepared.missing {
                    let hint = local_file_missing(&file, &copy, lang);
                    warnings.push(json!({ "reason": "local-file-missing", "wave": wave, "file": file, "hint": hint }));
                }
                copies.insert(wave, WaveCopy { path: copy, reused: prepared.reused });
            }
            Err(detail) => warnings.push(failed(wave, detail)),
        }
    }
    (copies, warnings)
}

/// O aviso do arquivo local `file` que não chegou à cópia `copy`.
pub(crate) fn local_file_missing(file: &str, copy: &str, lang: Locale) -> String {
    translate("round.local_file_missing", lang).replace("{file}", file).replace("{copy}", copy)
}

/// O que a preparação de uma cópia deixou: os itens da lista de arquivos
/// locais que não chegaram a ela e, na vaga reaproveitada, o que mudou desde
/// o último uso dela.
pub(crate) struct Prepared {
    /// Os itens de `localFiles` que não chegaram à cópia.
    pub(crate) missing: Vec<String>,
    /// O commit em que a vaga estava e os arquivos que mudaram de lá até o
    /// commit atual; `None` na vaga nova.
    pub(crate) reused: Option<Reuse>,
}

/// A cópia em `path`, no commit `head` do checkout `root`, com o que ela já
/// tinha preservado. A vaga que já é uma cópia viva e está limpa vai para o
/// commit `head`, porque um commit fora da rodada pode ter avançado o
/// checkout principal desde o último uso dela. A que tem mudança, como a de
/// uma retomada em andamento, fica como está. A pasta que não é cópia viva
/// nasce de novo ([`new_copy`]).
///
/// A cópia recebe os arquivos locais do projeto ([`copy_local_files`]): o git
/// não os leva. A cópia sai mesmo com um deles faltando.
pub(crate) fn ensure_copy(root: &Path, path: &Path, head: &str) -> Result<Prepared, String> {
    if !live_copy(path) {
        return new_copy(root, path, head);
    }
    let before = git::run(path, &["rev-parse", "HEAD"]).out().unwrap_or_default();
    let clean = git::run(path, &["status", "--porcelain", "--untracked-files=all"])
        .out()
        .is_some_and(|status| status.is_empty());
    if !clean {
        return Ok(Prepared { missing: Vec::new(), reused: Some(Reuse { since: before, changed: Vec::new() }) });
    }
    git::run(path, &["checkout", "--detach", head]).result()?;
    Ok(Prepared { missing: copy_local_files(root, path), reused: changed_since(root, &before, head) })
}

/// A vaga em `path` zerada no commit `head` do checkout `root`: a cópia viva
/// descarta toda mudança e todo arquivo novo que o git não ignora (`checkout
/// --force` e `clean -fd`, sem `-x`), e cada submódulo dentro dela faz o
/// mesmo no commit dele. O que o git ignora — a compilação, as dependências
/// instaladas — fica, e o git só troca o arquivo que mudou: o resto guarda a
/// data, e a compilação refaz só o que mudou. A pasta que não é cópia viva
/// nasce de novo ([`new_copy`]). Depois, os arquivos locais do projeto.
pub(crate) fn reset_slot(root: &Path, path: &Path, head: &str) -> Result<Prepared, String> {
    if !live_copy(path) {
        return new_copy(root, path, head);
    }
    let before = git::run(path, &["rev-parse", "HEAD"]).out().unwrap_or_default();
    if !super::commit::reset_with_submodules(root, path, head) {
        return Err(format!("git checkout --detach --force {head}: {}", shown(path)));
    }
    Ok(Prepared { missing: copy_local_files(root, path), reused: changed_since(root, &before, head) })
}

/// A pasta `path` é uma cópia viva do git: tem o arquivo `.git` de uma cópia
/// ligada e o git ainda acha o commit dela. A pasta cujo registro o git já
/// esqueceu não é.
fn live_copy(path: &Path) -> bool {
    path.join(".git").is_file() && git::run(path, &["rev-parse", "--verify", "HEAD"]).ok
}

/// Cria a cópia `path` no commit `head` do checkout `root`. A pasta que
/// existe sem ser cópia viva sai antes, só dentro da pasta das cópias do
/// projeto; o registro velho do git, de uma pasta que sumiu, sai pelo
/// `worktree prune` antes do `add`. A pasta mãe nasce antes da cópia.
fn new_copy(root: &Path, path: &Path, head: &str) -> Result<Prepared, String> {
    if path.exists() {
        if !inside_copies(root, path) {
            return Err(format!("not a copy folder: {}", shown(path)));
        }
        std::fs::remove_dir_all(path).map_err(|err| format!("{}: {err}", shown(path)))?;
    }
    if let Some(parent) = path.parent() {
        mustard_core::io::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    git::run(root, &["worktree", "prune"]).result()?;
    let target = path.to_string_lossy();
    git::run(root, &["worktree", "add", "--detach", &target, head]).result()?;
    Ok(Prepared { missing: copy_local_files(root, path), reused: None })
}

/// Os arquivos que mudaram do commit `before` ao commit `head`, que o git do
/// checkout `root` conhece. `None` quando a vaga não sabia o commit dela ou o
/// git não achou um dos dois: aí ela prepara como nova.
fn changed_since(root: &Path, before: &str, head: &str) -> Option<Reuse> {
    if before.is_empty() {
        return None;
    }
    let listed = git::run(root, &["diff", "--name-only", before, head]);
    if !listed.ok {
        return None;
    }
    let changed = listed.stdout.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect();
    Some(Reuse { since: before.to_string(), changed })
}

/// A pasta `path` mora dentro de um dos lugares de cópia do projeto `root`:
/// a pasta das cópias dele, fora do projeto, ou o lugar antigo, dentro dele
/// (`.claude/worktrees`). Só aí o Mustard apaga pasta; a pasta principal e a
/// compilação dela nunca entram.
pub(crate) fn inside_copies(root: &Path, path: &Path) -> bool {
    let places = [copies_dir(root), root.join(".claude").join("worktrees")];
    places.iter().any(|place| path.starts_with(place) && path != place.as_path())
        && path.components().all(|part| !matches!(part, std::path::Component::ParentDir))
}

/// Tira a cópia `path` do checkout `root`: primeiro a cópia de cada
/// submódulo dentro dela, depois a própria cópia (`git worktree remove
/// --force`), o que sobrar da pasta e o registro do git que ficou sem pasta
/// (`git worktree prune`). Só dentro dos lugares de cópia do projeto
/// ([`inside_copies`]). Devolve o motivo quando a cópia não saiu: a pasta
/// ficou, ou o git ainda a lista.
pub(crate) fn remove_copy(root: &Path, path: &Path) -> Result<(), String> {
    if !inside_copies(root, path) {
        return Err(format!("not a copy folder: {}", shown(path)));
    }
    let mut detail = String::new();
    for sub in submodules_of(root) {
        let inner = path.join(&sub);
        if inner.join(".git").is_file()
            && let Err(err) = git::run(&root.join(&sub), &["worktree", "remove", "--force", &shown(&inner)]).result()
        {
            detail = err;
        }
    }
    if path.join(".git").is_file()
        && let Err(err) = git::run(root, &["worktree", "remove", "--force", &shown(path)]).result()
    {
        detail = err;
    }
    if path.exists()
        && let Err(err) = std::fs::remove_dir_all(path)
    {
        detail = format!("{}: {err}", shown(path));
    }
    let _ = git::run(root, &["worktree", "prune"]);
    for sub in submodules_of(root) {
        let _ = git::run(&root.join(&sub), &["worktree", "prune"]);
    }
    if path.exists() || registered_copies(root).contains(&shown(path)) {
        return Err(if detail.is_empty() { shown(path) } else { detail });
    }
    Ok(())
}

/// As cópias que o git do checkout `root` ainda lista, com barras normais.
fn registered_copies(root: &Path) -> BTreeSet<String> {
    git::run(root, &["worktree", "list", "--porcelain"])
        .out()
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|path| path.trim().replace('\\', "/"))
        .collect()
}

/// As vagas que a obra `spec` tem no disco, cada uma pelo caminho, em
/// ordem: o que [`remove_spec_copies`] tiraria agora.
pub(crate) fn spec_copies(root: &Path, spec: &str) -> Vec<String> {
    let mut slots: Vec<String> = std::fs::read_dir(spec_copies_dir(root, spec))
        .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).map(|path| shown(&path)).collect())
        .unwrap_or_default();
    slots.sort();
    slots
}

/// Tira todas as cópias da obra `spec` — cada vaga, com as cópias dos
/// submódulos dentro dela ([`remove_copy`]) — e apaga a pasta dela
/// ([`spec_copies_dir`]), com a trava do passo do git presa. Chamado pelo
/// fechamento, pelo descarte e por `mustard-rt run clean`. Devolve cada
/// cópia que não saiu, com o motivo; a pasta principal e a compilação dela
/// nunca entram.
pub(crate) fn remove_spec_copies(root: &Path, spec: &str) -> Vec<(String, String)> {
    let dir = spec_copies_dir(root, spec);
    let single = Path::new(spec).components().count() == 1
        && matches!(Path::new(spec).components().next(), Some(std::path::Component::Normal(_)));
    if !single || dir.parent() != Some(copies_dir(root).as_path()) {
        return vec![(shown(&dir), format!("not a copy folder: {}", shown(&dir)))];
    }
    if !dir.exists() {
        return Vec::new();
    }
    let _held = match crate::commands::git_settle::git_step_lock(root) {
        Ok(held) => held,
        Err(detail) => return vec![(shown(&dir), detail)],
    };
    let slots: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect())
        .unwrap_or_default();
    let mut left: Vec<(String, String)> =
        slots.iter().filter_map(|slot| remove_copy(root, slot).err().map(|detail| (shown(slot), detail))).collect();
    if left.is_empty()
        && dir.exists()
        && let Err(err) = std::fs::remove_dir_all(&dir)
    {
        left.push((shown(&dir), err.to_string()));
    }
    left
}

/// Os arquivos locais que o projeto `root` declara (`localFiles`, no
/// `mustard.json`) levados à cópia `copy`, cada um no mesmo caminho relativo,
/// sempre pelo conteúdo: nenhum atalho, junção ou link para o repositório
/// principal, em nenhum sistema, então apagar a cópia nunca toca arquivo
/// dele. Devolve, na ordem da lista, os itens que não foram copiados: o que o
/// git não ignora ([`local_file_ignored`]), o que falta no principal, o que
/// não é arquivo e o que o disco recusou. Lista vazia ou ausente não copia
/// nada.
fn copy_local_files(root: &Path, copy: &Path) -> Vec<String> {
    let listed = mustard_core::ProjectConfig::load(root).local_files.unwrap_or_default();
    listed
        .iter()
        .map(|file| file.trim())
        .filter(|file| !file.is_empty())
        .filter(|file| !copy_local_file(root, copy, file))
        .map(str::to_string)
        .collect()
}

/// Copia o arquivo local `file` do checkout `root` para a cópia `copy`;
/// `false` quando ele não foi copiado. O arquivo que o git não ignora nunca é
/// copiado: a cópia já o tem pelo git, na versão do commit. O link que já
/// estiver no lugar dele na cópia sai antes: copiar por cima escreveria no
/// alvo do link. O arquivo que já está igual na cópia não é escrito de novo:
/// ele guarda a data, e a compilação não o refaz.
fn copy_local_file(root: &Path, copy: &Path, file: &str) -> bool {
    if !local_file_ignored(root, file) {
        return false;
    }
    let (from, to) = (root.join(file), copy.join(file));
    if !from.is_file() {
        return false;
    }
    if let Some(parent) = to.parent()
        && mustard_core::io::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    let linked = std::fs::symlink_metadata(&to).is_ok_and(|meta| meta.file_type().is_symlink());
    if linked && mustard_core::io::fs::remove_file(&to).is_err() {
        return false;
    }
    if !linked && std::fs::read(&to).is_ok_and(|old| std::fs::read(&from).is_ok_and(|new| new == old)) {
        return true;
    }
    std::fs::copy(&from, &to).is_ok()
}

/// Os projetos de teste desta linha de execução e a pasta das cópias de cada
/// um ([`mustard_core::io::wave_prompt::copies_dir`]). Quando a linha termina
/// — o teste passou ou falhou —, cada pasta sai do disco e, com o repositório
/// ainda no lugar, o git dele esquece as cópias que sumiram.
#[cfg(test)]
struct TestCopies(Vec<(PathBuf, PathBuf)>);

#[cfg(test)]
impl Drop for TestCopies {
    fn drop(&mut self) {
        for (root, copies) in &self.0 {
            let _ = std::fs::remove_dir_all(copies);
            // O git direto: no fim da linha de execução, outro valor dela que
            // a porta do git lesse já pode ter saído.
            if root.join(".git").exists() {
                let _ = std::process::Command::new("git").args(["worktree", "prune"]).current_dir(root).output();
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    static TEST_COPIES: std::cell::RefCell<TestCopies> = const { std::cell::RefCell::new(TestCopies(Vec::new())) };
}

/// As cópias que um teste cria para o projeto `root` saem no fim dele,
/// também quando ele falha, e o git do projeto deixa de listá-las. Elas moram
/// fora da pasta temporária do teste — na pasta que `MUSTARD_COPIES_DIR`
/// indica —, e a pasta temporária, quando sai, não as leva. Chame da linha de
/// execução do próprio teste, com o projeto já no lugar.
#[cfg(test)]
pub(crate) fn copies_leave_with_the_test(root: &Path) {
    let copies = mustard_core::io::wave_prompt::copies_dir(root);
    TEST_COPIES.with(|made| made.borrow_mut().0.push((root.to_path_buf(), copies)));
}

/// O item `file` da lista de arquivos locais é um arquivo que o git do
/// checkout `root` ignora: um caminho relativo dentro do projeto
/// ([`local_file_inside`]) que casa com uma regra de ignorar e não está
/// versionado. É a conferência única da lista, nos dois pontos: o `upsert`
/// não grava o item que não passa, e a cópia da onda não o copia. O arquivo
/// que o git não ignora chega à cópia pelo próprio git, na versão do commit;
/// copiá-lo da pasta principal por cima trocaria essa versão pela de lá. Sem
/// git, nenhum arquivo passa.
pub(crate) fn local_file_ignored(root: &Path, file: &str) -> bool {
    local_file_inside(file) && git::run(root, &["check-ignore", "-q", "--", file]).ok
}

/// A cópia do submódulo `sub` dentro da cópia `copy`: o submódulo do
/// repositório principal entra na branch `unit` da spec, criada na primeira
/// vez sobre a base dele, e a cópia dele sai do commit em que ele fica. A que
/// já existe na vaga é a mesma, zerada junto com ela.
fn copy_submodule(root: &Path, copy: &Path, sub: &str, unit: &str) -> Result<(), String> {
    let inner = copy.join(sub);
    if inner.join(".git").is_file() {
        return Ok(());
    }
    let repo = root.join(sub);
    enter_unit_branch(&repo, unit)?;
    git::run(&repo, &["worktree", "prune"]).result()?;
    let target = inner.to_string_lossy();
    git::run(&repo, &["worktree", "add", "--detach", &target, "HEAD"]).result().map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::path::Component;

    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A spec `x`, como a rodada a deixou no arquivo de eventos.
    fn spec_log(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// A cópia gravada no envio mais novo da onda `wave` da spec `x`.
    fn sent_copy(root: &Path, wave: u64) -> String {
        recorded_copy(&spec_log(root), wave).unwrap_or_else(|| panic!("wave {wave}")).path
    }

    /// A vaga da revisão final aberta fica presa como a de uma onda em
    /// andamento: a onda que sai enquanto o revisor trabalha na vaga a, a da
    /// última onda, vai para a vaga b, e o que o revisor mudou na a, sem
    /// comitar, fica.
    #[test]
    fn an_open_final_review_holds_its_slot_and_the_new_wave_takes_another() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        let first = round(root, "x", None);
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
        let review = slot_path(root, "x", 0);
        assert_eq!(sent_copy(root, 1), shown(&review), "{first}");
        let done = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(done["ok"], json!(true), "{done}");

        seed_review(root);
        assert_eq!(final_copy_path(root, "x", &spec_log(root)), review, "o revisor usa a vaga da última onda");
        std::fs::write(review.join("src/a.rs"), "fn revisto() {}\n").unwrap();
        let log = spec_log(root);
        let first_of = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
        let (said, crit) = (first_of("message"), first_of("criterion"));
        write(root, "x", "wave", json!({"n": 2, "text": "Onda 2.", "criteria": [crit], "done_when": "A suíte passa.",
            "origin": said}));
        write(root, "x", "task", json!({"wave": 2, "text": "Tarefa da onda 2.", "files": [{"path": "src/b.rs"}],
            "depends_on": [], "origin": said}));

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "{out}");
        assert_eq!(sent_copy(root, 2), shown(&slot_path(root, "x", 1)), "a vaga da revisão aberta está presa: {out}");
        let kept = std::fs::read_to_string(review.join("src/a.rs")).unwrap();
        assert_eq!(kept, "fn revisto() {}\n", "a mudança do revisor fica na vaga dele");
    }

    /// Um projeto git vazio, com as cópias dele saindo no fim do teste.
    fn bare_project(root: &Path) {
        git_at(root, &["init", "-q"]);
        copies_leave_with_the_test(root);
    }

    /// O caminho que entra na pasta das cópias do projeto `root` e volta, por
    /// `..`, até a pasta `target`, fora dela.
    fn climbing_out(root: &Path, target: &Path) -> PathBuf {
        let copies = copies_dir(root);
        let up = copies.components().filter(|part| matches!(part, Component::Normal(_))).count();
        let mut path = copies;
        path.extend(std::iter::repeat_n("..", up));
        path.extend(target.components().filter(|part| matches!(part, Component::Normal(_))));
        path
    }

    /// Tirar uma cópia só vale dentro dos lugares de cópia do projeto: a pasta
    /// fora deles, como a compilação da pasta principal, e o caminho que entra
    /// na pasta das cópias e sai dela por `..` são recusados, e a pasta fica
    /// no disco com o que tinha.
    #[test]
    fn a_copy_outside_the_copy_places_is_never_removed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        bare_project(root);
        let built = root.join("target").join("debug");
        std::fs::create_dir_all(&built).unwrap();
        std::fs::write(built.join("mustard"), "compilado").unwrap();

        for path in [built.clone(), climbing_out(root, &built)] {
            let refused = remove_copy(root, &path);
            assert_eq!(refused, Err(format!("not a copy folder: {}", shown(&path))), "{}", path.display());
        }
        assert_eq!(std::fs::read_to_string(built.join("mustard")).unwrap(), "compilado", "a compilação principal fica");
    }

    /// Tirar as cópias de uma obra só vale para um nome de uma pasta só,
    /// direto na pasta das cópias do projeto: o nome com barra, que cairia
    /// dentro da vaga de outra obra, e o `..`, que subiria para a pasta de
    /// todos os projetos, são recusados, e nada sai do disco.
    #[test]
    fn the_copies_of_a_spec_name_with_a_slash_or_dots_are_never_removed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        bare_project(root);
        let slot = slot_path(root, "x", 0);
        std::fs::create_dir_all(slot.join("src")).unwrap();
        std::fs::write(slot.join("src").join("lib.rs"), "fn um() {}\n").unwrap();

        for spec in ["x/a", ".."] {
            let folder = shown(&spec_copies_dir(root, spec));
            let refused = remove_spec_copies(root, spec);
            assert_eq!(refused, vec![(folder.clone(), format!("not a copy folder: {folder}"))], "{spec}");
        }
        assert_eq!(std::fs::read_to_string(slot.join("src").join("lib.rs")).unwrap(), "fn um() {}\n", "a vaga fica");
    }
}
