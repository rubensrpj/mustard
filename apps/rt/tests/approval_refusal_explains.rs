// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A testemunha da aprovação diz por que nada foi gravado.
//!
//! A testemunha grava o estado aprovado só quando o usuário escolhe uma das
//! opções oferecidas que começa por "Aprovar" ("Approve"). Uma opção como
//! "Sim, pode ir" não aprova, e uma resposta digitada, que não é nenhuma das
//! opções, também não. Nos dois casos, quem fez a pergunta precisa saber o
//! motivo na hora, e o texto de um gancho no stderr não chega ao assistente:
//! a testemunha responde no contexto do `PostToolUse`.
//!
//! Roda `mustard-rt on PostToolUse` como processo, porque os ganchos são
//! privados da biblioteca, e confere o `state` da spec no `spec.ndjson`.
//!
//! Mora em `tests/` porque o critério roda `cargo test -p mustard-rt
//! approval_refusal_names_the_unmet_condition -- --exact`, e o `--exact` só
//! casa o nome da função na raiz de um binário de teste de integração.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use mustard_core::io::spec_events as store;
use serde_json::{json, Value};

const QUESTION: &str = "Aprovar esta spec?";

/// Responde a pergunta com as opções `offered` e a resposta `answers`, na
/// pasta `cwd`, e devolve o que a testemunha disse ao assistente, ou vazio.
fn answer(cwd: &Path, session: &str, offered: &[&str], answers: Value) -> String {
    let bin = env!("CARGO_BIN_EXE_mustard-rt");
    let options: Vec<Value> = offered.iter().map(|l| json!({ "label": l })).collect();
    let input = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "AskUserQuestion",
        "tool_input": { "questions": [{ "question": QUESTION, "options": options }] },
        "tool_response": { "questions": [], "answers": answers },
        "session_id": session,
        "cwd": cwd.to_str().unwrap()
    });
    let mut child = Command::new(bin)
        .args(["on", "PostToolUse"])
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", cwd)
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mustard-rt");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = write!(stdin, "{input}");
    }
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0), "mustard-rt PostToolUse must exit 0 (fail-open)");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<Value>(stdout.trim())
        .ok()
        .and_then(|v| v["hookSpecificOutput"]["additionalContext"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Uma spec `epic` na fase de plano, ligada à sessão, num projeto em pt-BR.
fn spec_in_plan(project: &Path, session: &str) {
    fs::write(project.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
    let path = store::spec_file(project, "epic").unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let plan = json!({ "phase": "plan" });
    store::write(&path, "state", plan.as_object().cloned().unwrap(), &[]).unwrap();
    let session_dir = project.join(".claude").join(".session").join(session);
    fs::create_dir_all(&session_dir).unwrap();
    fs::write(session_dir.join("active-spec"), "epic").unwrap();
}

/// A fase atual da spec `epic`, lida do último `state` do arquivo.
fn phase(project: &Path) -> String {
    let body = fs::read_to_string(store::spec_file(project, "epic").unwrap()).unwrap();
    body.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["type"] == "state")
        .and_then(|v| v["phase"].as_str().map(str::to_string))
        .unwrap_or_default()
}

#[test]
fn approval_refusal_names_the_unmet_condition() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-decline");

    // Uma escolha de verdade, com palavras que não começam por "Aprovar".
    let said = answer(
        project,
        "s-decline",
        &["Sim, pode ir", "Não, revisar"],
        json!({ QUESTION: "Sim, pode ir" }),
    );

    assert!(said.contains("epic"), "names the spec awaiting approval:\n{said}");
    assert!(said.contains("Sim, pode ir"), "quotes the option that failed:\n{said}");
    assert!(said.contains("\"Aprovar\""), "names the word that would approve:\n{said}");
    assert_eq!(phase(project), "plan", "explaining the decline records nothing");
}

#[test]
fn a_recognised_approval_records_the_state_and_suggests_clear() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-ok");

    let said = answer(project, "s-ok", &["Aprovar", "Ajustar"], json!({ QUESTION: "Aprovar" }));

    assert_eq!(phase(project), "approved", "a recognised approval records the state");
    assert!(said.contains("/clear"), "the witness suggests clearing the window:\n{said}");
    assert!(!said.contains("nada foi gravado"), "nothing was declined:\n{said}");
}

/// A resposta digitada em vez de escolhida, com uma palavra de aprovação no
/// meio, não aprova, e quem perguntou fica sabendo que é preciso escolher a
/// opção.
#[test]
fn free_text_is_declined_and_told_to_pick_instead() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-typed");

    let said = answer(
        project,
        "s-typed",
        &["Aprovar", "Ajustar"],
        json!({ QUESTION: "o relato diz que ninguém conseguia aprovar a spec" }),
    );

    assert_eq!(phase(project), "plan", "free text never approves:\n{said}");
    assert!(
        said.contains("Texto livre nunca aprova") && said.contains("escolhendo a opção"),
        "the operator is told to pick the option instead of typing:\n{said}"
    );
}

#[test]
fn a_dismissed_dialog_explains_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    spec_in_plan(project, "s-cancel");

    // Uma pergunta cancelada não respondeu nada, e não há o que explicar.
    let said = answer(project, "s-cancel", &["Aprovar"], json!({}));

    assert!(said.is_empty(), "a dismissed dialog is not a failed condition:\n{said}");
    assert_eq!(phase(project), "plan");
}
