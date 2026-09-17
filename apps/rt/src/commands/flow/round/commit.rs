//! O commit da rodada: a mensagem montada do resumo de cada entrega e
//! conferida, a formatação só dos arquivos da rodada, o commit e a gravação
//! dele na spec.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

use mustard_core::domain::spec_events::{check_message, MessageRefusal, Refusal, MESSAGE_BODY_MAX, MESSAGE_TITLE_MAX};
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::platform::git as git_exec;
use mustard_core::ClaudePaths;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::answer::RoundRefusal;
use super::report::WaveReport;
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

/// O arquivo da trava do passo do git, na pasta das specs do checkout.
const GIT_LOCK_FILE: &str = "round-git.lock";

/// O código que a conferência antes do commit usa no lugar do que o git ainda
/// vai dar, com a mesma forma.
pub(super) const UNMADE_SHA: &str = "0000000000000000000000000000000000000000";

/// Pega a trava do passo do git do checkout `root`, esperando a de outra
/// rodada soltar. É uma trava própria, e não a do arquivo de eventos da spec:
/// o gancho do commit pode demorar, e a trava da spec seguraria todo leitor
/// dela enquanto isso.
pub(super) fn git_lock(root: &Path) -> Result<LockedFile, RoundRefusal> {
    let io = |detail: String| RoundRefusal::Refused(Refusal::Io { detail });
    let paths = ClaudePaths::for_project(root).map_err(|e| io(e.to_string()))?;
    LockedFile::exclusive(&paths.spec_dir().join(GIT_LOCK_FILE)).map_err(|e| io(e.to_string()))
}

/// Faz o commit da rodada com a mensagem já conferida e devolve o código dele.
/// Não grava nada: roda antes de qualquer gravação, e a recusa do git para a
/// rodada com a spec intacta.
pub(super) fn make_commit(root: &Path, title: &str, body: &str, files: &[String]) -> Result<String, RoundRefusal> {
    // O passo do git roda com a trava dele presa: duas rodadas ao mesmo tempo
    // no mesmo checkout não dividem o índice, e um commit nunca leva o
    // arquivo da outra.
    let _held = git_lock(root)?;
    // O arquivo que ainda existe entra pelo `add`. O apagado sai do índice por
    // outra porta: o `add` recusa o caminho que já saiu do índice, e a remoção
    // que só aconteceu no disco sai do mesmo jeito. O caminho que já não está
    // no índice não é erro — a remoção dele já estava pronta para o commit.
    let (present, gone): (Vec<&str>, Vec<&str>) =
        files.iter().map(String::as_str).partition(|file| root.join(file).exists());
    if !present.is_empty() {
        let mut add: Vec<&str> = vec!["add", "--"];
        add.extend(&present);
        git(root, &add).map_err(|detail| RoundRefusal::Git { detail })?;
    }
    if !gone.is_empty() {
        let mut remove: Vec<&str> = vec!["rm", "-r", "-q", "--cached", "--ignore-unmatch", "--"];
        remove.extend(&gone);
        git(root, &remove).map_err(|detail| RoundRefusal::Git { detail })?;
    }
    let mut args: Vec<&str> = vec!["commit", "-m", title];
    if !body.is_empty() {
        args.push("-m");
        args.push(body);
    }
    git(root, &args).map_err(|detail| RoundRefusal::Git { detail })?;
    let sha = git(root, &["rev-parse", "HEAD"]).map_err(|detail| RoundRefusal::Git { detail })?;
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

/// Cada arquivo entregue está no disco ou no índice, contando o que saiu do
/// índice desde o último commit; senão, a recusa vem antes de gravar, e não
/// do commit, depois (a remoção aceita calada o caminho que não existe).
pub(super) fn unknown_file(root: &Path, waves: &[WaveReport]) -> Result<(), RoundRefusal> {
    for wave in waves {
        for file in &wave.files {
            let known = root.join(file).exists()
                || git(root, &["ls-files", "--error-unmatch", "--with-tree=HEAD", "--", file]).is_ok();
            if !known {
                return Err(RoundRefusal::FileUnknown { file: file.clone(), wave: wave.wave });
            }
        }
    }
    Ok(())
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
