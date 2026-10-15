//! Gravações e leituras do mapa que se cruzam: o carimbo do banco, lido antes
//! das linhas e conferido com a trava, impede que uma gravação planejada num
//! banco grave por cima de outra que passou depois.

use std::cell::RefCell;

use serde_json::json;
use tempfile::tempdir;

use super::tests::{as_saved_by_the_pass, carded_map, languages, scan_map};
use super::*;

thread_local! {
    /// O que roda no meio da leitura do mapa, logo depois do texto dele.
    static AFTER_THE_MAP_TEXT: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

/// Roda, uma vez, o que o teste pôs no meio da leitura do mapa.
pub(super) fn after_the_map_text() {
    if let Some(hook) = AFTER_THE_MAP_TEXT.with(|hook| hook.borrow_mut().take()) {
        hook();
    }
}

/// O plano da gravação inteira de `map`, feito do banco aberto `db`.
fn whole_plan<'b>(db: &MapDb, map: &Value) -> Planned<'b> {
    let fresh = rows_of(map).unwrap();
    plan(db.conn(), Fresh::Rows(BLOCKS.iter().zip(fresh).collect()), "scan 1").unwrap().unwrap()
}

/// Duas gravações inteiras de mapas diferentes, planejadas do mesmo banco e
/// gravadas uma depois da outra, deixam o mapa igual ao da segunda gravada
/// sozinha: a segunda acha o banco mudado já com a trava e refaz o plano.
#[test]
fn of_two_whole_saves_planned_from_the_same_state_the_later_is_what_stays() {
    let dir = tempdir().unwrap();
    let model = model_path(dir.path());
    assert!(save_at(&model, &scan_map(), "scan 1", &languages()).unwrap());
    let mut first_map = scan_map();
    first_map["modules"][1]["loc"] = json!(21);
    let mut second_map = scan_map();
    second_map["modules"][0]["declarations"][0]["name"] = json!("gamma");
    second_map["modules"].as_array_mut().unwrap().push(json!({
        "path": "web/tela.ts", "language": "typescript", "loc": 5, "imports": [], "namespaces": [],
        "declarations": [{"kind": "function", "name": "carregar", "line": 3, "end_line": 4, "supertypes": []}]
    }));

    let (mut first, mut second) = (open(&model).unwrap(), open(&model).unwrap());
    let first_plan = whole_plan(&first, &first_map);
    let second_plan = whole_plan(&second, &second_map);
    assert_eq!(commit(&mut first, first_plan, "scan 1", Some(&languages())).unwrap(), Some(true));
    assert_eq!(commit(&mut second, second_plan, "scan 1", Some(&languages())).unwrap(), Some(true));

    let alone = tempdir().unwrap();
    let alone_model = model_path(alone.path());
    assert!(save_at(&alone_model, &second_map, "scan 1", &languages()).unwrap());
    assert_eq!(read_stored_at(&model).unwrap().json, read_stored_at(&alone_model).unwrap().json);
}

/// Outra gravação que cai no meio da leitura do mapa fica fora do carimbo
/// lido: a gravação parcial que parte dessa leitura não grava, e quem a
/// chamou faz a gravação inteira.
#[test]
fn a_write_inside_the_read_sends_the_save_to_the_whole_write() {
    let dir = tempdir().unwrap();
    let model = model_path(dir.path());
    assert!(save_at(&model, &carded_map(false), "scan 1", &languages()).unwrap());
    let other = model.clone();
    AFTER_THE_MAP_TEXT.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            assert!(save_at(&other, &carded_map(true), "scan 1", &languages()).unwrap());
        }));
    });
    let previous = read_previous_at(&model).unwrap();
    let before: Value = serde_json::from_str(&previous.json).unwrap();
    let map = as_saved_by_the_pass(carded_map(false));
    assert_eq!(save_since_at(&model, &map, &before, &previous.stamp, "scan 1", &languages()).unwrap(), None);
}
