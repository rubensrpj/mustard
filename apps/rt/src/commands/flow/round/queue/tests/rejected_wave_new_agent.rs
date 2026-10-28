//! A rodada e o gancho do despacho contam a onda em andamento pela mesma
//! leitura: a onda que a rodada solta e a onda cuja volta ela recusa são
//! aceitas pelo gancho na mesma hora, sem outra rodada.

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};

use super::*;
use crate::hooks::task::subagent_inject::SubagentInject;

/// O gancho do despacho diante do título da onda `wave` seguido de `body`, como
/// o condutor escreve o despacho do agente.
fn hook(root: &Path, wave: u64, body: &str) -> Verdict {
    let title = mustard_core::domain::wave_prompt::wave_title("x", wave, Locale::PtBr);
    let input = HookInput {
        hook_event_name: Some("PreToolUse".to_string()),
        tool_name: Some("Agent".to_string()),
        tool_input: json!({ "prompt": format!("{title}\n\n{body}"), "subagent_type": "mustard-wave", "description": "onda" }),
        ..HookInput::default()
    };
    let mut ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(Trigger::PreToolUse));
    ctx.config = mustard_core::ProjectConfig::load(root);
    SubagentInject.evaluate(&input, &ctx).expect("never errors")
}

/// Três ondas e duas vagas. As duas primeiras saem e enchem as vagas: o
/// gancho aceita o despacho de cada uma que a rodada soltou e barra a terceira,
/// que a rodada não soltou, e a leitura do andamento é a mesma dos dois lados.
/// Depois de o agente da onda 1 voltar e de quem conduz reprovar a volta, a
/// rodada manda lançar um agente novo pelo título, e o gancho o aceita nessa
/// mesma hora, com o motivo no pedido; a onda 2 segue com o agente dela e a 3
/// segue de fora, com as vagas ainda cheias.
#[test]
fn a_rejected_wave_takes_a_new_agent_right_after_the_rejecting_round() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["web/a.ts"], &[]), (2, &["web/b.ts"], &[]), (3, &["web/c.ts"], &[])]);
    std::fs::write(root.join("mustard.json"), br#"{"maxCompilingWaves":2}"#).unwrap();

    let sent = round_without_launch(root, None);
    assert_eq!(waves_in(&sent, "dispatch"), vec![1, 2], "the slots fill with two waves: {sent}");
    let running: Vec<u64> = waves_in_progress(&spec_now(root)).into_keys().collect();
    assert_eq!(running, vec![1, 2], "the hook reads the waves the round released");
    for wave in 1..=3 {
        let read = sent["dispatch"].as_array().unwrap().iter().find(|d| d["wave"] == json!(wave)).and_then(|d| d["read"].as_str()).unwrap_or("mustard-rt run read request-3");
        let verdict = hook(root, wave, read);
        assert_eq!(!matches!(verdict, Verdict::Deny { .. }), running.contains(&wave), "wave {wave}: {verdict:?}");
    }
    launched(root, "x", sent);

    delivered(root, 1, "Saiu.", &["web/a.ts"]);
    let reason = "Falta o teste do caso vazio.";
    let line = format!(r#"<REJECTED>{{"wave":1,"reason":"{reason}"}}</REJECTED>"#);
    let out = round(root, "x", Some(&line));
    assert_eq!(waves_in(&out, "dispatch"), Vec::<u64>::new(), "the slots stay full: {out}");
    assert_eq!(warning_of(&out, "round-wave-rejected")["wave"], json!(1), "{out}");
    let log = spec_now(root);
    assert_eq!(waves_in_progress(&log).into_keys().collect::<Vec<u64>>(), vec![2], "the rejected wave is not in progress: {out}");
    assert!(waves_awaiting_new_agent(root, "x", &log).contains_key(&1), "the round asks for a new agent: {out}");

    match hook(root, 1, "Conserte a onda.") {
        Verdict::Rewrite { tool_input, .. } => {
            let prompt = tool_input["prompt"].as_str().unwrap_or_default();
            assert!(prompt.contains(reason), "the new agent gets the reason: {prompt}");
        }
        other => panic!("the hook takes the new agent right after the round, got {other:?}"),
    }
    for wave in [2, 3] {
        let verdict = hook(root, wave, "mustard-rt run read request-3");
        assert!(matches!(verdict, Verdict::Deny { .. }), "wave {wave}: {verdict:?}");
    }
}
