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

use super::keep::{Kept, Keeping};
use super::queue::{max_parallel, open_review, orphaned_waves, unanswered_sends};
use crate::commands::git_settle::{enter_unit_branch, submodule_holding, submodules_of};
use crate::commands::wave::wave_overlap_check::wave_graph;

/// Cada cópia gravada por uma onda que ainda a segura, com as ondas que a
/// seguram: a do envio sem volta oficial ([`unanswered_sends`]) — a órfã
/// inclusive, que a rodada reenvia na mesma vaga, a que ganhou plano novo
/// depois do pedido e a da onda que voltou e espera a rodada, cuja entrega
/// ainda não é oficial e guarda o código até o commit. A cópia é a gravada no
/// último envio de cada onda, como o envio a grava.
fn copy_holders(log: &SpecLog) -> BTreeMap<String, BTreeSet<u64>> {
    let mut holders: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    for wave in unanswered_sends(log).into_keys() {
        if let Some(copy) = recorded_copy(log, wave) {
            holders.entry(copy.path).or_default().insert(wave);
        }
    }
    holders
}

/// As ondas de `waves` cuja cópia gravada é também a de outra onda que a
/// segura ([`copy_holders`]): a cópia que duas ondas dividem não é de
/// nenhuma das duas, e nenhuma delas a apaga nem volta a ela.
pub(super) fn sharing_copy(log: &SpecLog, waves: impl IntoIterator<Item = u64>) -> BTreeSet<u64> {
    let holders = copy_holders(log);
    waves
        .into_iter()
        .filter(|wave| {
            recorded_copy(log, *wave)
                .and_then(|copy| holders.get(&copy.path).cloned())
                .is_some_and(|by| by.iter().any(|other| other != wave))
        })
        .collect()
}

/// As ondas de `waves` cuja cópia gravada deixou de ser uma cópia viva
/// ([`live_copy`]): a pasta foi apagada do disco, ou o git esqueceu o registro
/// dela. A onda que sai de novo não volta a uma cópia assim: a rodada prepara
/// outra antes de gravar o envio ([`open_copies`]), e o código que a pasta
/// ainda tinha fica guardado ([`new_copy`]).
pub(super) fn without_live_copy(log: &SpecLog, waves: impl IntoIterator<Item = u64>) -> BTreeSet<u64> {
    waves
        .into_iter()
        .filter(|wave| recorded_copy(log, *wave).is_some_and(|copy| !live_copy(Path::new(&copy.path))))
        .collect()
}

/// As vagas presas da spec `spec`, lida em `log`, cada uma pelo caminho como
/// o envio a grava: a cópia de cada onda que a segura ([`copy_holders`]) — a
/// do envio aberto, a órfã inclusive, e a da onda com volta ainda não
/// assumida — e, com a revisão final aberta, a vaga que o envio dela gravou.
/// A vaga da revisão é a que o fechamento preparou, e continua dela até o
/// veredito, mesmo que outra onda saia e comite depois. O envio de revisão
/// antigo, sem a cópia gravada, cai na vaga da última onda
/// ([`final_copy_path`]). É a conta única da vaga ocupada: o despacho não
/// entrega nenhuma delas a outra onda, e a busca dos processos presos não
/// encerra o que roda nelas.
pub(crate) fn held_slots(root: &Path, spec: &str, log: &SpecLog) -> BTreeSet<String> {
    let mut held: BTreeSet<String> =
        copy_holders(log).into_keys().filter(|copy| is_slot_of(root, spec, copy)).collect();
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
/// mudança fica como está ([`ensure_copy`]); a que sumiu do disco nasce de
/// novo na mesma vaga, no commit atual ([`new_copy`]) — é o que a onda a
/// reenviar com a cópia apagada pede ([`without_live_copy`]). Toda outra vaga
/// é zerada no commit atual ([`reset_slot`]). A vaga traz cada submódulo que as tarefas
/// da onda tocam. A onda cuja cópia não pôde ser preparada não sai, e o
/// aviso diz por quê; a onda sem vaga livre também não sai, e fica para a
/// rodada seguinte. Roda com a trava do passo do git que o despacho já
/// prendeu (`_held`): duas rodadas ao mesmo tempo não pegam a mesma vaga.
pub(super) fn open_copies(
    root: &Path,
    spec: &str,
    log: &SpecLog,
    _held: &LockedFile,
    waves: &[u64],
    lang: Locale,
) -> (BTreeMap<u64, WaveCopy>, Vec<Value>) {
    // A cópia da onda órfã — em andamento sem o processo que a mandou — volta
    // ao commit atual sozinha, nesta rodada, sem esperar o reenvio pedir
    // isso: a onda falhou no meio do trabalho, e o que ela deixou para trás
    // não é uma retomada em curso. A vaga continua dela até o reenvio.
    // A cópia que outra onda também segura fica como está: limpá-la apagaria o
    // trabalho da outra.
    // O que ela tem e o commit atual não tem fica guardado antes; a que não
    // pôde guardar segue como está, com a vaga presa, e o aviso diz por quê.
    let mut warnings = Vec::new();
    let shared = sharing_copy(log, orphaned_waves(log).into_keys());
    for wave in orphaned_waves(log).keys().filter(|wave| !shared.contains(wave)) {
        match super::keep::clean_orphan_copy(root, log, spec, *wave) {
            Ok(kept) => warnings.extend(kept.iter().map(|one| code_kept(one, lang))),
            Err(detail) => {
                let hint = translate("round.copy_not_cleaned", lang)
                    .replace("{wave}", &wave.to_string())
                    .replace("{detail}", &detail);
                warnings.push(json!({ "reason": "copy-not-cleaned", "wave": wave, "hint": hint }));
            }
        }
    }
    let held = held_slots(root, spec, log);
    // A onda que sai de novo e é a única dona da vaga que o último envio dela
    // gravou — a replanejada — volta a ela: é a vaga dela, não a de outra.
    let holders = copy_holders(log);
    let own: BTreeSet<String> = waves
        .iter()
        .filter_map(|wave| recorded_copy(log, *wave).map(|copy| (*wave, copy.path)))
        .filter(|(wave, path)| holders.get(path).is_some_and(|by| by.iter().all(|other| other == wave)))
        .map(|(_, path)| path)
        .collect();
    let mut free: Vec<PathBuf> = (0..max_parallel(root))
        .map(|slot| slot_path(root, spec, slot))
        .filter(|slot| !held.contains(&shown(slot)) || own.contains(&shown(slot)))
        .collect();

    // A onda que volta à vaga que o último envio dela gravou pega essa vaga
    // antes de as outras escolherem.
    let sends = log.last_dispatch_by_wave();
    let delivered = log.last_by_wave("delivered");
    let mut chosen: BTreeMap<u64, (PathBuf, bool)> = BTreeMap::new();
    for wave in waves {
        let Some(sent) = sends.get(wave) else { continue };
        if delivered.get(wave).is_some_and(|id| id > sent) {
            continue;
        }
        let Some(copy) = recorded_copy(log, *wave).map(|copy| copy.path) else { continue };
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
            Ok(head) => if own {
                ensure_copy(root, &path, head, &slot_owner(spec, log, &path))
            } else {
                reset_slot(root, &path, head, &slot_owner(spec, log, &path))
            }
            .and_then(|prepared| {
                    touched.iter().try_for_each(|sub| copy_submodule(root, &path, sub, &unit)).map(|()| prepared)
                }),
        };
        match made {
            Ok(prepared) => {
                let copy = shown(&path);
                warnings.extend(prepared.kept.iter().map(|one| code_kept(one, lang)));
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
    /// O que a preparação guardou antes de zerar ou apagar a vaga: o código
    /// que ela tinha e o commit atual não tem.
    pub(crate) kept: Vec<Kept>,
}

/// A cópia em `path`, no commit `head` do checkout `root`, com o que ela já
/// tinha preservado. A vaga que já é uma cópia viva e está limpa vai para o
/// commit `head`, porque um commit fora da rodada pode ter avançado o
/// checkout principal desde o último uso dela. A que tem mudança, como a de
/// uma retomada em andamento, fica como está. A pasta que não é cópia viva
/// nasce de novo ([`new_copy`]), guardando antes o que ela tinha de `owner`.
///
/// A cópia recebe os arquivos locais do projeto ([`copy_local_files`]): o git
/// não os leva. A cópia sai mesmo com um deles faltando.
pub(crate) fn ensure_copy(root: &Path, path: &Path, head: &str, owner: &Keeping) -> Result<Prepared, String> {
    if !live_copy(path) {
        return new_copy(root, path, head, owner);
    }
    let before = git::run(path, &["rev-parse", "HEAD"]).out().unwrap_or_default();
    let clean = git::run(path, &["status", "--porcelain", "--untracked-files=all"])
        .out()
        .is_some_and(|status| status.is_empty());
    if !clean {
        return Ok(Prepared { missing: Vec::new(), reused: Some(Reuse { since: before, changed: Vec::new() }), kept: Vec::new() });
    }
    git::run(path, &["checkout", "--detach", head]).result()?;
    Ok(Prepared { missing: copy_local_files(root, path), reused: changed_since(root, &before, head), kept: Vec::new() })
}

/// A vaga em `path` zerada no commit `head` do checkout `root`: a cópia viva
/// descarta toda mudança e todo arquivo novo que o git não ignora (`checkout
/// --force` e `clean -fd`, sem `-x`), e cada submódulo dentro dela faz o
/// mesmo no commit dele. O que o git ignora — a compilação, as dependências
/// instaladas — fica, e o git só troca o arquivo que mudou: o resto guarda a
/// data, e a compilação refaz só o que mudou. A pasta que não é cópia viva
/// nasce de novo ([`new_copy`]), guardando antes o que ela tinha de `owner`.
/// Depois, os arquivos locais do projeto.
pub(crate) fn reset_slot(root: &Path, path: &Path, head: &str, owner: &Keeping) -> Result<Prepared, String> {
    // O commit em que a vaga estava, lido antes de zerá-la: só a vaga que sai
    // de novo o usa, para dizer o que mudou desde o último uso.
    let mut before = String::new();
    let zeroed = zero_live_copy(root, path, || {
        before = git::run(path, &["rev-parse", "HEAD"]).out().unwrap_or_default();
        super::keep::reset_with_submodules(root, path, head, owner)
    })?;
    let Some(zeroed) = zeroed else { return new_copy(root, path, head, owner) };
    Ok(Prepared { missing: zeroed.missing, reused: changed_since(root, &before, head), kept: zeroed.value })
}

/// A vaga em `path`, cópia viva, de uma onda cujo código a rodada acabou de
/// comitar, zerada sem guardar nada ([`super::keep::reset_in_place`]), com os
/// arquivos locais do projeto de volta como em [`reset_slot`]. A pasta que
/// não é cópia viva fica como está, e o motivo é o erro.
pub(super) fn reset_committed_slot(root: &Path, path: &Path) -> Result<(), String> {
    zero_live_copy(root, path, || super::keep::reset_in_place(root, path))?
        .map(|_| ())
        .ok_or_else(|| format!("not a live copy: {}", shown(path)))
}

/// O que zerar uma cópia viva deixou: o que o zerar devolveu e os itens da
/// lista de arquivos locais que não voltaram a ela.
struct Zeroed<T> {
    value: T,
    missing: Vec<String>,
}

/// A cópia viva em `path` zerada por `reset`, com os arquivos locais do
/// projeto `root` de volta ([`copy_local_files`]): é o que toda volta de vaga
/// ao commit faz, guardando antes ou não. `None` na pasta que não é cópia viva
/// ([`live_copy`]), sem rodar `reset`; o `reset` que falha devolve o erro, e
/// nenhum arquivo local é levado.
fn zero_live_copy<T>(
    root: &Path,
    path: &Path,
    reset: impl FnOnce() -> Result<T, String>,
) -> Result<Option<Zeroed<T>>, String> {
    if !live_copy(path) {
        return Ok(None);
    }
    let value = reset()?;
    Ok(Some(Zeroed { value, missing: copy_local_files(root, path) }))
}

/// De quem é o que a vaga `path` da obra `spec` tem, para o que a limpeza
/// guarda ([`Keeping`]): a onda do envio mais novo que gravou essa vaga, e
/// esse envio no nome da ref; a vaga que nenhum envio gravou fica com o nome
/// dela.
pub(crate) fn slot_owner(spec: &str, log: &SpecLog, path: &Path) -> Keeping {
    let slot = shown(path);
    let owner = log
        .visible()
        .into_iter()
        .filter(|sent| sent.event_type == "send" && sent.str_field("copy") == Some(slot.as_str()))
        .filter_map(|sent| sent.wave().map(|wave| (wave, sent.id)))
        .max_by_key(|(_, id)| *id);
    match owner {
        Some((wave, sent)) => Keeping { label: format!("{spec}/{wave}-{sent}"), wave: Some(wave) },
        None => {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            Keeping { label: format!("{spec}/{name}"), wave: None }
        }
    }
}

/// O texto que diz o código que uma limpeza guardou: de quem era, onde ficou
/// e como trazê-lo de volta.
pub(crate) fn code_kept_hint(kept: &Kept, lang: Locale) -> String {
    match kept.wave {
        Some(wave) => translate("round.code_kept", lang).replace("{wave}", &wave.to_string()),
        None => translate("round.code_kept_slot", lang).to_string(),
    }
    .replace("{copy}", &kept.copy)
    .replace("{ref}", &kept.refname)
}

/// O aviso do código que uma limpeza guardou, com o texto de
/// [`code_kept_hint`].
fn code_kept(kept: &Kept, lang: Locale) -> Value {
    json!({
        "reason": "code-kept", "wave": kept.wave, "ref": kept.refname, "files": kept.files,
        "hint": code_kept_hint(kept, lang),
    })
}

/// A pasta `path` é uma cópia viva do git: tem o arquivo `.git` de uma cópia
/// ligada e o git ainda acha o commit dela. A pasta cujo registro o git já
/// esqueceu não é.
pub(super) fn live_copy(path: &Path) -> bool {
    path.join(".git").is_file() && git::run(path, &["rev-parse", "--verify", "HEAD"]).ok
}

/// Cria a cópia `path` no commit `head` do checkout `root`. A pasta que
/// existe sem ser cópia viva sai antes, só dentro da pasta das cópias do
/// projeto, e só depois de o que ela tem de arquivo e o commit não tem ficar
/// guardado sob uma ref do repositório principal ([`keep_copy_code`], com o
/// dono `owner`): a pasta que não pôde guardar fica como está, e o motivo é o
/// erro. A pasta sem arquivo nenhum sai sem guardar nada. O registro velho do
/// git, de uma pasta que sumiu, sai pelo `worktree prune` antes do `add`. A
/// pasta mãe nasce antes da cópia.
fn new_copy(root: &Path, path: &Path, head: &str, owner: &Keeping) -> Result<Prepared, String> {
    let mut kept = Vec::new();
    if path.exists() {
        if !inside_copies(root, path) {
            return Err(format!("not a copy folder: {}", shown(path)));
        }
        kept = super::keep::keep_copy_code(root, path, owner)?;
        std::fs::remove_dir_all(path).map_err(|err| format!("{}: {err}", shown(path)))?;
    }
    if let Some(parent) = path.parent() {
        mustard_core::io::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    git::run(root, &["worktree", "prune"]).result()?;
    let target = path.to_string_lossy();
    git::run(root, &["worktree", "add", "--detach", &target, head]).result()?;
    Ok(Prepared { missing: copy_local_files(root, path), reused: None, kept })
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

/// De quem é o que a cópia `path` tem, quando nenhum envio a gravou: o nome
/// da obra (`spec`, ou `copy` sem obra) e o nome da pasta dela.
fn named_keeping(spec: &str, path: &Path) -> Keeping {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let owner = if spec.is_empty() { "copy" } else { spec };
    Keeping { label: format!("{owner}/{name}"), wave: None }
}

/// Guarda o código de `slot` além do commit atual ([`keep_copy_code`]) e só
/// então a tira ([`remove_copy`]): a cópia que não pôde guardar fica como
/// está, em `removal.unkept`, com o motivo. O que guardou vai em
/// `removal.kept`, e a que o git não deixou sair, em `removal.left`.
fn remove_keeping(root: &Path, slot: &Path, keeping: &Keeping, removal: &mut Removal) {
    match super::keep::keep_copy_code(root, slot, keeping) {
        Ok(kept) => removal.kept.extend(kept),
        Err(detail) => {
            removal.unkept.push((shown(slot), detail));
            return;
        }
    }
    if let Err(detail) = remove_copy(root, slot) {
        removal.left.push((shown(slot), detail));
    }
}

/// Tira a cópia solta `path` — de um lugar antigo, sem a pasta da obra em
/// volta — guardando antes o que ela tem além do commit, como
/// [`remove_spec_copies`]. `spec` é a obra dela, quando se sabe.
pub(crate) fn remove_single_copy(root: &Path, path: &Path, spec: &str) -> Removal {
    let mut removal = Removal::default();
    if !inside_copies(root, path) {
        removal.left.push((shown(path), format!("not a copy folder: {}", shown(path))));
        return removal;
    }
    remove_keeping(root, path, &named_keeping(spec, path), &mut removal);
    removal
}

/// O que [`remove_spec_copies`] fez: as cópias que não saíram e o código que
/// guardou antes de apagar.
#[derive(Debug, Default)]
pub(crate) struct Removal {
    /// Cada cópia que não saiu, com o motivo que o git deu.
    pub(crate) left: Vec<(String, String)>,
    /// Cada cópia que ficou intacta porque o código que ela tem além do
    /// commit não pôde ser guardado antes, com o motivo.
    pub(crate) unkept: Vec<(String, String)>,
    /// O código que a remoção guardou antes de apagar, cópia por cópia.
    pub(crate) kept: Vec<Kept>,
}

/// Tira todas as cópias da obra `spec` — cada vaga, com as cópias dos
/// submódulos dentro dela ([`remove_copy`]) — e apaga a pasta dela
/// ([`spec_copies_dir`]), com a trava do passo do git presa. Chamado pelo
/// fechamento, pelo descarte e por `mustard-rt run clean`. Antes de apagar
/// cada vaga, guarda o que ela tem além do commit atual ([`keep_copy_code`],
/// com o dono lido de `log` quando há): a vaga que não pôde guardar não é
/// apagada, e o motivo vai em `unkept`. Devolve cada cópia que não saiu, com
/// o motivo, e o que guardou; a pasta principal e a compilação dela nunca
/// entram.
pub(crate) fn remove_spec_copies(root: &Path, spec: &str, log: Option<&SpecLog>) -> Removal {
    let dir = spec_copies_dir(root, spec);
    let single = Path::new(spec).components().count() == 1
        && matches!(Path::new(spec).components().next(), Some(std::path::Component::Normal(_)));
    if !single || dir.parent() != Some(copies_dir(root).as_path()) {
        return Removal { left: vec![(shown(&dir), format!("not a copy folder: {}", shown(&dir)))], ..Removal::default() };
    }
    if !dir.exists() {
        return Removal::default();
    }
    let _held = match crate::commands::git_settle::git_step_lock(root) {
        Ok(held) => held,
        Err(detail) => return Removal { left: vec![(shown(&dir), detail)], ..Removal::default() },
    };
    let mut slots: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect())
        .unwrap_or_default();
    slots.sort();
    let mut removal = Removal::default();
    for slot in &slots {
        let keeping = match log {
            Some(log) => slot_owner(spec, log, slot),
            None => named_keeping(spec, slot),
        };
        remove_keeping(root, slot, &keeping, &mut removal);
    }
    if removal.left.is_empty()
        && removal.unkept.is_empty()
        && dir.exists()
        && let Err(err) = std::fs::remove_dir_all(&dir)
    {
        removal.left.push((shown(&dir), err.to_string()));
    }
    removal
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

    /// A volta da onda 1 com `files` entregues e a rodada que a comita.
    fn deliver_and_commit(root: &Path, files: &[&str]) -> Value {
        let done = json!({"wave": 1, "text": "A onda 1 saiu.", "files": files, "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, done)["ok"], json!(true));
        let taken = round(root, "x", None);
        assert_eq!(taken["ok"], json!(true), "{taken}");
        taken
    }

    /// Depois do commit da onda, a vaga volta limpa por inteiro: a cópia do
    /// submódulo de dentro dela também volta ao commit em que nasceu, e o que
    /// a onda mudou nele, que já está no commit do submódulo do projeto, não
    /// fica solto na vaga.
    #[test]
    fn the_submodule_copy_inside_a_slot_goes_back_clean_after_the_commit() {
        let dir = tempdir().unwrap();
        let root = &dir.path().join("principal");
        with_submodule(root, dir.path());
        approved(root, "x", &[(1, &["libs/sub/lib.txt"], &[])]);
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let inside = PathBuf::from(sent_copy(root, 1)).join("libs/sub");
        assert!(inside.join(".git").is_file(), "the slot carries the copy of the submodule: {out}");
        std::fs::write(inside.join("lib.txt"), "fn um() {}\n// onda 1\n").unwrap();

        let taken = deliver_and_commit(root, &["libs/sub/lib.txt"]);

        let kept = std::fs::read_to_string(root.join("libs/sub/lib.txt")).unwrap();
        assert_eq!(kept, "fn um() {}\n// onda 1\n", "the code is in the submodule of the project: {taken}");
        assert_eq!(git_text(&inside, &["status", "--porcelain", "--untracked-files=all"]), "", "{taken}");
        assert_eq!(std::fs::read_to_string(inside.join("lib.txt")).unwrap(), "fn um() {}\n", "{taken}");
    }

    /// Depois do commit da onda, os arquivos locais do projeto voltam à cópia
    /// com o conteúdo de agora, como na volta de uma vaga ao commit: o que a
    /// onda apagou ou deixou velho na vaga chega de novo.
    #[test]
    fn the_local_files_come_back_to_a_slot_wiped_after_the_commit() {
        let dir = tempdir().unwrap();
        let root = &dir.path().join("projeto");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        std::fs::write(root.join(".env"), "SEGREDO=1\n").unwrap();
        std::fs::write(root.join("mustard.json"), json!({ "localFiles": [".env"] }).to_string()).unwrap();
        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        let copy = PathBuf::from(sent_copy(root, 1));
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=1\n", "{out}");
        std::fs::write(copy.join("src/a.rs"), "fn um() {}\n// onda 1\n").unwrap();
        std::fs::remove_file(copy.join(".env")).unwrap();
        std::fs::write(root.join(".env"), "SEGREDO=2\n").unwrap();

        let taken = deliver_and_commit(root, &["src/a.rs"]);

        assert_eq!(git_text(&copy, &["status", "--porcelain", "--untracked-files=all"]), "", "{taken}");
        assert_eq!(std::fs::read_to_string(copy.join(".env")).unwrap(), "SEGREDO=2\n", "the local file came back: {taken}");
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
            let refused = remove_spec_copies(root, spec, None).left;
            assert_eq!(refused, vec![(folder.clone(), format!("not a copy folder: {folder}"))], "{spec}");
        }
        assert_eq!(std::fs::read_to_string(slot.join("src").join("lib.rs")).unwrap(), "fn um() {}\n", "a vaga fica");
    }
}
