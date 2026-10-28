//! A resposta do fechamento na sessão de quem conduz: uma linha por critério,
//! e as instruções fixas uma vez por sessão.

use std::path::Path;

use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::translate;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::CloseOpts;
use super::tests::{close_for, ready_with_waves, verdict_written};

const SESSION: &str = "close-session";

fn close(root: &Path, report: Option<String>) -> Value {
    close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, Some(SESSION))
}

/// O primeiro fechamento traz uma linha por critério da obra, com o código e
/// o resultado, sem a saída da prova nem os números da execução.
#[test]
fn the_first_close_answer_carries_one_line_per_criterion() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ready_with_waves(root, "x", &["git --version", "git --help"], 1);
    let asked = close(root, None);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let codes = log.codes();
    let mut expected: Vec<String> = log.visible().into_iter().filter(|e| e.event_type == "criterion").filter_map(|e| codes.get(&e.id).cloned()).collect();
    expected.sort();
    let lines = asked["criteria"].as_array().cloned().unwrap_or_default();
    let mut shown: Vec<String> = lines.iter().filter_map(|line| line["criterion"].as_str().map(str::to_string)).collect();
    shown.sort();
    assert_eq!(shown, expected, "uma linha por critério: {asked}");
    for line in &lines {
        assert_eq!(line["result"], json!("pass"), "{line}");
        assert!(line.get("output").is_none() && line.get("exit").is_none() && line.get("ms").is_none(), "{line}");
    }
}

/// O primeiro fechamento traz uma linha por comando do servidor que rodou, na
/// ordem em que rodaram e com o resultado; o que o projeto não declara não
/// ganha linha.
#[test]
fn the_first_close_answer_carries_one_line_per_command_that_ran() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ready_with_waves(root, "x", &["git --version"], 1);
    std::fs::write(root.join("mustard.json"), json!({"buildCommand": "git --version", "testCommand": "git --version"}).to_string()).unwrap();
    let asked = close(root, None);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let stages = asked["stages"].as_array().cloned().unwrap_or_default();
    let shown: Vec<(&str, &str)> = stages.iter().map(|line| (line["stage"].as_str().unwrap_or_default(), line["result"].as_str().unwrap_or_default())).collect();
    assert_eq!(shown, [("buildCommand", "pass"), ("testCommand", "pass")], "{asked}");
}

/// O segundo fechamento da sessão que pede a revisão final de novo não repete
/// as instruções de despachar o revisor: só o pedido novo e o que mudou.
#[test]
fn a_second_close_in_the_session_does_not_repeat_the_fixed_instructions() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ready_with_waves(root, "x", &["git --version"], 1);
    let lang = crate::commands::spec_events::project(root).lang;
    let fixed = translate("close.final_review", lang).replace("{spec}", "x");
    let first = close(root, None);
    assert_eq!(first["next"], json!(fixed), "{first}");
    let rejected = json!({"final": true, "result": "rejected", "text": "Refazer a conferência."});
    let second = close(root, verdict_written(root, "x", rejected));
    assert_eq!(second["review"]["final"], json!(true), "{second}");
    assert!(!second["next"].as_str().unwrap_or_default().contains(&fixed), "{second}");
    assert!(second["review"]["read"].as_str().is_some_and(|read| !read.is_empty()), "o pedido novo vai assim mesmo: {second}");
}
