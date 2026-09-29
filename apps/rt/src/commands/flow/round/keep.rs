//! O código que uma cópia tem além do commit e a limpeza guarda antes de
//! zerá-la ou apagá-la: a ref do repositório principal em que ele fica, de
//! quem é, e a cópia que volta ao commit só depois de guardado. A pasta que já
//! não é cópia viva do git também é guardada, pelo repositório principal.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::wave_prompt::shown;

use super::commit::{copy_of, git, git_with, head};
use super::slots::live_copy;
use crate::commands::git_settle::submodules_of;

/// O que uma limpeza guardou antes de zerar uma cópia: a ref, no repositório
/// principal, sob a qual ficou o código que ainda não tinha ido ao commit, e
/// quais arquivos ele trazia.
#[derive(Debug, Clone)]
pub(crate) struct Kept {
    /// A onda de quem era o código, quando o envio que gravou a cópia diz qual.
    pub(crate) wave: Option<u64>,
    /// A ref que guarda o código.
    pub(crate) refname: String,
    /// Os arquivos que a cópia tinha e o commit atual não tem, como estavam.
    pub(crate) files: Vec<String>,
    /// A cópia de onde o código saiu.
    pub(crate) copy: String,
}

/// De quem é o código que uma limpeza pode guardar: o nome da ref (`label`,
/// já sem a barra do começo e sem o que o git recusa) e a onda, quando se
/// sabe.
#[derive(Debug, Clone)]
pub(crate) struct Keeping {
    pub(crate) label: String,
    pub(crate) wave: Option<u64>,
}

/// Limpa a cópia da onda órfã `wave` — um Claude Code que fechou no meio do
/// trabalho: o que ele deixou sem commitar, na cópia da onda e na cópia de
/// cada submódulo dentro dela, volta ao commit atual, sem esperar o reenvio
/// pedir isso — quem falhou no meio não deixou uma retomada em curso, deixou
/// só o resto do que não terminou. Antes, o que a cópia tem e o commit atual
/// não tem fica guardado ([`keep_unsaved`]): a cópia que não pôde guardar não
/// é limpa, e o erro diz por quê. A compilação, que o git ignora, fica. A
/// cópia que nunca existiu, ou que já não é mais um checkout ligado ao
/// repositório, não faz nada.
pub(super) fn clean_orphan_copy(root: &Path, log: &SpecLog, spec: &str, wave: u64) -> Result<Vec<Kept>, String> {
    let Some(copy) = copy_of(log, wave) else { return Ok(Vec::new()) };
    let head = head(root);
    if !copy.join(".git").is_file() || head.is_empty() {
        return Ok(Vec::new());
    }
    let sent = log.last_by_wave("send").get(&wave).copied().unwrap_or_default();
    reset_with_submodules(root, &copy, &head, &Keeping { label: format!("{spec}/{wave}-{sent}"), wave: Some(wave) })
}

/// Cada cópia que uma limpeza mexe, com o commit a que ela volta e de quem é
/// o que ela guarda: a própria `copy`, no commit `head`, e cada cópia de
/// submódulo dentro dela, no commit do submódulo no checkout `root`.
fn copy_targets(root: &Path, copy: &Path, head: &str, keeping: &Keeping) -> Vec<(PathBuf, String, Keeping)> {
    let mut targets: Vec<(PathBuf, String, Keeping)> = vec![(copy.to_path_buf(), head.to_string(), keeping.clone())];
    for sub in submodules_of(root).iter().filter(|sub| copy.join(sub).join(".git").is_file()) {
        let inside = Keeping { label: format!("{}-{}", keeping.label, sub.replace('/', "_")), wave: keeping.wave };
        targets.push((copy.join(sub), self::head(&root.join(sub)), inside));
    }
    targets
}

/// Guarda o que cada cópia de `targets` tem e o commit dela não tem
/// ([`keep_unsaved`]), sem mexer em nenhuma. Com `strict`, a cópia que não é
/// mais um checkout ligado ao git é erro; sem ele, a que o git não lê mais
/// não tem o que guardar, e fica de fora.
fn keep_targets(targets: &[(PathBuf, String, Keeping)], strict: bool) -> Result<Vec<Kept>, String> {
    let mut kept = Vec::new();
    for (dir, head, keeping) in targets {
        let linked = dir.join(".git").is_file() && !head.is_empty();
        if !linked || (!strict && git(dir, &["rev-parse", "HEAD"]).is_err()) {
            if strict {
                return Err(format!("git checkout --detach --force {head}: {}", shown(dir)));
            }
            continue;
        }
        // O refresh sai com erro quando algum arquivo mudou de fato; é o
        // checkout que o desfaz logo abaixo.
        let _ = git(dir, &["update-index", "-q", "--refresh"]);
        kept.extend(keep_unsaved(dir, head, keeping)?);
    }
    Ok(kept)
}

/// Guarda o código que a cópia `copy` — e cada cópia de submódulo dentro
/// dela — tem além do commit atual do checkout `root`, antes de a cópia ser
/// apagada. A pasta que já não é cópia viva do git ([`live_copy`]) não tem
/// commit próprio a comparar: o que ela tem de arquivo e o commit atual não
/// tem, ou tem diferente, é guardado pelo repositório principal
/// ([`keep_loose_folder`]). O erro diz por que não guardou, e a cópia não deve
/// ser apagada.
pub(super) fn keep_copy_code(root: &Path, copy: &Path, keeping: &Keeping) -> Result<Vec<Kept>, String> {
    let head = head(root);
    if !live_copy(copy) {
        return keep_loose_folder(root, copy, &head, keeping).map(|kept| kept.into_iter().collect());
    }
    keep_targets(&copy_targets(root, copy, &head, keeping), false)
}

/// Volta a cópia `copy` ao commit `head` ([`reset_copy`]) e cada cópia de
/// submódulo dentro dela ao commit do submódulo no checkout `root`. Antes de
/// zerar qualquer uma, guarda o que ela tem e o commit não tem: se uma não
/// pôde guardar, nenhuma é zerada, e o erro diz por quê. Devolve o que ficou
/// guardado.
pub(super) fn reset_with_submodules(
    root: &Path,
    copy: &Path,
    head: &str,
    keeping: &Keeping,
) -> Result<Vec<Kept>, String> {
    let targets = copy_targets(root, copy, head, keeping);
    let kept = keep_targets(&targets, true)?;
    let mut failed = Vec::new();
    for (dir, head, _) in &targets {
        if let Err(detail) = reset_copy(dir, head) {
            failed.push(format!("{}: {detail}", shown(dir)));
        }
    }
    if failed.is_empty() { Ok(kept) } else { Err(failed.join("; ")) }
}

/// Volta o checkout ligado `dir` ao commit `head`, descartando qualquer
/// mudança sem commitar e qualquer arquivo novo que o git não ignora. O que
/// ele ignora — a compilação, as dependências instaladas — fica, e é por
/// isso que a vaga não compila do zero. O que se perderia já foi guardado
/// por quem chama ([`keep_unsaved`]).
fn reset_copy(dir: &Path, head: &str) -> Result<(), String> {
    git(dir, &["checkout", "--detach", "--force", head])?;
    git(dir, &["clean", "-fd"]).map(|_| ())
}

/// Guarda o que a cópia `dir` tem e o commit `head` não tem, antes de ela ser
/// zerada: o que mudou em arquivo versionado, o arquivo novo que o git não
/// ignora e o commit que só a cópia fez. O que o commit já traz — a entrega
/// que a rodada juntou e comitou, e que a cópia guarda de uma onda que já
/// terminou — não conta: só o arquivo que o agente mudou na cópia e que o
/// commit atual tem diferente. Tudo vai num commit solto, sob uma ref do
/// repositório principal que as cópias dividem, e o código volta com
/// `git cherry-pick --no-commit <ref>`. `None` quando não há nada a guardar; o
/// erro é o motivo de o git não ter guardado, e quem chama não zera a cópia.
fn keep_unsaved(dir: &Path, head: &str, keeping: &Keeping) -> Result<Option<Kept>, String> {
    let at = git(dir, &["rev-parse", "HEAD"])?.trim().to_string();
    let clean = git(dir, &["status", "--porcelain", "--untracked-files=all"])?.trim().is_empty();
    if clean && (at == head || git(dir, &["merge-base", "--is-ancestor", &at, head]).is_ok()) {
        return Ok(None);
    }
    git(dir, &["add", "-A"])?;
    let tree = git(dir, &["write-tree"])?.trim().to_string();
    let names = |from: &str| -> Result<BTreeSet<String>, String> {
        let listed = git(dir, &["diff", "--name-only", "-z", "--no-renames", from, &tree])?;
        Ok(listed.split('\0').filter(|name| !name.is_empty()).map(str::to_string).collect())
    };
    let differs = names(head)?;
    let lost: BTreeSet<String> = match git(dir, &["merge-base", &at, head]) {
        Ok(base) => differs.intersection(&names(base.trim())?).cloned().collect(),
        Err(_) => differs,
    };
    if lost.is_empty() {
        return Ok(None);
    }
    store_tree(dir, dir, &tree, &at, lost, keeping).map(Some)
}

/// Guarda a árvore `tree` num commit solto, com `parent` por pai, sob a ref
/// de `keeping` no repositório principal que as cópias dividem. O git roda em
/// `at`; `copy` é a pasta de onde o código saiu. `files` são os arquivos que
/// a ref traz além do commit atual.
fn store_tree(
    at: &Path,
    copy: &Path,
    tree: &str,
    parent: &str,
    files: BTreeSet<String>,
    keeping: &Keeping,
) -> Result<Kept, String> {
    let refname = format!("refs/mustard/kept/{}-{}", keeping.label, &tree[..tree.len().min(8)]);
    let said = format!("Código guardado antes de zerar a cópia {}", shown(copy));
    let commit = git(
        at,
        &["-c", "user.name=Mustard", "-c", "user.email=mustard@localhost", "-c", "commit.gpgsign=false",
            "commit-tree", tree, "-p", parent, "-m", &said],
    )?;
    git(at, &["update-ref", &refname, commit.trim()])?;
    Ok(Kept { wave: keeping.wave, refname, files: files.into_iter().collect(), copy: shown(copy) })
}

/// A pasta `folder` tem algum arquivo, em qualquer fundo, fora o `.git` do
/// topo — que sozinho não guarda código. Não segue link.
fn has_files(folder: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else { return false };
    entries.flatten().any(|entry| match entry.file_type() {
        Ok(kind) if kind.is_dir() => has_files(&entry.path()),
        Ok(_) => true,
        Err(_) => true,
    })
}

/// Guarda o que a pasta `folder` tem e o commit `head` do checkout `root` não
/// tem, quando ela já não é uma cópia viva do git (a pasta que sobrou de um
/// processo que caiu, ou cujo registro o git esqueceu): sem commit próprio
/// para comparar, vale o arquivo que ela tem e o commit `head` não tem ou tem
/// diferente. O repositório principal lê a pasta como área de trabalho
/// (`--work-tree`) num índice temporário, sem tocar o dele, e o resultado vai
/// para a mesma ref das outras limpezas ([`store_tree`]). O que o git ignora
/// não entra. `None` quando a pasta não existe ou não tem nada a guardar; o
/// erro é o motivo de o git não ter guardado, e quem chama não apaga a pasta.
fn keep_loose_folder(root: &Path, folder: &Path, head: &str, keeping: &Keeping) -> Result<Option<Kept>, String> {
    if !has_files(folder) {
        return Ok(None);
    }
    if head.is_empty() {
        return Err(format!("git rev-parse HEAD: {}", shown(root)));
    }
    let gitdir = git(root, &["rev-parse", "--absolute-git-dir"])?;
    let index = PathBuf::from(gitdir.trim()).join(format!("mustard-kept-index-{}", std::process::id()));
    let result = keep_folder_through(root, folder, head, keeping, &index);
    let _ = std::fs::remove_file(&index);
    let _ = std::fs::remove_file(index.with_extension("lock"));
    result
}

/// O trabalho de [`keep_loose_folder`], com o índice temporário em `index`.
fn keep_folder_through(
    root: &Path,
    folder: &Path,
    head: &str,
    keeping: &Keeping,
    index: &Path,
) -> Result<Option<Kept>, String> {
    let work = format!("--work-tree={}", folder.to_string_lossy());
    let env = [("GIT_INDEX_FILE", index.to_string_lossy().into_owned())];
    let on_folder = |args: &[&str]| -> Result<String, String> {
        let mut all = vec![work.as_str()];
        all.extend_from_slice(args);
        git_with(root, &all, &env)
    };
    on_folder(&["add", "-A"])?;
    let tree = on_folder(&["write-tree"])?.trim().to_string();
    let listed = git(root, &["diff", "--name-only", "-z", "--no-renames", "--diff-filter=AM", head, &tree])?;
    let files: BTreeSet<String> = listed.split('\0').filter(|name| !name.is_empty()).map(str::to_string).collect();
    if files.is_empty() {
        return Ok(None);
    }
    store_tree(root, folder, &tree, head, files, keeping).map(Some)
}
