//! O commit de cada onda: um por onda aceita, em sequência sobre o HEAD, na
//! ordem da junção, cada um montado num índice à parte a partir do HEAD, e a
//! gravação de cada um na spec, com a onda dele.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use super::answer::RoundRefusal;
use super::commit::{ForCommit, Joined, RoundRepos, commit_draft, git, git_with, head, round_repos, write_joined};
use super::commit_title::WaveCommit;
use crate::commands::git_settle::enter_unit_branch;
use crate::commands::spec_events::write::record;

/// Um commit feito pela rodada: o do principal e o de cada submódulo, pelo
/// caminho dele, com o título, as ondas que ele cobre — e, à parte, as que
/// entraram nele sem mudança — e os arquivos que leva, separados por
/// repositório.
pub(super) struct Made {
    pub(super) sha: String,
    pub(super) subs: Vec<(String, String)>,
    pub(super) title: String,
    pub(super) waves: Vec<u64>,
    pub(super) unchanged: Vec<u64>,
    pub(super) files: Vec<String>,
    pub(super) repos: RoundRepos,
}

/// Por que o commit de uma onda não saiu: o que o git disse e, quando foi o
/// gancho do projeto que barrou, o nome dele e os arquivos do commit que a
/// saída dele citou.
///
/// A recusa chega ao usuário montada por quem a mostra, e é por isso que o
/// motivo viaja assim: quem descobriu que havia um gancho é quem comitou, e
/// essa informação se perdia no caminho até a resposta.
#[derive(Debug, Clone)]
pub(super) struct GitHold {
    pub(super) detail: String,
    pub(super) hook: Option<String>,
    pub(super) files: Vec<String>,
}

impl GitHold {
    /// A recusa que esta parada é: a do gancho do projeto, quando havia um, e
    /// a do git nos outros casos.
    pub(super) fn refusal(&self) -> RoundRefusal {
        match &self.hook {
            Some(hook) => RoundRefusal::ProjectHook { hook: hook.clone(), detail: self.detail.clone(), files: self.files.clone() },
            None => RoundRefusal::Git { detail: self.detail.clone() },
        }
    }

    /// A parada de uma onda que ficou de fora pelo arquivo de outra: o motivo
    /// é o da que barrou, e o gancho não é dela.
    fn tainted(detail: String) -> Self {
        Self { detail, hook: None, files: Vec::new() }
    }
}

/// O que a recusa do gancho `hook` diz ao usuário: a barreira é do projeto, os
/// arquivos do commit que a saída dele citou — ou que ele não citou nenhum — e
/// o que ele disse, inteiro.
pub(super) fn project_hook_said(hook: &str, detail: &str, files: &[String], lang: Locale) -> String {
    let named = if files.is_empty() { translate("round.project_hook_no_file", lang).to_string() } else { files.join(", ") };
    translate("round.project_hook_refused", lang).replace("{hook}", hook).replace("{files}", &named).replace("{detail}", detail)
}

/// As ondas cujo commit não saiu, cada uma com o motivo.
pub(super) type GitRefused = Vec<(u64, GitHold)>;

/// O nome do gancho que o projeto roda antes de cada commit.
const PRE_COMMIT: &str = "pre-commit";

/// O gancho de pre-commit que o projeto `root` roda, quando existe: o nome
/// dele, do caminho que o git usaria — `core.hooksPath`, quando o projeto o
/// declara, e `.git/hooks` quando não.
///
/// É um fato lido do disco, não um palpite sobre o texto do git: o git não diz
/// que foi um gancho que barrou o commit, e o texto que o usuário vê é o do
/// gancho, que cada projeto escreve como quer.
fn pre_commit_hook(root: &Path) -> Option<String> {
    let declared = git(root, &["config", "--get", "core.hooksPath"]).ok().map(|path| path.trim().to_string()).filter(|path| !path.is_empty());
    let dir = match declared {
        Some(path) if Path::new(&path).is_absolute() => std::path::PathBuf::from(path),
        Some(path) => root.join(path),
        None => {
            let git_dir = git(root, &["rev-parse", "--git-dir"]).ok()?.trim().to_string();
            let git_dir = if Path::new(&git_dir).is_absolute() { std::path::PathBuf::from(git_dir) } else { root.join(git_dir) };
            git_dir.join("hooks")
        }
    };
    let hook = dir.join(PRE_COMMIT);
    hook.is_file().then(|| PRE_COMMIT.to_string())
}

/// A recusa do commit em `root` que o git negou dizendo `said`: a do gancho do
/// projeto, quando o projeto tem um, nomeando os arquivos do commit que a
/// saída dele citou; a do git nos outros casos.
fn commit_refused(root: &Path, files: &[String], said: String) -> RoundRefusal {
    match pre_commit_hook(root) {
        Some(hook) => {
            let named = files.iter().filter(|file| said.contains(file.as_str())).cloned().collect();
            RoundRefusal::ProjectHook { hook, detail: said, files: named }
        }
        None => RoundRefusal::Git { detail: said },
    }
}

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
/// novo, à parte das que escreveram nele: o commit fecha a entrega dela, mas
/// a falha numa linha dele não a implica. Sem commit anterior, o git diz por
/// quê.
pub(super) fn commit_waves(
    root: &Path,
    held: &LockedFile,
    unit: &str,
    planned: &[WaveCommit],
    steps: &[(u64, Vec<Joined>)],
) -> Result<(Vec<Made>, GitRefused), RoundRefusal> {
    let mut made: Vec<Made> = Vec::new();
    let mut refused: GitRefused = Vec::new();
    let mut tainted: BTreeMap<String, String> = BTreeMap::new();
    let step_of = |wave: u64| steps.iter().find(|(n, _)| *n == wave).map_or(&[][..], |(_, step)| step.as_slice());
    for (at, one) in planned.iter().enumerate() {
        if let Some(detail) = one.files.iter().find_map(|file| tainted.get(file)).cloned() {
            tainted.extend(one.files.iter().map(|file| (file.clone(), detail.clone())));
            refused.push((one.wave, GitHold::tainted(detail)));
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
                unchanged: Vec::new(),
                files: one.files.clone(),
                repos,
            }),
            Ok(None) => {
                if let Some(last) = made.last_mut() {
                    let joined: Vec<u64> = one.covered.iter().filter(|wave| !last.waves.contains(wave)).copied().collect();
                    last.waves.extend(&joined);
                    last.unchanged.extend(joined);
                    last.files.extend(one.files.iter().filter(|file| !last.files.contains(file)).cloned().collect::<Vec<_>>());
                }
            }
            Err(RoundRefusal::Git { detail }) => {
                tainted.extend(one.files.iter().map(|file| (file.clone(), detail.clone())));
                refused.push((one.wave, GitHold::tainted(detail)));
            }
            Err(RoundRefusal::ProjectHook { hook, detail, files }) => {
                tainted.extend(one.files.iter().map(|file| (file.clone(), detail.clone())));
                refused.push((one.wave, GitHold { detail, hook: Some(hook), files }));
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
    // O commit é o único passo que o projeto pode barrar: é nele que o gancho
    // de pre-commit roda, e a recusa dele não é uma recusa do git.
    if let Err(said) = git_with(root, &args, &env) {
        return Err(commit_refused(root, files, said));
    }
    let sha = git(root, &["rev-parse", "HEAD"]).map_err(refused)?;
    // Quem pergunta é quem decide: a conferência só olha, e desfazer e recusar
    // acontece aqui.
    let extra = carried_beyond(root, files).map_err(refused)?;
    if !extra.is_empty() {
        let undo = if start == "HEAD" { vec!["reset", "--soft", "HEAD~1"] } else { vec!["update-ref", "-d", "HEAD"] };
        git(root, &undo).map_err(refused)?;
        return Err(RoundRefusal::CommitCarriedMore { extra, repo: root.to_string_lossy().into_owned() });
    }
    let mut synced: Vec<&str> = vec!["reset", "-q", "--"];
    synced.extend(files.iter().map(String::as_str));
    let _ = git(root, &synced);
    Ok(Some(sha.trim().to_string()))
}

/// Os arquivos que o commit recém-feito em `root` levou além de `files`.
///
/// Só olha: desfazer e recusar é de quem pergunta.
///
/// POR QUE DEPOIS, e não antes. [`commit_paths`] monta um índice à parte e põe
/// nele só os arquivos da entrega, mas os ganchos do projeto rodam dentro do
/// `git commit`, de propósito — é assim que o portão de testes do projeto vale
/// para o commit da onda. Um gancho que termine em `git add -A` junta ao
/// commit tudo o que estiver alterado no checkout, e o índice cuidadoso do
/// Mustard não protege de nada. Em campo, a configuração do editor do usuário,
/// alterada antes da obra, foi para a branch, e o condutor só percebeu no
/// fechamento (21/10/2026).
fn carried_beyond(root: &Path, files: &[String]) -> Result<Vec<String>, String> {
    let listed = git(root, &["show", "--name-only", "--no-renames", "--pretty=format:", "HEAD"])?;
    Ok(listed
        .lines()
        .map(str::trim)
        .filter(|file| !file.is_empty())
        .filter(|file| !files.iter().any(|one| one == file))
        .map(str::to_string)
        .collect())
}

/// Os eventos de cada commit de `made`, como a spec os grava, o de cada
/// submódulo como o do principal: é o que a conferência da gravação ensaia
/// quando parte das ondas ficou de fora depois dos commits.
pub(super) fn made_drafts(root: &Path, made: &[Made]) -> Vec<Map<String, Value>> {
    let mut drafts = Vec::new();
    for one in made {
        drafts.push(made_draft(root, &one.sha, one, &one.files));
        drafts.extend(one.subs.iter().map(|(sub, sha)| {
            let own = one.repos.subs.get(sub).map(Vec::as_slice).unwrap_or_default();
            made_draft(&root.join(sub), sha, one, own)
        }));
    }
    drafts
}

/// O evento do commit `sha` de `one`, no repositório `root`, com os arquivos
/// `files`, como a spec o grava: o de [`commit_draft`] e, quando alguma onda
/// entrou nele sem mudança, a lista delas à parte, que a falha numa linha do
/// commit não implica.
fn made_draft(root: &Path, sha: &str, one: &Made, files: &[String]) -> Map<String, Value> {
    let mut draft = commit_draft(root, sha, &one.title, &one.waves, files);
    if !one.unchanged.is_empty() {
        draft.insert("unchanged".into(), json!(one.unchanged));
    }
    draft
}

/// Grava na spec cada commit de `made`, com as ondas dele, na ordem em que
/// saíram, e o de cada submódulo como o do principal. Devolve o que a
/// resposta da rodada diz do commit — o último e, quando a rodada fez mais de
/// um, a lista de todos em `commits` — e os arquivos que eles levaram.
pub(super) fn record_made(start: &Path, root: &Path, spec: &str, made: &[Made]) -> Result<(Option<Value>, Vec<String>), RoundRefusal> {
    let keep = |draft: Map<String, Value>| record(start, spec, "commit", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused);
    let mut commit: Option<Value> = None;
    let mut listed: Vec<Value> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for one in made {
        keep(made_draft(root, &one.sha, one, &one.files))?;
        let mut entry = json!({ "sha": one.sha, "title": one.title });
        let mut subs: Vec<Value> = Vec::new();
        for (sub, sha) in &one.subs {
            let own = one.repos.subs.get(sub).map(Vec::as_slice).unwrap_or_default();
            keep(made_draft(&root.join(sub), sha, one, own))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Um gancho de pre-commit do projeto que termina em `git add -A` junta ao
    /// commit tudo o que está alterado na pasta. O commit é desfeito, a recusa
    /// nomeia o arquivo que entrou de carona, e nada no disco é tocado: o
    /// trabalho da onda e a mudança local do usuário ficam onde estavam.
    ///
    /// Em campo (21/10/2026) a configuração do editor do usuário, alterada
    /// antes da obra e sem relação com ela, foi para a branch, e o condutor só
    /// percebeu no fechamento, com o pull request já carregando o ruído.
    #[test]
    fn a_hook_that_stages_everything_has_its_commit_undone_and_the_dirt_kept() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path();
        for args in [
            vec!["init", "-q", "."],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "T"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            git(root, &args).expect("git");
        }
        std::fs::write(root.join("obra.txt"), "antes\n").expect("write");
        std::fs::write(root.join("meu.txt"), "meu\n").expect("write");
        git(root, &["add", "-A"]).expect("add");
        git(root, &["commit", "-q", "-m", "base"]).expect("commit");

        // O gancho do projeto, que junta tudo o que está alterado.
        let hooks = root.join(".git/hooks");
        std::fs::create_dir_all(&hooks).expect("hooks");
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\ngit add -A\n").expect("hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }

        // A onda mexeu num arquivo; o usuário tinha mexido no outro, antes.
        std::fs::write(root.join("obra.txt"), "depois\n").expect("write");
        std::fs::write(root.join("meu.txt"), "minha mudanca local\n").expect("write");

        let files = vec!["obra.txt".to_string()];
        let refusal = commit_paths(root, "fix: a onda", "", &files, &BTreeMap::new(), false)
            .expect_err("o commit que leva arquivo de fora é recusado");
        match refusal {
            RoundRefusal::CommitCarriedMore { extra, .. } => {
                assert_eq!(extra, vec!["meu.txt".to_string()], "a recusa nomeia o arquivo de carona");
            }
            other => panic!("another refusal: {}", other.reason()),
        }
        assert_eq!(head_title(root), "base", "o commit foi desfeito");
        assert_eq!(read(root, "meu.txt"), "minha mudanca local\n", "a mudança local fica intacta");
        assert_eq!(read(root, "obra.txt"), "depois\n", "o trabalho da onda fica intacto");
    }

    /// O gancho de pre-commit do projeto que reprova barra o commit da rodada.
    /// A recusa diz que a barreira é do projeto, nomeia o gancho, nomeia o
    /// arquivo do commit que a saída dele citou e repassa o que ele disse —
    /// mesmo quando ele escreve na saída normal, e não na de erro. Sem gancho
    /// nenhum, a recusa segue sendo a do git.
    ///
    /// Em campo (21/10/2026) a conferência que o projeto do usuário roda antes
    /// de cada commit barrou a rodada, e ela parou sem dizer o que era.
    #[test]
    fn a_project_pre_commit_hook_that_rejects_says_the_check_is_the_projects() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path();
        for args in [
            vec!["init", "-q", "."],
            vec!["config", "user.email", "t@t"],
            vec!["config", "user.name", "T"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            git(root, &args).expect("git");
        }
        std::fs::write(root.join("obra.txt"), "antes\n").expect("write");
        git(root, &["add", "-A"]).expect("add");
        git(root, &["commit", "-q", "-m", "base"]).expect("commit");
        std::fs::write(root.join("obra.txt"), "depois\n").expect("write");
        let files = vec!["obra.txt".to_string()];

        // Sem gancho, quem recusa é o git: aqui, porque não há nada a comitar
        // num commit que não pode pular.
        git(root, &["stash", "-q"]).expect("stash");
        let plain = commit_paths(root, "fix: a onda", "", &files, &BTreeMap::new(), false).expect_err("nada a comitar");
        assert_eq!(plain.reason(), "git-refused", "{}", plain.reason());
        git(root, &["stash", "pop", "-q"]).expect("pop");

        // O gancho do projeto reprova, escrevendo na saída normal.
        let hooks = root.join(".git/hooks");
        std::fs::create_dir_all(&hooks).expect("hooks");
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho 'obra.txt: linha longa demais'\nexit 1\n").expect("hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }

        let refusal = commit_paths(root, "fix: a onda", "", &files, &BTreeMap::new(), false).expect_err("o gancho reprovou");
        assert_eq!(refusal.reason(), "project-hook-refused", "{}", refusal.reason());
        match &refusal {
            RoundRefusal::ProjectHook { hook, detail, files } => {
                assert_eq!(hook, "pre-commit");
                assert_eq!(files, &vec!["obra.txt".to_string()], "a recusa nomeia o arquivo que ele citou");
                assert!(detail.contains("linha longa demais"), "o que o gancho disse chega inteiro: {detail}");
            }
            other => panic!("another refusal: {}", other.reason()),
        }
        let said = refusal.message(mustard_core::platform::i18n::Locale::PtBr);
        assert!(said.contains("pre-commit") && said.contains("obra.txt"), "{said}");
        assert!(said.contains("linha longa demais"), "{said}");
        assert_eq!(head_title(root), "base", "nada foi comitado");
        assert_eq!(read(root, "obra.txt"), "depois\n", "o trabalho da onda fica intacto");
    }

    fn head_title(root: &Path) -> String {
        git(root, &["log", "-1", "--pretty=format:%s"]).expect("log").trim().to_string()
    }

    fn read(root: &Path, file: &str) -> String {
        std::fs::read_to_string(root.join(file)).expect("read")
    }
}
