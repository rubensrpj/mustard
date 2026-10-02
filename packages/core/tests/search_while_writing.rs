#![allow(clippy::unwrap_used)]
//! A busca no mapa responde enquanto outra conexão segura uma gravação: quem
//! lê não espera quem grava e não volta com o banco travado. O scan grava a
//! história do projeto em lotes enquanto o Claude busca, e a busca dele não
//! pode falhar nesse meio tempo.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

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
/// forte que o SQLite dá) e só a solta quando a busca acabou ou passados
/// `hold`. Diz se soltou porque a busca acabou.
fn hold_the_write(model: std::path::PathBuf, hold: Duration, searched: Arc<AtomicBool>, held: mpsc::Sender<()>) -> bool {
    let mut writer = Connection::open(model).unwrap();
    writer.busy_timeout(Duration::from_secs(5)).unwrap();
    let tx = writer.transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive).unwrap();
    tx.execute("CREATE TABLE IF NOT EXISTS held(at INTEGER)", []).unwrap();
    tx.execute("INSERT INTO held VALUES (1)", []).unwrap();
    // Quem não espera o aviso deixa o outro lado do canal fechar.
    let _ = held.send(());
    let until = Instant::now() + hold;
    while !searched.load(Ordering::SeqCst) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    let searched_in_time = searched.load(Ordering::SeqCst);
    tx.commit().unwrap();
    searched_in_time
}

#[test]
fn a_search_answers_while_another_connection_holds_the_write_and_does_not_wait_for_it() {
    let (_dir, model, languages) = saved_map();
    assert_eq!(search(&model, &languages), 1, "the search finds the file before any write");

    let searched = Arc::new(AtomicBool::new(false));
    let (held, holding) = mpsc::channel();
    let writer = {
        let (model, searched) = (model.clone(), Arc::clone(&searched));
        std::thread::spawn(move || hold_the_write(model, Duration::from_secs(4), searched, held))
    };
    holding.recv().unwrap();
    let started = Instant::now();
    let files = search(&model, &languages);
    let took = started.elapsed();
    searched.store(true, Ordering::SeqCst);

    assert_eq!(files, 1, "the search sees what was stored, as before the write");
    assert!(writer.join().unwrap(), "the search ended while the write was still held: it took {took:?}");
    assert!(took < Duration::from_secs(2), "the search did not wait for the write: {took:?}");
}

#[test]
fn a_search_held_for_two_hundred_milliseconds_by_a_write_never_fails() {
    let (_dir, model, languages) = saved_map();
    let searched = Arc::new(AtomicBool::new(false));
    // Uma gravação atrás da outra, cada uma segurando a trava por 200 ms, e a
    // busca perguntando sem parar no meio delas.
    let writer = {
        let (model, searched) = (model.clone(), Arc::clone(&searched));
        std::thread::spawn(move || {
            let mut writes = 0;
            while !searched.load(Ordering::SeqCst) {
                let (held, _) = mpsc::channel();
                let never = Arc::new(AtomicBool::new(false));
                hold_the_write(model.clone(), Duration::from_millis(200), never, held);
                writes += 1;
            }
            writes
        })
    };
    let until = Instant::now() + Duration::from_secs(1);
    let mut asked = 0;
    while Instant::now() < until {
        assert_eq!(search(&model, &languages), 1);
        asked += 1;
    }
    searched.store(true, Ordering::SeqCst);
    let writes = writer.join().unwrap();
    assert!(asked >= 3 && writes >= 2, "the search asked {asked} times across {writes} writes");
}

/// A busca que precisa refazer o índice, porque ele foi feito em outras
/// línguas, também grava: essa gravação espera a vez pela outra que segura a
/// trava, em vez de falhar, e a busca responde depois.
#[test]
fn a_search_that_has_to_redo_the_index_waits_for_the_write_that_holds_the_lock() {
    let (_dir, model, _) = saved_map();
    let other_languages = Languages::new(["pt-BR"]);
    let (held, holding) = mpsc::channel();
    let never = Arc::new(AtomicBool::new(false));
    let writer = {
        let model = model.clone();
        std::thread::spawn(move || hold_the_write(model, Duration::from_millis(200), never, held))
    };
    holding.recv().unwrap();
    let started = Instant::now();
    candidates_at(&model, "cobrar pedido", "", &other_languages, any_path).expect("the search waits for the write instead of failing");
    let took = started.elapsed();
    assert!(!writer.join().unwrap(), "nobody told the write to let go: it held for its own 200 ms");
    assert!(took >= Duration::from_millis(100), "the redo of the index waited for the write that held the lock: {took:?}");
}
