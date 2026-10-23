//! O próximo passo da resposta da rodada que chegou ao despacho: despachar o
//! que saiu agora; esperar as que estão em andamento; dizer qual onda falta,
//! quando nada se move; rodar de novo, ou nomear as tarefas presas, quando o
//! backlog ainda tem tarefa; esperar o veredito, quando a revisão final segue
//! aberta; ou fechar, com tudo entregue e aprovado e o backlog vazio. A
//! revisão do diff da onda antecede o commit; a revisão final permanece no
//! fechamento.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::platform::i18n::{Locale, translate};

use super::DONE_STEP;
use super::queue::{backlog_left, backlog_ready, first_unfinished, open_review};

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
    } else if open_review(log).is_some() {
        // Toda onda está entregue, mas a revisão final segue aberta, sem o
        // veredito do revisor: a obra ainda não está aprovada, e o fechamento
        // recusaria. A resposta manda esperar o veredito, sem o comando de
        // fechar.
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
