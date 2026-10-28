//! Os rascunhos que as recusas do `run write` guardaram e ninguém usou saem
//! da pasta da spec quando ela fecha.

use serde_json::{Value, json};
use tempfile::tempdir;

use super::tests::{close, ready_with_waves};
use crate::commands::spec_events::refusal_draft::Resolved;

/// Uma recusa de gravação na spec `x`, que guarda o rascunho, e o número dele.
fn refused_task(root: &std::path::Path) -> u64 {
    let draft = Resolved::of(root, Some("x"), None, "task", &json!({ "text": "Conferir." }).to_string()).expect("a gravação sem rascunho citado");
    let report = draft.settle(json!({ "ok": false, "reason": "task-declaration-missing", "hint": "Falta o arquivo." }));
    report["draft"].as_u64().expect("a recusa devolve o número do rascunho")
}

/// O fechamento tira os rascunhos que sobraram: a pasta da spec fechada guarda
/// só o arquivo de eventos, e o número que a recusa deu deixa de valer.
#[test]
fn the_close_removes_leftover_drafts() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ready_with_waves(root, "x", &["git --version"], 1);
    let draft = refused_task(root);
    let folder = root.join(".claude").join("spec").join("x");
    assert!(folder.join("drafts.json").is_file(), "a recusa guardou o rascunho na pasta da spec");

    let closed = close(root, "x");
    assert_eq!(closed["phase"], json!("closed"), "{closed}");

    let mut names: Vec<String> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, ["spec.ndjson"], "o fechamento tirou o que sobrou: {names:?}");
    let reused = Resolved::of(root, Some("x"), None, "task", &json!({ "draft": draft }).to_string());
    assert_eq!(reused.err().map(|report| report["reason"].clone()), Some(Value::from("draft-unknown")));
}
