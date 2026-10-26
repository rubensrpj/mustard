//! As contas do andamento de uma spec — ondas, tarefas, critérios e revisão —
//! pela mesma leitura que decide o que a rodada despacha
//! ([`crate::commands::flow::round::wave_states`]). O painel, a barra de status
//! e a retomada contam pelas mesmas funções, para os três dizerem o mesmo
//! número: as que a retomada também lê, e que não dependem da rodada, moram
//! na parte comum ([`crate::commands::flow::common::progress`]).

use std::path::Path;

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::domain::spec_state::qa;
use mustard_core::view::document::{WaveState, WaveStates};
use serde_json::{Value, json};

/// O estado de cada onda do plano, o da rodada.
pub(crate) fn wave_states(log: &SpecLog) -> WaveStates {
    crate::commands::flow::round::wave_states(log)
}

/// As vagas da obra `spec`, lida em `log`, pela conta única da rodada
/// ([`crate::commands::flow::round::vacancy::vacancies`]): `(vagas do projeto,
/// vagas livres)`. A onda que voltou guarda a cópia sem ocupar vaga.
pub(crate) fn slots(root: &Path, spec: &str, log: &SpecLog) -> (usize, usize) {
    let vacancies = crate::commands::flow::round::vacancy::vacancies(root, spec, log);
    (vacancies.capacity, vacancies.free())
}

/// As ondas terminadas e as do plano: `(terminadas, planejadas)`. Terminada é
/// a entregue e aprovada, com commit próprio ou integrada sem ele.
pub(crate) fn wave_counts(log: &SpecLog) -> (usize, usize) {
    let states = wave_states(log);
    (states.values().filter(|state| state.finished()).count(), states.len())
}

/// As ondas `states` que estão em `state`, em ordem de número.
pub(crate) fn waves_in(states: &WaveStates, state: WaveState) -> Vec<u64> {
    states.iter().filter(|(_, s)| **s == state).map(|(n, _)| *n).collect()
}

/// A divisão das ondas do plano por estado, com a lista das que pedem ação:
/// as sem agente e as reprovadas.
pub(crate) fn waves(states: &WaveStates) -> Value {
    let count = |state| states.values().filter(|s| **s == state).count();
    json!({"planned":states.len(),"finished":states.values().filter(|s|s.finished()).count(),
        "committed":count(WaveState::Committed),"integrated":count(WaveState::Approved),
        "running":count(WaveState::Running),"todo":count(WaveState::Todo),
        "no_agent":waves_in(states,WaveState::NoAgent),"rejected":waves_in(states,WaveState::Rejected)})
}

/// O estado de uma tarefa: ela não tem estado próprio, vale o da onda que a
/// versão vigente dela aponta. Sem onda, a tarefa está no backlog; a onda
/// que não está no plano conta como por fazer.
pub(crate) fn task_state(task: &SpecEvent, states: &WaveStates) -> &'static str {
    task.wave().map_or("backlog", |n| states.get(&n).copied().unwrap_or(WaveState::Todo).word())
}

/// As tarefas vigentes, as terminadas e as do backlog.
pub(crate) fn tasks(log: &SpecLog, states: &WaveStates) -> Value {
    let visible = log.visible();
    let tasks: Vec<&SpecEvent> = visible.iter().copied().filter(|e| e.event_type == "task").collect();
    let finished = tasks.iter().filter(|task| task.wave().and_then(|n| states.get(&n)).is_some_and(|s| s.finished())).count();
    json!({"total":tasks.len(),"finished":finished,"backlog":tasks.iter().filter(|task|task.wave().is_none()).count()})
}

/// Os critérios vigentes e as execuções da prova de cada um
/// ([`mustard_core::domain::spec_state::qa`]): passando, falhando e sem prova.
pub(crate) fn criteria(log: &SpecLog) -> Value {
    let qa = qa(log);
    json!({"total":qa.criteria,"passed":qa.passed,"failed":qa.failed,"without_proof":qa.criteria.saturating_sub(qa.ran)})
}

