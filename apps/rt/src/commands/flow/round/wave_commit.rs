//! O commit de cada onda: um por onda aceita, em sequência sobre o HEAD, na
//! ordem da junção, cada um montado num índice à parte a partir do HEAD, e a
//! gravação de cada um na spec, com a onda dele.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::io::fs::lock::LockedFile;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::answer::RoundRefusal;
use super::commit::{ForCommit, Joined, RoundRepos, commit_draft, git, git_with, head, record_commit, round_repos, write_joined};
use super::commit_title::WaveCommit;
use crate::commands::git_settle::enter_unit_branch;

/// Um commit feito pela rodada: o do principal e o de cada submódulo, pelo
/// caminho dele, com o título, as ondas que ele cobre e os arquivos que
/// leva, separados por repositório.
pub(super) struct Made {
    pub(super) sha: String,
    pub(super) subs: Vec<(String, String)>,
    pub(super) title: String,
    pub(super) waves: Vec<u64>,
    pub(super) files: Vec<String>,
    pub(super) repos: RoundRepos,
}

/// As ondas cujo commit o git recusou, cada uma com o motivo dele.
pub(super) type GitRefused = Vec<(u64, String)>;

/// O código de um commit e o de cada submódulo, pelo caminho dele.
type Shas = (String, Vec<(String, String)>);

/// Faz o commit de cada onda de `planned`, um por onda, em sequência sobre o
/// HEAD, na ordem da junção, com a trava do passo do git que quem chama já
/// prendeu (`held`). Não grava nada na spec. Devolve os commits feitos e as
/// ondas que o git recusou, cada uma com o motivo dele.
///
/// O commit de cada onda leva só os arquivos dela ([`make_commit`]). O
/// arquivo que uma onda seguinte também muda vai no commit desta com o que
/// ele era depois dela, pelo passo dela na junção (`steps`), e não com o
/// disco, que já tem as duas: as ondas no mesmo arquivo comitam na ordem da
/// junção, cada uma com a sua mudança.
///
/// A recusa do git segura só a onda dela, e as que vêm depois e mexem num
/// arquivo dela: nenhum commit leva nada da recusada. Cada arquivo dessas
/// ondas volta, no disco, ao que era antes delas — desfeitas da última para
/// a primeira. A onda cujo commit não teria o que levar — o código dela já
/// está num commit desta mesma rodada — entra no commit anterior, sem commit
/// novo; sem commit anterior, o git diz por quê.
pub(super) fn commit_waves(
    root: &Path,
    held: &LockedFile,
    unit: &str,
    planned: &[WaveCommit],
    steps: &[(u64, Vec<Joined>)],
) -> Result<(Vec<Made>, GitRefused), RoundRefusal> {
    let mut made: Vec<Made> = Vec::new();
    let mut refused: Vec<(u64, String)> = Vec::new();
    let mut tainted: BTreeMap<String, String> = BTreeMap::new();
    let step_of = |wave: u64| steps.iter().find(|(n, _)| *n == wave).map_or(&[][..], |(_, step)| step.as_slice());
    for (at, one) in planned.iter().enumerate() {
        if let Some(detail) = one.files.iter().find_map(|file| tainted.get(file)).cloned() {
            tainted.extend(one.files.iter().map(|file| (file.clone(), detail.clone())));
            refused.push((one.wave, detail));
            continue;
        }
        let later = |file: &str| planned[at + 1..].iter().any(|next| next.files.iter().any(|f| f == file));
        let mut step: Vec<Joined> = step_of(one.wave)
            .iter()
            .map(|change| {
                let commit = match &change.commit {
                    ForCommit::Disk if later(&change.file) => ForCommit::Content(change.after.clone()),
                    other => other.clone(),
                };
                Joined { file: change.file.clone(), before: change.before.clone(), after: change.after.clone(), commit }
            })
            .collect();
        // O arquivo desta onda que a junção não mudou — o da onda sem cópia,
        // que já está no disco — e que a junção de uma onda seguinte muda vai
        // com o que era antes dela.
        let untouched: Vec<&String> = one.files.iter().filter(|file| step.iter().all(|change| &change.file != *file)).collect();
        for file in untouched {
            let ahead = planned[at + 1..].iter().find_map(|next| step_of(next.wave).iter().find(|change| &change.file == file));
            if let Some(ahead) = ahead.filter(|ahead| matches!(ahead.commit, ForCommit::Disk)) {
                let before = ahead.before.clone();
                step.push(Joined { file: file.clone(), before: before.clone(), after: before.clone(), commit: ForCommit::Content(before) });
            }
        }
        let repos = round_repos(root, &one.files);
        let message = (one.message.title.as_str(), one.message.body.as_str());
        match make_commit(root, held, unit, message, &repos, &step, !made.is_empty()) {
            Ok(Some((sha, subs))) => made.push(Made {
                sha,
                subs,
                title: one.message.title.clone(),
                waves: one.covered.clone(),
                files: one.files.clone(),
                repos,
            }),
            Ok(None) => {
                if let Some(last) = made.last_mut() {
                    last.waves.extend(one.covered.iter().filter(|wave| !last.waves.contains(wave)).copied().collect::<Vec<_>>());
                    last.files.extend(one.files.iter().filter(|file| !last.files.contains(file)).cloned().collect::<Vec<_>>());
                }
            }
            Err(RoundRefusal::Git { detail }) => {
                tainted.extend(one.files.iter().map(|file| (file.clone(), detail.clone())));
                refused.push((one.wave, detail));
            }
            Err(refusal) => return Err(refusal),
        }
    }
    for (wave, _) in refused.iter().rev() {
        write_joined(root, step_of(*wave), false)?;
    }
    Ok((made, refused))
}

/// Faz o commit de uma onda com a mensagem já conferida e devolve o código
/// dele e os dos submódulos. Não grava nada na spec: roda antes de qualquer
/// gravação. Roda com a trava do passo do git que quem chama já prendeu
/// antes da junção (`_held`), e não a pega de novo.
///
/// O commit leva só os arquivos da onda, sobre o HEAD, montado num índice
/// à parte ([`commit_paths`]), e nada do que estiver preparado fora deles. O
/// conteúdo é o do disco, salvo o do arquivo que vai com outro conteúdo
/// ([`Joined::commit`]): o que tinha edição sem commit antes da junção, que
/// vai sem ela — a edição fica só no disco —, e o que uma onda seguinte
/// também muda. O preparo do checkout só muda quando o commit sai: a rodada
/// que morre no meio dele não deixa nada preparado. O arquivo de dentro de um
/// submódulo é comitado no submódulo, na branch `unit` da spec, criada na
/// primeira vez sobre a base dele, e o commit do principal leva o ponteiro
/// novo junto com os arquivos dele. Quando o git recusa, o bloco inteiro é
/// desfeito ali mesmo, dentro da trava: o commit já feito num submódulo
/// volta, o registro dos arquivos novos sai do índice e cada arquivo que a
/// junção mudou (`joined`) volta ao que era no disco. Com `may_skip`, o
/// commit que não teria o que levar não é feito, e a resposta é `None`.
fn make_commit(
    root: &Path,
    _held: &LockedFile,
    unit: &str,
    message: (&str, &str),
    repos: &RoundRepos,
    joined: &[Joined],
    may_skip: bool,
) -> Result<Option<Shas>, RoundRefusal> {
    let mut done: Vec<SubCommit> = Vec::new();
    let pinned: BTreeMap<&str, Option<&[u8]>> = joined
        .iter()
        .filter_map(|one| match &one.commit {
            ForCommit::Content(content) => Some((one.file.as_str(), content.as_deref())),
            ForCommit::Disk => None,
        })
        .collect();
    let made = commit_repos(root, unit, message, repos, &pinned, may_skip, &mut done);
    if made.is_err() {
        for one in done.iter().rev() {
            let dir = root.join(&one.sub);
            let _ = git(&dir, &["reset", "-q", "--soft", &one.before]);
            let mut undo: Vec<&str> = vec!["reset", "-q", &one.before, "--"];
            undo.extend(one.inner.iter().map(String::as_str));
            let _ = git(&dir, &undo);
        }
        write_joined(root, joined, false)?;
    }
    made.map(|sha| sha.map(|sha| (sha, done.into_iter().map(|one| (one.sub, one.sha)).collect())))
}

/// O commit feito num submódulo: onde, o commit em que ele estava, os
/// arquivos dele e o código do novo.
struct SubCommit {
    sub: String,
    before: String,
    inner: Vec<String>,
    sha: String,
}

/// Os commits de [`make_commit`], primeiro os dos submódulos, em `done`, e
/// por último o do principal, que leva o ponteiro de cada um. `pinned` é o
/// conteúdo que o commit leva no lugar do disco, pelo caminho a partir do
/// principal. Com `may_skip`, o repositório que não teria o que levar fica
/// sem commit, e o principal sem nada responde `None`.
fn commit_repos(
    root: &Path,
    unit: &str,
    (title, body): (&str, &str),
    repos: &RoundRepos,
    pinned: &BTreeMap<&str, Option<&[u8]>>,
    may_skip: bool,
    done: &mut Vec<SubCommit>,
) -> Result<Option<String>, RoundRefusal> {
    let mut own = repos.own.clone();
    for (sub, files) in &repos.subs {
        let dir = root.join(sub);
        enter_unit_branch(&dir, unit).map_err(|detail| RoundRefusal::Git { detail })?;
        let prefix = format!("{sub}/");
        let inner: Vec<String> = files.iter().filter_map(|file| file.strip_prefix(&prefix).map(str::to_string)).collect();
        let inner_pins: BTreeMap<&str, Option<&[u8]>> = pinned.iter().filter_map(|(file, content)| file.strip_prefix(&prefix).map(|inner| (inner, *content))).collect();
        let before = head(&dir);
        if let Some(sha) = commit_paths(&dir, title, body, &inner, &inner_pins, may_skip)? {
            done.push(SubCommit { sub: sub.clone(), before, inner, sha });
            own.push(sub.clone());
        }
    }
    commit_paths(root, title, body, &own, pinned, may_skip)
}

/// O commit de [`make_commit`] no repositório `root`, feito num índice à
/// parte que parte do HEAD: cada arquivo de `files` entra com o conteúdo do
/// disco, ou com o de `pinned` quando está lá, e nada mais do índice do
/// checkout vai junto. O apagado que o git nunca conheceu fica de fora,
/// porque não há o que levar dele; o que só saiu do disco, ou já saiu do
/// índice, vai como remoção. Os ganchos do commit rodam como num commit
/// comum. Com o commit feito, o índice do checkout passa a ter o que o commit
/// levou nesses arquivos; com a recusa, nada no checkout muda. Com
/// `may_skip`, o índice que fica igual ao HEAD não vira commit: a resposta é
/// `None`, e nada no checkout muda.
fn commit_paths(
    root: &Path,
    title: &str,
    body: &str,
    files: &[String],
    pinned: &BTreeMap<&str, Option<&[u8]>>,
    may_skip: bool,
) -> Result<Option<String>, RoundRefusal> {
    let refused = |detail: String| RoundRefusal::Git { detail };
    let scratch = tempfile::tempdir().map_err(|e| refused(e.to_string()))?;
    let index = scratch.path().join("index");
    let env = [("GIT_INDEX_FILE", index.to_string_lossy().to_string())];
    let start = if git(root, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok() { "HEAD" } else { "--empty" };
    git_with(root, &["read-tree", start], &env).map_err(refused)?;
    let from_disk: Vec<&str> = files.iter().map(String::as_str).filter(|file| !pinned.contains_key(file)).collect();
    if !from_disk.is_empty() {
        let mut add: Vec<&str> = vec!["update-index", "--add", "--remove", "--"];
        add.extend(&from_disk);
        git_with(root, &add, &env).map_err(refused)?;
    }
    for (file, content) in pinned.iter().filter(|(file, _)| files.iter().any(|one| one == *file)) {
        match content {
            Some(bytes) => {
                let held = scratch.path().join("conteudo");
                std::fs::write(&held, bytes).map_err(|e| refused(e.to_string()))?;
                let id = git(root, &["hash-object", "-w", "--path", file, &held.to_string_lossy()]).map_err(refused)?;
                let staged = git_with(root, &["ls-files", "-s", "--", file], &env).unwrap_or_default();
                let mode = staged.split_whitespace().next().filter(|mode| !mode.is_empty()).unwrap_or("100644").to_string();
                let entry = format!("{mode},{},{file}", id.trim());
                git_with(root, &["update-index", "--add", "--cacheinfo", &entry], &env).map_err(refused)?;
            }
            None => {
                git_with(root, &["update-index", "--force-remove", "--", file], &env).map_err(refused)?;
            }
        }
    }
    if may_skip && start == "HEAD" && git_with(root, &["diff-index", "--cached", "--quiet", "HEAD", "--"], &env).is_ok() {
        return Ok(None);
    }
    let mut args: Vec<&str> = vec!["commit", "-m", title];
    if !body.is_empty() {
        args.push("-m");
        args.push(body);
    }
    git_with(root, &args, &env).map_err(refused)?;
    let sha = git(root, &["rev-parse", "HEAD"]).map_err(refused)?;
    let mut synced: Vec<&str> = vec!["reset", "-q", "--"];
    synced.extend(files.iter().map(String::as_str));
    let _ = git(root, &synced);
    Ok(Some(sha.trim().to_string()))
}


/// Os eventos de cada commit de `made`, como a spec os grava, o de cada
/// submódulo como o do principal: é o que a conferência da gravação ensaia
/// quando parte das ondas ficou de fora depois dos commits.
pub(super) fn made_drafts(root: &Path, made: &[Made]) -> Vec<Map<String, Value>> {
    let mut drafts = Vec::new();
    for one in made {
        drafts.push(commit_draft(root, &one.sha, &one.title, &one.waves, &one.files));
        drafts.extend(one.subs.iter().map(|(sub, sha)| {
            let own = one.repos.subs.get(sub).map(Vec::as_slice).unwrap_or_default();
            commit_draft(&root.join(sub), sha, &one.title, &one.waves, own)
        }));
    }
    drafts
}

/// Grava na spec cada commit de `made`, com as ondas dele, na ordem em que
/// saíram, e o de cada submódulo como o do principal. Devolve o que a
/// resposta da rodada diz do commit — o último e, quando a rodada fez mais de
/// um, a lista de todos em `commits` — e os arquivos que eles levaram.
pub(super) fn record_made(start: &Path, root: &Path, spec: &str, made: &[Made]) -> Result<(Option<Value>, Vec<String>), RoundRefusal> {
    let mut commit: Option<Value> = None;
    let mut listed: Vec<Value> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for one in made {
        let mut entry = record_commit(start, root, spec, &one.sha, &one.title, &one.waves, &one.files)?;
        let mut subs: Vec<Value> = Vec::new();
        for (sub, sha) in &one.subs {
            let own = one.repos.subs.get(sub).map(Vec::as_slice).unwrap_or_default();
            record_commit(start, &root.join(sub), spec, sha, &one.title, &one.waves, own)?;
            subs.push(json!({ "path": sub, "sha": sha }));
        }
        if !subs.is_empty() {
            entry["submodules"] = json!(subs);
        }
        listed.push(json!({ "sha": one.sha, "title": one.title, "waves": one.waves }));
        files.extend(one.files.iter().filter(|file| !files.contains(file)).cloned().collect::<Vec<_>>());
        commit = Some(entry);
    }
    if let Some(last) = commit.as_mut().filter(|_| listed.len() > 1) {
        last["commits"] = json!(listed);
    }
    Ok((commit, files))
}

/// O aviso de cada onda comitada de cujo resumo saiu o que um commit nunca
/// leva: o que saiu e o título com que o commit saiu.
pub(super) fn removed_warnings(commits: &[WaveCommit], made: &[Made], lang: Locale) -> Vec<Value> {
    commits
        .iter()
        .filter(|one| !one.message.removed.is_empty() && made.iter().any(|done| done.waves.contains(&one.wave)))
        .map(|one| {
            let hint = translate("round.commit_text_removed", lang)
                .replace("{wave}", &one.wave.to_string())
                .replace("{removed}", &one.message.removed.join(" "))
                .replace("{title}", &one.message.title);
            json!({ "reason": "commit-text-removed", "wave": one.wave, "hint": hint })
        })
        .collect()
}
