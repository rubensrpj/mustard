//! As cópias de obra que sobraram no disco, na limpeza (`mustard-rt run
//! clean`).
//!
//! Cada obra prepara cópias do projeto para as ondas e a revisão, com a
//! compilação dentro delas. O fechamento e o descarte as apagam; a limpeza
//! recolhe o que ficou de obras que já acabaram: fechadas, com o pull request
//! aberto, entregues, descartadas ou que não existem mais. A cópia de obra
//! aberta fica, com o motivo no relatório. A regra de idade da limpeza não
//! vale aqui: a obra que acabou não volta a usar a cópia.
//!
//! Os lugares olhados são dois, e nada fora deles sai:
//!
//! - a pasta das cópias do projeto ([`copies_dir`]), com a pasta nova de cada
//!   obra (`<spec>/<vaga>`) e as antigas (`<spec>-<n>` e
//!   `<spec>-final-review`);
//! - o lugar antigo dentro do projeto, `.claude/worktrees/mustard-<spec>-<n>`.
//!   Pasta dali sem o prefixo não é do Mustard e nem entra na lista.
//!
//! A pasta das cópias de página dos descartes ([`DISCARD_COPIES`]) mora na
//! pasta das cópias, mas não é cópia de obra e não entra na lista delas. Dela
//! sai só a cópia de um descarte que passou do prazo, pela mesma varredura do
//! descarte ([`sweep_old_discard_copies`]), numa linha própria do relatório;
//! a de um descarte recém-feito fica.
//!
//! A pasta principal e a compilação dela nunca entram. Link não é seguido.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::io::spec_index::DISCARDED_DIR;
use mustard_core::io::wave_prompt::{copies_dir, shown};
use serde::Serialize;

use super::scratch_gc::ErrorRecord;
use crate::commands::flow::discard::{sweep_old_discard_copies, DISCARD_COPIES};
use crate::commands::flow::round::{remove_copy, remove_spec_copies};

/// O prefixo das cópias antigas dentro do projeto.
const OLD_PREFIX: &str = "mustard-";

/// O sufixo da cópia antiga do revisor final.
const FINAL_REVIEW_SUFFIX: &str = "-final-review";

/// As fases de obra que acabou: a cópia dela sai.
const FINISHED: [&str; 4] = ["closed", "pr_open", "delivered", "discarded"];

/// O motivo, no relatório, da cópia de página de descarte que sai.
const OLD_DISCARD_COPY: &str = "discard page copy older than a day";

/// Uma cópia de obra achada, com a obra dela e o motivo de sair ou ficar.
#[derive(Debug, Serialize)]
pub(crate) struct CopyRecord {
    pub path: String,
    pub spec: String,
    /// `"spec <fase>"` ou `"spec missing"` para a que sai; `"spec still
    /// open: <fase>"` ou o que impediu de ler a obra, para a que fica. A
    /// cópia de página de descarte que passou do prazo, sem obra, sai com
    /// [`OLD_DISCARD_COPY`].
    pub reason: String,
    /// A pasta a tirar, fora do JSON.
    #[serde(skip)]
    dir: PathBuf,
    /// A pasta de todas as vagas da obra, e não uma cópia só.
    #[serde(skip)]
    whole: bool,
    /// A obra acabou ou não existe mais: a cópia sai.
    #[serde(skip)]
    leaves: bool,
}

/// A parte das cópias de obra no relatório da limpeza.
#[derive(Debug, Serialize)]
pub(crate) struct CopiesReport {
    /// O projeto cujas cópias foram olhadas.
    pub root: String,
    pub candidates: Vec<CopyRecord>,
    pub kept: Vec<CopyRecord>,
    pub removed: Vec<String>,
    pub errors: Vec<ErrorRecord>,
}

/// As cópias de obra do projeto em que `start` está: lista as que saem e as
/// que ficam e, com `apply`, tira as que saem. As cópias de página de
/// descarte que passaram do prazo entram entre as que saem, cada uma numa
/// linha. `None` fora de projeto git.
pub(crate) fn clean(start: &Path, apply: bool) -> Option<CopiesReport> {
    let root = crate::commands::spec_events::project(start).root;
    if !root.join(".git").exists() {
        return None;
    }
    let (candidates, kept): (Vec<CopyRecord>, Vec<CopyRecord>) = found(&root).into_iter().partition(|record| record.leaves);
    let mut report = CopiesReport { root: shown(&root), candidates, kept, removed: Vec::new(), errors: Vec::new() };
    if apply {
        remove(&root, &mut report);
    }
    for (dir, error) in sweep_old_discard_copies(&root, apply) {
        let path = shown(&dir);
        if apply {
            match error {
                None => report.removed.push(path.clone()),
                Some(error) => report.errors.push(ErrorRecord { path: path.clone(), error }),
            }
        }
        let reason = OLD_DISCARD_COPY.to_string();
        report.candidates.push(CopyRecord { path, spec: String::new(), reason, dir, whole: false, leaves: true });
    }
    report.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    report.removed.sort();
    Some(report)
}

/// Tira as candidatas: a pasta nova de cada obra por [`remove_spec_copies`],
/// que prende a trava do passo do git; as cópias antigas por
/// [`remove_copy`], com a mesma trava presa.
fn remove(root: &Path, report: &mut CopiesReport) {
    let (whole, single): (Vec<&CopyRecord>, Vec<&CopyRecord>) = report.candidates.iter().partition(|record| record.whole);
    let mut removed = Vec::new();
    let mut errors = Vec::new();
    for record in whole {
        let left = remove_spec_copies(root, &record.spec);
        if left.is_empty() {
            removed.push(record.path.clone());
        }
        errors.extend(left.into_iter().map(|(path, error)| ErrorRecord { path, error }));
    }
    if !single.is_empty() {
        match crate::commands::git_settle::git_step_lock(root) {
            Ok(_held) => {
                for record in single {
                    match remove_copy(root, &record.dir) {
                        Ok(()) => removed.push(record.path.clone()),
                        Err(error) => errors.push(ErrorRecord { path: record.path.clone(), error }),
                    }
                }
            }
            Err(error) => errors.push(ErrorRecord { path: shown(root), error }),
        }
    }
    removed.sort();
    report.removed = removed;
    report.errors = errors;
}

/// Toda cópia de obra dos dois lugares, em ordem de caminho, cada uma com o
/// motivo de sair ou ficar. A pasta das cópias de página dos descartes não
/// é cópia de obra e não entra.
fn found(root: &Path) -> Vec<CopyRecord> {
    let mut records = Vec::new();
    for dir in folders(&copies_dir(root)) {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name == DISCARD_COPIES {
            continue;
        }
        let whole = !dir.join(".git").exists();
        let spec = if whole { Some(name) } else { old_spec(&name) };
        records.push(record(root, dir, spec, whole));
    }
    for dir in folders(&root.join(".claude").join("worktrees")) {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let Some(rest) = name.strip_prefix(OLD_PREFIX) else { continue };
        if dir.join(".git").is_file() {
            let spec = old_spec(rest);
            records.push(record(root, dir, spec, false));
        }
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    records
}

/// A cópia `dir`, da obra `spec` quando o nome a diz, com o motivo.
fn record(root: &Path, dir: PathBuf, spec: Option<String>, whole: bool) -> CopyRecord {
    let (leaves, reason) = match &spec {
        Some(spec) => fate(root, spec),
        None => (false, "not a copy of a work".to_string()),
    };
    CopyRecord { path: shown(&dir), spec: spec.unwrap_or_default(), reason, dir, whole, leaves }
}

/// As pastas de dentro de `place`, sem seguir link.
fn folders(place: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(place)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default()
}

/// A obra do nome antigo de cópia: `<spec>-final-review` ou `<spec>-<n>`.
fn old_spec(name: &str) -> Option<String> {
    if let Some(spec) = name.strip_suffix(FINAL_REVIEW_SUFFIX) {
        return Some(spec.to_string()).filter(|spec| !spec.is_empty());
    }
    let (spec, wave) = name.rsplit_once('-')?;
    (!spec.is_empty() && !wave.is_empty() && wave.bytes().all(|b| b.is_ascii_digit())).then(|| spec.to_string())
}

/// Se a cópia da obra `spec` sai, e por quê: sai a da obra que acabou ou
/// que não existe mais — nem entre as specs, nem entre as descartadas —; fica
/// a da obra aberta e a da obra que não se pôde ler.
fn fate(root: &Path, spec: &str) -> (bool, String) {
    let Ok(file) = store::spec_file(root, spec) else {
        return (false, "not a spec name".to_string());
    };
    match store::read(&file) {
        Ok(Some(log)) => {
            let phase = State::from_log(&log).phase.unwrap_or("unknown");
            if FINISHED.contains(&phase) {
                (true, format!("spec {phase}"))
            } else {
                (false, format!("spec still open: {phase}"))
            }
        }
        Ok(None) => {
            let specs = file.parent().and_then(Path::parent).unwrap_or(root);
            if specs.join(DISCARDED_DIR).join(spec).is_dir() {
                (true, "spec discarded".to_string())
            } else {
                (true, "spec missing".to_string())
            }
        }
        Err(_) => (false, "spec unreadable".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::record_open;
    use mustard_core::io::wave_prompt::slot_path;
    use serde_json::json;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Um projeto git com o que a pasta principal compilou em `target`, que
    /// o git ignora.
    fn project(root: &Path) {
        git(root, &["init", "-q", "."]);
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n.claude/\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "semente"]);
        std::fs::create_dir_all(root.join("target").join("debug")).unwrap();
        std::fs::write(root.join("target").join("debug").join("mustard"), "compilado").unwrap();
        crate::commands::flow::round::copies_leave_with_the_test(root);
    }

    /// A obra `spec` levada, passo a passo do fluxo, até a fase `phase`.
    fn work(root: &Path, spec: &str, phase: &str) {
        const STEPS: &[&str] = &["survey", "plan", "approved", "running", "closed"];
        let path = store::spec_file(root, spec).expect("spec file");
        std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
        for step in STEPS {
            let mut fields = json!({"phase": step, "author": "binary"});
            if *step == "survey" {
                fields["branch"] = json!(format!("feature/{spec}"));
                fields["base"] = json!("dev");
            }
            if *step == "approved" {
                fields["witness"] = json!({"question": "Aprovar?", "answer": "Aprovar"});
            }
            store::write(&path, "state", fields.as_object().cloned().expect("an object"), &[]).expect("state");
            if *step == phase {
                return;
            }
        }
    }

    /// Uma cópia registrada no git em `dir`, com o que ela compilou.
    fn copy(root: &Path, dir: &Path) {
        git(root, &["worktree", "add", "-q", "--detach", &dir.to_string_lossy()]);
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("target").join("compilado"), "x").unwrap();
    }

    fn registered(root: &Path) -> String {
        let out = std::process::Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(root)
            .output()
            .expect("git");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn paths(records: &[CopyRecord]) -> Vec<String> {
        records.iter().map(|record| record.path.clone()).collect()
    }

    /// A limpeza lista as cópias de obra fechada, descartada e que não existe
    /// mais, nos dois lugares e no desenho novo e no antigo, e deixa a da
    /// obra aberta, com o motivo. Sem a opção de apagar, nada sai; com ela,
    /// saem as listadas — pasta e registro do git — e a da obra aberta fica.
    /// A pasta principal e o que ela compilou ficam sempre.
    #[test]
    fn clean_removes_the_copies_of_finished_or_missing_works_and_keeps_the_open_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        work(root, "fechada", "closed");
        work(root, "aberta", "running");
        work(root, "velha", "closed");
        assert_eq!(record_open(root, "descartada", "feature/descartada", "dev"), Ok(true));
        let copies = copies_dir(root);
        let closed = [slot_path(root, "fechada", 0), slot_path(root, "fechada", 1)];
        let open = slot_path(root, "aberta", 0);
        let missing = slot_path(root, "sumiu", 0);
        let old_here = copies.join("velha-3");
        let old_inside = root.join(".claude").join("worktrees").join("mustard-velha-7");
        let old_open = copies.join("aberta-final-review");
        let theirs = root.join(".claude").join("worktrees").join("outra-coisa");
        for slot in closed.iter().chain([&open, &missing, &old_here, &old_inside, &old_open, &theirs]) {
            copy(root, slot);
        }
        let discarded = slot_path(root, "descartada", 0);
        copy(root, &discarded);
        let code = crate::commands::flow::discard::discard_for(
            &crate::commands::flow::discard::DiscardOpts {
                root: root.to_path_buf(),
                spec: Some("descartada".into()),
                remote: false,
                delete: false,
                confirm: None,
            },
            None,
        )["token"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        // O descarte já tira a cópia; uma nova, deixada depois, é a sobra.
        crate::commands::flow::discard::discard_for(
            &crate::commands::flow::discard::DiscardOpts {
                root: root.to_path_buf(),
                spec: Some("descartada".into()),
                remote: false,
                delete: false,
                confirm: Some(code),
            },
            None,
        );
        assert!(!discarded.exists(), "o descarte tirou a cópia dele");
        copy(root, &discarded);

        let leaves = |spec: &str| shown(&copies.join(spec));
        let listed = clean(root, false).expect("um projeto git");
        assert_eq!(
            paths(&listed.candidates),
            vec![shown(&old_inside), leaves("descartada"), leaves("fechada"), leaves("sumiu"), shown(&old_here)]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>(),
            "{listed:?}"
        );
        assert_eq!(paths(&listed.kept), {
            let mut kept = vec![leaves("aberta"), shown(&old_open)];
            kept.sort();
            kept
        });
        assert!(listed.kept.iter().all(|record| record.reason == "spec still open: running"), "{listed:?}");
        assert!(listed.removed.is_empty(), "{listed:?}");
        assert!(closed.iter().all(|slot| slot.join("target").is_dir()), "sem a opção de apagar, nada sai");
        assert!(old_inside.join(".git").is_file() && missing.is_dir() && discarded.is_dir());

        let applied = clean(root, true).expect("um projeto git");
        assert!(applied.errors.is_empty(), "{applied:?}");
        assert_eq!(applied.removed, paths(&listed.candidates), "{applied:?}");
        for gone in closed.iter().chain([&missing, &old_here, &old_inside, &discarded]) {
            assert!(!gone.exists(), "{} saiu", gone.display());
        }
        for spec in ["fechada", "sumiu", "descartada"] {
            assert!(!copies.join(spec).exists(), "a pasta das cópias de {spec} saiu");
        }
        let still = registered(root);
        for gone in closed.iter().chain([&missing, &old_here, &old_inside, &discarded]) {
            assert!(!still.contains(&shown(gone)), "{still}");
        }
        assert!(open.join("target").join("compilado").is_file(), "a cópia da obra aberta fica");
        assert!(old_open.join(".git").is_file(), "a cópia antiga da obra aberta fica");
        assert!(theirs.join(".git").is_file(), "a pasta sem o prefixo não é do Mustard e fica");
        let built = root.join("target").join("debug").join("mustard");
        assert_eq!(std::fs::read_to_string(built).unwrap(), "compilado", "a compilação principal fica");
        assert!(root.join("mustard.json").is_file() && root.join(".git").is_dir(), "a pasta principal fica");
    }

    /// Fora de projeto git, a limpeza não olha cópia de obra nenhuma.
    #[test]
    fn outside_a_git_project_clean_leaves_the_copies_alone() {
        let dir = tempdir().unwrap();
        assert!(clean(dir.path(), true).is_none());
    }

    /// O nome antigo de cópia diz a obra; o que não segue o desenho não diz.
    #[test]
    fn the_old_copy_name_tells_the_work() {
        assert_eq!(old_spec("obra-de-exemplo-12").as_deref(), Some("obra-de-exemplo"));
        assert_eq!(old_spec("x-final-review").as_deref(), Some("x"));
        assert_eq!(old_spec("sem-numero"), None);
        assert_eq!(old_spec("-3"), None);
    }

    /// Abre a obra `spec` e a descarta apagando a pasta dela, pelo caminho de
    /// quem usa: a prévia e o sim com o código. Devolve a pasta da cópia da
    /// página que o descarte deixou na pasta das cópias do projeto.
    fn deleted(root: &Path, spec: &str) -> PathBuf {
        use crate::commands::flow::discard::{discard_for, DiscardOpts};
        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        assert!(mustard_core::io::spec_index::rebuild(root).is_ok());
        let opts = |confirm: Option<String>| DiscardOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            remote: false,
            delete: true,
            confirm,
        };
        let code = discard_for(&opts(None), None)["token"].as_str().unwrap_or_default().to_string();
        let done = discard_for(&opts(Some(code)), None);
        assert_eq!(done["ok"], json!(true), "{done}");
        let folder = root.join(done["copy"]["folder"].as_str().unwrap_or_default());
        assert!(folder.is_dir() && folder.starts_with(copies_dir(root).join(DISCARD_COPIES)), "{done}");
        folder
    }

    /// Toda linha do relatório, das que saem e das que ficam.
    fn every_record(report: &CopiesReport) -> impl Iterator<Item = &CopyRecord> {
        report.candidates.iter().chain(&report.kept)
    }

    /// A cópia da página de um descarte recém-feito fica depois da limpeza
    /// com a opção de apagar, com os lotes dela, e nem aparece no relatório.
    #[test]
    fn a_fresh_discard_page_copy_stays_after_clean_with_apply() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        let fresh = deleted(root, "apagada");
        let batches = std::fs::read_dir(&fresh).unwrap().count();
        assert!(batches > 0, "{fresh:?}: the discard left no batch");

        let applied = clean(root, true).expect("um projeto git");
        assert!(fresh.is_dir(), "{fresh:?}: the fresh discard page copy left: {applied:?}");
        assert_eq!(std::fs::read_dir(&fresh).unwrap().count(), batches, "{applied:?}");
        assert!(applied.removed.is_empty() && applied.errors.is_empty(), "{applied:?}");
        assert!(every_record(&applied).all(|record| !record.path.contains(DISCARD_COPIES)), "{applied:?}");
    }

    /// A pasta das cópias de página dos descartes não entra no relatório
    /// como cópia de obra: nem como obra que não existe mais, nem como nome
    /// que não é de spec, nem entre as que ficam.
    #[test]
    fn the_report_does_not_list_the_discard_copies_folder_as_a_work() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        deleted(root, "apagada");
        let place = shown(&copies_dir(root).join(DISCARD_COPIES));

        let listed = clean(root, false).expect("um projeto git");
        assert!(
            every_record(&listed).all(|record| record.spec != DISCARD_COPIES && record.path != place),
            "{listed:?}"
        );
        assert!(listed.candidates.is_empty() && listed.kept.is_empty(), "{listed:?}");
    }

    /// A cópia da página de um descarte que não muda há um dia e um minuto
    /// ganha uma linha própria no relatório, com o motivo em palavras, e sai
    /// só com a opção de apagar; a de um dia menos um minuto fica sempre.
    #[test]
    fn a_discard_page_copy_older_than_a_day_leaves_at_clean_with_apply() {
        use crate::commands::flow::discard::DISCARD_COPY_KEPT;
        use crate::commands::maint::scratch_gc::set_mtime;
        use std::time::{Duration, SystemTime};
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root);
        let old = deleted(root, "velha");
        let recent = deleted(root, "recente");
        let now = SystemTime::now();
        set_mtime(&old, now - DISCARD_COPY_KEPT - Duration::from_secs(60));
        set_mtime(&recent, now - DISCARD_COPY_KEPT + Duration::from_secs(60));

        let listed = clean(root, false).expect("um projeto git");
        let lines: Vec<(&str, &str, &str)> =
            listed.candidates.iter().map(|r| (r.path.as_str(), r.spec.as_str(), r.reason.as_str())).collect();
        assert_eq!(lines, vec![(shown(&old).as_str(), "", OLD_DISCARD_COPY)], "{listed:?}");
        assert!(listed.kept.is_empty() && listed.removed.is_empty(), "{listed:?}");
        assert!(old.is_dir(), "{old:?}: without the option to remove, nothing leaves");

        let applied = clean(root, true).expect("um projeto git");
        assert!(applied.errors.is_empty(), "{applied:?}");
        assert_eq!(applied.removed, vec![shown(&old)], "{applied:?}");
        assert!(!old.exists(), "{old:?}: the copy older than a day stayed");
        assert!(recent.is_dir(), "{recent:?}: the copy younger than a day left");
    }
}
