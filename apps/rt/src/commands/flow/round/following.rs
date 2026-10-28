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
use super::queue::{open_sends, waves_pending_fix};

/// O campo do grupo da tarefa com o número da onda em andamento que ela
/// segue.
pub(crate) const AFTER: &str = "after";

/// As ondas em andamento: com pedido aberto e ainda sem volta, e a reprovada
/// com o conserto pendente ([`waves_pending_fix`]) — a que espera vaga para
/// sair de novo ou a que devolveu o conserto e espera a rodada assumi-lo. Ela
/// não conta como voltada: a tarefa que a segue espera o conserto, e não só a
/// volta que a reprovou.
pub(crate) fn running_waves(log: &SpecLog) -> BTreeSet<u64> {
    let returned = log.waves_returned();
    let mut running: BTreeSet<u64> = open_sends(log).into_keys().filter(|wave| !returned.contains(wave)).collect();
    running.extend(waves_pending_fix(log).into_keys());
    running
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

#[cfg(test)]
mod tests {
    use mustard_core::platform::i18n::Locale;
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::backlog::{Hold, dispatch_backlog};
    use super::super::queue::{backlog_project, backlog_task_on, max_parallel, spec_now};
    use super::super::scheduling;
    use super::super::tests::{delivered, id_of, round, verdict, waves_in, write};
    use super::*;

    /// A onda 1 sai, volta, é reprovada e sai de novo para o conserto. A tarefa
    /// que a formação pôs a seguir a onda 1 segue no backlog: enquanto o
    /// conserto não for aprovado, mesmo depois de o agente devolvê-lo, a onda
    /// conta como em andamento e a tarefa fica segura com o motivo da onda
    /// seguida. Aprovado o conserto, a tarefa sai.
    #[test]
    fn a_rejected_wave_holds_the_task_that_follows_it_until_the_fix_is_approved() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task_on(root, said, crit, "Mexer no código de um.", &["src/a.rs"]);
        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        round(root, "x", Some(&delivered(root, 1, "A onda saiu.", &["src/a.rs"])));
        let fix = round(root, "x", Some(&verdict(root, 1, "rejected", "faltou o teste")));
        assert_eq!(waves_in(&fix, "dispatch"), vec![1], "a onda reprovada sai de novo para o conserto: {fix}");
        let follower = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Mexer no código de dois.", "files": [{"path": "src/b.rs"}], "depends_on": [],
            "covers": [crit], "origin": said, "group": {"name": "Mexer nos dois", "position": 1, "formed": true, AFTER: 1}}),
        ));
        // O agente devolve o conserto, e a devolução espera a rodada, com a
        // reprovação ainda de pé.
        delivered(root, 1, "O teste entrou.", &["src/a.rs"]);

        let log = spec_now(root);
        assert_eq!(running_waves(&log), BTreeSet::from([1]), "a onda reprovada, com o conserto por assumir, está em andamento");
        assert_eq!(waiting_for_wave(&log, &[follower], &[]), BTreeMap::from([(follower, 1)]));
        let (formed, holds) = dispatch_backlog(root, "x", &log, &log, max_parallel(root), (None, None)).unwrap();
        assert_eq!(formed, Vec::<u64>::new(), "a tarefa que segue a onda não sai");
        assert_eq!(holds.get(&log.codes()[&follower]), Some(&Hold::Following(1)));
        let queued = scheduling::held(root, "x", &spec_now(root), Locale::EnUs, &holds).expect("the follower stays queued");
        assert_eq!(queued.tasks[0]["reason"], json!("following-running-wave"), "{:?}", queued.tasks);

        let approved = round(root, "x", Some(&verdict(root, 1, "approved", "ficou certo")));
        assert_eq!(waves_in(&approved, "dispatch"), vec![2], "com o conserto aprovado a tarefa sai: {approved}");
        let log = spec_now(root);
        assert_eq!(running_waves(&log), BTreeSet::from([2]), "só a onda da tarefa que saiu está em andamento");
        assert_eq!(waiting_for_wave(&log, &[follower], &[]), BTreeMap::new());
    }
}
