//! O código que uma cópia tem além do commit e a limpeza guarda antes de
//! zerá-la ou apagá-la: a ref do repositório principal em que ele fica, de
//! quem é, e a cópia que volta ao commit só depois de guardado. A pasta que já
//! não é cópia viva do git também é guardada, pelo repositório principal. A
//! cópia da onda que a rodada acabou de comitar não guarda nada: o código dela
//! já está no commit.

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

/// Cada cópia que uma limpeza mexe, na ordem: a própria `copy` (sem
/// submódulo) e cada cópia de submódulo dentro dela, com o caminho do
/// submódulo no checkout `root`. É a única lista de cópias de toda limpeza,
/// a que volta ao commit de nascimento e a que guarda antes de voltar.
fn copies_inside(root: &Path, copy: &Path) -> Vec<(PathBuf, Option<String>)> {
    let subs = submodules_of(root).into_iter().filter(|sub| copy.join(sub).join(".git").is_file());
    std::iter::once((copy.to_path_buf(), None)).chain(subs.map(|sub| (copy.join(&sub), Some(sub)))).collect()
}

/// Cada cópia que uma limpeza mexe ([`copies_inside`]), com o commit a que ela
/// volta e de quem é o que ela guarda: a própria `copy`, no commit `head`, e
/// cada cópia de submódulo dentro dela, no commit do submódulo no checkout
/// `root`.
fn copy_targets(root: &Path, copy: &Path, head: &str, keeping: &Keeping) -> Vec<(PathBuf, String, Keeping)> {
    copies_inside(root, copy)
        .into_iter()
        .map(|(dir, sub)| match sub {
            None => (dir, head.to_string(), keeping.clone()),
            Some(sub) => {
                let inside = Keeping { label: format!("{}-{}", keeping.label, sub.replace('/', "_")), wave: keeping.wave };
                (dir, self::head(&root.join(&sub)), inside)
            }
        })
        .collect()
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
        refresh_index(dir);
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
    reset_each(targets.iter().map(|(dir, head, _)| (dir.as_path(), head.as_str()))).map(|()| kept)
}

/// Volta a cópia `copy` da onda que a rodada acabou de comitar, e cada cópia
/// de submódulo dentro dela, ao commit em que cada uma já estava, sem guardar
/// nada: o código delas está no commit da rodada, e guardá-lo, depois que a
/// base andou, o contaria como perdido ([`keep_unsaved`]). O commit é o de
/// nascimento, e não o novo: o que a vaga mudou desde que foi preparada é o
/// que o pedido da onda seguinte lista para decidir se prepara de novo, e
/// mudança que entrou no commit por outra onda não está no preparo dela.
pub(super) fn reset_in_place(root: &Path, copy: &Path) -> Result<(), String> {
    let dirs: Vec<PathBuf> = copies_inside(root, copy).into_iter().map(|(dir, _)| dir).collect();
    let heads: Vec<String> = dirs.iter().map(|dir| head(dir)).collect();
    for dir in &dirs {
        refresh_index(dir);
    }
    reset_each(dirs.iter().map(PathBuf::as_path).zip(heads.iter().map(String::as_str)))
}

/// Volta cada cópia de `targets`, com o commit dela, e junta o motivo de cada
/// uma que o git não deixou voltar.
fn reset_each<'a>(targets: impl Iterator<Item = (&'a Path, &'a str)>) -> Result<(), String> {
    let failed: Vec<String> = targets
        .filter_map(|(dir, head)| reset_copy(dir, head).err().map(|detail| format!("{}: {detail}", shown(dir))))
        .collect();
    if failed.is_empty() { Ok(()) } else { Err(failed.join("; ")) }
}

/// Atualiza no índice de `dir` a data que o disco tem de cada arquivo cujo
/// conteúdo não mudou. Sem isso, o `checkout --force` que volta a cópia ao
/// commit reescreve todo arquivo cuja data difere da do índice, mesmo com o
/// mesmo conteúdo, e a compilação refaz o que não mudou. O refresh sai com
/// erro quando algum arquivo mudou de fato; é o checkout que o desfaz logo
/// depois.
fn refresh_index(dir: &Path) {
    let _ = git(dir, &["update-index", "-q", "--refresh"]);
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
/// ignora e o commit que só a cópia fez. Conta como perdido o arquivo que a
/// cópia mudou e que o commit atual tem diferente, e que também difere do
/// ponto onde a cópia nasceu. Só o commit atual é conferido: a entrega que a
/// rodada juntou e comitou, depois reeditada na base, parece perdida aqui
/// mesmo estando na história. Por isso a rodada zera a cópia da onda comitada
/// logo depois do commit ([`reset_in_place`]), e a conferência só vê o código
/// que nenhum commit tem. Tudo vai num commit solto, sob uma ref do
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
    let lost = lost_files(dir, &at, head, &tree)?;
    if lost.is_empty() {
        return Ok(None);
    }
    store_tree(dir, dir, &tree, &at, lost, keeping).map(Some)
}

/// Os arquivos que a árvore `tree`, tirada de uma cópia no commit `at`, traz
/// e o commit `head` não tem: os que diferem de `head` e também do ponto onde
/// a cópia nasceu (o ancestral comum de `at` e `head`). Sem ancestral comum,
/// vale todo arquivo que difere de `head`. O git roda em `dir`.
fn lost_files(dir: &Path, at: &str, head: &str, tree: &str) -> Result<BTreeSet<String>, String> {
    let names = |from: &str| -> Result<BTreeSet<String>, String> {
        let listed = git(dir, &["diff", "--name-only", "-z", "--no-renames", from, tree])?;
        Ok(listed.split('\0').filter(|name| !name.is_empty()).map(str::to_string).collect())
    };
    let differs = names(head)?;
    Ok(match git(dir, &["merge-base", at, head]) {
        Ok(base) => differs.intersection(&names(base.trim())?).cloned().collect(),
        Err(_) => differs,
    })
}

/// O resto que um agente de onda deixou numa cópia que a rodada zerou, como o
/// reenvio da onda o cita: a ref que o guarda, a pasta da cópia em que ela
/// vale — a do submódulo, ou nenhuma para a própria cópia — e os arquivos que
/// ela traz e o commit atual não tem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KeptRest {
    pub(super) refname: String,
    pub(super) folder: Option<String>,
    pub(super) files: Vec<String>,
}

/// O resto guardado da onda `wave` da spec `spec` pelos envios que `sent_by`
/// aceita: cada ref `refs/mustard/kept/<spec>/<wave>-<envio>-…` — o nome que
/// a limpeza da cópia órfã ([`clean_orphan_copy`]) e a da vaga dão —, no
/// repositório do checkout `root` e no de cada submódulo dele, com os arquivos
/// que ela traz e o commit atual de cada um não tem ([`lost_files`]). A ref
/// cujo código o commit atual já tem fica de fora. A mais nova vem primeiro.
pub(super) fn kept_rest(root: &Path, spec: &str, wave: u64, sent_by: impl Fn(u64) -> bool) -> Vec<KeptRest> {
    let prefix = format!("refs/mustard/kept/{spec}/{wave}-");
    let repos = std::iter::once((root.to_path_buf(), None))
        .chain(submodules_of(root).into_iter().map(|sub| (root.join(&sub), Some(sub))))
        .filter(|(dir, _)| dir.join(".git").exists());
    let mut found: Vec<(u64, KeptRest)> = Vec::new();
    for (dir, folder) in repos {
        let head = head(&dir);
        let Ok(listed) = git(&dir, &["for-each-ref", "--sort=-committerdate", "--format=%(refname)", &format!("{prefix}*")]) else { continue };
        for refname in listed.lines().filter(|name| !name.is_empty()) {
            let sent = refname.strip_prefix(&prefix).and_then(|rest| rest.split('-').next()).and_then(|n| n.parse::<u64>().ok());
            let Some(sent) = sent.filter(|sent| sent_by(*sent)) else { continue };
            let (Ok(at), Ok(tree)) = (git(&dir, &["rev-parse", &format!("{refname}^")]), git(&dir, &["rev-parse", &format!("{refname}^{{tree}}")])) else {
                continue;
            };
            let files = lost_files(&dir, at.trim(), &head, tree.trim()).unwrap_or_default();
            if !files.is_empty() {
                found.push((sent, KeptRest { refname: refname.to_string(), folder: folder.clone(), files: files.into_iter().collect() }));
            }
        }
    }
    found.sort_by_key(|(sent, _)| std::cmp::Reverse(*sent));
    found.into_iter().map(|(_, rest)| rest).collect()
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

/// As refs guardadas da spec `spec` (`refs/mustard/kept/<spec>/…`) cujo
/// código a base dela já tem, conferido pelo git: cada arquivo que a ref
/// mudou em relação ao commit de onde saiu está igual na base — na branch
/// local ou na que ela acompanha no remoto. Com `apply`, cada uma sai. Só a
/// spec entregue entra; a ref que o git não confere, ou cujo código a base
/// não tem, fica.
pub(crate) fn landed_kept(root: &Path, spec: &str, apply: bool) -> Vec<String> {
    use mustard_core::domain::spec_state::{SpecState as _, State};
    let Some(log) = crate::shared::spec_state::DiskSpecState::new(root).log(spec) else { return Vec::new() };
    let state = State::from_log(&log);
    let (Some("delivered"), Some(base)) = (state.phase, state.base.as_deref()) else { return Vec::new() };
    let bases: Vec<String> = [format!("refs/heads/{base}"), format!("{base}@{{upstream}}")]
        .iter()
        .filter_map(|name| git(root, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).ok())
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty())
        .collect();
    if bases.is_empty() {
        return Vec::new();
    }
    let listed = git(root, &["for-each-ref", "--format=%(refname)", &format!("refs/mustard/kept/{spec}/")]).unwrap_or_default();
    let in_base = |refname: &str| {
        let Ok(changed) = git(root, &["diff", "--name-only", "-z", "--no-renames", &format!("{refname}^"), refname]) else { return false };
        let files: Vec<&str> = changed.split('\0').filter(|file| !file.is_empty()).collect();
        !files.is_empty()
            && bases.iter().any(|base| {
                let mut args = vec!["--literal-pathspecs", "diff", "--quiet", base.as_str(), refname, "--"];
                args.extend(files.iter().copied());
                git(root, &args).is_ok()
            })
    };
    listed
        .lines()
        .filter(|refname| !refname.is_empty() && in_base(refname))
        .filter(|refname| !apply || git(root, &["update-ref", "-d", refname]).is_ok())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::commands::flow::round::tests::{git_at, git_text};

    /// A kept ref of `spec` holding `file` with `text` on top of `parent`.
    fn kept_ref(root: &Path, spec: &str, name: &str, parent: &str, file: &str, text: &str) -> String {
        git_at(root, &["checkout", "-q", "--detach", parent]);
        std::fs::write(root.join(file), text).unwrap();
        git_at(root, &["add", file]);
        git_at(root, &["commit", "-q", "-m", "guardado"]);
        let commit = git_text(root, &["rev-parse", "HEAD"]);
        git_at(root, &["checkout", "-q", "dev"]);
        let refname = format!("refs/mustard/kept/{spec}/{name}");
        git_at(root, &["update-ref", &refname, &commit]);
        refname
    }

    /// A project on `dev` with the spec `spec` in `phase`, based on `dev`.
    fn project(root: &Path, spec: &str, phase: &str) {
        git_at(root, &["init", "-q", "-b", "dev", "."]);
        std::fs::write(root.join(".gitignore"), ".claude/\n").unwrap();
        std::fs::write(root.join("a.txt"), "antes\n").unwrap();
        git_at(root, &["add", "-A"]);
        git_at(root, &["commit", "-q", "-m", "semente"]);
        let path = mustard_core::io::spec_events::spec_file(root, spec).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let rows = [json!({"v":1,"id":1,"type":"state","author":"binary","phase":"survey","branch":format!("feature/{spec}"),"base":"dev"}),
            json!({"v":1,"id":2,"type":"state","author":"binary","phase":phase})];
        std::fs::write(path, rows.iter().map(|row| row.to_string() + "\n").collect::<String>()).unwrap();
    }

    /// Of a delivered spec, only the kept ref whose code the base already has
    /// leaves; the one whose code the base lacks stays, and nothing leaves
    /// without `apply`, nor of a spec not yet delivered.
    #[test]
    fn only_the_kept_code_the_base_already_has_leaves_once_the_spec_is_delivered() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        project(root, "x", "delivered");
        let seed = git_text(root, &["rev-parse", "HEAD"]);
        let landed = kept_ref(root, "x", "1-3-aaaa", &seed, "a.txt", "depois\n");
        let lost = kept_ref(root, "x", "2-4-bbbb", &seed, "b.txt", "só aqui\n");
        std::fs::write(root.join("a.txt"), "depois\n").unwrap();
        git_at(root, &["commit", "-q", "-am", "a mudança entrou na base"]);

        assert_eq!(landed_kept(root, "x", false), vec![landed.clone()], "listed, not removed");
        assert_eq!(git_text(root, &["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]).lines().count(), 2);
        assert_eq!(landed_kept(root, "x", true), vec![landed]);
        assert_eq!(git_text(root, &["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]), lost);

        let open = tempfile::tempdir().unwrap();
        project(open.path(), "y", "running");
        let seed = git_text(open.path(), &["rev-parse", "HEAD"]);
        kept_ref(open.path(), "y", "1-3-aaaa", &seed, "a.txt", "depois\n");
        std::fs::write(open.path().join("a.txt"), "depois\n").unwrap();
        git_at(open.path(), &["commit", "-q", "-am", "a mudança entrou na base"]);
        assert!(landed_kept(open.path(), "y", true).is_empty(), "a spec not delivered keeps every ref");
    }
}
