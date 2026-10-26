//! As tarefas que nascem dos achados do veredito final aprovado.
//!
//! O fechamento grava cada achado do veredito final aprovado como um pedido,
//! que aponta o veredito em `origin`, e uma tarefa que cobre esse pedido, os
//! dois com o autor `review` (`flow::round::record_final_leftovers`). A tarefa
//! não leva `origin`: o caminho dela até o veredito passa pelo pedido que ela
//! cobre. Enquanto o veredito vale, o fechamento seguinte regrava a tarefa que
//! faltar, então o `remove` do modelo que a tirasse só adiaria o achado. Ele é
//! recusado ([`removal_rule`]), e a tarefa sai pela entrega.

use mustard_core::domain::spec_events::{Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::final_approval;

/// O autor que o fechamento põe no pedido e na tarefa de cada achado.
const REVIEW: &str = "review";

/// O `remove` que o modelo grava não tira a tarefa de um achado do veredito
/// final aprovado vigente em `before` ([`final_approval`]): o alvo é uma
/// tarefa de autor `review` que cobre um pedido do mesmo autor cuja `origin`
/// é esse veredito. Os alvos são os que a leitura tira, contados em `after`
/// ([`SpecLog::removal_targets`]), depois de a gravação os conferir: os
/// números do `remove`, o que pelo código virou todas as versões, e o que o
/// filtro de horário pegou. Outra tarefa sai como sempre, e sem aprovação
/// final vigente nenhuma tarefa é guardada: a entrega que veio depois dela já
/// tirou o achado do caminho.
///
/// # Errors
///
/// [`Refusal::FinalFindingRemoved`], com o código da primeira tarefa guardada
/// entre os alvos.
pub(super) fn removal_rule(event_type: &str, before: &SpecLog, after: &SpecLog) -> Result<(), Refusal> {
    if event_type != "remove" {
        return Ok(());
    }
    let Some(verdict) = final_approval(before) else {
        return Ok(());
    };
    let Some(removal) = after.events.iter().rev().find(|event| event.event_type == "remove" && event.id > before.max_id()) else {
        return Ok(());
    };
    let guarded = after.removal_targets(removal).into_iter().filter_map(|id| before.get(id)).find(|target| is_final_finding(before, target, verdict.id));
    match guarded {
        Some(task) => Err(Refusal::FinalFindingRemoved { code: before.codes().get(&task.id).cloned().unwrap_or_else(|| task.id.to_string()) }),
        None => Ok(()),
    }
}

/// `true` quando `event` é a tarefa de um achado do veredito `verdict`: de
/// autor `review`, cobrindo um pedido de autor `review` que aponta o veredito
/// em `origin`. O pedido de um achado e a nota dele não são tarefa.
fn is_final_finding(log: &SpecLog, event: &SpecEvent, verdict: u64) -> bool {
    event.event_type == "task"
        && event.str_field("author") == Some(REVIEW)
        && event.ints("covers").into_iter().filter_map(|id| log.get(id)).any(|request| {
            request.event_type == "request" && request.str_field("author") == Some(REVIEW) && request.int("origin") == Some(verdict)
        })
}
