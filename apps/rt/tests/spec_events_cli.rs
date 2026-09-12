//! O arquivo de eventos da spec pelo binário de verdade.
//!
//! Duas gravações ao mesmo tempo, em dois processos, recebem números seguidos
//! e nenhuma linha sai estragada: a trava é a do sistema, a mesma no Linux, no
//! macOS e no Windows. Uma spec gravada pelo `write` é lida bloco a bloco pelo
//! `read`, e `read wave-2` devolve só a onda 2.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn rt(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
    cmd.arg("run").args(args).arg("--root").arg(root).current_dir(root);
    cmd
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)))
}

fn write(root: &Path, event_type: &str, fields: &Value) -> u64 {
    let out = rt(root, &["write", event_type, "--spec", "teste", "--json", &fields.to_string()])
        .output()
        .expect("run write");
    assert!(out.status.success(), "write {event_type}: {}", String::from_utf8_lossy(&out.stdout));
    stdout_json(&out)["id"].as_u64().expect("the write reports its number")
}

fn read(root: &Path, block: &str) -> Value {
    let out = rt(root, &["read", block, "--spec", "teste"]).output().expect("run read");
    assert!(out.status.success(), "read {block}: {}", String::from_utf8_lossy(&out.stdout));
    stdout_json(&out)
}

#[test]
fn two_processes_writing_at_once_get_consecutive_numbers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let rounds = 10;
    for round in 0..rounds {
        let writers: Vec<_> = (0..2)
            .map(|w| {
                let fields = json!({"author": "user", "text": format!("rodada {round}, gravação {w}")});
                rt(root, &["write", "message", "--spec", "teste", "--json", &fields.to_string()])
                    .stdout(Stdio::piped())
                    .spawn()
                    .expect("spawn write")
            })
            .collect();
        for writer in writers {
            let out = writer.wait_with_output().expect("wait write");
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        }
    }

    let raw = std::fs::read_to_string(root.join(".claude").join("spec").join("teste").join("spec.ndjson"))
        .expect("the spec file exists");
    let ids: Vec<u64> = raw
        .lines()
        .map(|line| {
            let event: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("torn line ({e}): {line}"));
            event["id"].as_u64().expect("id")
        })
        .collect();
    assert_eq!(ids, (1..=2 * rounds).collect::<Vec<u64>>(), "consecutive, in file order, none repeated");
}

/// A página e o `.md` são refeitos dentro da trava do arquivo de eventos:
/// depois de duas gravações ao mesmo tempo, os dois têm os dois itens.
#[test]
fn two_processes_writing_at_once_leave_both_items_on_the_page_and_the_md() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let spec = root.join(".claude").join("spec").join("teste");
    for round in 0..10 {
        let texts: Vec<String> = (0..2).map(|w| format!("rodada {round} escrita {w}")).collect();
        let writers: Vec<_> = texts
            .iter()
            .map(|text| {
                let fields = json!({"author": "user", "text": text});
                rt(root, &["write", "message", "--spec", "teste", "--json", &fields.to_string()])
                    .stdout(Stdio::piped())
                    .spawn()
                    .expect("spawn write")
            })
            .collect();
        for writer in writers {
            let out = writer.wait_with_output().expect("wait write");
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
            assert!(stdout_json(&out).get("warnings").is_none(), "{}", String::from_utf8_lossy(&out.stdout));
        }
        for page in ["spec.md", "spec.html"] {
            let shown = std::fs::read_to_string(spec.join(page)).expect("the page exists");
            for text in &texts {
                assert!(shown.contains(text.as_str()), "round {round}: {page} lacks {text}");
            }
        }
    }
}

#[test]
fn a_spec_written_by_the_cli_is_read_block_by_block_and_wave_2_is_only_wave_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "state", &json!({"author": "binary", "phase": "survey", "branch": "feature/teste", "base": "dev"}));
    let msg = write(root, "message", &json!({"author": "user", "text": "Revise tudo"}));
    write(root, "context", &json!({"text": "O contexto.", "origin": msg}));
    let c1 = write(root, "criterion", &json!({"when": "a", "then": "b", "proof": "p", "origin": msg}));
    let c2 = write(root, "criterion", &json!({"when": "c", "then": "d", "proof": "q", "origin": msg}));
    write(root, "wave", &json!({"n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": msg}));
    write(root, "task", &json!({"wave": 1, "text": "T1.", "files": [{"path": "a.rs"}], "origin": msg}));
    write(root, "wave", &json!({"n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "depends_on": [1], "origin": msg}));
    write(root, "task", &json!({"wave": 2, "text": "T2.", "files": [{"path": "b.rs"}], "origin": msg}));
    write(root, "delivered", &json!({"author": "wave", "wave": 2, "text": "Feito.", "files": ["b.rs"]}));
    write(root, "verdict", &json!({"author": "review", "wave": 2, "result": "approved", "text": "Sem achados.", "criteria": [{"criterion": c2, "tests_rule": true}]}));

    let wave2 = read(root, "wave-2");
    let events = wave2["events"].as_array().expect("events");
    assert_eq!(wave2["count"], json!(3), "{wave2}");
    for event in events {
        let n = event.get("n").or_else(|| event.get("wave")).and_then(Value::as_u64);
        assert_eq!(n, Some(2), "{event}");
        assert!(event.get("search").is_none(), "the search field is never shown: {event}");
    }
    assert_eq!(read(root, "state")["count"], json!(1));
    assert_eq!(read(root, "criteria")["count"], json!(2));
    assert_eq!(read(root, "review")["count"], json!(1));
    assert_eq!(read(root, "conversation")["count"], json!(1));

    // Refusals leave with exit 1 and say what is wrong.
    let unknown = rt(root, &["write", "licao", "--spec", "teste", "--json", "{}"]).output().expect("run");
    assert_eq!(unknown.status.code(), Some(1));
    assert_eq!(stdout_json(&unknown)["reason"], json!("unknown-type"));
    let fields = json!({"text": "t", "keys": ["k"], "origin": msg}).to_string();
    let missing = rt(root, &["write", "rule", "--spec", "teste", "--json", &fields]).output().expect("run");
    assert_eq!(missing.status.code(), Some(1));
    let refusal = stdout_json(&missing);
    assert_eq!(refusal["reason"], json!("missing-field"));
    assert!(refusal["hint"].as_str().unwrap_or_default().contains("example"), "{refusal}");
    let block = rt(root, &["read", "everything", "--spec", "teste"]).output().expect("run");
    assert_eq!(block.status.code(), Some(1));
    assert_eq!(stdout_json(&block)["reason"], json!("unknown-block"));
}

fn index_file(root: &Path) -> std::path::PathBuf {
    root.join(".claude").join("spec").join("index.ndjson")
}

/// Cada gravação deixa a linha da spec no índice; apagado o índice, o
/// `index` pelo binário devolve o arquivo com os mesmos bytes.
#[test]
fn the_index_command_rebuilds_the_same_bytes_after_the_file_is_deleted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "state", &json!({"author": "binary", "phase": "survey", "branch": "feature/teste", "base": "dev"}));
    let msg = write(root, "message", &json!({"author": "user", "text": "Revise tudo"}));
    write(root, "context", &json!({"text": "Deixar o índice certo. Depois o resto.", "origin": msg}));
    write(root, "rule", &json!({"text": "**Uma linha por spec.** Com o objetivo.", "keys": ["índice"], "example": "e", "origin": msg}));
    let written = std::fs::read(index_file(root)).expect("the write left the index");
    let text = String::from_utf8_lossy(&written);
    assert!(text.contains("\"name\":\"teste\"") && text.contains("\"goal\":\"Deixar o índice certo.\""), "{text}");

    std::fs::remove_file(index_file(root)).expect("delete the index");
    let out = rt(root, &["index"]).output().expect("run index");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let report = stdout_json(&out);
    assert_eq!(report["index"], json!(".claude/spec/index.ndjson"), "{report}");
    assert_eq!(report["specs"], json!(1), "{report}");
    assert_eq!(std::fs::read(index_file(root)).expect("the index is back"), written);
}

/// Com o índice no lugar de uma pasta, o `index` recusa com exit 1, e o
/// `write` grava o evento mesmo assim, com o aviso.
#[test]
fn an_index_that_cannot_be_written_is_refused_with_exit_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(index_file(root)).expect("an index that is a folder");
    let fields = json!({"author": "user", "text": "fica gravado"}).to_string();
    let written = rt(root, &["write", "message", "--spec", "teste", "--json", &fields]).output().expect("run write");
    assert!(written.status.success(), "{}", String::from_utf8_lossy(&written.stdout));
    let warnings = stdout_json(&written)["warnings"].to_string();
    assert!(warnings.contains("mustard-rt run index"), "{warnings}");

    let out = rt(root, &["index"]).output().expect("run index");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out)["reason"], json!("io-failed"));
}
