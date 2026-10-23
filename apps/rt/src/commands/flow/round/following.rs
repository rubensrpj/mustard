//! A espera pela onda semelhante: a tarefa que a formação pôs no mesmo
//! sentido de uma onda em andamento grava, no grupo dela, o número dessa onda
//! ([`AFTER`]) e não sai enquanto ela roda. Quando a onda volta, a tarefa sai
//! na montagem seguinte, com o resumo que a onda deixou, quando deixou, e
//! junto das tarefas que ele acompanha; a onda que concluiu tudo não deixa
//! resumo, e a tarefa sai numa onda nova.

use std::collections::{BTreeMap, BTreeSet};

use mustard_core::domain::spec_events::{BlockQuery, SpecEvent, SpecLog};
use serde_json::Value;

use super::leftovers::GROUP;
use super::queue::{open_sends, waves_returned};

/// O campo do grupo da tarefa com o número da onda em andamento que ela
/// segue.
pub(crate) const AFTER: &str = "after";

/// As ondas em andamento: com pedido aberto e ainda sem volta.
pub(crate) fn running_waves(log: &SpecLog) -> BTreeSet<u64> {
    let returned = waves_returned(log);
    open_sends(log).into_keys().filter(|wave| !returned.contains(wave)).collect()
}

/// A onda que a tarefa `task` segue, gravada no grupo dela.
pub(crate) fn followed(task: &SpecEvent) -> Option<u64> {
    task.fields.get(GROUP)?.get(AFTER)?.as_u64()
}

/// O objetivo de cada onda de `waves`, lido em `log`, na ordem do número: o
/// nome do grupo que as tarefas dela gravam, que é o objetivo que a formação
/// deu a ela; sem grupo, o texto da onda, com os títulos das tarefas.
pub(crate) fn objectives(log: &SpecLog, waves: &BTreeSet<u64>) -> Vec<(u64, String)> {
    waves
        .iter()
        .filter_map(|wave| {
            let events = log.block(BlockQuery::Wave(*wave));
            let named = events
                .iter()
                .filter(|event| event.event_type == "task")
                .find_map(|task| task.fields.get(GROUP)?.get("name").and_then(Value::as_str).map(str::to_string));
            let said = || events.iter().rev().find(|event| event.event_type == "wave").and_then(|event| event.str_field("text")).map(str::to_string);
            named.or_else(said).filter(|objective| !objective.trim().is_empty()).map(|objective| (*wave, objective))
        })
        .collect()
}

/// As tarefas de `candidates` que esperam a onda que seguem, cada uma com o
/// número dela: a onda ainda em andamento, ou a que voltou com um resumo
/// cuja tarefa acompanhada chegou ao backlog nesta rodada (`arrived`) — a
/// tarefa espera mais uma rodada e sai junto dela, na mesma onda.
pub(super) fn waiting_for_wave(log: &SpecLog, candidates: &[u64], arrived: &[u64]) -> BTreeMap<u64, u64> {
    let running = running_waves(log);
    let summaries = log.live_summaries();
    let back_soon = |wave: u64| summaries.iter().any(|(summary, left)| summary.wave() == Some(wave) && left.iter().any(|task| arrived.contains(task)));
    candidates
        .iter()
        .filter_map(|id| {
            let wave = followed(log.get(*id)?)?;
            (running.contains(&wave) || back_soon(wave)).then_some((*id, wave))
        })
        .collect()
}
