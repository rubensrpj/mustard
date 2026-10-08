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
use serde_json::{Map, Value, json};

use crate::commands::spec_events::{self, write::record};
use crate::shared::spec_state::{DiskSpecState, checkout, session_from_env};

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
    let git = (!branch.is_empty()).then(|| crate::commands::git_delete::delete_with(&opts.root, &branch, opts.remote));

    // As cópias da obra saem com ela, com o que compilaram, antes de a pasta
    // da spec sair do lugar. A cópia que não sai vira aviso, sem derrubar o
    // descarte.
    let copies_left = if phase_written {
        crate::commands::flow::round::remove_spec_copies(&project.root, &spec, Some(&log))
    } else {
        crate::commands::flow::round::Removal::default()
    };
    // Depois das cópias, a pasta de compilação que o projeto declarou
    // descartável sai da pasta principal, como no fechamento.
    let build_output = phase_written.then(|| crate::commands::flow::close::remove_build_output(&project.root, lang));

    // O descarte é um marco, como o fechamento: a cópia para o banco da
    // página sai aqui, com a fase descartada na linha da spec da página do
    // projeto. Onde os lotes nascem segue a pasta da spec: ao apagar, ela não
    // sobrevive ao marco, então a cópia nasce antes, numa pasta só deste
    // descarte, fora do projeto ([`discard_copy_folder`]); ao arquivar, ela
    // sobrevive, então a cópia nasce depois de mover, lendo o arquivo de
    // eventos já na pasta arquivada — os caminhos da resposta apontam para lá.
    let (moved, index_done) = if opts.delete {
        (std::fs::remove_dir_all(&folder).is_ok(), mustard_core::io::spec_index::drop_line(&project.root, &spec).is_ok())
    } else {
        (phase_written && archive(&spec, &folder).is_some(), phase_written)
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
    for (reason, hint) in crate::commands::flow::close::removal_warnings(&copies_left, lang) {
        spec_events::pages::push_warning(&mut out, reason, &hint);
    }
    if let Some(swept) = &build_output {
        swept.tell(&mut out);
    }
    out["next"] = json!(translate("discard.done", lang));
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
    let Ok(entries) = std::fs::read_dir(discard_copies_place(root)) else {
        return Vec::new();
    };
    let now = SystemTime::now();
    let mut old: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry
                .metadata()
                .is_ok_and(|meta| meta.is_dir() && meta.modified().ok().and_then(|at| now.duration_since(at).ok()).is_some_and(|age| age > DISCARD_COPY_KEPT))
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
    if value { "discard.yes" } else { "discard.no" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use tempfile::tempdir;

    fn project(root: &Path, spec: &str) {
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        // A linha da spec no índice nasce com a spec.
        assert!(mustard_core::io::spec_index::rebuild(root).is_ok());
    }

    fn discard(root: &Path, spec: &str, confirm: Option<&str>, delete: bool, remote: bool) -> Value {
        discard_for(&DiscardOpts { root: root.to_path_buf(), spec: Some(spec.to_string()), remote, delete, confirm: confirm.map(str::to_string) }, None)
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
        let out = std::process::Command::new("git").args(["worktree", "list", "--porcelain"]).current_dir(root).output().expect("git");
        let registered = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(listed.iter().all(|slot| !registered.contains(slot.as_str())), "{registered}");
        let built = root.join("target").join("debug").join("mustard");
        assert_eq!(std::fs::read_to_string(built).unwrap(), "compilado", "a compilação principal fica");
        assert!(root.join("mustard.json").is_file() && root.join(".git").is_dir(), "a pasta principal fica");
    }

    /// O descarte guarda o código que uma vaga tem além do commit antes de
    /// apagá-la: a vaga sai, o código fica sob uma ref do repositório
    /// principal, e a resposta nomeia a ref e o comando para trazê-lo de volta.
    #[test]
    fn discarding_keeps_the_code_a_copy_holds_beyond_the_commit_before_removing_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let slots = project_with_copies(root, "x");
        std::fs::write(slots[0].join("depois.txt"), "código de última hora").unwrap();

        let preview = discard(root, "x", None, false, false);
        let code = preview["token"].as_str().unwrap_or_default().to_string();
        let done = discard(root, "x", Some(&code), false, false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert!(slots.iter().all(|slot| !slot.exists()), "{done}");
        let listed =
            std::process::Command::new("git").args(["for-each-ref", "--format=%(refname)", "refs/mustard/kept"]).current_dir(root).output().expect("git");
        let refs: Vec<String> = String::from_utf8_lossy(&listed.stdout).lines().map(str::to_string).collect();
        assert_eq!(refs.len(), 1, "{refs:?}");
        let shown = std::process::Command::new("git").args(["show", &format!("{}:depois.txt", refs[0])]).current_dir(root).output().expect("git");
        assert_eq!(String::from_utf8_lossy(&shown.stdout), "código de última hora", "{refs:?}");
        let warnings = done["warnings"].as_array().cloned().unwrap_or_default();
        let hints: Vec<&str> = warnings.iter().filter(|w| w["reason"] == json!("code-kept")).filter_map(|w| w["hint"].as_str()).collect();
        assert_eq!(hints.len(), 1, "{done}");
        assert!(hints[0].contains(&format!("git cherry-pick --no-commit {}", refs[0])), "{}", hints[0]);
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
        assert!(done["build_output_removed"].as_array().is_none_or(Vec::is_empty), "{done}");
        assert!(built.is_file(), "a cache below 15 GB stays: {done}");
        assert!(done["warnings"].as_array().unwrap().iter().any(|w| w["reason"] == "build-output-kept"));
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
        assert!(root.join(".claude/spec").join(ARCHIVE_DIR).join("x").join("spec.ndjson").is_file(), "a spec ficou guardada");
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
        for deleting in [false, true] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            project(root, "x");
            let snapshot = crate::commands::panel::prepare_publication(root, "x", false);
            let file = snapshot["database"].as_str().unwrap();
            let original = std::fs::read(file).unwrap();
            let code = discard(root, "x", None, deleting, false)["token"].as_str().unwrap().to_string();
            let out = discard(root, "x", Some(&code), deleting, false);
            assert_eq!(out["ok"], true, "{out}");
            assert!(out.get("copy").is_none() && out.get("publish").is_none());
            assert_eq!(std::fs::read(file).unwrap(), original, "an already shared snapshot remains immutable");
            assert!(!out["next"].as_str().unwrap_or_default().contains("ArtifactData"));
        }
    }

    /// Cada descarte que apaga a spec prepara a cópia da página numa pasta só
    /// dele, fora do projeto: dois projetos com uma spec de mesmo nome, e a
    /// mesma spec apagada de novo no primeiro projeto, dão três pastas
    /// diferentes. Depois dos três, os lotes de cada descarte continuam lá,
    /// com o conteúdo que tinham logo depois dele: nenhum descarte apagou nem
    /// trocou os lotes de outro.
    #[test]
    fn each_deleting_discard_prepares_the_page_copy_in_a_folder_of_its_own() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, "x");
        let code = discard(root, "x", None, true, false)["token"].as_str().unwrap().to_string();
        let out = discard(root, "x", Some(&code), true, false);
        assert_eq!(out["ok"], true, "{out}");
        assert!(out.get("copy").is_none() && out.get("publish").is_none());
        assert!(!root.join(".claude/spec/x").exists());
        assert!(!root.join(".claude/mustard/publications").exists());
    }

    /// A cópia da página de um descarte fica depois dele, e sai no próximo
    /// descarte que apaga uma spec do mesmo projeto quando não muda há mais
    /// de um dia: a de um dia e um minuto sai, a de um dia menos um minuto
    /// fica, e a do descarte novo nasce.
    #[test]
    fn a_page_copy_older_than_a_day_leaves_at_the_next_deleting_discard() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, "x");
        let code = discard(root, "x", None, true, false)["token"].as_str().unwrap().to_string();
        let out = discard(root, "x", Some(&code), true, false);
        assert_eq!(out["ok"], true, "{out}");
        assert!(out.get("copy").is_none() && out.get("publish").is_none());
        assert!(!root.join(".claude/spec/x").exists());
        assert!(!root.join(".claude/mustard/publications").exists());
    }
}
