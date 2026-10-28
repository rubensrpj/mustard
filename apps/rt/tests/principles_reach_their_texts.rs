// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os princípios do produto moram num texto só, e cada um chega a todo
//! texto a que se destina, e só a ele: o começo da sessão e a janela
//! renovada depois de `/clear`, que o condutor lê, o pedido da onda, que o
//! agente de onda lê, e os pedidos da revisão da onda e da revisão final,
//! que o revisor lê.
//!
//! Prova de ponta a ponta, pelo binário de verdade, nos dois idiomas: a spec
//! de uma onda anda pelos comandos e ganchos do uso real até o fechamento, e
//! cada texto é lido como a sessão ou o agente o recebe.

#![cfg(unix)]

use crate::one_wave_spec;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::platform::i18n::Locale;
use mustard_core::platform::seeds::{Reader, principles};
use serde_json::json;

/// O texto do envio mais novo de papel `role`, como o agente o lê.
fn request(log: &SpecLog, role: &str) -> String {
    let sent = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "send" && e.str_field("role") == Some(role))
        .unwrap_or_else(|| panic!("no `{role}` request was recorded"));
    log.send_text(sent.id).unwrap_or_else(|| panic!("the `{role}` request has no text")).to_string()
}

#[test]
fn every_principle_reaches_each_text_it_is_addressed_to() {
    for (lang, text) in [("pt-BR", Locale::PtBr), ("en-US", Locale::EnUs)] {
        let spec = "principios";
        let config = json!({"language": {"text": lang}, "inject": mustard_core::platform::project_seed::default_inject_entries()});
        let project = one_wave_spec::Project::new(spec, &config, &[]);
        mustard_core::platform::project_seed::seed_harness_texts(&project.root.join(".claude"), text, Default::default()).unwrap();
        project.approved();
        let started = project.session_start_from("s-principios", "startup");
        let cleared = project.session_start_from("s-principios", "clear");
        project.run(&["round", "--dispatch", "--spec", spec]);
        project.return_the_first_wave();
        // Sem o aceite de quem conduz, a rodada segura a volta e grava o
        // pedido do revisor da onda; com ele, a onda vai ao commit.
        let held = project.answer(&["round", "--dispatch", "--spec", spec]);
        assert_eq!(held["warnings"][0]["reason"], json!("wave-review-required"), "{held}");
        project.run(&["round", "--dispatch", "--spec", spec]);
        project.run(&["close", "--spec", spec]);
        let log = project.log();

        let texts = [
            (Reader::Conductor, "the session start", started),
            (Reader::Conductor, "the window after `/clear`", cleared),
            (Reader::Wave, "the wave request", request(&log, "wave")),
            (Reader::Review, "the wave review request", request(&log, "wave-review")),
            (Reader::Review, "the final review request", request(&log, "review")),
        ];
        let all = principles(text);
        assert!(all.len() >= 18, "the {lang} principles did not load: {all:?}");
        for (reader, name, said) in &texts {
            for principle in all {
                let line = format!("- **{}.** {}", principle.title, principle.text);
                assert_eq!(
                    said.contains(&line),
                    principle.readers.contains(reader),
                    "{lang}: {name} and the principle `{}` addressed to {:?}:\n{said}",
                    principle.title,
                    principle.readers,
                );
            }
        }
    }
}
