//! O commit da rodada: a junção de cada cópia ao repositório principal, a
//! mensagem montada do resumo de cada entrega e conferida, a formatação só dos
//! arquivos da rodada, o commit, a gravação dele na spec e a cópia apagada.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use mustard_core::domain::project_map::ProjectMap;
use mustard_core::domain::scan::ScanReport;
use mustard_core::domain::spec_events::{
    check_message, MessageRefusal, Refusal, SpecLog, MESSAGE_BODY_MAX, MESSAGE_TITLE_MAX,
};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::wave_prompt::recorded_copy;
use mustard_core::platform::git as git_exec;
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::platform::process;
use serde_json::{json, Map, Value};

use super::answer::RoundRefusal;
use super::report::WaveReport;
use super::slots::{reset_committed_slot, sharing_copy};
use crate::commands::git_settle::{enter_unit_branch, submodule_holding, submodules_of};
use crate::commands::review::qa_run::ProofFault;
use crate::commands::spec_events::write::record;

/// A entrega pede commit? Só quando mudou arquivo: a onda que volta sem
/// arquivo mudado fecha como conferência, sem commit. É a leitura única dessa
/// decisão — a rodada a usa ao escolher o que comitar ([`commit_message`]) e
/// o fechamento, por [`waves_checked_only`], ao cobrar o commit de cada onda
/// —, para as duas nunca discordarem de quando uma onda termina sem commit.
pub(crate) fn needs_commit(files: &[String]) -> bool {
    !files.is_empty()
}

/// As ondas cuja entrega mais recente não mudou arquivo: fecharam como
/// conferência, pela mesma leitura da rodada ([`needs_commit`]), e não têm
/// commit a cobrar. A onda cuja entrega mais recente mudou arquivo fica de
/// fora, e a onda sem entrega nenhuma também.
pub(crate) fn waves_checked_only(log: &SpecLog) -> BTreeSet<u64> {
    log.last_by_wave("delivered")
        .into_iter()
        .filter(|(_, id)| {
            let files: Vec<String> = log
                .get(*id)
                .and_then(|delivery| delivery.fields.get("files"))
                .and_then(Value::as_array)
                .map(|files| files.iter().map(|f| f.as_str().map_or_else(|| f.to_string(), str::to_string)).collect())
                .unwrap_or_default();
            !needs_commit(&files)
        })
        .map(|(wave, _)| wave)
        .collect()
}

/// Quantos caracteres do resumo da primeira onda o título guarda, no mínimo,
/// quando o escopo com todas as ondas não deixa espaço para ele.
const SUMMARY_ROOM_MIN: usize = 20;

/// A mensagem do commit da rodada, montada do resumo que cada entrega traz e
/// já conferida: o título no molde do repositório (`tipo(escopo): frase`),
/// com o resumo da primeira onda, e o corpo com uma linha por onda. O escopo
/// do título cita todas as ondas que o commit leva; quando o resumo da
/// primeira não cabe ao lado delas, é o resumo que encolhe. Só quando o
/// escopo sozinho não deixa espaço para o começo do resumo é ele que encolhe:
/// cita as primeiras ondas que deixam espaço e conta as outras (`ondas-1-2+9`). O tipo é `fix` quando a rodada traz um conserto, e `feat` nos outros casos.
/// `None` quando nenhuma entrega traz arquivo.
pub(super) fn commit_message(waves: &[WaveReport], lang: Locale) -> Result<Option<(String, String)>, RoundRefusal> {
    let committed: Vec<(&WaveReport, &str)> = waves
        .iter()
        .filter(|w| needs_commit(&w.files))
        .filter_map(|w| w.commit.as_deref().map(|summary| (w, summary)))
        .collect();
    let Some((_, first)) = committed.first() else {
        return Ok(None);
    };
    let numbers: Vec<String> = committed.iter().map(|(w, _)| w.wave.to_string()).collect();
    let kind = if committed.iter().any(|(w, _)| !w.fixes.is_empty()) { "fix" } else { "feat" };
    let prefix = |kept: usize| {
        let key = if numbers.len() == 1 { "round.commit.scope.one" } else { "round.commit.scope.many" };
        let mut waves = numbers[..kept].join("-");
        if kept < numbers.len() {
            let _ = write!(waves, "+{}", numbers.len() - kept);
        }
        format!("{kind}({}): ", translate(key, lang).replace("{waves}", &waves))
    };
    // O escopo com todas as ondas pode passar o teto do título quando há mais
    // de uma. Perder uma onda do escopo esconde do histórico do git o que o
    // commit leva, então o escopo fica inteiro e o resumo da primeira onda é
    // cortado até caber; o corpo traz o resumo de cada onda por inteiro. Só
    // quando nem o começo do resumo cabe ao lado do escopo inteiro é o escopo
    // que encolhe: cita as primeiras ondas que deixam espaço e conta as outras (`ondas-1-2+9`),
    // de modo que o título nunca sai vazio nem recusado.
    let lead = prefix(numbers.len());
    let title = if numbers.len() > 1 && lead.chars().count() + first.chars().count() > MESSAGE_TITLE_MAX {
        let wanted = first.chars().count().min(SUMMARY_ROOM_MIN);
        let kept = (1..=numbers.len()).rev().find(|kept| prefix(*kept).chars().count() + wanted <= MESSAGE_TITLE_MAX);
        let lead = prefix(kept.unwrap_or(1));
        let room = MESSAGE_TITLE_MAX.saturating_sub(lead.chars().count());
        let summary = shorten_to(first, room);
        let summary = if summary.is_empty() { first.chars().take(room).collect() } else { summary };
        format!("{lead}{summary}")
    } else {
        format!("{lead}{first}")
    };
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

/// O começo de `text` que cabe em `room` caracteres, cortado numa palavra
/// inteira quando há palavra para cortar, sem sobra de espaço nem de
/// pontuação no fim. O texto que já cabe volta como está.
fn shorten_to(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let kept: String = text.chars().take(room).collect();
    let whole_word = text.chars().nth(room).is_some_and(char::is_whitespace);
    let cut = if whole_word { kept.as_str() } else { kept.rsplit_once(char::is_whitespace).map_or(kept.as_str(), |(head, _)| head) };
    cut.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '.')).to_string()
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

/// Roda um programa na raiz do projeto, pelo arquivo que o `PATH` tem para
/// ele (no Windows, o `npx.cmd` do `npx`); `false` quando ele não está lá ou
/// saiu com erro.
fn run(root: &Path, program: &str, args: &[&str]) -> bool {
    process::command(program)
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

/// O commit em que a cópia `copy_repo` nasceu: o antepassado comum dela com
/// o repositório `root_repo` agora, e não o HEAD dela. Usar o HEAD faria a
/// junção não enxergar nada para trazer de uma cópia que ganhou commit
/// próprio — o disco dela já bate com esse HEAD, então a comparação sairia
/// sempre igual, e o arquivo comitado dentro da cópia nunca chegaria ao
/// repositório principal. A cópia é um checkout ligado (`git worktree`) do
/// mesmo repositório, então o antepassado comum sempre existe.
fn fork_point(copy_repo: &Path, root_repo: &Path) -> Result<String, String> {
    let root_head = git(root_repo, &["rev-parse", "HEAD"])?;
    let base = git(copy_repo, &["merge-base", "HEAD", root_head.trim()])?;
    Ok(base.trim().to_string())
}

/// Depois do commit da rodada, o mapa relê só os arquivos que mudaram, pela
/// leitura por partes que a ferramenta do scan já faz sozinha: sem isso, a
/// sugestão de skill e de arquivos parecidos, antes do envio da onda
/// seguinte, apontaria um arquivo que este commit acabou de apagar. Depois
/// do mapa, lê do provedor o texto dos pull requests que a história dele
/// cita e ainda não tem. Nunca trava a rodada nem avisa: sem o mapa, sem a
/// ferramenta ou sem o provedor, a sugestão e a história seguem com o que já
/// tinham.
pub(super) fn refresh_map(root: &Path, mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>) {
    let _ = mine(root, &mustard_core::io::project_map::model_path(root));
    crate::shared::pr_history::refresh(root);
}

/// A conferência do mapa com o conteúdo de agora, antes de toda resposta
/// dele: quando o commit do checkout `root` ou o conteúdo de algum arquivo
/// não é o da passada que gravou o mapa — um arquivo editado sem commit,
/// uma troca de branch, um commit à mão ou um pull —, quando um bloco que
/// a passada grava voltou vazio numa troca de formato, ou quando o mapa é
/// de outra compilação do scan que a de agora, mesmo com o projeto parado,
/// chama [`refresh_map`] com o mesmo `mine`, que relê só os arquivos de blob
/// novo, ou todos quando o bloco voltou vazio ou a marca é outra. Decide
/// pelo estado gravado e pelas marcas dos blocos, sem ler o mapa inteiro. A
/// marca da compilação de agora é a que o scan achado ao lado deste programa
/// diz ([`mustard_core::Scan::format`]), pedida só quando o mapa traz marca
/// com que comparar. Dentro do git e sem o arquivo do mapa, o mapa é criado
/// pela mesma passada: é o único lugar que o cria fora da instalação. Sem git
/// ou com o mapeador falhando, segue sem travar e sem aviso novo.
pub(crate) fn refresh_map_if_stale(root: &Path, mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>) {
    refresh_map_if_behind(root, mine, &installed_scan_format);
}

/// A marca de formato do scan achado ao lado deste programa. Os testes da
/// biblioteca nunca a pedem a um scan de verdade: o que existe na máquina que
/// os roda mudaria o resultado deles, e os mapas que eles gravam trazem a
/// marca que o teste escolheu. O programa inteiro, com o scan ao lado, é
/// provado pelo teste de integração `map_of_another_scan`.
fn installed_scan_format() -> Option<String> {
    if cfg!(test) {
        return None;
    }
    mustard_core::Scan::locate().format()
}

/// [`refresh_map_if_stale`] com a marca de formato do scan dada por `format`,
/// em vez da do scan achado ao lado deste programa.
fn refresh_map_if_behind(
    root: &Path,
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>,
    format: &dyn Fn() -> Option<String>,
) {
    if mustard_core::io::project_map::is_behind(root, format) {
        refresh_map(root, mine);
    }
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
pub(super) fn repo_of(dir: &Path, subs: &[String], file: &str) -> (PathBuf, String) {
    match submodule_holding(subs, file) {
        Some((sub, inner)) => (dir.join(sub), inner),
        None => (dir.to_path_buf(), file.to_string()),
    }
}

/// A pasta da cópia que a rodada criou para a onda `wave`, quando o envio
/// dela gravou uma e ela ainda está no disco.
pub(super) fn copy_of(log: &SpecLog, wave: u64) -> Option<PathBuf> {
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
                let base = fork_point(&copy_repo, &root_repo).map_err(|detail| RoundRefusal::Git { detail })?;
                bases.insert(copy_repo.clone(), base);
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

/// Os arquivos que a cópia `copy` mudou de fato, pelo `git status` dela, com
/// o arquivo de dentro de um submódulo prefixado pelo caminho dele: é o dono
/// de verdade do que entra no commit da rodada, e não a lista que a entrega
/// citou, que vira só conferência.
fn copy_changed(copy: &Path, subs: &[String]) -> Vec<String> {
    let status = ["status", "--porcelain", "-z", "--untracked-files=all", "--ignore-submodules=all"];
    let mut changed = changed_paths(&git(copy, &status).unwrap_or_default());
    for sub in subs.iter().filter(|sub| copy.join(sub).join(".git").is_file()) {
        let theirs = changed_paths(&git(&copy.join(sub), &status).unwrap_or_default());
        changed.extend(theirs.into_iter().map(|path| format!("{sub}/{path}")));
    }
    changed
}

/// Depois do commit da rodada, zera a cópia de cada onda de `waves`, as que
/// entraram nele ([`reset_committed_slot`]), sem guardar nada: o código dela
/// está no commit, e deixá-la suja faria a próxima onda na vaga ver, com a
/// base já adiante, um código que a história já tem como se tivesse ficado
/// sem commit. `files` são os arquivos que o commit levou.
///
/// Fica como está a pasta que não é cópia viva do git, a cópia que outra onda
/// também segura ([`sharing_copy`]) e a que tem mudança que o commit não
/// levou: essa não é zerada aqui, e a próxima abertura da vaga a guarda como
/// guarda qualquer outra. A onda segurada por conflito não vem em `waves`, e
/// a cópia dela segue com o código. A cópia que o git não deixou zerar também
/// fica, e o código dela já está no commit.
pub(super) fn reset_committed_copies(root: &Path, log: &SpecLog, waves: &[u64], files: &[String]) {
    let shared = sharing_copy(log, waves.iter().copied());
    let subs = submodules_of(root);
    let committed: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    for wave in waves.iter().filter(|wave| !shared.contains(wave)) {
        let Some(copy) = copy_of(log, *wave) else { continue };
        if copy_changed(&copy, &subs).iter().any(|file| !committed.contains(file.as_str())) {
            continue;
        }
        let _ = reset_committed_slot(root, &copy);
    }
}

/// Os arquivos que a onda `wave` mudou de fato, pela cópia gravada no envio
/// mais novo dela; `None` sem cópia gravada, ou sem ela mais no disco. É o
/// conjunto que a rodada comita, mesmo quando a linha de entrega cita outro.
pub(super) fn real_changed_files(root: &Path, log: &SpecLog, wave: u64) -> Option<Vec<String>> {
    let copy = copy_of(log, wave)?;
    let subs = submodules_of(root);
    Some(copy_changed(&copy, &subs))
}

/// Compila o repositório principal com o comando de compilação do projeto, o
/// mesmo que o pedido de cada onda já ensina; sem ele declarado, nada é
/// rodado, porque não há como compilar sem saber o comando. A rodada não
/// comita nada quando a compilação falha.
pub(super) fn ensure_builds(root: &Path) -> Result<(), RoundRefusal> {
    let Some(build) = mustard_core::ProjectConfig::load(root).commands().build else {
        return Ok(());
    };
    let out = crate::commands::review::qa_run::run_command(&build, root);
    if out.result == "pass" {
        return Ok(());
    }
    Err(RoundRefusal::BuildFailed { command: build, output: out.output })
}

/// Um achado da conferência depois da onda, da onda `wave`: a frase pronta e
/// se ele recusa a volta ou só avisa.
pub(super) struct Finding {
    pub(super) wave: u64,
    pub(super) refuses: bool,
    pub(super) text: String,
}

/// O que a conferência depois da onda compara: o mapa da base da rodada, o
/// de depois da junção das ondas, e os arquivos que cada onda mudou, pela
/// onda.
pub(super) struct AfterWave {
    pub(super) base: ProjectMap,
    pub(super) after: ProjectMap,
    pub(super) changed: Vec<(u64, Vec<String>)>,
}

/// O que a conferência depois da onda devolve sem recusa: os avisos da
/// rodada e, à parte, a linha de tamanho de cada onda, pelo número dela.
type AfterWaveChecks = (Vec<Value>, Vec<(u64, String)>);

/// A conferência depois da onda, antes do commit da rodada, com o disco já
/// juntado: as importações novas contra o padrão do projeto
/// ([`super::imports_check`]), os restos do que as ondas tiraram
/// ([`super::removed_check`]) e o tamanho de cada onda contra o que a tarefa
/// pede ([`super::size_check`]). Tudo passou: nada, nem texto. Só avisos: os
/// avisos, e a rodada segue. Algum achado que recusa: a rodada não comita,
/// e a mensagem lista tudo de uma vez, por onda, com a rodada de conserto de
/// cada uma — a volta que a onda grava de novo conta como uma, até
/// [`super::stops::MAX_FIX_ROUNDS`]. A onda que já passou por todas vira
/// pergunta ao usuário. Sem mapa da base, com o mapa sem arquivos, ou com o
/// mapeador falhando, não há com que comparar e a rodada segue.
///
/// Com a conferência sem recusa, junta aos avisos uma linha de tamanho por
/// onda e a devolve também à parte, para o corpo do commit. A linha é dado
/// da onda: o achado de tamanho é outro, e passa pela mesma resposta dos
/// outros.
pub(super) fn ensure_after_wave(
    root: &Path,
    log: &SpecLog,
    waves: &[WaveReport],
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>,
    lang: Locale,
) -> Result<AfterWaveChecks, RoundRefusal> {
    let changed: Vec<(u64, Vec<String>)> =
        waves.iter().filter(|w| !w.files.is_empty()).map(|w| (w.wave, w.files.clone())).collect();
    let Some(maps) = after_wave_maps(root, changed, mine) else { return Ok(Default::default()) };
    let sizes = super::size_check::measure(root, &maps);
    let mut found = super::imports_check::findings(root, &maps, log, lang);
    found.extend(super::removed_check::findings(root, &maps, lang));
    found.extend(super::size_check::findings(root, &sizes, log, lang));
    let mut warnings = after_wave_answer(waves, &found, lang)?;
    let sizes = super::size_check::lines(&sizes, lang);
    warnings.extend(sizes.iter().map(|(wave, hint)| json!({ "reason": "wave-size", "wave": wave, "hint": hint })));
    Ok((warnings, sizes))
}

/// Os dois mapas da conferência depois da onda: o da base, lido do mapa do
/// projeto, e o de depois, que o mapeador relê numa cópia do da base, fora
/// do projeto — só o que mudou é lido de novo, e o mapa do projeto fica
/// como estava até o commit.
fn after_wave_maps(
    root: &Path,
    changed: Vec<(u64, Vec<String>)>,
    mine: &dyn Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport>,
) -> Option<AfterWave> {
    if changed.is_empty() {
        return None;
    }
    let base = mustard_core::io::project_map::read(root).ok().filter(|map| !map.modules.is_empty())?;
    let dir = tempfile::tempdir().ok()?;
    let model = dir.path().join("grain.db");
    std::fs::copy(mustard_core::io::project_map::model_path(root), &model).ok()?;
    mine(root, &model).ok()?;
    let after = mustard_core::io::project_map::read_at(&model).ok()?;
    Some(AfterWave { base, after, changed })
}

/// A resposta da conferência depois da onda aos achados `found`.
fn after_wave_answer(waves: &[WaveReport], found: &[Finding], lang: Locale) -> Result<Vec<Value>, RoundRefusal> {
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let max = super::stops::MAX_FIX_ROUNDS;
    let done = |wave: u64| waves.iter().find(|w| w.wave == wave).map_or(0, |w| w.returns.len().saturating_sub(1));
    let refusing: BTreeSet<u64> = found.iter().filter(|f| f.refuses).map(|f| f.wave).collect();
    let stuck: Vec<String> = refusing.iter().filter(|w| done(**w) >= max).map(u64::to_string).collect();
    let fill = |key: &str, wave: u64| {
        translate(key, lang)
            .replace("{wave}", &wave.to_string())
            .replace("{round}", &(done(wave) + 1).min(max).to_string())
            .replace("{max}", &max.to_string())
            .replace("{waves}", &stuck.join(", "))
    };
    let head = match (refusing.is_empty(), stuck.is_empty()) {
        (true, _) => "round.after_wave.warnings",
        (false, true) => "round.after_wave",
        (false, false) => "round.after_wave.limit",
    };
    let mut text = fill(head, 0);
    let listed: BTreeSet<u64> = found.iter().map(|f| f.wave).collect();
    for wave in listed {
        let key = if refusing.contains(&wave) { "round.after_wave.wave" } else { "round.after_wave.wave_warnings" };
        text.push_str("\n\n");
        text.push_str(&fill(key, wave));
        let lines = found.iter().filter(|f| f.wave == wave);
        text.extend(lines.clone().filter(|f| f.refuses).chain(lines.filter(|f| !f.refuses)).map(|f| format!("\n- {}", f.text)));
    }
    if refusing.is_empty() {
        return Ok(vec![json!({ "reason": "round-after-wave-warnings", "hint": text })]);
    }
    let question = (!stuck.is_empty()).then(|| fill("round.after_wave.question", 0));
    Err(RoundRefusal::AfterWave { text, question })
}

/// A prova de cada critério que as ondas de `waves` cobrem roda, uma de cada
/// vez e na ordem do código, antes do commit da rodada — o mesmo laço que o
/// fechamento roda para os critérios da spec inteira
/// ([`crate::commands::review::qa_run::run_criteria_proofs`]), aqui só com
/// os critérios que estas ondas apontam. A que não passa recusa pelo motivo
/// que o laço leu, com o código do critério: a que não executa ou sai com
/// erro traz o comando inteiro e a saída de erro, a que sai verde sem rodar
/// teste traz o comando, e a que cita um teste inexistente traz o nome que
/// faltou. A rodada não comita nada.
///
/// O critério para o qual uma onda da rodada entregou prova nova (`delivered`,
/// cada uma pelo número vigente do critério, já resolvida pela conferência
/// que a grava depois do commit) roda a entregue no lugar da gravada: a onda
/// que muda o nome de um teste entrega a prova com o nome novo, e a gravada,
/// que cita o nome antigo, recusaria a entrega por um teste que ela mesma
/// tirou. Os outros critérios rodam a gravada.
///
/// O critério que outra tarefa ainda por entregar também cobre — no backlog,
/// numa onda que não está entre as de `waves` ou entre as tarefas que a
/// volta diz não ter feito (`undone`, pelo número de cada uma) — não roda
/// agora: a prova dele depende do que essa tarefa ainda vai entregar, e
/// recusaria a entrega de uma onda por um trabalho que não é dela. Ele volta
/// a rodar na rodada em que entra a última tarefa que o cobre; o fechamento
/// roda todos os critérios da spec de qualquer jeito. A prova nova entregue
/// para um critério que ficou de fora roda uma vez depois do commit, como a
/// de um critério que nenhuma onda da rodada cobre.
///
/// Devolve os comandos que rodaram, todos verdes: a prova nova que já passou
/// aqui não roda de novo depois do commit.
pub(super) fn ensure_criteria_proofs(
    root: &Path,
    log: &SpecLog,
    waves: &[u64],
    undone: &[u64],
    delivered: &[(u64, String)],
) -> Result<Vec<String>, RoundRefusal> {
    let codes = log.codes();
    let returning: BTreeSet<u64> = waves.iter().copied().collect();
    let mut waiting = super::agreed::covered_codes(log, &returning);
    waiting.extend(
        undone
            .iter()
            .filter_map(|id| log.get(*id))
            .flat_map(|task| task.ints("covers"))
            .filter_map(|id| codes.get(&id).cloned()),
    );
    let criteria: Vec<(u64, String, String)> = log
        .criteria_for_waves(waves)
        .into_iter()
        .filter_map(|e| {
            let code = codes.get(&e.id).cloned().unwrap_or_else(|| e.id.to_string());
            if waiting.contains(&code) {
                return None;
            }
            let proof = match delivered.iter().find(|(id, _)| *id == e.id) {
                Some((_, proof)) => proof.clone(),
                None => e.str_field("proof")?.trim().to_string(),
            };
            Some((e.id, code, proof))
        })
        .collect();
    let (_, failed) = crate::commands::review::qa_run::run_criteria_proofs(root, &criteria);
    let Some(failed) = failed else { return Ok(criteria.into_iter().map(|(_, _, proof)| proof).collect()) };
    Err(match failed.fault {
        ProofFault::RanNoTest(tests) => {
            RoundRefusal::CriterionRanNoTest { code: failed.code, command: failed.command, tests }
        }
        ProofFault::MissingTest(name) => RoundRefusal::CriterionMissingTest { code: failed.code, name },
        ProofFault::Failed(output) => {
            RoundRefusal::CriterionProofFailed { code: failed.code, command: failed.command, output }
        }
    })
}

/// Os arquivos de uma rodada que mudam o programa: o que mora sob `apps/` ou
/// `packages/`. Um documento, uma spec ou um texto do plugin não pedem
/// compilação nenhuma.
fn changes_the_program(files: &[String]) -> bool {
    files.iter().any(|file| file.starts_with("apps/") || file.starts_with("packages/"))
}

/// Compila a versão em construção do Mustard depois do commit de uma onda que
/// mexeu no programa, em primeiro plano, e devolve o aviso da falha: a sessão
/// roda o programa compilado, e quem o refaz a cada commit é a rodada, para
/// que a próxima chamada já rode o código recém-comitado.
///
/// Nada roda fora do repositório do Mustard (`mustard_core::mustard_checkout`),
/// e nada roda quando os arquivos do commit não tocaram `apps/` nem
/// `packages/`. A compilação vai para a pasta de
/// `mustard_core::io::wave_prompt::development_build_dir`, e nenhum programa
/// é instalado em lugar nenhum. Com a compilação vermelha, a sessão segue no
/// programa compilado anterior, e o aviso traz o fim da saída — é ela que diz o
/// que consertar —; a rodada nunca recusa por isso, porque o commit já saiu.
pub(super) fn build_development_version(root: &Path, files: &[String], lang: Locale) -> Option<Value> {
    build_development_version_with(root, files, lang, &|command, cwd| {
        crate::commands::review::qa_run::run_server_command(command, cwd)
    })
}

/// [`build_development_version`] com o executor recebido, que é como um teste
/// prova a regra sem rodar o `cargo` de verdade.
fn build_development_version_with(
    root: &Path,
    files: &[String],
    lang: Locale,
    exec: &dyn Fn(&str, &Path) -> crate::commands::review::qa_run::ProofRun,
) -> Option<Value> {
    if !changes_the_program(files) {
        return None;
    }
    let main = mustard_core::mustard_checkout(root)?;
    let target = mustard_core::io::wave_prompt::development_build_dir(&main);
    let built = exec(&crate::shared::development_build::build_command(&target), &main);
    if built.result == "pass" {
        return None;
    }
    let hint = translate("round.development_build_failed", lang).replace("{output}", &built.output);
    Some(json!({ "reason": "development-build-failed", "hint": hint }))
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
pub(super) fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    git_with(root, args, &[])
}

/// O mesmo que [`git`], com variáveis de ambiente a mais para esta chamada.
pub(super) fn git_with(root: &Path, args: &[&str], env: &[(&str, String)]) -> Result<String, String> {
    let env: Vec<(&str, &str)> = env.iter().map(|(name, value)| (*name, value.as_str())).collect();
    let out = git_exec::run_env(root, args, &env);
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
    use std::process::Command;

    use mustard_core::io::project_map;
    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use super::*;
    use crate::commands::flow::round::tests::*;

    /// A ferramenta que grava quantas vezes foi chamada e falha sempre, como
    /// um scan que não está instalado.
    fn mine_counting(calls: &std::cell::Cell<usize>) -> impl Fn(&Path, &Path) -> mustard_core::platform::error::Result<ScanReport> + '_ {
        move |_, _| {
            calls.set(calls.get() + 1);
            Err(mustard_core::platform::error::Error::check_failed("scan: not found"))
        }
    }

    /// Sem git no diretório e com um mapa que já é o do commit e do conteúdo
    /// de agora, a ferramenta do scan nunca roda por [`refresh_map_if_stale`];
    /// um arquivo editado sem commit, um commit à mão e o mapa gravado sem a
    /// listagem a fazem rodar; e, quando ela falha, a chamada não trava nem
    /// propaga o erro.
    #[test]
    fn a_stale_map_without_what_it_needs_never_breaks() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let calls = std::cell::Cell::new(0);
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        // O mapa gravado com o estado de uma passada: o commit e a marca da
        // listagem que ela leu.
        let map_at = |head: &str, listing: &str| {
            let base = project_map::base_of(root);
            let state = format!(
                r#"{{"state": {{"head": "{head}", "listing": "{listing}", "base": "{}", "base_tip": "{}"}}}}"#,
                base.name, base.tip
            );
            project_map::write_text(root, &state).unwrap();
        };

        // Sem mapa e sem git: nada de que ler o mapa, a ferramenta não roda,
        // e nenhum mapa nasce.
        refresh_map_if_stale(root, &mine_counting(&calls));
        assert_eq!(calls.get(), 0, "sem mapa e sem git, a ferramenta não é chamada");
        assert!(!project_map::model_path(root).exists(), "fora do git a conferência não cria mapa");

        // Mapa fora de um repositório git: sem como comparar, a ferramenta
        // não roda.
        map_at("abc123", "x");
        refresh_map_if_stale(root, &mine_counting(&calls));
        assert_eq!(calls.get(), 0, "sem git, a ferramenta não é chamada");

        // Um repositório git de verdade, com o mapa já no commit e no
        // conteúdo de agora: a ferramenta segue sem rodar. O próprio mapa,
        // fora do git e sem regra que o ignore, não conta como mudança.
        git(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "x").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "semente"]);
        let now = project_map::listing(root).expect("dentro do git");
        map_at(&now.head, &now.digest());
        refresh_map_if_stale(root, &mine_counting(&calls));
        assert_eq!(calls.get(), 0, "o mapa já é o do commit e do conteúdo de agora");

        // Um arquivo editado sem commit: a ferramenta relê, e a falha dela
        // não trava nem propaga.
        std::fs::write(root.join("a.txt"), "y").unwrap();
        refresh_map_if_stale(root, &mine_counting(&calls));
        assert_eq!(calls.get(), 1, "o conteúdo mudou sem commit: a ferramenta é chamada, mesmo falhando");

        // O commit andou com o mesmo conteúdo: a história do mapa ficou para
        // trás.
        std::fs::write(root.join("a.txt"), "x").unwrap();
        git(&["commit", "--allow-empty", "-q", "-m", "fora da rodada"]);
        refresh_map_if_stale(root, &mine_counting(&calls));
        assert_eq!(calls.get(), 2, "o commit andou: a ferramenta é chamada");

        // O mapa de antes da listagem, só com o commit: relê.
        let now = project_map::listing(root).expect("dentro do git");
        map_at(&now.head, "");
        refresh_map_if_stale(root, &mine_counting(&calls));
        assert_eq!(calls.get(), 3, "o mapa sem a listagem é relido");
    }

    /// Dentro do git e sem o arquivo do mapa, [`refresh_map_if_stale`] chama a
    /// ferramenta do scan uma vez e o mapa que ela grava passa a existir; com
    /// ele em dia, a chamada seguinte não a roda de novo. Uma ferramenta que
    /// falha deixa o projeto sem mapa, sem travar nem propagar o erro, e a
    /// conferência seguinte tenta de novo. Apagado o mapa, ele volta.
    #[test]
    fn a_project_in_git_without_a_map_has_it_created_and_again_after_it_is_deleted() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("a.txt", "x")]);
        let calls = std::cell::Cell::new(0);
        let model = project_map::model_path(root);

        // Uma ferramenta que falha: o projeto segue sem mapa, e a conferência
        // seguinte tenta de novo.
        refresh_map_if_stale(root, &mine_counting(&calls));
        assert_eq!(calls.get(), 1, "sem o mapa, a ferramenta roda uma vez");
        assert!(!model.exists(), "a ferramenta falhou: nada foi criado");
        refresh_map_if_stale(root, &mine_counting(&calls));
        assert_eq!(calls.get(), 2, "o mapa continua faltando: a conferência tenta de novo");

        // Uma ferramenta que grava o mapa do commit e do conteúdo de agora.
        let writing = |root: &Path, _out: &Path| {
            calls.set(calls.get() + 1);
            let now = project_map::listing(root).expect("dentro do git");
            let state = format!(
                r#"{{"state": {{"head": "{}", "listing": "{}", "base": "{}", "base_tip": "{}"}}}}"#,
                now.head,
                now.digest(),
                now.base.name,
                now.base.tip
            );
            project_map::write_text(root, &state).expect("o mapa é gravado");
            Ok(ScanReport::default())
        };
        refresh_map_if_stale(root, &writing);
        assert_eq!(calls.get(), 3, "a ferramenta cria o mapa que faltava");
        assert!(model.exists(), "o mapa existe depois da conferência");
        refresh_map_if_stale(root, &writing);
        assert_eq!(calls.get(), 3, "o mapa criado já é o do conteúdo de agora: a ferramenta não roda de novo");

        std::fs::remove_file(&model).unwrap();
        refresh_map_if_stale(root, &writing);
        assert_eq!(calls.get(), 4, "o mapa apagado é criado de novo");
        assert!(model.exists(), "o mapa apagado voltou");
    }

    /// Com o commit e o conteúdo da passada que gravou o mapa, a ferramenta
    /// do scan roda por [`refresh_map_if_behind`] só quando a marca de formato
    /// que o scan diz não é a dos blocos do mapa: a mesma, ou a de um scan que
    /// não responde, deixa o mapa como está. A marca dos blocos é a de uma
    /// compilação do scan; o projeto parado e o mapa no mesmo commit não a
    /// mudam.
    #[test]
    fn a_map_of_another_scan_build_is_read_again_even_with_the_project_parked() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("a.txt", "x")]);
        let now = project_map::listing(root).expect("dentro do git");
        let map = serde_json::json!({
            "state": {"head": now.head, "listing": now.digest(), "base": now.base.name, "base_tip": now.base.tip},
            "modules": [{"path": "src/a.rs", "loc": 1, "declarations": [{"kind": "function", "name": "a", "line": 1, "end_line": 1}]}]
        });
        let languages = mustard_core::domain::normalize::Languages::new(["pt-BR", "en-US"]);
        project_map::save_at(&project_map::model_path(root), &map, "scan 1", &languages).unwrap();

        let calls = std::cell::Cell::new(0);
        let same = || Some("scan 1".to_string());
        refresh_map_if_behind(root, &mine_counting(&calls), &same);
        assert_eq!(calls.get(), 0, "a marca é a do scan: a ferramenta não roda");
        let silent = || None;
        refresh_map_if_behind(root, &mine_counting(&calls), &silent);
        assert_eq!(calls.get(), 0, "o scan que não responde não põe o mapa atrás");

        let newer = || Some("scan 2".to_string());
        refresh_map_if_behind(root, &mine_counting(&calls), &newer);
        assert_eq!(calls.get(), 1, "o scan é de outra compilação: a ferramenta roda, mesmo com o projeto parado");
    }

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
                undone: Vec::new(),
                leftovers: Vec::new(),
                agreed: Vec::new(),
                returns: Vec::new(),
                usage: Default::default(),
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

    /// Duas ondas no mesmo commit dão um título dentro do teto e com as duas
    /// ondas no escopo, mesmo quando o resumo da primeira passaria de 60 ao
    /// lado delas: na divisa, um resumo de 43 caracteres cabe por inteiro, e
    /// um de 44 é cortado até caber, sem perder a segunda onda do escopo; o
    /// corpo continua com uma linha por onda, com o resumo inteiro, nos dois
    /// casos.
    #[test]
    fn a_commit_of_two_waves_keeps_the_title_limit() {
        let report = |wave: u64, summary: String| WaveReport {
            wave,
            delivered: "A onda saiu.".into(),
            files: vec![format!("src/{wave}.rs")],
            commit: Some(summary),
            proofs: Vec::new(),
            fixes: Vec::new(),
            replan: None,
            undone: Vec::new(),
            leftovers: Vec::new(),
            agreed: Vec::new(),
            returns: Vec::new(),
            usage: Default::default(),
        };

        let waves = [report(1, "a".repeat(43)), report(2, "a".repeat(43))];
        let (title, body) = commit_message(&waves, Locale::PtBr)
            .unwrap_or_else(|_| panic!("a 43-character summary fits the joint scope of two waves"))
            .expect("a message");
        assert_eq!(title.chars().count(), MESSAGE_TITLE_MAX, "{title}");
        assert!(title.starts_with("feat(ondas-1-2): "), "{title}");
        assert_eq!(body.lines().count(), 2, "{body}");

        let waves = [report(1, "a".repeat(44)), report(2, "a".repeat(44))];
        let (title, body) = commit_message(&waves, Locale::PtBr)
            .unwrap_or_else(|_| panic!("a 44-character summary is cut to fit beside the scope of two waves"))
            .expect("a message");
        assert_eq!(title, format!("feat(ondas-1-2): {}", "a".repeat(43)), "{title}");
        assert_eq!(body.lines().count(), 2, "{body}");
        assert!(body.lines().all(|line| line.ends_with(&"a".repeat(44))), "o corpo traz o resumo inteiro: {body}");
    }

    /// O caso que gerou o defeito: duas ondas voltam na mesma rodada e o
    /// resumo da primeira, com o escopo das duas, passa de 60 caracteres. O
    /// título cita as duas ondas e corta o resumo numa palavra inteira; antes
    /// ele caía para a primeira só e o histórico do git escondia a segunda.
    #[test]
    fn a_long_summary_never_drops_a_wave_from_the_scope() {
        let report = |wave: u64, summary: &str| WaveReport {
            wave,
            delivered: "A onda saiu.".into(),
            files: vec![format!("src/{wave}.rs")],
            commit: Some(summary.into()),
            proofs: Vec::new(),
            fixes: Vec::new(),
            replan: None,
            undone: Vec::new(),
            leftovers: Vec::new(),
            agreed: Vec::new(),
            returns: Vec::new(),
            usage: Default::default(),
        };
        let waves = [
            report(101, "Cadastro lido só nos itens da tabela de loja"),
            report(107, "Aviso da rodada e envio do resumo"),
        ];
        let (title, body) = commit_message(&waves, Locale::PtBr).unwrap_or_else(|_| panic!("fits")).expect("a message");
        assert_eq!(title, "feat(ondas-101-107): Cadastro lido só nos itens da tabela de", "{title}");
        assert!(title.chars().count() <= MESSAGE_TITLE_MAX, "{title}");
        assert_eq!(
            body,
            "- onda 101: Cadastro lido só nos itens da tabela de loja\n- onda 107: Aviso da rodada e envio do resumo"
        );
    }

    /// Com tantas ondas no mesmo commit que o escopo sozinho passa do teto do
    /// título, o título não sai vazio nem é recusado: o escopo cita as
    /// primeiras ondas que deixam espaço e conta as outras, e o começo do
    /// resumo cabe ao lado.
    #[test]
    fn a_scope_too_long_for_the_title_is_shortened_and_the_title_never_comes_out_empty() {
        let report = |wave: u64| WaveReport {
            wave,
            delivered: "A onda saiu.".into(),
            files: vec![format!("src/{wave}.rs")],
            commit: Some("Relatório do mês gerado na hora certa".into()),
            proofs: Vec::new(),
            fixes: Vec::new(),
            replan: None,
            undone: Vec::new(),
            leftovers: Vec::new(),
            agreed: Vec::new(),
            returns: Vec::new(),
            usage: Default::default(),
        };
        let waves: Vec<WaveReport> = (101..113).map(report).collect();
        let (title, body) = commit_message(&waves, Locale::PtBr)
            .unwrap_or_else(|_| panic!("twelve waves in one commit must not refuse the round"))
            .expect("a message");
        assert!(title.chars().count() <= MESSAGE_TITLE_MAX, "{title}");
        assert_eq!(title, "feat(ondas-101-102-103-104-105-106+6): Relatório do mês", "{title}");
        assert_eq!(body.lines().count(), 12, "o corpo traz uma linha por onda: {body}");
        assert!(body.contains("onda 112:") && body.contains("onda 101:"), "{body}");

        let (title, _) = commit_message(&waves, Locale::EnUs).unwrap_or_else(|_| panic!("fits")).expect("a message");
        assert!(title.starts_with("feat(waves-101-") && title.contains('+'), "{title}");
        assert!(title.chars().count() <= MESSAGE_TITLE_MAX && !title.ends_with(": "), "{title}");
    }

    /// Duas entregas tomadas na mesma rodada, cada uma com o seu arquivo,
    /// viram um commit só, e o título e o registro dele na spec citam as
    /// duas ondas, mesmo com o resumo da primeira grande para o título.
    #[test]
    fn two_deliveries_taken_in_one_round_give_one_commit_that_names_both_waves() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);
        let copy = |wave: u64| mustard_core::io::wave_prompt::slot_path(root, "x", usize::try_from(wave).unwrap() - 1);
        std::fs::write(copy(1).join("src/a.rs"), "fn one() { first(); }\nfn first() {}\n").unwrap();
        std::fs::write(copy(2).join("src/b.rs"), "fn one() { second(); }\nfn second() {}\n").unwrap();
        let first = "Cadastro lido só nos itens da tabela de loja";
        let one = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs"], "commit": first});
        let two = json!({"wave": 2, "text": "Saiu.", "files": ["src/b.rs"], "commit": "Aviso da rodada e envio"});
        assert_eq!(returned(root, one)["ok"], json!(true));
        assert_eq!(returned(root, two)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");

        let expected = "feat(ondas-1-2): Cadastro lido só nos itens da tabela de";
        assert_eq!(out["commit"]["title"], json!(expected), "{out}");
        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let commit = log.visible().into_iter().find(|e| e.event_type == "commit").expect("commit");
        assert_eq!(commit.str_field("title"), Some(expected));
        assert_eq!(commit.ints("waves"), vec![1, 2]);
        let shown = Command::new("git").args(["show", "--name-only", "--format=%s", "HEAD"]).current_dir(root).output();
        let shown = String::from_utf8_lossy(&shown.unwrap().stdout).to_string();
        let shown: Vec<&str> = shown.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(shown, [expected, "src/a.rs", "src/b.rs"], "{shown:?}");
    }

    /// A mensagem do commit é conferida antes de qualquer gravação: a volta
    /// com e-mail no resumo é recusada na gravação, sem escrever nada, e a
    /// volta seguinte, com a mensagem limpa, é assumida uma vez só.
    #[test]
    fn a_report_with_a_bad_commit_message_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        std::fs::write(root.join("src/a.rs"), "fn one() { dois(); }\nfn dois() {}\n").unwrap();
        let delivered =
            |summary: &str| json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": summary});
        let refused = returned(root, delivered("pedido de fulano@empresa.com.br"));
        assert_eq!(refused["reason"], json!("commit-forbidden-text"), "{refused}");
        assert_eq!(written_deliveries(root), 0, "nada foi gravado");

        assert_eq!(returned(root, delivered("a soma sai"))["ok"], json!(true));
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "sem duplicar");
    }

    /// A rodada faz o commit da rodada e grava o código dele na spec.
    #[test]
    fn the_round_commits_and_records_the_commit_on_the_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn one() { dois(); }\nfn dois() {}\n").unwrap();

        let report = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai"});
        assert_eq!(returned(root, report)["ok"], json!(true));
        let out = round(root, "x", None);
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

        std::fs::remove_file(root.join("src/a.rs")).unwrap();
        git_at(root, &["rm", "-q", "src/b.rs"]);
        std::fs::write(root.join("src/c.rs"), "fn one() { tres(); }\nfn tres() {}\n").unwrap();

        let files = ["src/a.rs", "src/b.rs", "src/c.rs"];
        let report = json!({"wave": 1, "text": "Dois arquivos saíram.", "files": files, "commit": "tira dois arquivos"});
        assert_eq!(returned(root, report)["ok"], json!(true));
        let out = round(root, "x", None);
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
    /// o commit leva a remoção. O arquivo que a cópia mudou e a entrega não
    /// citou entra no commit do mesmo jeito, com um aviso de divergência; as
    /// duas cópias ficam no disco depois do commit, prontas para a próxima
    /// onda.
    #[test]
    fn a_file_deleted_in_the_copy_is_deleted_and_an_undeclared_file_enters_the_commit_with_a_warning() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs", "src/b.rs"], &[]), (2, &["src/c.rs"], &[])]);
        round(root, "x", None);
        let copy = |wave: u64| mustard_core::io::wave_prompt::slot_path(root, "x", usize::try_from(wave).unwrap() - 1);
        std::fs::remove_file(copy(1).join("src/a.rs")).unwrap();
        std::fs::write(copy(1).join("src/b.rs"), "fn one() { second(); }\nfn second() {}\n").unwrap();
        std::fs::write(copy(2).join("src/c.rs"), "fn one() { third(); }\nfn third() {}\n").unwrap();
        std::fs::write(copy(2).join("src/esquecido.rs"), "fn main() {}\n").unwrap();

        let one = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs", "src/b.rs"], "commit": "a sai"});
        let two = json!({"wave": 2, "text": "Saiu.", "files": ["src/c.rs"], "commit": "c muda"});
        assert_eq!(returned(root, one)["ok"], json!(true));
        assert_eq!(returned(root, two)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert!(!root.join("src/a.rs").exists());
        assert_eq!(std::fs::read_to_string(root.join("src/c.rs")).unwrap(), "fn one() { third(); }\nfn third() {}\n");
        assert_eq!(std::fs::read_to_string(root.join("src/esquecido.rs")).unwrap(), "fn main() {}\n",
            "o arquivo fora da lista entra no commit mesmo assim");
        let shown = Command::new("git").args(["show", "--name-status", "--format=", "HEAD"]).current_dir(root).output();
        let shown = String::from_utf8_lossy(&shown.unwrap().stdout).to_string();
        assert_eq!(
            shown.lines().collect::<Vec<_>>(),
            ["D\tsrc/a.rs", "M\tsrc/b.rs", "M\tsrc/c.rs", "A\tsrc/esquecido.rs"],
            "{out}"
        );

        assert!(copy(1).join(".git").is_file(), "the copy stays after the commit of the wave");
        assert!(copy(2).join(".git").is_file(), "the other copy stays too");
        let hint = translate("round.files_diverged", Locale::PtBr)
            .replace("{wave}", "2")
            .replace("{changed}", "2")
            .replace("{declared}", "1")
            .replace("{missing}", "src/esquecido.rs");
        // O aviso da onda que entregou sem linha de consumo é de outro
        // assunto e sai junto: aqui se olha o resto.
        let warned: Vec<Value> = out["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|w| !matches!(w["reason"].as_str(), Some("usage-missing" | "wave-size")))
            .collect();
        assert_eq!(json!(warned), json!([{"reason": "files-diverged", "wave": 2, "hint": hint}]), "{out}");
    }

    /// A volta da onda 1 com os arquivos `files`, gravada sem mexer neles.
    fn listing(files: &[&str]) -> Value {
        json!({"wave": 1, "text": "Saiu.", "files": files, "commit": "a onda 1 saiu"})
    }

    /// Cada volta de `wrong`, gravada pelo agente, é recusada pelo git na
    /// rodada, com o motivo que o git deu, e a rodada não deixa nada gravado;
    /// depois de `fix`, a volta que entrega `fixed` é assumida uma vez só.
    fn refused_by_git_records_nothing(root: &Path, wrong: &[Value], fix: impl FnOnce(), fixed: &[&str]) {
        let spec_lines = || std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count();
        for body in wrong {
            assert_eq!(returned(root, body.clone())["ok"], json!(true), "{body}");
            let before = spec_lines();
            let refused = round(root, "x", None);
            assert_eq!(refused["reason"], json!("git-refused"), "{refused}");
            assert_eq!(spec_lines(), before, "nothing was recorded: {refused}");
            let bare = translate("round.git_refused", Locale::PtBr).replace("{detail}", "");
            assert_ne!(refused["hint"], json!(bare), "the refusal carries git's reason: {refused}");
        }
        fix();
        delivered(root, 1, "Saiu.", fixed);
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(delivered_count(root), 1, "the corrected call records the delivery once");
    }

    /// O caminho que existe fora do repositório, absoluto ou com `../`, e o
    /// que o `.gitignore` ignora são recusados pelo git sem gravar nada. A
    /// chamada corrigida leva ao commit um arquivo novo, que ainda não estava
    /// no git, e deixa de fora o ignorado.
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
        std::fs::write(root.join(".gitignore"), "src/gerado.rs\n").unwrap();
        std::fs::write(root.join("src/gerado.rs"), "fn gerado() {}\n").unwrap();
        let wrong: Vec<Value> = [absolute, format!("../{name}/fora.rs"), "src/gerado.rs".to_string()]
            .iter()
            .map(|path| listing(&["src/a.rs", path.as_str()]))
            .collect();
        std::fs::write(root.join("src/novo.rs"), "fn main() {}\n").unwrap();
        refused_by_git_records_nothing(root, &wrong, || {}, &["src/a.rs", "src/novo.rs"]);
        let shown = Command::new("git").args(["show", "--name-only", "--format=", "HEAD"]).current_dir(root).output();
        let shown = String::from_utf8_lossy(&shown.unwrap().stdout).to_string();
        assert!(shown.lines().any(|line| line == "src/novo.rs"), "the new file went into the commit: {shown}");
        assert!(!shown.contains("gerado"), "the ignored file stayed out of the commit: {shown}");
    }

    /// O gancho do commit que recusa não deixa nada gravado, e a chamada
    /// depois de o gancho sair grava a entrega uma vez só.
    #[cfg(unix)]
    #[test]
    fn a_commit_hook_that_refuses_records_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        let hooks = root.join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        crate::executable::write_executable(&hook, "#!/bin/sh\necho 'o gancho recusou' >&2\nexit 1\n");
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);

        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// Saiu.\n").unwrap();
        let wrong = [listing(&["src/a.rs"])];
        refused_by_git_records_nothing(root, &wrong, || std::fs::remove_file(&hook).unwrap(), &["src/a.rs"]);
    }

    /// O arquivo de dentro de um submódulo é comitado no submódulo, e o
    /// principal leva o ponteiro. Quando o git recusa o commit do principal,
    /// o commit já feito no submódulo volta, com o disco e o índice dele; a
    /// chamada depois de o gancho sair comita uma vez só nos dois.
    #[cfg(unix)]
    #[test]
    fn a_refused_main_commit_undoes_the_submodule_commit() {
        let dir = tempdir().unwrap();
        let root = &dir.path().join("principal");
        with_submodule(root, dir.path());
        approved(root, "x", &[(1, &["src/a.rs", "libs/sub/lib.txt"], &[])]);
        round(root, "x", None);
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", 0);
        assert!(copy.join("libs/sub/.git").is_file(), "the copy brings the submodule");
        std::fs::write(copy.join("src/a.rs"), "fn one() { dois(); }\nfn dois() {}\n").unwrap();
        std::fs::write(copy.join("libs/sub/lib.txt"), "fn one() { sub(); }\nfn sub() {}\n").unwrap();
        let sub = root.join("libs/sub");
        assert_eq!(git_text(&sub, &["rev-parse", "--abbrev-ref", "HEAD"]), "feature/x", "the same branch name");
        let before = git_text(&sub, &["rev-parse", "HEAD"]);

        let hooks = dir.path().join("ganchos");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        crate::executable::write_executable(&hook, "#!/bin/sh\necho 'o gancho recusou' >&2\nexit 1\n");
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);
        let report = json!({"wave": 1, "text": "Saiu.", "files": ["src/a.rs", "libs/sub/lib.txt"], "commit": "a onda 1 sai"});
        assert_eq!(returned(root, report)["ok"], json!(true));
        let refused = round(root, "x", None);
        assert_eq!(refused["reason"], json!("git-refused"), "{refused}");
        assert_eq!(git_text(&sub, &["rev-parse", "HEAD"]), before, "the submodule commit was undone");
        assert_eq!(git_text(&sub, &["status", "--porcelain"]), "", "the submodule disk and index are back");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn one() {}\n");

        std::fs::remove_file(&hook).unwrap();
        let went = round(root, "x", None);
        assert_eq!(went["ok"], json!(true), "{went}");
        assert_eq!(git_text(&sub, &["rev-list", "--count", &format!("{before}..HEAD")]), "1", "one submodule commit");
        assert_eq!(git_text(root, &["rev-parse", "HEAD:libs/sub"]), git_text(&sub, &["rev-parse", "HEAD"]));
        assert_eq!(std::fs::read_to_string(sub.join("lib.txt")).unwrap(), "fn one() { sub(); }\nfn sub() {}\n");
        assert!(copy.join("libs/sub/.git").is_file(), "the copy stays, with the submodule copy inside it: {went}");
    }

    /// O passo do git roda com uma trava própria, e não com a da spec:
    /// enquanto o gancho do commit demora, quem lê a spec não espera por ele,
    /// e outro passo do git no mesmo checkout espera a vez até o commit
    /// terminar.
    #[cfg(unix)]
    #[test]
    fn a_slow_commit_hook_holds_the_git_step_and_not_the_readers_of_the_spec() {
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
        crate::executable::write_executable(&hook, &script);
        git_at(root, &["config", "core.hooksPath", &hooks.to_string_lossy()]);
        delivered(root, 1, "Saiu.", &["src/a.rs"]);
        let spec = store::spec_file(root, "x").unwrap();

        std::thread::scope(|scope| {
            let going = scope.spawn(|| round(root, "x", None));
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

    /// Enquanto o agente grava a volta da onda, a trava do passo do git fica
    /// presa até a volta estar no arquivo e a gravação terminar: a linha da
    /// spec no índice é refeita depois da escrita, e é ali que a gravação
    /// espera, com a volta já no arquivo. Outro passo do git no mesmo checkout
    /// espera a gravação inteira, e só passa quando ela termina.
    #[cfg(unix)]
    #[test]
    fn a_return_being_written_holds_the_git_step_until_the_write_is_done() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let spec = store::spec_file(root, "x").unwrap();
        let (index, _) = mustard_core::io::spec_index::index_for(&spec).expect("the spec index");
        let body = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai"});
        let in_the_file = |spec: &Path| {
            std::fs::read_to_string(spec).unwrap_or_default().lines().any(|l| l.contains("\"returned\":true"))
        };

        // O agente lê o pedido antes de gravar: com o índice preso, nem a leitura entraria.
        crate::commands::flow::round::read_request(root, "x", 1);
        let index_lock = mustard_core::io::fs::lock::LockedFile::exclusive(&index).unwrap();
        std::thread::scope(|scope| {
            let writing = scope.spawn(|| returned(root, body));
            for _ in 0..3000 {
                if in_the_file(&spec) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(in_the_file(&spec), "the return never reached the spec file");

            let (turn_tx, turn_rx) = mpsc::channel();
            scope.spawn(move || turn_tx.send(git_lock(root).is_ok()));
            let early = turn_rx.recv_timeout(Duration::from_millis(500));

            drop(index_lock);
            let wrote = writing.join().unwrap();
            assert!(early.is_err(), "another git step waits while the return is being written");
            assert_eq!(turn_rx.recv_timeout(Duration::from_secs(30)), Ok(true), "its turn comes after the write");
            assert_eq!(wrote["ok"], json!(true), "{wrote}");
        });
    }

    /// A onda que grava a volta duas vezes, e a onda seguinte, saem da rodada
    /// com uma entrega oficial cada, de números seguidos: a entrega oficial
    /// fica com o número da volta que assume, e a segunda volta da mesma onda
    /// divide o número da primeira.
    #[test]
    fn the_official_deliveries_of_the_round_take_consecutive_numbers() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        round(root, "x", None);
        delivered(root, 1, "Primeira volta.", &["src/a.rs"]);
        delivered(root, 1, "Segunda volta.", &["src/a.rs"]);
        delivered(root, 2, "A dobra saiu.", &["src/b.rs"]);

        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let official: Vec<(Option<u64>, String)> = log
            .visible()
            .into_iter()
            .filter(|e| e.event_type == "delivered")
            .map(|e| (e.wave(), codes[&e.id].clone()))
            .collect();
        assert_eq!(
            official,
            vec![(Some(1), "MSTD-DELIV-0001".to_string()), (Some(2), "MSTD-DELIV-0002".to_string())],
            "{out}"
        );
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

        let unchanged = [listing(&["src/a.rs"])];
        refused_by_git_records_nothing(root, &unchanged, || {}, &["src/a.rs"]);
    }

    /// Quando a cópia de uma onda chega com commit próprio, à frente do
    /// commit em que nasceu, e nada mudado fora dele — o `git status` dela
    /// sai limpo —, a rodada junta ao repositório principal o que esse
    /// commit mudou, em vez de recusar como no teste acima: usar o HEAD da
    /// cópia como base da comparação (o defeito de 22/09/2026) faria a
    /// junção comparar a cópia contra ela mesma, sem achar diferença
    /// nenhuma, e o commit do repositório principal saísse sem nada a
    /// comitar.
    #[test]
    fn round_merges_the_copy_that_came_committed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        round(root, "x", None);

        let path = store::spec_file(root, "x").unwrap();
        let log = store::read(&path).unwrap().unwrap();
        let copy = copy_of(&log, 1).expect("a onda 1 ganhou cópia");
        std::fs::write(copy.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        git_at(&copy, &["add", "-A"]);
        git_at(&copy, &["commit", "-q", "-m", "o agente comitou dentro da cópia"]);

        let report = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a onda 1 saiu"});
        assert_eq!(returned(root, report)["ok"], json!(true));
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let content = std::fs::read_to_string(root.join("src/a.rs")).unwrap();
        assert!(content.contains("A soma saiu."), "o que a cópia comitou chegou ao repositório principal: {content}");
        assert_eq!(delivered_count(root), 1, "{out}");
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

    /// A raiz de um repositório que constrói o próprio `mustard-rt`, com o git
    /// de que a detecção precisa.
    fn mustard_like_root(root: &Path) {
        std::fs::create_dir_all(root.join("apps/rt")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("apps/rt/Cargo.toml"), b"[package]\nname=\"mustard-rt\"\n").unwrap();
    }

    fn proof(result: &'static str, output: &str) -> crate::commands::review::qa_run::ProofRun {
        crate::commands::review::qa_run::ProofRun {
            result,
            exit: if result == "pass" { 0 } else { 1 },
            ms: 0,
            output: output.to_string(),
            ran_no_test: None,
            missing_test: None,
        }
    }

    /// Só a onda que tocou `apps/` ou `packages/` compila: um documento, um
    /// arquivo do plugin ou da raiz nunca chama o executor, e fora do
    /// repositório do Mustard nada compila nem com o código dele tocado.
    #[test]
    fn the_development_build_only_runs_when_the_commit_touched_the_program() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        mustard_like_root(root);
        let commands = std::cell::RefCell::new(Vec::new());
        let exec = |command: &str, cwd: &Path| {
            commands.borrow_mut().push((command.to_string(), cwd.to_path_buf()));
            proof("pass", "")
        };
        let files = |names: &[&str]| names.iter().map(|name| name.to_string()).collect::<Vec<_>>();

        for untouched in [files(&[]), files(&["docs/guia.md", "plugin/hooks/hooks.json", "README.md", "scripts/x.sh"])] {
            assert!(build_development_version_with(root, &untouched, Locale::PtBr, &exec).is_none());
            assert!(commands.borrow().is_empty(), "{untouched:?} não toca o programa: {:?}", commands.borrow());
        }
        for touched in [files(&["apps/rt/src/main.rs"]), files(&["docs/guia.md", "packages/core/src/lib.rs"])] {
            assert!(build_development_version_with(root, &touched, Locale::PtBr, &exec).is_none(), "compilação verde: sem aviso");
        }
        let commands = commands.into_inner();
        assert_eq!(commands.len(), 2, "uma compilação por rodada que tocou o programa: {commands:?}");
        let (command, cwd) = &commands[0];
        let target = mustard_core::io::wave_prompt::development_build_dir(root);
        assert_eq!(
            command,
            &format!("cargo build --release --locked -p mustard-rt -p scan -p mustard-cli --target-dir '{}'", target.display())
        );
        assert_eq!(std::fs::canonicalize(cwd).unwrap(), std::fs::canonicalize(root).unwrap(), "no checkout principal");

        let other = tempdir().unwrap();
        let counting = |_: &str, _: &Path| -> crate::commands::review::qa_run::ProofRun { panic!("fora do Mustard nada compila") };
        assert!(build_development_version_with(other.path(), &files(&["apps/rt/src/main.rs"]), Locale::PtBr, &counting).is_none());
    }

    /// A compilação vermelha vira o aviso, com o fim da saída do `cargo`, nos
    /// dois idiomas, e nunca recusa a rodada.
    #[test]
    fn a_red_development_build_warns_with_the_end_of_the_output() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        mustard_like_root(root);
        let exec = |_: &str, _: &Path| proof("fail", "error[E0425]: cannot find value `x`");
        let files = vec!["apps/rt/src/main.rs".to_string()];
        for lang in [Locale::PtBr, Locale::EnUs] {
            let warning = build_development_version_with(root, &files, lang, &exec).expect("aviso de compilação vermelha");
            assert_eq!(warning["reason"], json!("development-build-failed"), "{warning}");
            let hint = warning["hint"].as_str().unwrap_or_default();
            assert!(hint.contains("error[E0425]: cannot find value `x`"), "{lang:?}: {warning}");
            assert!(!hint.contains("{output}"), "{lang:?}: {warning}");
        }
    }
}
