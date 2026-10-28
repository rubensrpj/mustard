//! A resposta da rodada na sessão de quem conduz: as instruções fixas vão uma
//! vez por sessão e voltam depois da compactação.

use std::path::Path;

use mustard_core::domain::model::contract::HookInput;
use serde_json::{Value, json};

use super::tests::{approved, launched};
use super::{RoundOpts, checks, review, round_for};

const SESSION: &str = "round-session";

fn round(root: &Path) -> Value {
    review::test_conductor_review(root, "x");
    let opts = RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None };
    launched(root, "x", round_for(&opts, Some(SESSION)))
}

fn says(out: &Value, text: &str) -> bool {
    out["next"].as_str().unwrap_or_default().contains(text)
}

/// O agente `name` do projeto volta a um texto de outra versão: a próxima
/// rodada o regrava e avisa.
fn stale_agent(root: &Path, name: &str) {
    let folder = root.join(".claude/agents/mustard");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("{name}.md")), "---\nname: antigo\n---\nO texto antigo.\n").unwrap();
}

/// A frase de cada aviso de agentes regravados que a resposta traz.
fn refreshed(out: &Value) -> Vec<String> {
    let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
    warnings.iter().filter(|w| w["reason"] == json!("agents-refreshed")).filter_map(|w| w["hint"].as_str().map(str::to_string)).collect()
}

/// A segunda rodada da sessão segue dizendo o que está em andamento, sem a
/// ordem de despachar nem o que fazer na volta, que a primeira já deu.
#[test]
fn a_second_round_in_the_session_does_not_repeat_the_fixed_instructions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    let lang = crate::commands::spec_events::project(root).lang;
    let back = checks::report_back(root, lang);
    let first = round(root);
    assert_eq!(first["ok"], json!(true), "{first}");
    assert!(says(&first, &back), "{first}");
    let second = round(root);
    assert_eq!(second["ok"], json!(true), "{second}");
    assert!(!says(&second, &back), "{second}");
    assert_eq!(second["running"].as_array().map(Vec::len), Some(1), "what is running still goes: {second}");
}

/// O aviso que a sessão já recebeu não volta na rodada seguinte, e o aviso
/// novo continua vindo.
#[test]
fn a_warning_the_session_already_got_is_not_repeated_but_a_new_one_still_comes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    // Os agentes todos em dia: só o que o teste desgasta é regravado.
    std::fs::create_dir_all(root.join(".claude/agents/mustard")).unwrap();
    mustard_core::refresh_agent_texts(root).unwrap();

    stale_agent(root, "review");
    let first = round(root);
    assert_eq!(refreshed(&first).len(), 1, "the first answer carries the warning: {first}");
    stale_agent(root, "review");
    let repeated = round(root);
    assert_eq!(repeated["ok"], json!(true), "{repeated}");
    assert!(refreshed(&repeated).is_empty(), "the same warning does not come twice: {repeated}");
    stale_agent(root, "wave");
    let new = refreshed(&round(root));
    assert_eq!(new.len(), 1, "a different warning still comes");
    assert!(new[0].contains("wave.md") && !new[0].contains("review.md"), "{new:?}");
}

/// Depois da compactação confirmada pelo host, a rodada dá de novo as
/// instruções fixas, que a conversa resumida perdeu.
#[test]
fn the_fixed_instructions_come_back_after_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    let lang = crate::commands::spec_events::project(root).lang;
    let back = checks::report_back(root, lang);
    round(root);
    assert!(!says(&round(root), &back));
    let compacted = HookInput { session_id: Some(SESSION.into()), ..HookInput::default() };
    crate::hooks::session::conversation_size::reset_conductor_notice(&compacted, root);
    let after = round(root);
    assert!(says(&after, &back), "{after}");
}
