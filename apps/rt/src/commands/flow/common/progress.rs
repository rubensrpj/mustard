//! As contas do andamento que a retomada e o painel leem igual e que não
//! dependem da leitura da rodada: a revisão final e as ondas que faltam.

use std::collections::BTreeSet;

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::domain::spec_state::{final_approval, last_reopening};
use mustard_core::view::document::WaveStates;
use serde_json::{Value, json};

/// A revisão final: se a aprovação vale agora, a data da última aprovação
/// dada e a da reabertura que veio depois dela, quando veio. A aprovação some
/// de [`final_approval`] quando a spec é reaberta; a data dela fica aqui.
pub(crate) fn review(log: &SpecLog) -> Value {
    let approvals: Vec<&SpecEvent> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "verdict" && e.fields.get("final") == Some(&Value::Bool(true)))
        .filter(|e| e.str_field("result").map(str::trim) == Some("approved"))
        .collect();
    let approved = approvals.last().copied();
    let reopened = last_reopening(log).filter(|id| approved.is_some_and(|event| *id > event.id)).and_then(|id| log.get(id));
    json!({"valid":final_approval(log).is_some(),"approved_at":approved.map(SpecEvent::at),"reopened_at":reopened.map(SpecEvent::at)})
}

/// As ondas que a retomada lista como "falta": as do plano que não terminaram
/// e que não estão em `running` nem em `returned`.
pub(crate) fn missing_waves(states: &WaveStates, running: &BTreeSet<u64>, returned: &BTreeSet<u64>) -> Vec<u64> {
    states.iter().filter(|(n, s)| !s.finished() && !running.contains(n) && !returned.contains(n)).map(|(n, _)| *n).collect()
}
