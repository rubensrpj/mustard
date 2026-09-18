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
//! A pasta da spec é arquivada por padrão, ao lado das outras, e só é apagada
//! quando quem chama pede: nada fica pela metade, e nada some sem se pedir.
//! A spec arquivada continua no índice, com a fase descartada: dá para achar
//! depois o que foi decidido e por que parou. A apagada sai do índice junto
//! com a pasta. O descarte não escreve página nenhuma.

use std::path::{Path, PathBuf};

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

    // A pasta arquivada fica com a linha do índice; a apagada a leva junto:
    // nada fica pela metade.
    let (moved, index_done) = if opts.delete {
        let removed = std::fs::remove_dir_all(&folder).is_ok();
        (removed, mustard_core::io::spec_index::drop_line(&project.root, &spec).is_ok())
    } else {
        (archive(&spec, &folder), phase_written)
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
    out
}

/// Guarda a pasta da spec ao lado das outras descartadas. `true` quando ela
/// saiu do lugar.
fn archive(spec: &str, folder: &Path) -> bool {
    let Some(specs) = folder.parent() else { return false };
    let target = specs.join(ARCHIVE_DIR).join(spec);
    if std::fs::create_dir_all(target.parent().unwrap_or(&target)).is_err() {
        return false;
    }
    if target.exists() {
        let _ = std::fs::remove_dir_all(&target);
    }
    std::fs::rename(folder, &target).is_ok()
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
    use crate::commands::spec_events::write::record_open;
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
}
