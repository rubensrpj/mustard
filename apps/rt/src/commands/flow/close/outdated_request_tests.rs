//! O fechamento diante do pedido de revisão final que ficou para trás de um
//! item gravado depois dele: o fechamento seguinte grava o pedido novo, que
//! lista o item, e a aprovação do pedido antigo não fecha a obra.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::spec_events as store;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::CloseOpts;
use super::tests::{approve, close_for, ready_with_waves, review_prompt, verdict_written};
use crate::commands::spec_events::write::{WriteOpts, seed_at};

fn log_of(root: &Path) -> SpecLog {
    store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
}

fn close(root: &Path, report: Option<String>) -> Value {
    close_for(&CloseOpts { root: root.to_path_buf(), spec: Some("x".into()), report, ..Default::default() }, None)
}

/// Quantos pedidos de revisão final a spec gravou.
fn review_sends(root: &Path) -> usize {
    mustard_core::domain::spec_state::review_sends(&log_of(root)).len()
}

/// Grava um critério novo na spec `x`, com a obra já pedida ao revisor, e
/// devolve o código dele.
fn criterion_after_the_request(root: &Path) -> String {
    let said = log_of(root).visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
    let body = json!({"when": "a busca roda", "then": "a citação sai com a pasta", "proof": "git --version",
        "form": "ubiquitous", "origin": said});
    let wrote = seed_at(&WriteOpts { root: root.to_path_buf(), spec: Some("x".into()), event_type: "criterion".into(), json: body.to_string() });
    let id = wrote["id"].as_u64().unwrap_or_else(|| panic!("o critério não gravou: {wrote}"));
    log_of(root).codes().get(&id).cloned().unwrap()
}

/// O texto do aviso de pedido refeito na resposta `out`, vazio sem ele.
fn renewed_hint(out: &Value) -> String {
    let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
    warnings.iter().filter(|w| w["reason"] == json!("review-request-renewed")).filter_map(|w| w["hint"].as_str()).collect::<Vec<_>>().join("\n")
}

/// O critério gravado depois do pedido de revisão aberto chega ao
/// revisor: o fechamento seguinte não espera o veredito do pedido antigo,
/// grava o pedido novo, que lista o critério, e avisa por quê.
#[test]
fn a_criterion_recorded_after_the_review_request_is_listed_by_the_next_close() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ready_with_waves(root, "x", &["git --version"], 1);
    assert_eq!(close(root, None)["review"]["final"], json!(true));
    // A revisão reprova sem apontar onda: o pedido de volta lista o que
    // mudou desde o veredito.
    let rejected = json!({"final": true, "result": "rejected", "text": "Refazer a conferência."});
    let asked = close(root, verdict_written(root, "x", rejected));
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let sent = review_sends(root);

    let code = criterion_after_the_request(root);
    let renewed = close(root, None);
    assert_eq!(renewed["review"]["final"], json!(true), "the close renews the request instead of waiting: {renewed}");
    assert_eq!(review_sends(root), sent + 1, "{renewed}");
    let prompt = review_prompt(&renewed);
    assert!(prompt.contains(&code), "the new request lists {code}: {prompt}");
    assert!(renewed_hint(&renewed).contains(&code), "{renewed}");
}

/// A aprovação do pedido gravado antes de um critério novo não fecha a
/// obra: o fechamento grava o pedido novo, que lista o critério, e só a
/// aprovação dele fecha.
#[test]
fn an_approval_of_the_request_before_a_new_criterion_does_not_close_the_work() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ready_with_waves(root, "x", &["git --version"], 1);
    assert_eq!(close(root, None)["review"]["final"], json!(true));

    let code = criterion_after_the_request(root);
    let renewed = close(root, approve(root, "x"));
    assert_eq!(renewed["phase"], json!("running"), "{renewed}");
    assert_eq!(renewed["review"]["final"], json!(true), "{renewed}");
    assert!(review_prompt(&renewed).contains(&code), "{renewed}");

    let closed = close(root, approve(root, "x"));
    assert_eq!(closed["phase"], json!("closed"), "the approval of the new request closes the work: {closed}");
    assert!(renewed_hint(&closed).is_empty(), "{closed}");
}
