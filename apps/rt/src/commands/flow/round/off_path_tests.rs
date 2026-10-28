//! A rodada fora do caminho de quem entrega: o Jev, o preparo das cópias e o
//! scan não seguram a trava do passo do git, e a formação pendente não segura
//! a onda que o classificador já formou.

use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::mpsc::{Receiver, Sender, channel};
#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use mustard_core::domain::map_filter::FilterError;
use mustard_core::io::spec_events as store;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::tests::*;
#[cfg(target_os = "linux")]
use crate::shared::judgement::{Board, ItemsBoard, ItemsJudged, Judged, WaveJudge};

/// O número do arquivo da trava `name` da pasta das specs de `root`, quando
/// ele já existe.
#[cfg(target_os = "linux")]
fn lock_inode(root: &Path, name: &str) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(root.join(".claude").join("spec").join(name)).ok().map(|meta| meta.ino())
}

/// O Jev que avisa quando a montagem o chama e só responde quando o teste
/// solta: a resposta é a falha de rede, e a montagem segue sem ele.
#[cfg(target_os = "linux")]
#[derive(Debug)]
struct SlowJudge {
    asked: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
}

#[cfg(target_os = "linux")]
impl WaveJudge for SlowJudge {
    fn judge_backlog(&self, _: &Board) -> Result<Judged, FilterError> {
        let _ = self.asked.lock().unwrap().send(());
        let _ = crate::wait_for::wait_for_message("a soltura do Jev", &self.release.lock().unwrap());
        Err(FilterError::Network("slow judge".into()))
    }
    fn judge_items(&self, _: &ItemsBoard) -> Result<ItemsJudged, FilterError> {
        Err(FilterError::Network("slow judge".into()))
    }
}

/// O preparo que avisa quando começa, criando `started`, e só termina
/// quando o teste escreve no cano `gate`.
#[cfg(target_os = "linux")]
struct SlowPreparation {
    started: PathBuf,
    gate: PathBuf,
}

#[cfg(target_os = "linux")]
impl SlowPreparation {
    fn in_project(root: &Path, dir: &Path, cap: u64) -> Self {
        let started = dir.join("prepare-started");
        let gate = dir.join("prepare-gate");
        let made = std::process::Command::new("mkfifo").arg(&gate).status().unwrap();
        assert!(made.success(), "mkfifo");
        let command = format!("touch '{}' && cat '{}' > /dev/null", started.display(), gate.display());
        std::fs::write(root.join("mustard.json"), json!({ "maxCompilingWaves": cap, "prepareCommand": command }).to_string()).unwrap();
        Self { started, gate }
    }

    fn started(&self) -> bool {
        self.started.exists()
    }

    fn release(&self) {
        drop(std::fs::OpenOptions::new().write(true).open(&self.gate).unwrap());
    }
}

/// As ondas 1 e 2 em arquivos separados, com só a 1 despachada e o agente
/// dela lançado.
#[cfg(target_os = "linux")]
fn first_wave_out(root: &Path) {
    approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
    std::fs::write(root.join("mustard.json"), r#"{"maxCompilingWaves":1}"#).unwrap();
    let first = round(root, "x", None);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
}

/// Enquanto a rodada prepara a cópia da onda 2, a entrega da onda 1 grava
/// sem esperar por ela: o preparo não segura a trava do passo do git.
#[cfg(target_os = "linux")]
#[test]
fn a_delivery_never_waits_for_a_round_preparing_a_copy() {
    let dir = tempdir().unwrap();
    let root = &dir.path().join("p");
    std::fs::create_dir_all(root).unwrap();
    first_wave_out(root);
    let slow = SlowPreparation::in_project(root, dir.path(), 2);
    let git = lock_inode(root, "round-git.lock").expect("the first round made the git lock");

    let (waited, out) = std::thread::scope(|scope| {
        let second = scope.spawn(|| round(root, "x", None));
        while !slow.started() {
            assert!(!second.is_finished(), "the round never prepared a copy");
            std::thread::yield_now();
        }
        let delivering = scope.spawn(|| delivered(root, 1, "A entrega saiu.", &["src/a.rs"]));
        let mut waited = false;
        while !delivering.is_finished() {
            if crate::commands::git_settle::waiting_for_lock(git) {
                waited = true;
                break;
            }
            std::thread::yield_now();
        }
        slow.release();
        delivering.join().unwrap();
        (waited, second.join().unwrap())
    });
    assert!(!waited, "the delivery waited for the round preparing a copy: {out}");
    assert_eq!(waves_in(&out, "dispatch"), vec![2], "{out}");
}

/// Uma spec aprovada sem onda, com duas tarefas soltas em arquivos
/// separados, e só uma vaga: a primeira rodada monta e despacha uma onda.
#[cfg(target_os = "linux")]
fn one_batch_out(root: &Path) -> u64 {
    std::fs::create_dir_all(root.join("src")).unwrap();
    for name in ["a.rs", "b.rs"] {
        std::fs::write(root.join("src").join(name), "fn one() {}\n").unwrap();
    }
    approved(root, "x", &[]);
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let first = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
    let (said, crit) = (first("message"), first("criterion"));
    for (text, file) in [("Mexer no código de um.", "src/a.rs"), ("Mexer no código de dois.", "src/b.rs")] {
        let task = json!({"text": text, "files": [{"path": file}], "depends_on": [], "covers": [crit], "origin": said});
        assert!(write(root, "x", "task", task)["id"].is_u64());
    }
    std::fs::write(root.join("mustard.json"), r#"{"maxCompilingWaves":1}"#).unwrap();
    let out = round(root, "x", None);
    let sent = waves_in(&out, "dispatch");
    assert_eq!(sent.len(), 1, "{out}");
    sent[0]
}

/// Enquanto a rodada espera o Jev montar a onda seguinte, a entrega da onda
/// que já saiu grava sem esperar por ela: o Jev não segura a trava do passo
/// do git.
#[cfg(target_os = "linux")]
#[test]
fn a_delivery_never_waits_for_a_round_asking_jev() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let wave = one_batch_out(root);
    std::fs::write(root.join("mustard.json"), r#"{"maxCompilingWaves":2}"#).unwrap();
    let git = lock_inode(root, "round-git.lock").expect("the first round made the git lock");
    let (asked_tx, asked) = channel();
    let (release, release_rx) = channel();
    let judge: Arc<dyn WaveJudge> = Arc::new(SlowJudge { asked: Mutex::new(asked_tx), release: Mutex::new(release_rx) });

    let (waited, out) = std::thread::scope(|scope| {
        let second = scope.spawn(move || {
            crate::shared::jev::judge_in_test(Some(judge));
            round(root, "x", None)
        });
        crate::wait_for::wait_for_message("a pergunta da rodada ao Jev", &asked).expect("the round asked the Jev");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(wave)).cloned().expect("the task of the wave");
        let file = task.fields["files"][0]["path"].as_str().unwrap().to_string();
        let delivering = scope.spawn(move || delivered(root, wave, "A entrega saiu.", &[file.as_str()]));
        let mut waited = false;
        while !delivering.is_finished() {
            if crate::commands::git_settle::waiting_for_lock(git) {
                waited = true;
                break;
            }
            std::thread::yield_now();
        }
        release.send(()).unwrap();
        delivering.join().unwrap();
        (waited, second.join().unwrap())
    });
    assert!(!waited, "the delivery waited for the round asking the Jev: {out}");
    assert_eq!(out["ok"], json!(true), "{out}");
}

/// A volta que chega enquanto a rodada prepara a cópia de outra onda tira do
/// despacho a onda órfã que a rodada ia reenviar: a releitura com a trava do
/// passo do git presa vê a volta, e o reenvio não sai por cima dela.
#[cfg(target_os = "linux")]
#[test]
fn a_return_that_arrives_while_the_round_prepares_a_copy_is_never_resent() {
    let dir = tempdir().unwrap();
    let root = &dir.path().join("p");
    std::fs::create_dir_all(root).unwrap();
    let wave = one_batch_out(root);
    orphan_the_send(root, wave);
    let slow = SlowPreparation::in_project(root, dir.path(), 2);

    let out = std::thread::scope(|scope| {
        let second = scope.spawn(|| round(root, "x", None));
        while !slow.started() {
            assert!(!second.is_finished(), "the round never prepared a copy");
            std::thread::yield_now();
        }
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let task = log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(wave)).cloned().expect("the task of the wave");
        let file = task.fields["files"][0]["path"].as_str().unwrap().to_string();
        delivered(root, wave, "A entrega saiu.", &[file.as_str()]);
        slow.release();
        second.join().unwrap()
    });
    assert_eq!(out["ok"], json!(true), "{out}");
    assert!(!waves_in(&out, "dispatch").contains(&wave), "the returned wave is not resent: {out}");
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    assert_eq!(log.visible().iter().filter(|e| e.event_type == "send" && e.wave() == Some(wave)).count(), 1, "{out}");
}

/// O commit de uma onda não espera o scan: a onda seguinte sai na mesma
/// rodada antes de o mapa do projeto ser relido, e a passada, em segundo
/// plano, vem depois do envio dela.
#[test]
fn the_next_waves_go_out_before_the_scan() {
    crate::commands::flow::round::map_after_rounds();
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[1])]);
    let first = round(root, "x", None);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    delivered(root, 1, "A entrega saiu.", &["src/a.rs"]);

    let model = mustard_core::io::project_map::model_path(root);
    let scans: std::cell::RefCell<Vec<bool>> = std::cell::RefCell::new(Vec::new());
    let mine = |_: &Path, out: &Path| {
        if out == model {
            let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
            scans.borrow_mut().push(log.last_by_wave("send").contains_key(&2));
        }
        Ok(mustard_core::domain::scan::ScanReport::default())
    };
    let out = round_with_mine(root, "x", None, &mine);
    assert!(out["commit"]["sha"].is_string(), "{out}");
    assert_eq!(waves_in(&out, "dispatch"), vec![2], "{out}");
    let scans = scans.into_inner();
    assert!(!scans.is_empty(), "the map follows the commit: {out}");
    assert!(scans.iter().all(|after_the_send| *after_the_send), "the next wave never waits for the project map: {scans:?} {out}");
}

/// Com a leitura do classificador ainda pendente para as tarefas novas, a
/// onda que ele já formou sai: a formação pendente só segura as tarefas que
/// ela ainda vai ler.
#[test]
fn a_pending_formation_never_holds_a_wave_already_formed() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    for name in ["a.rs", "b.rs", "c.rs"] {
        std::fs::write(root.join("src").join(name), "fn one() {}\n").unwrap();
    }
    approved(root, "x", &[]);
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let first = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
    let (said, crit) = (first("message"), first("criterion"));
    let formed = crate::shared::spec_state::seed_event(
        root,
        "x",
        "task",
        json!({"title": "A sessão renova o token", "text": "Renovar o token.", "files": [{"path": "src/a.rs"}], "depends_on": [],
            "covers": [crit], "origin": said, "group": {"name": "A sessão renova o token", "position": 1, "formed": true}}),
    );
    for (title, file) in [("A busca não espera a trava", "src/b.rs"), ("O commit leva título e corpo", "src/c.rs")] {
        let task = json!({"title": title, "text": format!("{title}."), "files": [{"path": file}], "depends_on": [], "covers": [crit], "origin": said});
        assert!(write(root, "x", "task", task)["id"].is_u64());
    }
    std::fs::write(root.join("mustard.json"), r#"{"maxCompilingWaves":3}"#).unwrap();

    let out = session_round(root, "conduz");
    assert!(out.get("formation").is_some(), "the reading of the new tasks is pending: {out}");
    let sent = waves_in(&out, "dispatch");
    assert_eq!(sent.len(), 1, "only the formed wave goes out: {out}");
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let carried: Vec<u64> = log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(sent[0])).map(|e| e.id).collect();
    let current = log.current(formed).map(|task| task.id);
    assert_eq!(carried.len(), 1, "{out}");
    assert_eq!(Some(carried[0]), current, "the formed task goes in its wave: {out}");
}

/// Uma rodada da spec `x` com a sessão `session` conduzindo.
fn session_round(root: &Path, session: &str) -> Value {
    use super::usage::Caller;
    super::review::test_conductor_review(root, "x");
    let opts = super::RoundOpts { root: root.to_path_buf(), spec: Some("x".to_string()), report: None };
    let project = crate::commands::spec_events::project(root);
    let mine = |_: &Path, _: &Path| Ok(mustard_core::domain::scan::ScanReport::default());
    match super::answer::run_round_with_mine(&opts, &project.root, project.lang, Caller { session: Some(session), config_dir: None }, &mine) {
        Ok(out) => out,
        Err(refusal) => refusal.to_value(project.lang),
    }
}

/// A rodada que prepara a cópia de uma onda e a limpeza das cópias da obra
/// — a do descarte e a do `run clean` — não pisam na mesma cópia: a limpeza
/// espera a rodada terminar o preparo e só então tira as cópias.
#[cfg(target_os = "linux")]
#[test]
fn a_round_and_the_removal_of_the_copies_never_touch_the_same_copy() {
    let dir = tempdir().unwrap();
    let root = &dir.path().join("p");
    std::fs::create_dir_all(root).unwrap();
    first_wave_out(root);
    let slow = SlowPreparation::in_project(root, dir.path(), 2);

    let (stepped, removal) = std::thread::scope(|scope| {
        let second = scope.spawn(|| round(root, "x", None));
        while !slow.started() {
            assert!(!second.is_finished(), "the round never prepared a copy");
            std::thread::yield_now();
        }
        let removing = scope.spawn(|| super::remove_spec_copies(root, "x", None));
        let waiting = || {
            ["round-git.lock", "round-copies.lock"]
                .into_iter()
                .filter_map(|name| lock_inode(root, name))
                .any(crate::commands::git_settle::waiting_for_lock)
        };
        while !removing.is_finished() && !waiting() {
            std::thread::yield_now();
        }
        let stepped = removing.is_finished();
        slow.release();
        second.join().unwrap();
        (stepped, removing.join().unwrap())
    });
    assert!(!stepped, "the removal ran while the round was preparing a copy");
    assert!(removal.left.is_empty() && removal.unkept.is_empty(), "{removal:?}");
    assert!(super::spec_copies(root, "x").is_empty(), "the copies left after the round");
}

