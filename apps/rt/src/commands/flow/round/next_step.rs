//! O próximo passo da resposta da rodada que chegou ao despacho: despachar o
//! que saiu agora; esperar as que estão em andamento; dizer qual onda falta,
//! quando nada se move; rodar de novo, ou nomear as tarefas presas, quando o
//! backlog ainda tem tarefa; esperar o veredito, quando a revisão final segue
//! aberta para o que a obra entregou; ou fechar, com tudo entregue e o backlog
//! vazio, inclusive quando o pedido aberto é mais velho que a última mudança ou
//! ficou para trás de um item gravado depois dele. A revisão do diff da onda
//! antecede o commit; a revisão final permanece no fechamento.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::platform::i18n::{Locale, translate};

use super::queue::{backlog_left, backlog_ready, first_unfinished};
use super::{DONE_STEP, awaits_verdict};

/// Onde as ondas estão depois dos envios da rodada.
pub(super) struct Standing<'a> {
    /// Alguma onda saiu ou voltou a sair nesta rodada.
    pub(super) dispatched: bool,
    /// As ondas com agente trabalhando, cada uma com o número do envio dela.
    pub(super) running: &'a BTreeMap<u64, u64>,
    /// As ondas em andamento, cada uma com o número do envio dela, contando
    /// também a onda cujo agente nunca começou.
    pub(super) in_progress: &'a BTreeMap<u64, u64>,
    /// Outra linha da resposta já diz o que falta: a onda parada, a volta
    /// que espera o clique ou a escolha que segura uma onda.
    pub(super) explained: bool,
}

/// O texto do próximo passo da spec `spec`, lido em `log` com os códigos
/// `codes`, e o comando dele, quando o passo tem um. Sem texto quando outra
/// linha da resposta já diz o que falta.
pub(super) fn next_step(root: &Path, spec: &str, (log, codes): (&SpecLog, &BTreeMap<u64, String>), standing: &Standing<'_>, lang: Locale) -> (String, Option<String>) {
    let report_back = super::checks::report_back(root, lang);
    let mut command: Option<String> = None;
    let then = if standing.dispatched {
        format!("{} {report_back}", translate("round.next", lang))
    } else if !standing.running.is_empty() {
        let waves: Vec<String> = standing.running.keys().map(u64::to_string).collect();
        format!("{} {report_back}", translate("round.waiting", lang).replace("{waves}", &waves.join(", ")))
    } else if standing.explained {
        String::new()
    } else if let Some(wave) = first_unfinished(log, standing.in_progress) {
        translate("round.missing", lang).replace("{wave}", &wave.to_string())
    } else if !backlog_left(log).is_empty() {
        // Toda onda planejada terminou, mas o backlog ainda tem tarefa: a obra
        // não fecha. A tarefa que ficou pronta pela entrega desta mesma
        // rodada só vira lote na rodada seguinte, porque o backlog foi formado
        // pela leitura de entrada; sem tarefa pronta e sem nada em andamento,
        // as que sobraram estão presas, e a resposta as nomeia, com a que
        // espera plano à parte, dizendo o que falta a ela.
        let shown = |ids: &mut dyn Iterator<Item = u64>| -> String {
            ids.map(|id| codes.get(&id).cloned().unwrap_or_else(|| id.to_string())).collect::<Vec<_>>().join(", ")
        };
        let ready = backlog_ready(log);
        if ready.is_empty() {
            super::planning::stuck_backlog_line(log, codes, lang)
        } else {
            let state = State::from_log(log);
            let step = crate::commands::flow::resume::step_command("round", spec, &state);
            let text = translate("round.backlog_left", lang)
                .replace("{tasks}", &shown(&mut ready.into_iter()))
                .replace("{command}", step.as_deref().unwrap_or_default());
            command = step;
            text
        }
    } else if awaits_verdict(log) {
        // Toda onda está entregue, mas a revisão final segue aberta, sem o
        // veredito do revisor: a obra ainda não está aprovada, e o fechamento
        // recusaria. A resposta manda esperar o veredito, sem o comando de
        // fechar. O pedido aberto mais velho que a última mudança não entra
        // aqui, nem o que ficou para trás de um item gravado depois dele, pela
        // mesma conferência do fechamento: o veredito dele já não aprovaria a
        // obra, e a resposta manda fechar, que grava o pedido novo.
        translate("round.review_open", lang).to_string()
    } else {
        let state = State::from_log(log);
        // A obra já fechada não fecha de novo: a rodada acabou de receber o
        // conserto do pull request reprovado, e o passo é empurrá-lo pela
        // mesma porta que o abriu.
        let (step, key) = if state.phase == Some("pr_open") {
            let reason = translate("round.fix_reason", lang);
            (Some(format!("mustard-rt run reopen --fix --spec {spec} --reason \"{reason}\"")), "round.fix_push")
        } else {
            (crate::commands::flow::resume::step_command(DONE_STEP, spec, &state), "round.close")
        };
        let text = translate(key, lang).replace("{command}", step.as_deref().unwrap_or_default());
        command = step;
        text
    };
    (then, command)
}

#[cfg(test)]
mod tests {
    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{Locale, translate};
    use serde_json::json;
    use tempfile::tempdir;

    use crate::commands::flow::round::tests::{approved, delivered, id_of, round, seed_review, waves_in, write};

    /// Com a revisão final pedida depois da última onda, o critério gravado
    /// depois do pedido deixa o pedido para trás: a rodada não espera mais o
    /// veredito dele e manda fechar, que grava o pedido novo.
    #[test]
    fn a_criterion_recorded_after_the_review_request_turns_the_wait_into_close() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let committed = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert!(committed["commit"]["sha"].is_string(), "a última onda foi comitada: {committed}");
        seed_review(root);
        let waiting = round(root, "x", None);
        assert!(waiting.get("command").is_none(), "o pedido de agora espera o veredito: {waiting}");

        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let said = log.visible().into_iter().find(|e| e.event_type == "message").map(|e| e.id).unwrap();
        id_of(&write(root, "x", "criterion", json!({"when": "a busca roda", "then": "a citação sai com a pasta",
            "proof": "git --version", "form": "ubiquitous", "origin": said})));

        let past = round(root, "x", None);
        assert_eq!(past["ok"], json!(true), "{past}");
        assert_eq!(past["command"], json!("mustard-rt run close --spec x"), "{past}");
        let next = past["next"].as_str().unwrap_or_default();
        assert!(!next.contains(translate("round.review_open", Locale::PtBr)), "a rodada não espera o veredito do pedido para trás: {past}");
    }
}
