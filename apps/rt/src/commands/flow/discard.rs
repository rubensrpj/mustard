//! `mustard-rt run discard [--spec <nome>]` — descartar uma spec, em dois
//! passos.
//!
//! Descartar fecha o pull request e apaga a branch local, e isso não tem
//! volta. Por isso a porta é a mesma da remoção de pendências: a primeira
//! chamada só mostra o que vai sair — o pull request, a branch local, a do
//! servidor quando a opção vier, a pasta da spec e a linha dela no índice — e
//! devolve um código; a segunda, com esse código e depois do sim do usuário,
//! faz. O código vem do que seria tirado, então um sim nunca serve para outro
//! descarte.
//!
//! A pasta da spec é arquivada por padrão, ao lado das outras, e só é apagada
//! quando quem chama pede: nada fica pela metade, e nada some sem se pedir.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::translate;
use serde_json::{json, Map, Value};

use crate::commands::spec_events::{self, read::checkout, write::record};
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// A pasta em que as specs descartadas ficam guardadas, dentro da pasta das
/// specs.
const ARCHIVE_DIR: &str = ".descartadas";

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
    // da spec, e é ele que diz por que ela some do índice.
    let mut draft = Map::new();
    draft.insert("phase".to_string(), json!("discarded"));
    draft.insert("author".to_string(), json!("binary"));
    draft.insert("reason".to_string(), json!(translate("discard.reason", lang)));
    let phase_written = record(&opts.root, &spec, "state", draft, PhaseWriter::Binary).is_ok();

    // O pull request e as branches, pela mesma porta que descarta uma unidade
    // abandonada. A do servidor só com a opção.
    let git = (!branch.is_empty())
        .then(|| crate::commands::git_delete::delete_with(&opts.root, &branch, opts.remote));

    // A pasta e a linha do índice saem juntas: nada fica pela metade.
    let moved = if opts.delete {
        std::fs::remove_dir_all(&folder).is_ok()
    } else {
        archive(&project.root, &spec, &folder)
    };
    let index_dropped = mustard_core::io::spec_index::drop_line(&project.root, &spec).is_ok();
    if let Some(sid) = session {
        crate::shared::context::unbind_session_spec(&opts.root.to_string_lossy(), sid);
    }

    let mut out = json!({
        "ok": moved && index_dropped,
        "spec": spec,
        "discarded": leaving,
        "phase": phase_written.then(|| json!("discarded")),
        "indexDropped": index_dropped,
    });
    if let Some(git) = git {
        out["git"] = git;
    }
    if !(moved && index_dropped) {
        out["reason"] = json!("discard-incomplete");
        out["hint"] = json!(translate("discard.incomplete", lang));
    }
    out
}

/// Guarda a pasta da spec ao lado das outras descartadas. `true` quando ela
/// saiu do lugar.
fn archive(root: &Path, spec: &str, folder: &Path) -> bool {
    let Some(specs) = folder.parent() else { return false };
    let target = specs.join(ARCHIVE_DIR).join(spec);
    let _ = root;
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

/// Descarta a spec e imprime o relatório; sai com 1 na recusa.
pub fn run_cmd(opts: &DiscardOpts) {
    let report = discard_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
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

    fn discard(root: &Path, spec: &str, confirm: Option<&str>, delete: bool) -> Value {
        discard_for(
            &DiscardOpts {
                root: root.to_path_buf(),
                spec: Some(spec.to_string()),
                remote: false,
                delete,
                confirm: confirm.map(str::to_string),
            },
            None,
        )
    }

    /// A primeira chamada só mostra o que vai sair e devolve um código; nada
    /// sai do disco. A segunda, com o código, arquiva a spec e tira a linha
    /// dela do índice.
    #[test]
    fn discarding_takes_two_calls_and_the_first_one_touches_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, "x");
        let folder = root.join(".claude").join("spec").join("x");

        let preview = discard(root, "x", None, false);
        assert_eq!(preview["preview"], json!(true), "{preview}");
        assert_eq!(preview["leaving"]["branch"], json!("feature/x"), "{preview}");
        assert_eq!(preview["leaving"]["action"], json!("archived"), "{preview}");
        let code = preview["token"].as_str().unwrap_or_default().to_string();
        assert!(!code.is_empty(), "{preview}");
        assert!(folder.join("spec.ndjson").is_file(), "a primeira chamada não tira nada");

        let wrong = discard(root, "x", Some("outro-codigo"), false);
        assert_eq!(wrong["reason"], json!("confirm-mismatch"), "{wrong}");
        assert!(folder.join("spec.ndjson").is_file(), "o código errado não tira nada");

        let done = discard(root, "x", Some(&code), false);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert!(!folder.exists(), "a pasta saiu do lugar");
        assert!(
            root.join(".claude/spec").join(ARCHIVE_DIR).join("x").join("spec.ndjson").is_file(),
            "a spec ficou guardada"
        );
        let index = std::fs::read_to_string(root.join(".claude/spec/index.ndjson")).unwrap_or_default();
        assert!(!index.contains("\"x\""), "a linha da spec saiu do índice: {index}");
    }

    /// Com o pedido de apagar, a pasta some em vez de ser guardada, e o código
    /// de um descarte não serve para o outro.
    #[test]
    fn deleting_removes_the_folder_and_its_code_is_not_the_code_of_the_archive() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, "x");

        let archived = discard(root, "x", None, false)["token"].as_str().unwrap_or_default().to_string();
        let deleted = discard(root, "x", None, true)["token"].as_str().unwrap_or_default().to_string();
        assert_ne!(archived, deleted, "cada escolha tem o seu código");

        let done = discard(root, "x", Some(&deleted), true);
        assert_eq!(done["ok"], json!(true), "{done}");
        assert!(!root.join(".claude").join("spec").join("x").exists(), "a pasta foi apagada");
        assert!(!root.join(".claude/spec").join(ARCHIVE_DIR).join("x").exists(), "nada foi guardado");
    }
}
