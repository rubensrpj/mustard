//! O commit da rodada: a junção de cada cópia ao repositório principal, a
//! mensagem montada do resumo de cada entrega e conferida, a formatação só dos
//! arquivos da rodada, o commit, a gravação dele na spec e a cópia apagada.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::domain::spec_events::{
    check_message, MessageRefusal, Refusal, SpecLog, MESSAGE_BODY_MAX, MESSAGE_TITLE_MAX,
};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::wave_prompt::recorded_copy;
use mustard_core::platform::git as git_exec;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::answer::RoundRefusal;
use super::report::WaveReport;
use crate::commands::git_settle::{enter_unit_branch, submodule_holding, submodules_of};
use crate::commands::spec_events::write::record;

/// A mensagem do commit da rodada, montada do resumo que cada entrega traz e
/// já conferida: o título no molde do repositório (`tipo(escopo): frase`),
/// com o resumo da primeira onda, e o corpo com uma linha por onda. O tipo é
/// `fix` quando a rodada traz um conserto, e `feat` nos outros casos. `None`
/// quando nenhuma entrega traz arquivo.
pub(super) fn commit_message(waves: &[WaveReport], lang: Locale) -> Result<Option<(String, String)>, RoundRefusal> {
    let committed: Vec<(&WaveReport, &str)> = waves
        .iter()
        .filter(|w| !w.files.is_empty())
        .filter_map(|w| w.commit.as_deref().map(|summary| (w, summary)))
        .collect();
    let Some((_, first)) = committed.first() else {
        return Ok(None);
    };
    let numbers: Vec<String> = committed.iter().map(|(w, _)| w.wave.to_string()).collect();
    let scope_key = if numbers.len() == 1 { "round.commit.scope.one" } else { "round.commit.scope.many" };
    let scope = translate(scope_key, lang).replace("{waves}", &numbers.join("-"));
    let kind = if committed.iter().any(|(w, _)| !w.fixes.is_empty()) { "fix" } else { "feat" };
    let title = format!("{kind}({scope}): {first}");
    let body: Vec<String> = committed
        .iter()
        .map(|(w, summary)| {
            let mut line = translate("round.commit.line", lang)
                .replace("{wave}", &w.wave.to_string())
                .replace("{summary}", summary);
            if !w.fixes.is_empty() {
                let fixed: Vec<String> = w.fixes.iter().map(u64::to_string).collect();
                line.push(' ');
                line.push_str(&translate("round.commit.fixes", lang).replace("{waves}", &fixed.join(", ")));
            }
            line
        })
        .collect();
    let body = body.join("\n");
    check_commit_text(&title, &body)?;
    Ok(Some((title, body)))
}

/// A mensagem de commit cabe no modelo, pela MESMA conferência que o pull
/// request usa.
///
/// As duas eram a mesma regra escrita duas vezes — os mesmos tetos, a mesma
/// lista do que nunca vai, o mesmo achador de e-mail — e uma regra escrita duas
/// vezes é uma regra que vale em um lugar só assim que alguém mexer no outro.
/// A conferência mora no núcleo; aqui fica só a tradução para a recusa da
/// rodada, que é o que muda entre as duas portas.
pub(super) fn check_commit_text(title: &str, body: &str) -> Result<(), RoundRefusal> {
    check_message(title, body, MESSAGE_TITLE_MAX, MESSAGE_BODY_MAX).map_err(|refusal| match refusal
    {
        MessageRefusal::TooLong { part, chars, max } => {
            RoundRefusal::CommitTooLong { part: part.to_string(), chars, max }
        }
        MessageRefusal::Forbidden { found, .. } => RoundRefusal::CommitForbidden { found },
        // O commit não tira o título da spec: o relatório o traz. Uma spec sem
        // objetivo não é recusa desta porta.
        MessageRefusal::NoTitle => RoundRefusal::CommitForbidden { found: String::new() },
    })
}

/// As extensões que o Prettier trata.
const PRETTIER_EXTS: &[&str] =
    &[".ts", ".tsx", ".js", ".jsx", ".json", ".css", ".md", ".html", ".scss"];

/// Os sinais de que o projeto tem Prettier configurado.
const PRETTIER_SIGNS: &[&str] = &[
    "node_modules/.bin/prettier",
    ".prettierrc",
    ".prettierrc.js",
    ".prettierrc.json",
    "prettier.config.js",
];

/// O que a formatação da rodada fez: os arquivos formatados e os formatadores
/// que o projeto declara e que não foram achados.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Formatting {
    pub(super) formatted: Vec<String>,
    pub(super) missing: Vec<String>,
}

/// Formata só os arquivos da rodada, com o formatador que o projeto já tem:
/// o Prettier configurado ou o `dotnet format` de um projeto .NET. Num projeto
/// sem formatador configurado nada é formatado e nada é avisado; o formatador
/// declarado e não achado sai pelo nome, em vez de a formatação ser pulada em
/// silêncio.
pub(super) fn format_round_files(root: &Path, files: &[String]) -> Formatting {
    format_with(root, files, &|program, args| run(root, program, args))
}

/// [`format_round_files`] com o executor recebido, que é como um teste o
/// escolhe sem depender do que está instalado na máquina.
pub(super) fn format_with(root: &Path, files: &[String], exec: &dyn Fn(&str, &[&str]) -> bool) -> Formatting {
    let mut out = Formatting::default();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let prettier: Vec<&String> = files
        .iter()
        .filter(|f| seen.insert(f.as_str()))
        .filter(|f| PRETTIER_EXTS.contains(&extension(f).as_str()))
        .filter(|f| root.join(f).is_file())
        .collect();
    if !prettier.is_empty() && PRETTIER_SIGNS.iter().any(|sign| root.join(sign).exists()) {
        let mut args: Vec<&str> = vec!["prettier", "--write"];
        args.extend(prettier.iter().map(|f| f.as_str()));
        if exec("npx", &args) {
            out.formatted.extend(prettier.iter().map(|f| (*f).clone()));
        } else {
            out.missing.push("Prettier".to_string());
        }
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let sharp: Vec<&String> = files
        .iter()
        .filter(|f| seen.insert(f.as_str()))
        .filter(|f| extension(f) == ".cs")
        .filter(|f| root.join(f).is_file())
        .collect();
    if !sharp.is_empty() && let Some(project) = dotnet_project(root) {
        let mut ok = true;
        for file in &sharp {
            ok &= exec("dotnet", &["format", &project, "--include", file, "--no-restore"]);
        }
        if ok {
            out.formatted.extend(sharp.iter().map(|f| (*f).clone()));
        } else {
            out.missing.push("dotnet format".to_string());
        }
    }
    out
}

/// A extensão de um caminho, em minúsculas e com o ponto; vazia sem extensão.
fn extension(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match base.rfind('.') {
        Some(i) if i > 0 => base[i..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// O `.sln` ou o `.csproj` da raiz do projeto, que diz que ele é um projeto
/// .NET. `None` quando não há nenhum.
fn dotnet_project(root: &Path) -> Option<String> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut sln = None;
    let mut csproj = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().ends_with(".sln") {
            sln = Some(name);
        } else if name.to_ascii_lowercase().ends_with(".csproj") {
            csproj = Some(name);
        }
    }
    sln.or(csproj)
}

/// Roda um programa na raiz do projeto; `false` quando ele não está lá ou
/// saiu com erro.
fn run(root: &Path, program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// O código que a conferência antes do commit usa no lugar do que o git ainda
/// vai dar, com a mesma forma.
pub(super) const UNMADE_SHA: &str = "0000000000000000000000000000000000000000";

/// A trava do passo do git do checkout `root`, na recusa da rodada. A trava
/// mora num lugar só ([`crate::commands::git_settle::git_step_lock`]), porque
/// o commit da rodada e o ponteiro dos submódulos disputam o mesmo índice.
pub(super) fn git_lock(root: &Path) -> Result<LockedFile, RoundRefusal> {
    crate::commands::git_settle::git_step_lock(root)
        .map_err(|detail| RoundRefusal::Refused(Refusal::Io { detail }))
}

/// O commit atual do checkout `root`; vazio quando o git não responde.
pub(super) fn head(root: &Path) -> String {
    git(root, &["rev-parse", "HEAD"]).unwrap_or_default().trim().to_string()
}

/// Os arquivos da rodada separados por repositório: os do principal e, por
/// submódulo, os de dentro dele, escritos a partir do principal.
#[derive(Debug, Default)]
pub(super) struct RoundRepos {
    own: Vec<String>,
    pub(super) subs: BTreeMap<String, Vec<String>>,
}

/// Os arquivos `files` da rodada do checkout `root`, cada um no repositório
/// que o guarda.
pub(super) fn round_repos(root: &Path, files: &[String]) -> RoundRepos {
    let subs = submodules_of(root);
    let mut repos = RoundRepos::default();
    for file in files {
        match submodule_holding(&subs, file) {
            Some((sub, _)) => repos.subs.entry(sub.to_string()).or_default().push(file.clone()),
            None => repos.own.push(file.clone()),
        }
    }
    repos
}

/// Os commits da rodada: o do principal e o de cada submódulo, pelo caminho
/// dele.
pub(super) struct Made {
    pub(super) sha: String,
    pub(super) subs: Vec<(String, String)>,
}

/// Faz o commit da rodada com a mensagem já conferida e devolve o código dele.
/// Não grava nada na spec: roda antes de qualquer gravação, e a recusa do git
/// para a rodada com a spec intacta. Roda com a trava do passo do git que quem
/// chama já prendeu antes da junção (`_held`), e não a pega de novo.
///
/// O commit leva só os arquivos da rodada, por caminho, com o conteúdo do
/// disco, e nada do que estiver preparado fora deles. O preparo do checkout só
/// muda quando o commit sai: a rodada que morre no meio dele não deixa nada
/// preparado. O arquivo de dentro de um submódulo é comitado no submódulo, na
/// branch `unit` da spec, criada na primeira vez sobre a base dele, e o commit
/// do principal leva o ponteiro novo junto com os arquivos dele. Quando o git
/// recusa, o bloco inteiro é desfeito ali mesmo, dentro da trava: o commit já
/// feito num submódulo volta, o registro dos arquivos novos sai do índice e
/// cada arquivo que a junção mudou (`joined`) volta ao que era no disco.
pub(super) fn make_commit(
    root: &Path,
    _held: &LockedFile,
    unit: &str,
    message: (&str, &str),
    repos: &RoundRepos,
    joined: &[Joined],
) -> Result<Made, RoundRefusal> {
    let mut done: Vec<SubCommit> = Vec::new();
    let made = commit_repos(root, unit, message, repos, &mut done);
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
    made.map(|sha| Made { sha, subs: done.into_iter().map(|one| (one.sub, one.sha)).collect() })
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
/// por último o do principal, que leva o ponteiro de cada um.
fn commit_repos(
    root: &Path,
    unit: &str,
    (title, body): (&str, &str),
    repos: &RoundRepos,
    done: &mut Vec<SubCommit>,
) -> Result<String, RoundRefusal> {
    let mut own = repos.own.clone();
    for (sub, files) in &repos.subs {
        let dir = root.join(sub);
        enter_unit_branch(&dir, unit).map_err(|detail| RoundRefusal::Git { detail })?;
        let inner: Vec<String> =
            files.iter().filter_map(|file| file.strip_prefix(&format!("{sub}/")).map(str::to_string)).collect();
        let before = head(&dir);
        let sha = committed(&dir, title, body, &inner)?;
        done.push(SubCommit { sub: sub.clone(), before, inner, sha });
        own.push(sub.clone());
    }
    committed(root, title, body, &own)
}

/// O commit por caminho de [`commit_paths`] no repositório `dir`; na recusa,
/// o registro dos arquivos novos sai do índice.
fn committed(dir: &Path, title: &str, body: &str, files: &[String]) -> Result<String, RoundRefusal> {
    let mut registered: Vec<String> = Vec::new();
    let made = commit_paths(dir, title, body, files, &mut registered);
    if made.is_err() && !registered.is_empty() {
        let mut undo: Vec<&str> = vec!["rm", "-q", "--cached", "--ignore-unmatch", "--"];
        undo.extend(registered.iter().map(String::as_str));
        let _ = git(dir, &undo);
    }
    made
}

/// O commit por caminho de [`make_commit`]. O commit por caminho só aceita o
/// que o git já conhece: o arquivo novo é registrado antes, só como intenção,
/// sem conteúdo, e vai para `registered`, que é o que o desfazer tira. O
/// apagado que o git nunca conheceu fica de fora, porque não há o que levar
/// dele; o que só saiu do disco, ou já saiu do índice, vai como remoção.
fn commit_paths(
    root: &Path,
    title: &str,
    body: &str,
    files: &[String],
    registered: &mut Vec<String>,
) -> Result<String, RoundRefusal> {
    let refused = |detail: String| RoundRefusal::Git { detail };
    let (present, gone): (Vec<&str>, Vec<&str>) =
        files.iter().map(String::as_str).partition(|file| root.join(file).exists());
    if !present.is_empty() {
        let mut others: Vec<&str> = vec!["ls-files", "-z", "--others", "--"];
        others.extend(&present);
        let listed = git(root, &others).map_err(refused)?;
        registered.extend(listed.split('\0').filter(|path| !path.is_empty()).map(str::to_string));
    }
    if !registered.is_empty() {
        let mut add: Vec<&str> = vec!["add", "--intent-to-add", "--"];
        add.extend(registered.iter().map(String::as_str));
        git(root, &add).map_err(refused)?;
    }
    let known_gone = gone
        .into_iter()
        .filter(|file| git(root, &["ls-files", "--error-unmatch", "--with-tree=HEAD", "--", file]).is_ok());
    let mut args: Vec<&str> = vec!["commit", "--only", "-m", title];
    if !body.is_empty() {
        args.push("-m");
        args.push(body);
    }
    args.push("--");
    args.extend(present);
    args.extend(known_gone);
    git(root, &args).map_err(refused)?;
    let sha = git(root, &["rev-parse", "HEAD"]).map_err(refused)?;
    Ok(sha.trim().to_string())
}

/// O evento do commit `sha` feito pela rodada no checkout `root`, como a spec
/// o grava.
pub(super) fn commit_draft(root: &Path, sha: &str, title: &str, waves: &[u64], files: &[String]) -> Map<String, Value> {
    let mut draft = Map::new();
    draft.insert("sha".into(), json!(sha));
    draft.insert("title".into(), json!(title));
    draft.insert("waves".into(), json!(waves));
    draft.insert("files".into(), json!(files));
    draft.insert("repo".into(), json!(repo_name(root)));
    draft.insert("author".into(), json!("binary"));
    draft
}

/// Grava no arquivo de eventos o commit `sha` feito pela rodada.
pub(super) fn record_commit(
    start: &Path,
    root: &Path,
    spec: &str,
    sha: &str,
    title: &str,
    waves: &[u64],
    files: &[String],
) -> Result<Value, RoundRefusal> {
    let draft = commit_draft(root, sha, title, waves, files);
    record(start, spec, "commit", draft, PhaseWriter::Binary).map_err(RoundRefusal::Refused)?;
    Ok(json!({ "sha": sha, "title": title }))
}

/// Cada arquivo entregue está no disco ou no índice, do repositório principal
/// ou da cópia da onda, contando o que saiu do índice desde o último commit;
/// senão, a recusa vem antes de gravar, e não do commit, depois (a remoção
/// aceita calada o caminho que não existe). O arquivo de dentro de um
/// submódulo é procurado no índice do submódulo.
pub(super) fn unknown_file(root: &Path, log: &SpecLog, waves: &[WaveReport]) -> Result<(), RoundRefusal> {
    let subs = submodules_of(root);
    for wave in waves {
        let copy = copy_of(log, wave.wave);
        for file in &wave.files {
            let known = std::iter::once(root).chain(copy.as_deref()).any(|dir| {
                let (repo, inner) = repo_of(dir, &subs, file);
                dir.join(file).exists()
                    || git(&repo, &["ls-files", "--error-unmatch", "--with-tree=HEAD", "--", &inner]).is_ok()
            });
            if !known {
                return Err(RoundRefusal::FileUnknown { file: file.clone(), wave: wave.wave });
            }
        }
    }
    Ok(())
}

/// O repositório que guarda o arquivo `file` na pasta `dir` — o principal, ou
/// a cópia dele, ou o submódulo que o guarda ali — e o caminho do arquivo
/// dentro desse repositório.
fn repo_of(dir: &Path, subs: &[String], file: &str) -> (PathBuf, String) {
    match submodule_holding(subs, file) {
        Some((sub, inner)) => (dir.join(sub), inner),
        None => (dir.to_path_buf(), file.to_string()),
    }
}

/// A pasta da cópia que a rodada criou para a onda `wave`, quando o envio
/// dela gravou uma e ela ainda está no disco.
fn copy_of(log: &SpecLog, wave: u64) -> Option<PathBuf> {
    recorded_copy(log, wave).map(|copy| PathBuf::from(copy.path)).filter(|path| path.is_dir())
}

/// Um arquivo que a junção muda no repositório principal: o que ele era e o
/// que passa a ser. `None` é o arquivo que não existe.
pub(super) struct Joined {
    file: String,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

/// A entrega de uma onda que a junção segurou: os trechos em conflito e a
/// cópia em que eles se resolvem.
pub(super) struct Held {
    pub(super) wave: u64,
    copy: String,
    conflicts: Vec<String>,
}

impl Held {
    /// A recusa desta entrega, que manda levar a cópia ao commit `head`.
    pub(super) fn refusal(self, head: String) -> RoundRefusal {
        RoundRefusal::MergeConflict { wave: self.wave, copy: self.copy, conflicts: self.conflicts, head }
    }
}

/// Junta ao repositório principal cada arquivo entregue pelas ondas que têm
/// cópia, por uma fusão de três vias com o commit da cópia como base, sem
/// gravar nada: devolve o que gravar e as entregas seguradas. O arquivo que a
/// cópia não mudou fica como está; o que só a cópia mudou vem dela, apagado e
/// novo inclusive; o que os dois lados mudaram é fundido. A entrega com um
/// trecho que a fusão não resolve é segurada inteira, com os trechos e a
/// cópia dela, e nada dela entra na junção; as outras seguem. Duas ondas do
/// mesmo relatório no mesmo arquivo se somam, na ordem do relatório. O
/// arquivo de dentro de um submódulo é comparado no submódulo, com o commit
/// da cópia dele como base. O caminho que sai do repositório não é tocado: o
/// git o recusa no commit.
pub(super) fn join_copies(
    root: &Path,
    log: &SpecLog,
    waves: &[WaveReport],
) -> Result<(Vec<Joined>, Vec<Held>), RoundRefusal> {
    let subs = submodules_of(root);
    let mut joined: BTreeMap<String, Joined> = BTreeMap::new();
    let mut held: Vec<Held> = Vec::new();
    for wave in waves {
        let Some(copy) = copy_of(log, wave.wave) else { continue };
        let mut bases: BTreeMap<PathBuf, String> = BTreeMap::new();
        let mut conflicts: Vec<String> = Vec::new();
        // O que esta onda muda só entra na junção quando nenhum arquivo dela
        // conflita.
        let mut staged: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        for file in wave.files.iter().filter(|file| inside(file)) {
            let (copy_repo, inner) = repo_of(&copy, &subs, file);
            let (root_repo, _) = repo_of(root, &subs, file);
            if !bases.contains_key(&copy_repo) {
                let base = git(&copy_repo, &["rev-parse", "HEAD"]).map_err(|detail| RoundRefusal::Git { detail })?;
                bases.insert(copy_repo.clone(), base.trim().to_string());
            }
            let base = bases.get(&copy_repo).cloned().unwrap_or_default();
            let theirs = std::fs::read(copy.join(file)).ok();
            let base_id = git(&copy_repo, &["rev-parse", "--verify", "-q", &format!("{base}:{inner}")])
                .ok()
                .map(|id| id.trim().to_string());
            if blob_id(&copy_repo, &inner, theirs.as_deref()) == base_id {
                continue;
            }
            let ours = match joined.get(file) {
                Some(done) => done.after.clone(),
                None => std::fs::read(root.join(file)).ok(),
            };
            if ours == theirs {
                continue;
            }
            let after = if blob_id(&root_repo, &inner, ours.as_deref()) == base_id {
                theirs
            } else {
                let base_text = match base_id.as_deref() {
                    Some(id) => blob_text(&copy_repo, id).ok(),
                    None => Some(String::new()),
                };
                match merge_texts(root, ours.as_deref(), base_text.as_deref(), theirs.as_deref()) {
                    Ok(merged) => Some(merged.into_bytes()),
                    Err(lines) if lines.is_empty() => {
                        conflicts.push(file.clone());
                        continue;
                    }
                    Err(lines) => {
                        conflicts.extend(lines.iter().map(|line| format!("{file}:{line}")));
                        continue;
                    }
                }
            };
            staged.push((file.clone(), after));
        }
        if !conflicts.is_empty() {
            let copy = copy.to_string_lossy().replace('\\', "/");
            held.push(Held { wave: wave.wave, copy, conflicts });
            continue;
        }
        for (file, after) in staged {
            let before = std::fs::read(root.join(&file)).ok();
            joined
                .entry(file.clone())
                .and_modify(|done| done.after.clone_from(&after))
                .or_insert(Joined { file, before, after });
        }
    }
    Ok((joined.into_values().collect(), held))
}

/// Grava no repositório principal o que a junção decidiu (`after`), ou volta
/// cada arquivo ao que era (`!after`): é assim que a recusa do git depois da
/// junção deixa o disco como estava.
pub(super) fn write_joined(root: &Path, joined: &[Joined], after: bool) -> Result<(), RoundRefusal> {
    let io = |e: std::io::Error| RoundRefusal::Refused(Refusal::Io { detail: e.to_string() });
    for one in joined {
        let path = root.join(&one.file);
        match if after { &one.after } else { &one.before } {
            Some(bytes) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                std::fs::write(&path, bytes).map_err(io)?;
            }
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
                _ => {}
            },
        }
    }
    Ok(())
}

/// O caminho fica dentro do repositório: relativo, sem subir de pasta.
fn inside(file: &str) -> bool {
    Path::new(file).components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

/// O número que o controle de versão daria ao conteúdo `bytes` do arquivo
/// `file` da pasta `dir`, com as regras de fim de linha dela; `None` para o
/// arquivo que não existe. É a comparação que não depende de o conteúdo ser
/// texto.
fn blob_id(dir: &Path, file: &str, bytes: Option<&[u8]>) -> Option<String> {
    let bytes = bytes?;
    let scratch = tempfile::tempdir().ok()?;
    let held = scratch.path().join("conteudo");
    std::fs::write(&held, bytes).ok()?;
    git(dir, &["hash-object", "--path", file, &held.to_string_lossy()]).ok().map(|id| id.trim().to_string())
}

/// O texto do objeto `id`, como o controle de versão o guarda.
fn blob_text(dir: &Path, id: &str) -> Result<String, String> {
    let out = git_exec::run(dir, &["cat-file", "blob", id]);
    if out.ok { Ok(out.stdout) } else { Err(out.stderr) }
}

/// A fusão de três vias de três textos. `Err` traz a linha de cada trecho em
/// conflito, no texto fundido; vazio quando a fusão nem pôde ser feita — um
/// dos lados apagado, ou um conteúdo que não é texto.
fn merge_texts(dir: &Path, ours: Option<&[u8]>, base: Option<&str>, theirs: Option<&[u8]>) -> Result<String, Vec<usize>> {
    let text = |bytes: Option<&[u8]>| bytes.and_then(|b| std::str::from_utf8(b).ok()).map(str::to_string);
    let base = base.filter(|b| !b.contains('\u{fffd}')).map(str::to_string);
    let (Some(ours), Some(base), Some(theirs)) = (text(ours), base, text(theirs)) else {
        return Err(Vec::new());
    };
    let scratch = tempfile::tempdir().map_err(|_| Vec::new())?;
    let mut names: Vec<String> = Vec::new();
    for (name, body) in [("principal", &ours), ("base", &base), ("copia", &theirs)] {
        let path = scratch.path().join(name);
        std::fs::write(&path, body.as_bytes()).map_err(|_| Vec::new())?;
        names.push(path.to_string_lossy().to_string());
    }
    let labels = ["-L", "principal", "-L", "base", "-L", "copia"];
    let mut args: Vec<&str> = vec!["merge-file", "-p"];
    args.extend(labels);
    args.extend(names.iter().map(String::as_str));
    let out = git_exec::run(dir, &args);
    if out.ok {
        return Ok(out.stdout);
    }
    Err(out.stdout.lines().enumerate().filter(|(_, line)| line.starts_with("<<<<<<< ")).map(|(at, _)| at + 1).collect())
}

/// Apaga a cópia de cada onda do relatório, depois do commit, com a cópia de
/// cada submódulo dentro dela. A cópia com mudança fora da lista de arquivos
/// entregue fica, e o aviso diz quais: ela se perderia com a cópia.
pub(super) fn close_copies(root: &Path, log: &SpecLog, waves: &[WaveReport], lang: Locale) -> Vec<Value> {
    let subs = submodules_of(root);
    let mut warnings = Vec::new();
    for wave in waves {
        let Some(copy) = copy_of(log, wave.wave) else { continue };
        let shown = copy.to_string_lossy().replace('\\', "/");
        let status = ["status", "--porcelain", "-z", "--untracked-files=all", "--ignore-submodules=all"];
        let mut changed = changed_paths(&git(&copy, &status).unwrap_or_default());
        let inner: Vec<&String> = subs.iter().filter(|sub| copy.join(sub).join(".git").is_file()).collect();
        for sub in &inner {
            let theirs = changed_paths(&git(&copy.join(sub), &status).unwrap_or_default());
            changed.extend(theirs.into_iter().map(|path| format!("{sub}/{path}")));
        }
        let left: Vec<String> = changed.into_iter().filter(|path| !wave.files.contains(path)).collect();
        let removed = left.is_empty()
            && git_lock(root).is_ok_and(|_held| {
                inner.iter().all(|sub| {
                    let target = copy.join(sub).to_string_lossy().replace('\\', "/");
                    git(&root.join(sub), &["worktree", "remove", "--force", &target]).is_ok()
                }) && git(root, &["worktree", "remove", "--force", &shown]).is_ok()
            });
        if !removed {
            let files = if left.is_empty() { shown.clone() } else { left.join(", ") };
            let hint = translate("round.copy_kept", lang)
                .replace("{wave}", &wave.wave.to_string())
                .replace("{copy}", &shown)
                .replace("{files}", &files);
            warnings.push(json!({ "reason": "copy-kept", "wave": wave.wave, "hint": hint }));
        }
    }
    warnings
}

/// Os caminhos que o `status --porcelain -z` lista, inclusive o nome antigo
/// de um arquivo renomeado.
fn changed_paths(status: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut entries = status.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let (code, path) = entry.split_at(entry.len().min(3));
        out.push(path.to_string());
        if code.contains('R') || code.contains('C') {
            out.extend(entries.next().map(str::to_string));
        }
    }
    out
}

/// O nome do repositório: o da pasta do projeto.
fn repo_name(root: &Path) -> String {
    root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "?".into())
}

/// Roda o git na raiz do projeto e devolve a saída; o erro vem como texto.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_exec::run(root, args);
    if out.ok {
        return Ok(out.stdout);
    }
    // Alguns motivos, como o de não haver nada a comitar, o git escreve na
    // saída normal, e a de erro vem vazia.
    let said = if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr };
    Err(said.trim().to_string())
}

#[cfg(test)]
mod tests {
    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A mensagem do commit tem título e corpo dentro do teto e nunca traz o
    /// link da conversa, o nome do modelo, a assinatura de coautoria nem o
    /// e-mail de ninguém; o commit é recusado até o e-mail sair.
    #[test]
    fn the_commit_message_is_checked_before_the_commit_is_made() {
        assert!(check_commit_text("feat: a soma", "O corpo.").is_ok());
        let cases = [
            ("a".repeat(MESSAGE_TITLE_MAX + 1), String::new(), "commit-too-long"),
            ("feat: a soma".into(), "a".repeat(MESSAGE_BODY_MAX + 1), "commit-too-long"),
            ("feat: a soma".into(), "https://claude.ai/code/x".into(), "commit-forbidden-text"),
            ("feat: a soma".into(), "Feito com Claude.".into(), "commit-forbidden-text"),
            ("feat: a soma".into(), "Co-Authored-By: alguem".into(), "commit-forbidden-text"),
            ("feat: a soma".into(), "pedido de fulano@empresa.com.br".into(), "commit-forbidden-text"),
        ];
        for (title, body, reason) in cases {
            let refused = check_commit_text(&title, &body).expect_err(&format!("{title} / {body}"));
            assert_eq!(refused.reason(), reason, "{title} / {body}");
        }
    }

    /// O texto do agente de onda diz, nos dois idiomas, o limite do resumo do
    /// commit, e o limite é o que a rodada aceita: com o começo que ela põe na
    /// frente (o tipo e o número de uma onda de dois dígitos), um resumo de 45
    /// caracteres fecha o título em 60 e passa, e um de 46 passa de 60 e é
    /// recusado.
    #[test]
    fn the_wave_agent_text_states_the_commit_summary_limit_the_round_accepts() {
        let limit = 45;
        for (lang, said) in [(Locale::PtBr, "até 45 caracteres"), (Locale::EnUs, "at most 45 characters")] {
            let (_, agent) = mustard_core::agent_texts(lang)[0];
            let line = agent.lines().find(|l| l.starts_with("- `commit`")).expect("the commit line of the wave agent");
            assert!(line.contains(said), "{lang:?}: {line}");
            assert!(line.contains(&MESSAGE_TITLE_MAX.to_string()), "{lang:?}: {line}");
            let report = |summary: String| WaveReport {
                wave: 13,
                delivered: "A onda saiu.".into(),
                files: vec!["src/a.rs".into()],
                commit: Some(summary),
                proofs: Vec::new(),
                fixes: Vec::new(),
                replan: None,
            };
            let (title, _) = commit_message(&[report("a".repeat(limit))], lang)
                .unwrap_or_else(|_| panic!("{lang:?}: a {limit}-character summary fits"))
                .expect("a message");
            assert_eq!(title.chars().count(), MESSAGE_TITLE_MAX, "{lang:?}: {title}");
            let Err(over) = commit_message(&[report("a".repeat(limit + 1))], lang) else {
                panic!("{lang:?}: a summary over {limit} characters is refused");
            };
            assert_eq!(over.reason(), "commit-too-long", "{lang:?}");
        }
    }

    /// A mensagem do commit é conferida antes de qualquer gravação: o
    /// relatório com e-mail no corpo é recusado sem gravar o entregou, e a
    /// chamada seguinte, com a mensagem limpa, grava uma vez só.
    #[test]
    fn a_report_with_a_bad_commit_message_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        std::fs::write(root.join("src/a.rs"), "fn um() {}\nfn dois() {}\n").unwrap();
        let delivered = |summary: &str| {
            line("DELIVERED", json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": summary}))
        };
        let refused = round(root, "x", Some(&delivered("pedido de fulano@empresa.com.br")));
        assert_eq!(refused["reason"], json!("commit-forbidden-text"), "{refused}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 0, "nada foi gravado");

        let went = round(root, "x", Some(&delivered("a soma sai")));
        assert_eq!(went["ok"], json!(true), "{went}");
        let log = store::read(&path).unwrap().unwrap();
        assert_eq!(log.visible().iter().filter(|e| e.event_type == "delivered").count(), 1, "sem duplicar");
    }

    /// A rodada faz o commit da rodada e grava o código dele na spec.
    #[test]
    fn the_round_commits_and_records_the_commit_on_the_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn um() {}\nfn dois() {}\n").unwrap();
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        let report = line("DELIVERED", json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"],
            "commit": "a soma sai"}));
        let out = round(root, "x", Some(&report));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out["commit"]["title"], json!("feat(onda-1): a soma sai"), "{out}");
        let sha = out["commit"]["sha"].as_str().unwrap_or_default().to_string();
        assert_eq!(sha.len(), 40, "{out}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let commit = log.visible().into_iter().find(|e| e.event_type == "commit").expect("commit");
        assert_eq!(commit.str_field("sha"), Some(sha.as_str()));
        assert_eq!(commit.ints("waves"), vec![1]);
    }

    /// O commit da rodada leva a remoção nos dois casos: o arquivo apagado só
    /// no disco e o que já saiu do índice. A mudança comum vai junto.
    #[test]
    fn the_round_commits_a_file_deleted_on_disk_and_one_already_removed_from_the_index() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs", "src/c.rs"], &[])]);
        round(root, "x", None);
        git_at(root, &["config", "user.email", "t@t"]);
        git_at(root, &["config", "user.name", "t"]);
        git_at(root, &["config", "commit.gpgsign", "false"]);

        std::fs::remove_file(root.join("src/a.rs")).unwrap();
        git_at(root, &["rm", "-q", "src/b.rs"]);
        std::fs::write(root.join("src/c.rs"), "fn um() {}\nfn tres() {}\n").unwrap();

        let files = ["src/a.rs", "src/b.rs", "src/c.rs"];
        let report = line("DELIVERED", json!({"wave": 1, "text": "Dois arquivos saíram.", "files": files,
            "commit": "tira dois arquivos"}));
        let out = round(root, "x", Some(&report));
        assert_eq!(out["ok"], json!(true), "{out}");

        let shown = Command::new("git")
            .args(["show", "--name-status", "--format=", "HEAD"])
            .current_dir(root)
            .output()
            .unwrap();
        let shown = String::from_utf8_lossy(&shown.stdout).to_string();
        let changes: Vec<&str> = shown.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(changes, ["D\tsrc/a.rs", "D\tsrc/b.rs", "M\tsrc/c.rs"], "{out}");
        let status = Command::new("git").args(["status", "--porcelain"]).current_dir(root).output().unwrap();
        let pending = String::from_utf8_lossy(&status.stdout).to_string();
        assert!(!pending.contains("src/"), "nada da onda ficou fora do commit: {pending}");
    }

    /// A junção leva ao repositório principal o arquivo que a cópia apagou, e
    /// o commit leva a remoção. A cópia com uma mudança fora da lista
    /// entregue fica no disco, e o aviso diz qual arquivo e qual cópia; a que
    /// só mudou o que entregou é apagada.
    #[test]
    fn a_file_deleted_in_the_copy_is_deleted_and_a_copy_with_more_changes_stays_with_a_warning() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs"], &[]), (2, &["src/c.rs"], &[])]);
        round(root, "x", None);
        let copy = |wave: u64| mustard_core::io::wave_prompt::copy_path(root, "x", wave, false);
        std::fs::remove_file(copy(1).join("src/a.rs")).unwrap();
        std::fs::write(copy(1).join("src/b.rs"), "fn um() {}\nfn b() {}\n").unwrap();
        std::fs::write(copy(2).join("src/c.rs"), "fn um() {}\nfn c() {}\n").unwrap();
        std::fs::write(copy(2).join("src/esquecido.rs"), "fn esquecido() {}\n").unwrap();

        let one = line("DELIVERED", json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs", "src/b.rs"], "commit": "a sai"}));
        let two = line("DELIVERED", json!({"wave": 2, "text": "Saiu.", "files": ["src/c.rs"], "commit": "c muda"}));
        let out = round(root, "x", Some(&format!("{one}\n{two}")));
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(!root.join("src/a.rs").exists());
        assert_eq!(std::fs::read_to_string(root.join("src/c.rs")).unwrap(), "fn um() {}\nfn c() {}\n");
        assert!(!root.join("src/esquecido.rs").exists(), "only the delivered files are merged");
        let shown = Command::new("git").args(["show", "--name-status", "--format=", "HEAD"]).current_dir(root).output();
        let shown = String::from_utf8_lossy(&shown.unwrap().stdout).to_string();
        assert_eq!(shown.lines().collect::<Vec<_>>(), ["D\tsrc/a.rs", "M\tsrc/b.rs", "M\tsrc/c.rs"], "{out}");

        assert!(!copy(1).exists(), "the copy with only delivered changes is gone");
        assert!(copy(2).join("src/esquecido.rs").is_file(), "the copy with more changes stays");
        let kept = translate("round.copy_kept", Locale::PtBr)
            .replace("{wave}", "2")
            .replace("{copy}", &mustard_core::io::wave_prompt::shown(&copy(2)))
            .replace("{files}", "src/esquecido.rs");
        assert_eq!(out["warnings"], json!([{"reason": "copy-kept", "wave": 2, "hint": kept}]), "{out}");
    }

    /// Cada relatório de `wrong` é recusado pelo git, com o motivo que o git
    /// deu, e não deixa nada gravado; depois de `fix`, a chamada que entrega
    /// `fixed` grava a entrega uma vez só.
    fn refused_by_git_records_nothing(root: &Path, wrong: &[String], fix: impl FnOnce(), fixed: &[&str]) {
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        let before = spec_lines();
        for report in wrong {
            let refused = round(root, "x", Some(report));
            assert_eq!(refused["reason"], json!("git-refused"), "{refused}");
            assert_eq!(spec_lines(), before, "nothing was recorded: {refused}");
            let bare = translate("round.git_refused", Locale::PtBr).replace("{detail}", "");
            assert_ne!(refused["hint"], json!(bare), "the refusal carries git's reason: {refused}");
        }
        fix();
        let went = round(root, "x", Some(&delivered(root, 1, "Saiu.", fixed)));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "the corrected call records the delivery once");
    }

    /// O caminho que existe fora do repositório, absoluto ou com `../`, é
    /// recusado pelo git sem gravar nada. A chamada corrigida leva ao commit
    /// um arquivo novo, que ainda não estava no git.
    #[test]
    fn a_path_outside_the_repository_is_refused_by_git_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("fora.rs"), "fn fora() {}\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let name = outside.path().file_name().unwrap().to_string_lossy().to_string();
        let absolute = outside.path().join("fora.rs").to_string_lossy().to_string();
        let wrong: Vec<String> = [absolute, format!("../{name}/fora.rs")]
            .iter()
            .map(|path| delivered(root, 1, "Saiu.", &["src/a.rs", path.as_str()]))
            .collect();
        std::fs::write(root.join("src/novo.rs"), "fn novo() {}\n").unwrap();
        refused_by_git_records_nothing(root, &wrong, || {}, &["src/a.rs", "src/novo.rs"]);
        let shown = Command::new("git").args(["show", "--name-only", "--format=", "HEAD"]).current_dir(root).output();
        let shown = String::from_utf8_lossy(&shown.unwrap().stdout).to_string();
        assert!(shown.lines().any(|line| line == "src/novo.rs"), "the new file went into the commit: {shown}");
    }

    /// O caminho que o `.gitignore` ignora é recusado pelo git sem gravar
    /// nada, e a chamada sem ele grava a entrega uma vez só.
    #[test]
    fn an_ignored_path_is_refused_by_git_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join(".gitignore"), "src/gerado.rs\n").unwrap();
        std::fs::write(root.join("src/gerado.rs"), "fn gerado() {}\n").unwrap();

        let wrong = [delivered(root, 1, "Saiu.", &["src/a.rs", "src/gerado.rs"])];
        refused_by_git_records_nothing(root, &wrong, || {}, &["src/a.rs"]);
    }

    /// O gancho do commit que recusa não deixa nada gravado, e a chamada
    /// depois de o gancho sair grava a entrega uma vez só.
    #[cfg(unix)]
    #[test]
    fn a_commit_hook_that_refuses_records_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let hooks = root.join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho 'o gancho recusou' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);

        let wrong = [delivered(root, 1, "Saiu.", &["src/a.rs"])];
        refused_by_git_records_nothing(root, &wrong, || std::fs::remove_file(&hook).unwrap(), &["src/a.rs"]);
    }

    /// O arquivo de dentro de um submódulo é comitado no submódulo, e o
    /// principal leva o ponteiro. Quando o git recusa o commit do principal,
    /// o commit já feito no submódulo volta, com o disco e o índice dele; a
    /// chamada depois de o gancho sair comita uma vez só nos dois.
    #[cfg(unix)]
    #[test]
    fn a_refused_main_commit_undoes_the_submodule_commit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let root = &dir.path().join("principal");
        with_submodule(root, dir.path());
        approved(root, "x", &[(1, &["src/a.rs", "libs/sub/lib.txt"], &[])]);
        round(root, "x", None);
        let copy = mustard_core::io::wave_prompt::copy_path(root, "x", 1, false);
        assert!(copy.join("libs/sub/.git").is_file(), "the copy brings the submodule");
        std::fs::write(copy.join("src/a.rs"), "fn um() {}\nfn dois() {}\n").unwrap();
        std::fs::write(copy.join("libs/sub/lib.txt"), "fn um() {}\nfn sub() {}\n").unwrap();
        let sub = root.join("libs/sub");
        assert_eq!(git_text(&sub, &["rev-parse", "--abbrev-ref", "HEAD"]), "feature/x", "the same branch name");
        let before = git_text(&sub, &["rev-parse", "HEAD"]);

        let hooks = dir.path().join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho 'o gancho recusou' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);
        let report = line("DELIVERED", json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs", "libs/sub/lib.txt"],
            "commit": "a onda 1 sai"}));
        let refused = round(root, "x", Some(&report));
        assert_eq!(refused["reason"], json!("git-refused"), "{refused}");
        assert_eq!(git_text(&sub, &["rev-parse", "HEAD"]), before, "the submodule commit was undone");
        assert_eq!(git_text(&sub, &["status", "--porcelain"]), "", "the submodule disk and index are back");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn um() {}\n");

        std::fs::remove_file(&hook).unwrap();
        let went = round(root, "x", Some(&report));
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(git_text(&sub, &["rev-list", "--count", &format!("{before}..HEAD")]), "1", "one submodule commit");
        assert_eq!(git_text(root, &["rev-parse", "HEAD:libs/sub"]), git_text(&sub, &["rev-parse", "HEAD"]));
        assert_eq!(std::fs::read_to_string(sub.join("lib.txt")).unwrap(), "fn um() {}\nfn sub() {}\n");
        assert!(!copy.exists(), "the copy is gone, with the submodule copy inside it: {went}");
    }

    /// O passo do git roda com uma trava própria, e não com a da spec:
    /// enquanto o gancho do commit demora, quem lê a spec não espera por ele,
    /// e outro passo do git no mesmo checkout espera a vez até o commit
    /// terminar.
    #[cfg(unix)]
    #[test]
    fn a_slow_commit_hook_holds_the_git_step_and_not_the_readers_of_the_spec() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let hooks = root.join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let (started, release) = (hooks.join("comecou"), hooks.join("solta"));
        let hook = hooks.join("pre-commit");
        let script = format!(
            "#!/bin/sh\ntouch '{}'\nn=0\nwhile [ ! -f '{}' ] && [ $n -lt 600 ]; do sleep 0.05; n=$((n+1)); done\n",
            started.display(),
            release.display()
        );
        std::fs::write(&hook, script).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);
        let report = delivered(root, 1, "Saiu.", &["src/a.rs"]);
        let spec = store::spec_file(root, "x").unwrap();

        std::thread::scope(|scope| {
            let going = scope.spawn(|| round(root, "x", Some(&report)));
            for _ in 0..3000 {
                if started.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(started.exists(), "the commit hook never started");

            let (read_tx, read_rx) = mpsc::channel();
            let spec = &spec;
            scope.spawn(move || read_tx.send(store::read(spec).is_ok()));
            let read = read_rx.recv_timeout(Duration::from_secs(5));

            let (turn_tx, turn_rx) = mpsc::channel();
            scope.spawn(move || turn_tx.send(git_lock(root).is_ok()));
            let early = turn_rx.recv_timeout(Duration::from_millis(500));

            std::fs::write(&release, b"").unwrap();
            let out = going.join().unwrap();
            assert_eq!(read, Ok(true), "a reader of the spec does not wait for the commit hook");
            assert!(early.is_err(), "another git step waits while the commit runs");
            assert_eq!(turn_rx.recv_timeout(Duration::from_secs(30)), Ok(true), "its turn comes after the commit");
            assert_eq!(out["ok"], json!(true), "{out}");
        });
    }

    /// Com nada a comitar, o git recusa e dá o motivo na saída normal: a
    /// recusa traz esse motivo e não deixa nada gravado, e a chamada com o
    /// arquivo mudado grava a entrega uma vez só.
    #[test]
    fn nothing_to_commit_is_refused_with_gits_reason_and_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let unchanged = [line("DELIVERED", json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"],
            "commit": "a onda 1 saiu"}))];
        refused_by_git_records_nothing(root, &unchanged, || {}, &["src/a.rs"]);
    }

    /// Num projeto sem formatador configurado nada é formatado e nada é
    /// avisado; num projeto com Prettier configurado e sem Prettier no disco,
    /// o aviso sai com o nome do formatador, em vez de a formatação ser
    /// pulada em silêncio. Só os arquivos da rodada entram.
    #[test]
    fn the_formatter_of_the_project_runs_only_on_the_round_files_and_says_when_it_is_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["a.ts", "fora.ts"] {
            std::fs::write(root.join("src").join(name), "const x=1\n").unwrap();
        }
        let files = vec!["src/a.ts".to_string()];

        let never = |_: &str, _: &[&str]| false;
        let always = |program: &str, args: &[&str]| {
            assert_eq!(program, "npx");
            assert!(args.contains(&"src/a.ts"), "{args:?}");
            assert!(!args.contains(&"src/fora.ts"), "só os arquivos da rodada: {args:?}");
            true
        };

        assert_eq!(format_with(root, &files, &never), Formatting::default(), "sem formatador, nada");

        std::fs::write(root.join(".prettierrc"), b"{}").unwrap();
        let out = format_with(root, &files, &always);
        assert_eq!(out.formatted, vec!["src/a.ts".to_string()]);
        assert!(out.missing.is_empty(), "{out:?}");

        let out = format_with(root, &files, &never);
        assert!(out.formatted.is_empty(), "{out:?}");
        assert_eq!(out.missing, vec!["Prettier".to_string()], "o formatador some pelo nome");
        assert_eq!(
            std::fs::read_to_string(root.join("src/fora.ts")).unwrap(),
            "const x=1\n",
            "o arquivo fora da rodada fica byte a byte"
        );
    }

    /// O ramo do projeto .NET: o formatador roda uma vez por arquivo da
    /// rodada, sempre com o projeto da raiz, nunca num arquivo de fora, e some
    /// pelo nome quando não está na máquina.
    #[test]
    fn the_dotnet_formatter_runs_once_per_round_file_and_says_when_it_is_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["a.cs", "b.cs", "fora.cs"] {
            std::fs::write(root.join("src").join(name), "class A {}\n").unwrap();
        }
        let files = vec!["src/a.cs".to_string(), "src/b.cs".to_string()];

        let never = |_: &str, _: &[&str]| false;
        assert_eq!(format_with(root, &files, &never), Formatting::default(), "sem projeto .NET, nada");

        std::fs::write(root.join("Loja.csproj"), b"<Project />").unwrap();
        let calls: std::cell::RefCell<Vec<Vec<String>>> = std::cell::RefCell::new(Vec::new());
        let always = |program: &str, args: &[&str]| {
            assert_eq!(program, "dotnet");
            calls.borrow_mut().push(args.iter().map(|a| (*a).to_string()).collect());
            true
        };
        let out = format_with(root, &files, &always);
        assert_eq!(out.formatted, files);
        assert!(out.missing.is_empty(), "{out:?}");
        let calls = calls.into_inner();
        assert_eq!(calls.len(), 2, "uma chamada por arquivo da rodada: {calls:?}");
        assert!(calls.iter().all(|c| c.contains(&"Loja.csproj".to_string())), "{calls:?}");
        assert!(!calls.iter().any(|c| c.contains(&"src/fora.cs".to_string())), "{calls:?}");

        let out = format_with(root, &files, &never);
        assert!(out.formatted.is_empty(), "{out:?}");
        assert_eq!(out.missing, vec!["dotnet format".to_string()], "o formatador some pelo nome");
        assert_eq!(
            std::fs::read_to_string(root.join("src/fora.cs")).unwrap(),
            "class A {}\n",
            "o arquivo fora da rodada fica byte a byte"
        );
    }
}
