//! `mustard-rt run discard [--spec <nome>]` — descartar uma spec, em dois
//! passos.
//!
//! Descartar fecha o pull request e apaga a branch local, e isso não tem
//! volta. Por isso a porta é a mesma da remoção de pendências: a primeira
//! chamada só mostra o que vai sair — o pull request, a branch local, a do
//! servidor quando a opção vier e a pasta da spec — e devolve um código; a
//! segunda, com esse código e depois do sim do usuário, faz. O código vem do
//! que seria tirado, então um sim nunca serve para outro descarte.
//!
//! As cópias da obra, com o que compilaram, saem junto, e a prévia as
//! lista. Depois delas sai da pasta principal a pasta de compilação que o
//! projeto declarou descartável, como no fechamento; a prévia não apaga nada.
//!
//! A pasta da spec é arquivada por padrão, ao lado das outras, e só é apagada
//! quando quem chama pede: nada fica pela metade, e nada some sem se pedir.
//! A spec arquivada continua no índice, com a fase descartada: dá para achar
//! depois o que foi decidido e por que parou. A apagada sai do índice junto
//! com a pasta. O descarte é um marco, como o fechamento: a cópia para o
//! banco da página do projeto sai aqui, e a linha desta spec chega com a fase
//! descartada.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::translate;
use serde_json::{json, Map, Value};

use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// A pasta em que as specs descartadas ficam guardadas, dentro da pasta das
/// specs: a mesma de onde o índice as lê.
use mustard_core::io::spec_index::DISCARDED_DIR as ARCHIVE_DIR;

/// As opções de `mustard-rt run discard`.
pub struct DiscardOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec descartada; sem ela, a spec atual.
    pub spec: Option<String>,
    /// Apagar também a branch do servidor. Sem ela, só a local sai.
    pub remote: bool,
    /// Apagar a pasta da spec em vez de arquivá-la.
    pub delete: bool,
    /// O código que a primeira chamada devolveu, depois do sim do usuário.
    pub confirm: Option<String>,
}

/// O núcleo testável de [`run_cmd`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn discard_at(opts: &DiscardOpts) -> Value {
    discard_for(opts, session_from_env().as_deref())
}

/// [`discard_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn discard_for(opts: &DiscardOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let refuse = |refusal: &Refusal| spec_events::refused(refusal, lang);

    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => match DiskSpecState::new(&checkout(&opts.root)).active(session) {
            Some(spec) => spec,
            None => return refuse(&Refusal::NoCurrentSpec),
        },
    };
    let path = match store::spec_file(&project.root, &spec) {
        Ok(path) => path,
        Err(refusal) => return refuse(&refusal),
    };
    let log = match store::read(&path) {
        Ok(Some(log)) => log,
        Ok(None) => return refuse(&Refusal::NoSpecFile { spec }),
        Err(refusal) => return refuse(&refusal),
    };
    let state = State::from_log(&log);
    let branch = state.branch.unwrap_or_default();
    let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();

    let leaving = json!({
        "branch": branch,
        "remote": opts.remote,
        "spec": spec,
        "folder": crate::commands::spec_events::pages::relative(&project.root, &folder),
        "action": if opts.delete { "deleted" } else { "archived" },
        "copies": crate::commands::flow::round::spec_copies(&project.root, &spec),
    });
    let code = token(&spec, &branch, opts.remote, opts.delete);

    let Some(given) = opts.confirm.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
        let hint = translate("discard.preview", lang)
            .replace("{spec}", &spec)
            .replace("{branch}", if branch.is_empty() { "—" } else { &branch })
            .replace("{remote}", translate(yes_no(opts.remote), lang))
            .replace("{what}", translate(if opts.delete { "discard.delete" } else { "discard.archive" }, lang))
            .replace("{token}", &code);
        return json!({
            "ok": true,
            "spec": spec,
            "preview": true,
            "leaving": leaving,
            "token": code,
            "hint": hint,
        });
    };
    if given != code {
        return json!({
            "ok": false,
            "reason": "confirm-mismatch",
            "hint": translate("discard.confirm_mismatch", lang),
        });
    }

    // A fase fica gravada antes de o arquivo sair do lugar: é o último evento
    // da spec, e a mesma gravação deixa a linha dela no índice com a fase
    // descartada.
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("discarded"));
    draft.insert("author".to_string(), json!("binary"));
    draft.insert("reason".to_string(), json!(translate("discard.reason", lang)));
    let phase_written = record(&opts.root, &spec, "state", draft, PhaseWriter::Binary).is_ok();

    // O pull request e as branches, pela mesma porta que descarta uma unidade
    // abandonada. A do servidor só com a opção.
    let git = (!branch.is_empty())
        .then(|| crate::commands::git_delete::delete_with(&opts.root, &branch, opts.remote));

    // As cópias da obra saem com ela, com o que compilaram, antes de a pasta
    // da spec sair do lugar. A cópia que não sai vira aviso, sem derrubar o
    // descarte.
    let copies_left = if phase_written {
        crate::commands::flow::round::remove_spec_copies(&project.root, &spec)
    } else {
        Vec::new()
    };
    // Depois das cópias, a pasta de compilação que o projeto declarou
    // descartável sai da pasta principal, como no fechamento.
    let build_output =
        phase_written.then(|| crate::commands::flow::close::remove_build_output(&project.root, lang));

    // O descarte é um marco, como o fechamento: a cópia para o banco da
    // página sai aqui, com a fase descartada na linha da spec da página do
    // projeto. Onde os lotes nascem segue a pasta da spec: ao apagar, ela não
    // sobrevive ao marco, então a cópia nasce antes, numa pasta só deste
    // descarte, fora do projeto ([`discard_copy_folder`]); ao arquivar, ela
    // sobrevive, então a cópia nasce depois de mover, lendo o arquivo de
    // eventos já na pasta arquivada — os caminhos da resposta apontam para lá.
    let ndjson = folder.join("spec.ndjson");
    let (moved, index_done, prepared) = if opts.delete {
        let prepared = phase_written.then(|| {
            discard_copy_folder(&project.root, &spec)
                .map_err(|e| Refusal::Io { detail: e.to_string() })
                .and_then(|copy| {
                    crate::commands::spec_events::pages::copy::prepare_milestone_at(
                        &project.root,
                        &spec,
                        &ndjson,
                        copy,
                        lang,
                    )
                })
        });
        let removed = std::fs::remove_dir_all(&folder).is_ok();
        (removed, mustard_core::io::spec_index::drop_line(&project.root, &spec).is_ok(), prepared)
    } else {
        let archived = phase_written.then(|| archive(&spec, &folder)).flatten();
        let prepared = archived.as_ref().map(|target| {
            crate::commands::spec_events::pages::copy::prepare_milestone_at(
                &project.root,
                &spec,
                &target.join("spec.ndjson"),
                target.join(crate::commands::spec_events::pages::copy::FOLDER),
                lang,
            )
        });
        (archived.is_some(), phase_written, prepared)
    };
    if let Some(sid) = session {
        crate::shared::context::session::unbind_session_spec(&opts.root.to_string_lossy(), sid);
    }
    let mut out = json!({
        "ok": moved && index_done,
        "spec": spec,
        "discarded": leaving,
        "phase": phase_written.then(|| json!("discarded")),
        "index": if opts.delete { "dropped" } else { "kept" },
    });
    if let Some(git) = git {
        out["git"] = git;
    }
    if !(moved && index_done) {
        out["reason"] = json!("discard-incomplete");
        out["hint"] = json!(translate("discard.incomplete", lang));
    }
    if let Some(hint) = crate::commands::flow::close::copies_kept_hint(&copies_left, lang) {
        spec_events::pages::push_warning(&mut out, "copies-kept", &hint);
    }
    if let Some(swept) = &build_output {
        swept.tell(&mut out);
    }
    if let Some(prepared) = &prepared {
        let then = translate("discard.done", lang).to_string();
        crate::commands::spec_events::pages::end_milestone(&mut out, prepared.as_ref(), &spec, "discard", &then, lang);
    }
    out
}

/// Guarda a pasta da spec ao lado das outras descartadas. A pasta nova,
/// quando ela saiu do lugar.
fn archive(spec: &str, folder: &Path) -> Option<PathBuf> {
    let specs = folder.parent()?;
    let target = specs.join(ARCHIVE_DIR).join(spec);
    std::fs::create_dir_all(target.parent().unwrap_or(&target)).ok()?;
    if target.exists() {
        let _ = std::fs::remove_dir_all(&target);
    }
    std::fs::rename(folder, &target).ok()?;
    Some(target)
}

/// A pasta, dentro da pasta das cópias do projeto
/// ([`mustard_core::io::wave_prompt::copies_dir`]), em que nasce a cópia da
/// página de cada descarte que apaga a spec. O ponto do começo a separa das
/// pastas das cópias das obras, que levam o nome de uma spec; a limpeza a
/// deixa fora da lista das cópias de obra.
pub(crate) const DISCARD_COPIES: &str = ".discard-copy";

/// Por quanto tempo a cópia da página de um descarte fica depois dele. Os
/// lotes saem logo depois da resposta; o dia de folga cobre outro descarte
/// do mesmo projeto feito antes de os lotes do primeiro saírem.
pub(crate) const DISCARD_COPY_KEPT: Duration = Duration::from_secs(24 * 60 * 60);

/// A pasta só deste descarte para a cópia da página da spec `spec`, que vai
/// ser apagada: fora do projeto, na pasta das cópias dele, com o nome da
/// spec e uma marca única da chamada. Dois descartes nunca dividem a pasta,
/// nem em dois projetos com uma spec de mesmo nome, nem na mesma spec
/// apagada duas vezes. Ela fica depois do comando, porque os lotes saem
/// depois da resposta; antes de criá-la, sai a de cada descarte anterior do
/// projeto que não muda há mais de [`DISCARD_COPY_KEPT`].
fn discard_copy_folder(root: &Path, spec: &str) -> std::io::Result<PathBuf> {
    sweep_old_discard_copies(root, true);
    let place = discard_copies_place(root);
    std::fs::create_dir_all(&place)?;
    tempfile::Builder::new().prefix(&format!("{spec}-")).tempdir_in(&place).map(tempfile::TempDir::keep)
}

/// A pasta das cópias de página dos descartes do projeto `root`.
fn discard_copies_place(root: &Path) -> PathBuf {
    mustard_core::io::wave_prompt::copies_dir(root).join(DISCARD_COPIES)
}

/// A pasta de cada cópia de página de descarte do projeto `root` que não
/// muda há mais de [`DISCARD_COPY_KEPT`], em ordem de caminho; a mais nova
/// nunca entra. Com `apply`, cada uma sai, e o texto ao lado dela diz por
/// que não saiu; sem `apply`, nada sai. Link não é seguido, e a pasta que
/// não sai fica para a próxima varredura, sem derrubar quem chamou. O
/// descarte que apaga a spec varre antes de criar a pasta dele; a limpeza
/// varre com a escolha dela de apagar ou só listar.
pub(crate) fn sweep_old_discard_copies(root: &Path, apply: bool) -> Vec<(PathBuf, Option<String>)> {
    let Ok(entries) = std::fs::read_dir(discard_copies_place(root)) else { return Vec::new() };
    let now = SystemTime::now();
    let mut old: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry.metadata().is_ok_and(|meta| {
                meta.is_dir()
                    && meta
                        .modified()
                        .ok()
                        .and_then(|at| now.duration_since(at).ok())
                        .is_some_and(|age| age > DISCARD_COPY_KEPT)
            })
        })
        .map(|entry| entry.path())
        .collect();
    old.sort();
    old.into_iter()
        .map(|dir| {
            let error = apply.then(|| mustard_core::io::fs::remove_dir_all(&dir).err().map(|e| e.to_string())).flatten();
            (dir, error)
        })
        .collect()
}

/// O código do descarte: ele muda com a spec, a branch e as duas escolhas, e
/// com isso um sim nunca serve para outro descarte.
fn token(spec: &str, branch: &str, remote: bool, delete: bool) -> String {
    let seed = format!("{spec}|{branch}|{remote}|{delete}");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", hash & 0xffff_ffff)
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "discard.yes"
    } else {
        "discard.no"
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{record_open, write_at, WriteOpts};
    use clap::{Command, Subcommand};
    use tempfile::tempdir;

    fn project(root: &Path, spec: &str) {
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        // A linha da spec no índice nasce com a spec.
        assert!(mustard_core::io::spec_index::rebuild(root).is_ok());
    }

    fn discard(root: &Path, spec: &str, confirm: Option<&str>, delete: bool, remote: bool) -> Value {
        discard_for(
            &DiscardOpts {
                root: root.to_path_buf(),
                spec: Some(spec.to_string()),
                remote,
                delete,
                confirm: confirm.map(str::to_string),
            },
            None,
        )
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Um projeto com servidor: a base `dev`, a branch da spec no local e no
    /// servidor, e a spec aberta. Devolve a pasta da obra e a do servidor.
    fn project_with_server(root: &Path, spec: &str) -> (PathBuf, PathBuf) {
        let server = root.join("servidor.git");
        let work = root.join("obra");
        std::fs::create_dir_all(&work).unwrap();
        git(root, &["init", "--bare", "-q", "servidor.git"]);
        git(&work, &["init", "-q", "."]);
        git(&work, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(work.join("mustard.json"), br#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#).unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-q", "-m", "semente"]);
        git(&work, &["remote", "add", "origin", &server.to_string_lossy()]);
        git(&work, &["push", "-q", "origin", "dev"]);
        let branch = format!("feature/{spec}");
        git(&work, &["branch", &branch]);
        git(&work, &["push", "-q", "origin", &branch]);
        assert_eq!(record_open(&work, spec, &branch, "dev"), Ok(true));
        assert!(mustard_core::io::spec_index::rebuild(&work).is_ok());
        (work, server)
    }

    /// Um projeto git com a spec aberta, duas vagas de cópia dela, cada uma
    /// com o que compilou, e o que a pasta principal compilou em `target`,
    /// que o git ignora. Devolve as vagas.
    fn project_with_copies(root: &Path, spec: &str) -> Vec<PathBuf> {
        git(root, &["init", "-q", "."]);
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "semente"]);
        std::fs::create_dir_all(root.join("target").join("debug")).unwrap();
        std::fs::write(root.join("target").join("debug").join("mustard"), "compilado").unwrap();
        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        assert!(mustard_core::io::spec_index::rebuild(root).is_ok());
        crate::commands::flow::round::copies_leave_with_the_test(root);
        let slots: Vec<PathBuf> = (0..2).map(|n| mustard_core::io::wave_prompt::slot_path(root, spec, n)).collect();
        for slot in &slots {
            git(root, &["worktree", "add", "-q", "--detach", &slot.to_string_lossy()]);
            std::fs::create_dir_all(slot.join("target")).unwrap();
            std::fs::write(slot.join("target").join("compilado"), "x").unwrap();
        }
        slots
    }

    /// As cópias da obra saem com o descarte, com o que compilaram: a prévia
    /// as lista e não apaga nenhuma; o descarte confirmado tira cada vaga e a
    /// pasta das cópias da obra, e o git não as lista mais. A pasta principal
    /// e o que ela compilou ficam.
    #[test]
    fn discarding_removes_every_copy_of_the_work_and_the_preview_removes_none() {
        use mustard_core::io::wave_prompt::{shown, spec_copies_dir};
        let dir = tempdir().unwrap();
        let root = dir.path();
        let slots = project_with_copies(root, "x");
        let listed: Vec<String> = slots.iter().map(|slot| shown(slot)).collect();

        let preview = discard(root, "x", None, false, false);
        assert_eq!(preview["leaving"]["copies"], json!(listed), "{preview}");
        assert!(slots.iter().all(|slot| slot.join("target").join("compilado").is_file()), "a prévia não apaga cópia");

        let code = preview["token"].as_str().unwrap_or_default().to_string();
        let done = discard(root, "x", Some(&code), false, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(done["discarded"]["copies"], json!(listed), "{done}");
        let warnings = done["warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.iter().all(|w| w["reason"] != json!("copies-kept")), "{done}");
        assert!(slots.iter().all(|slot| !slot.exists()), "o descarte tirou as vagas: {done}");
        assert!(!spec_copies_dir(root, "x").exists(), "o descarte tirou a pasta das cópias da obra: {done}");
        let out = std::process::Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(root)
            .output()
            .expect("git");
        let registered = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(listed.iter().all(|slot| !registered.contains(slot.as_str())), "{registered}");
        let built = root.join("target").join("debug").join("mustard");
        assert_eq!(std::fs::read_to_string(built).unwrap(), "compilado", "a compilação principal fica");
        assert!(root.join("mustard.json").is_file() && root.join(".git").is_dir(), "a pasta principal fica");
    }

    /// O descarte confirmado apaga da pasta principal a pasta de compilação
    /// que o projeto declarou, e a resposta diz o que saiu; a prévia não
    /// apaga nada. O resto da pasta principal fica.
    #[test]
    fn discarding_removes_the_declared_build_output_and_the_preview_does_not() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project_with_copies(root, "x");
        std::fs::write(root.join("mustard.json"), br#"{"buildOutput":["target"]}"#).unwrap();
        let built = root.join("target").join("debug").join("mustard");

        let preview = discard(root, "x", None, false, false);
        assert!(built.is_file(), "the preview removes nothing: {preview}");
        assert!(preview.get("build_output_removed").is_none(), "{preview}");

        let code = preview["token"].as_str().unwrap_or_default().to_string();
        let done = discard(root, "x", Some(&code), false, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(done["build_output_removed"], json!(["target"]), "{done}");
        assert!(!root.join("target").exists(), "the declared folder is gone: {done}");
        assert!(root.join(".gitignore").is_file() && root.join(".git").is_dir(), "the main folder stays");
    }

    /// `true` quando o servidor ainda carrega a branch.
    fn on_server(server: &Path, branch: &str) -> bool {
        std::process::Command::new("git")
            .args(["--git-dir", &server.to_string_lossy()])
            .args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
            .output()
            .is_ok_and(|out| out.status.success())
    }

    /// A primeira chamada só mostra o que vai sair e devolve um código; nada
    /// sai do disco. A segunda, com o código, arquiva a spec e deixa a linha
    /// dela no índice com a fase descartada, sem escrever página.
    #[test]
    fn discarding_takes_two_calls_and_the_first_one_touches_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, "x");
        let folder = root.join(".claude").join("spec").join("x");

        let preview = discard(root, "x", None, false, false);
        assert_eq!(preview["preview"], json!(true), "{preview}");
        assert_eq!(preview["leaving"]["branch"], json!("feature/x"), "{preview}");
        assert_eq!(preview["leaving"]["action"], json!("archived"), "{preview}");
        let code = preview["token"].as_str().unwrap_or_default().to_string();
        assert!(!code.is_empty(), "{preview}");
        assert!(folder.join("spec.ndjson").is_file(), "a primeira chamada não tira nada");

        let wrong = discard(root, "x", Some("outro-codigo"), false, false);
        assert_eq!(wrong["reason"], json!("confirm-mismatch"), "{wrong}");
        assert!(folder.join("spec.ndjson").is_file(), "o código errado não tira nada");

        let done = discard(root, "x", Some(&code), false, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert!(!folder.exists(), "a pasta saiu do lugar");
        assert!(
            root.join(".claude/spec").join(ARCHIVE_DIR).join("x").join("spec.ndjson").is_file(),
            "a spec ficou guardada"
        );
        let index = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap_or_default();
        let line = index.lines().find(|l| l.contains("\"name\":\"x\"")).unwrap_or_else(|| panic!("{index}"));
        assert!(line.contains("\"phase\":\"discarded\""), "a linha fica, com a fase descartada: {line}");
        assert_eq!(done["index"], json!("kept"), "{done}");
        assert!(done.get("page").is_none(), "{done}");
        assert!(!root.join(".claude/spec/project.html").exists(), "o descarte não escreve a página do projeto");

        // O índice refeito do zero não a perde.
        std::fs::remove_file(root.join(".claude/spec/index.ndjson")).unwrap();
        assert!(mustard_core::io::spec_index::rebuild(root).is_ok());
        let rebuilt = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap();
        assert!(rebuilt.lines().any(|l| l == line), "{rebuilt}");
    }

    /// Com o pedido de apagar, a pasta some em vez de ser guardada, e o código
    /// de um descarte não serve para o outro.
    #[test]
    fn deleting_removes_the_folder_and_its_code_is_not_the_code_of_the_archive() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, "x");

        let archived = discard(root, "x", None, false, false)["token"].as_str().unwrap_or_default().to_string();
        let deleted = discard(root, "x", None, true, false)["token"].as_str().unwrap_or_default().to_string();
        assert_ne!(archived, deleted, "cada escolha tem o seu código");

        let done = discard(root, "x", Some(&deleted), true, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert!(!root.join(".claude").join("spec").join("x").exists(), "a pasta foi apagada");
        assert!(!root.join(".claude/spec").join(ARCHIVE_DIR).join("x").exists(), "nada foi guardado");
        assert_eq!(done["index"], json!("dropped"), "{done}");
        let index = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap_or_default();
        assert!(!index.contains("\"name\":\"x\""), "a apagada sai do índice: {index}");
        assert!(!root.join(".claude/spec/project.html").exists(), "o descarte não escreve a página do projeto");
    }

    /// A branch do servidor sai só com a opção: sem ela o descarte tira a
    /// local e deixa a do servidor no lugar; com ela, as duas saem. A branch
    /// do servidor é de todo mundo, e cada escolha tem o seu código.
    #[test]
    fn the_server_branch_goes_only_with_the_option() {
        let dir = tempdir().unwrap();
        let (work, server) = project_with_server(dir.path(), "x");
        let kept = discard(&work, "x", None, false, false);
        let taken = discard(&work, "x", None, false, true);
        assert_ne!(kept["token"], taken["token"], "cada escolha tem o seu código");
        assert_eq!(kept["leaving"]["remote"], json!(false), "{kept}");

        let code = kept["token"].as_str().unwrap_or_default().to_string();
        let done = discard(&work, "x", Some(&code), false, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(done["git"]["branchDeleted"], json!(true), "a local sai: {done}");
        assert_eq!(done["git"]["remoteDeleted"], json!(false), "{done}");
        assert!(on_server(&server, "feature/x"), "sem a opção, a do servidor fica");

        let other = tempdir().unwrap();
        let (work, server) = project_with_server(other.path(), "x");
        let code = discard(&work, "x", None, false, true)["token"].as_str().unwrap_or_default().to_string();
        let done = discard(&work, "x", Some(&code), false, true);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(done["git"]["remoteDeleted"], json!(true), "{done}");
        assert!(!on_server(&server, "feature/x"), "com a opção, a do servidor sai");
    }

    /// O descarte deixa a linha da spec na página do projeto com a fase
    /// descartada, arquivando ou apagando a pasta: a cópia nasce depois de
    /// mover, lendo o arquivo já na pasta arquivada, ou antes de apagar,
    /// gravando os lotes na pasta que a resposta diz, fora do projeto, e cada
    /// arquivo que a resposta cita existe e é JSON válido nos dois casos. A resposta não
    /// pede o registro da cópia, porque a spec descartada é terminal. A ajuda
    /// do comando de revisão de pull request não cita mais a seção de
    /// arquivos do formato antigo. E uma cópia gravada com o último item
    /// acima do que o arquivo tem sai com o último item do arquivo, sem
    /// recusa.
    #[test]
    fn the_copy_record_and_the_discard_keep_the_pages_right() {
        // O descarte arquivado.
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, "x");
        let code = discard(root, "x", None, false, false)["token"].as_str().unwrap_or_default().to_string();
        let done = discard(root, "x", Some(&code), false, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(done["copy"]["project"]["record"]["phase"], json!("discarded"), "{done}");
        let archived_batches = batches_of(&done);
        assert!(!archived_batches.is_empty(), "{done}");
        for batch in &archived_batches {
            assert!(batch.starts_with(".claude/spec/.descartadas/x/copy/"), "{batch}: not archived");
            assert_files_exist_and_parse(root, batch);
        }
        let next = done["next"].as_str().unwrap_or_default();
        assert!(!next.contains("run write copy"), "the discard still asks to record the copy: {next}");

        // O descarte apagado.
        project(root, "y");
        crate::commands::flow::round::copies_leave_with_the_test(root);
        let code = discard(root, "y", None, true, false)["token"].as_str().unwrap_or_default().to_string();
        let done = discard(root, "y", Some(&code), true, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert_eq!(done["copy"]["project"]["record"]["phase"], json!("discarded"), "{done}");
        assert!(!root.join(".claude/spec/y").exists(), "the folder is gone");
        let copy = copy_folder_of(root, &done);
        assert!(!copy.starts_with(root), "{copy:?}: the copy is outside the project");
        let deleted_batches = batches_of(&done);
        assert!(!deleted_batches.is_empty(), "{done}");
        for batch in &deleted_batches {
            let path = absolute(root, batch);
            assert!(path.starts_with(&copy), "{path:?}: not in the copy folder of the answer");
            assert_files_exist_and_parse(root, batch);
        }
        std::fs::remove_dir_all(&copy).ok();

        // A ajuda do comando de revisão de pull request.
        let tree = crate::commands::RunCmd::augment_subcommands(Command::new("run"));
        let pr_review = tree.find_subcommand("pr-review").expect("pr-review is registered");
        let help = pr_review.clone().render_long_help().to_string();
        assert!(!help.contains("## Files"), "the help still cites the old format's section: {help}");

        // O último item acima do que o arquivo tem.
        project(root, "z");
        let log_path = root.join(".claude/spec/z/spec.ndjson");
        let before = store::read(&log_path).unwrap().unwrap();
        let max = before.max_id();
        let report = write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("z".to_string()),
            event_type: "copy".to_string(),
            json: json!({"page": "spec", "last": max + 50}).to_string(),
        });
        assert_eq!(report["ok"], json!(true), "the copy record was refused: {report}");
        let after = store::read(&log_path).unwrap().unwrap();
        let recorded = after.events.iter().rev().find(|e| e.event_type == "copy").expect("the copy is in the file");
        assert_eq!(recorded.int("last"), Some(max), "{recorded:?}");
    }

    /// Abre a spec `spec` no projeto `root` e a descarta apagando a pasta, pelo
    /// caminho de quem usa: a prévia e o sim com o código dela. Devolve a
    /// pasta da cópia da página que a resposta diz e cada lote dela, com o
    /// conteúdo de agora. A pasta das cópias do projeto sai no fim do teste.
    fn deleted(root: &Path, spec: &str) -> (PathBuf, Vec<(PathBuf, String)>) {
        project(root, spec);
        crate::commands::flow::round::copies_leave_with_the_test(root);
        let code = discard(root, spec, None, true, false)["token"].as_str().unwrap_or_default().to_string();
        let done = discard(root, spec, Some(&code), true, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        let copy = copy_folder_of(root, &done);
        let batches: Vec<(PathBuf, String)> = batches_of(&done)
            .iter()
            .map(|batch| {
                let path = absolute(root, batch);
                let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
                (path, content)
            })
            .collect();
        assert!(!batches.is_empty(), "{done}");
        (copy, batches)
    }

    /// Cada descarte que apaga a spec prepara a cópia da página numa pasta só
    /// dele, fora do projeto: dois projetos com uma spec de mesmo nome, e a
    /// mesma spec apagada de novo no primeiro projeto, dão três pastas
    /// diferentes. Depois dos três, os lotes de cada descarte continuam lá,
    /// com o conteúdo que tinham logo depois dele: nenhum descarte apagou nem
    /// trocou os lotes de outro.
    #[test]
    fn each_deleting_discard_prepares_the_page_copy_in_a_folder_of_its_own() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let runs = [deleted(first.path(), "x"), deleted(second.path(), "x"), deleted(first.path(), "x")];
        let folders: Vec<&PathBuf> = runs.iter().map(|(copy, _)| copy).collect();
        assert_ne!(folders[0], folders[1], "two projects share the copy folder");
        assert_ne!(folders[0], folders[2], "two discards of the same spec share the copy folder");
        assert_ne!(folders[1], folders[2], "two projects share the copy folder");
        for (copy, batches) in &runs {
            assert!(!copy.starts_with(first.path()) && !copy.starts_with(second.path()), "{copy:?}: inside a project");
            for (path, content) in batches {
                assert!(path.starts_with(copy), "{path:?}: not in {copy:?}");
                let now = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
                assert_eq!(&now, content, "{path:?}: another discard changed the batch");
            }
        }
        for (copy, _) in &runs {
            std::fs::remove_dir_all(copy).ok();
        }
    }

    /// A cópia da página de um descarte fica depois dele, e sai no próximo
    /// descarte que apaga uma spec do mesmo projeto quando não muda há mais
    /// de um dia: a de um dia e um minuto sai, a de um dia menos um minuto
    /// fica, e a do descarte novo nasce.
    #[test]
    fn a_page_copy_older_than_a_day_leaves_at_the_next_deleting_discard() {
        use crate::commands::maint::scratch_gc::set_mtime;
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (old, _) = deleted(root, "a");
        let (recent, _) = deleted(root, "b");
        let now = SystemTime::now();
        set_mtime(&old, now - DISCARD_COPY_KEPT - Duration::from_secs(60));
        set_mtime(&recent, now - DISCARD_COPY_KEPT + Duration::from_secs(60));

        let (new, _) = deleted(root, "c");
        assert!(!old.exists(), "{old:?}: the copy older than a day stayed");
        assert!(recent.is_dir(), "{recent:?}: the copy younger than a day left");
        assert!(new.is_dir(), "{new:?}: the new copy is missing");
        for copy in [&recent, &new] {
            std::fs::remove_dir_all(copy).ok();
        }
    }

    /// A pasta da cópia da página que a resposta `report` de um descarte diz.
    fn copy_folder_of(root: &Path, report: &Value) -> PathBuf {
        let folder = report["copy"]["folder"].as_str().unwrap_or_default();
        assert!(!folder.is_empty(), "{report}");
        absolute(root, folder)
    }

    /// Os lotes que a cópia da spec e a do projeto da resposta `report` de um
    /// descarte citam, das duas páginas juntas.
    fn batches_of(report: &Value) -> Vec<String> {
        report["copy"]["spec"]["batches"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(report["copy"]["project"]["batches"].as_array().into_iter().flatten())
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect()
    }

    /// O caminho de `batch`, relativo a `root` quando ele é relativo, ou o
    /// caminho absoluto que a cópia deu, quando `root` não é o dono dele.
    fn absolute(root: &Path, batch: &str) -> PathBuf {
        let path = PathBuf::from(batch);
        if path.is_absolute() {
            path
        } else {
            root.join(path)
        }
    }

    /// O lote `batch` existe, é JSON válido, e cada documento que ele cita
    /// pelo `file_path` também existe e é JSON válido.
    fn assert_files_exist_and_parse(root: &Path, batch: &str) {
        let path = absolute(root, batch);
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let writes: Value = serde_json::from_str(&content).unwrap_or_else(|e| panic!("{path:?} is not JSON: {e}"));
        for write in writes.as_array().unwrap_or(&Vec::new()) {
            let Some(file) = write["file_path"].as_str() else { continue };
            let doc = absolute(root, file);
            let doc_content = std::fs::read_to_string(&doc).unwrap_or_else(|e| panic!("{doc:?}: {e}"));
            serde_json::from_str::<Value>(&doc_content).unwrap_or_else(|e| panic!("{doc:?} is not JSON: {e}"));
        }
    }
}
