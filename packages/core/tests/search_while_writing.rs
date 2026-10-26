#![allow(clippy::unwrap_used)]
//! A busca no mapa responde enquanto outra conexão segura uma gravação: quem
//! lê não espera quem grava e não volta com o banco travado. O scan grava a
//! história do projeto em lotes enquanto o Claude busca, e a busca dele não
//! pode falhar nesse meio tempo.

use std::sync::mpsc;
use std::time::Duration;

use mustard_core::domain::normalize::Languages;
use mustard_core::io::map_search::{any_path, candidates_at};
use mustard_core::io::map_triage::triage_at;
use mustard_core::io::project_map::{model_path, save_at};
use rusqlite::Connection;
use serde_json::json;

/// Um mapa com duas funções, gravado num projeto temporário.
fn saved_map() -> (tempfile::TempDir, std::path::PathBuf, Languages) {
    let dir = tempfile::tempdir().unwrap();
    let model = model_path(dir.path());
    let languages = Languages::new(["pt-BR", "en-US"]);
    let map = json!({"modules": [
        {"path": "src/pedido.rs", "loc": 5, "declarations": [
            {"kind": "function", "name": "cobrar_pedido", "line": 1, "end_line": 3, "doc": "Cobra o pedido do caixa."}]},
        {"path": "src/nota.rs", "loc": 5, "declarations": [
            {"kind": "function", "name": "emitir_nota", "line": 1, "end_line": 3, "doc": "Emite a nota fiscal."}]},
    ]});
    assert!(save_at(&model, &map, "scan 1", &languages).unwrap());
    (dir, model, languages)
}

/// A busca dos candidatos e a da triagem, as duas que o Claude faz, sobre o
/// mapa em `model`: quantos arquivos a triagem achou.
fn search(model: &std::path::Path, languages: &Languages) -> usize {
    candidates_at(model, "cobrar pedido", "", languages, any_path).expect("the candidates answer while the map is being written");
    triage_at(model, ("cobrar pedido", ""), languages, 10).expect("the search answers while the map is being written").files.len()
}

/// A gravação de uma conexão que pega a trava de escrita inteira (a mais
/// forte que o SQLite dá), avisa por `held` e só a solta quando `release`
/// manda ou some: nunca pelo relógio.
fn hold_the_write(model: std::path::PathBuf, release: mpsc::Receiver<()>, held: mpsc::Sender<()>) {
    let mut writer = Connection::open(model).unwrap();
    writer.busy_timeout(Duration::from_secs(5)).unwrap();
    let tx = writer.transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive).unwrap();
    tx.execute("CREATE TABLE IF NOT EXISTS held(at INTEGER)", []).unwrap();
    tx.execute("INSERT INTO held VALUES (1)", []).unwrap();
    // Quem não espera o aviso deixa o outro lado do canal fechar.
    let _ = held.send(());
    let _ = release.recv();
    tx.commit().unwrap();
}

/// A conexão que segura a gravação, na linha de execução dela, já com a
/// trava: o canal que a solta e a linha de execução.
fn holding_the_write(model: &std::path::Path) -> (mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let (release, released) = mpsc::channel();
    let (held, holding) = mpsc::channel();
    let model = model.to_path_buf();
    let writer = std::thread::spawn(move || hold_the_write(model, released, held));
    holding.recv().unwrap();
    (release, writer)
}

/// A busca responde com a gravação de outra conexão presa: a gravação só
/// solta depois que a busca voltou, então a busca que esperasse por ela
/// nunca voltaria com a resposta.
#[test]
fn a_search_answers_while_another_connection_holds_the_write_and_does_not_wait_for_it() {
    let (_dir, model, languages) = saved_map();
    assert_eq!(search(&model, &languages), 1, "the search finds the file before any write");

    let (release, writer) = holding_the_write(&model);
    let files = search(&model, &languages);
    release.send(()).unwrap();
    writer.join().unwrap();

    assert_eq!(files, 1, "the search sees what was stored, as before the write");
}

/// Gravações uma atrás da outra, e a busca perguntando sem parar no meio
/// delas: enquanto uma pega a trava, enquanto a segura e enquanto solta. Cada
/// gravação só solta depois de a busca responder `ANSWERS_PER_WRITE` vezes por
/// cima dela. A prova conta respostas, não tempo: a máquina lenta só alonga o
/// teste.
#[test]
fn a_search_never_fails_across_writes_that_hold_the_lock_back_to_back() {
    const WRITES: usize = 3;
    const ANSWERS_PER_WRITE: usize = 2;
    let (_dir, model, languages) = saved_map();
    let ask = || assert_eq!(search(&model, &languages), 1);
    for _ in 0..WRITES {
        let (release, released) = mpsc::channel();
        let (held, holding) = mpsc::channel();
        let writer = {
            let model = model.clone();
            std::thread::spawn(move || hold_the_write(model, released, held))
        };
        while matches!(holding.try_recv(), Err(mpsc::TryRecvError::Empty)) {
            ask();
        }
        for _ in 0..ANSWERS_PER_WRITE {
            ask();
        }
        release.send(()).unwrap();
        while !writer.is_finished() {
            ask();
        }
        writer.join().unwrap();
    }
}

/// A busca que precisa refazer o índice, porque ele foi feito em outras
/// línguas, também grava: com a trava presa por outra gravação quando a
/// busca começa, essa gravação espera a vez em vez de falhar, e a busca
/// responde depois que a outra solta.
#[test]
fn a_search_that_has_to_redo_the_index_waits_for_the_write_that_holds_the_lock() {
    let (_dir, model, _) = saved_map();
    let other_languages = Languages::new(["pt-BR"]);
    let (release, writer) = holding_the_write(&model);
    let searching = std::thread::spawn({
        let model = model.clone();
        move || candidates_at(&model, "cobrar pedido", "", &other_languages, any_path).map(|_| ())
    });
    release.send(()).unwrap();
    writer.join().unwrap();
    searching.join().unwrap().expect("the search waits for the write instead of failing");
}
