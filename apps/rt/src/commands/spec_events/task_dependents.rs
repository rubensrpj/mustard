//! A dependência entre tarefas que deixa de apontar uma tarefa da spec.
//!
//! A remoção de uma tarefa de que outra, ainda viva, depende deixaria a
//! dependente apontando o vazio: a próxima versão que a rodada gravasse dela
//! seria recusada e a rodada inteira parava. O `remove` do modelo que faria
//! isso é recusado, nomeando as dependentes ([`removal_rule`]); a dependência
//! que já está quebrada na spec é lida por [`broken_dependencies`], e a
//! rodada segura só a tarefa que a traz.

use std::collections::{BTreeMap, BTreeSet};

use mustard_core::domain::spec_events::{EventRef, Refusal, SpecLog};
use serde_json::Value;

use super::write::{dependency_label, task_ref};

/// O nome, na mensagem, do que `value` aponta: o código do item, também o da
/// tarefa que já saiu da leitura, ou exatamente o que a tarefa trazia.
fn dependency_name(codes: &BTreeMap<u64, String>, value: &Value) -> String {
    match EventRef::from_value(value) {
        Some(EventRef::Id(id)) => codes.get(&id).cloned().unwrap_or_else(|| id.to_string()),
        Some(EventRef::Code(code)) => code,
        None => dependency_label(value),
    }
}

/// As dependências de cada tarefa vigente que não apontam tarefa nenhuma
/// desta spec, pelo número da tarefa: a mesma leitura que a gravação de uma
/// tarefa faz para recusar o `depends_on` desconhecido. Só entram as
/// tarefas com alguma dependência quebrada.
pub(crate) fn broken_dependencies(log: &SpecLog) -> BTreeMap<u64, Vec<String>> {
    let codes = log.codes();
    log.visible()
        .into_iter()
        .filter(|event| event.event_type == "task")
        .filter_map(|task| {
            let broken: Vec<String> = task
                .fields
                .get("depends_on")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|value| task_ref(log, &codes, value).is_none())
                .map(|value| dependency_name(&codes, value))
                .collect();
            (!broken.is_empty()).then_some((task.id, broken))
        })
        .collect()
}

/// O `remove` que o modelo grava não deixa tarefa viva dependendo de uma
/// tarefa que ele tirou: a dependência que apontava uma tarefa em `before` e
/// não aponta em `after` recusa a gravação, com as tarefas que saem e as que
/// dependem delas. A dependente removida no mesmo `targets` não conta, e a
/// dependência que já estava quebrada antes não é culpa desta remoção.
///
/// # Errors
///
/// [`Refusal::TaskHasDependents`].
pub(super) fn removal_rule(event_type: &str, before: &SpecLog, after: &SpecLog) -> Result<(), Refusal> {
    if event_type != "remove" {
        return Ok(());
    }
    let (codes_before, codes_after) = (before.codes(), after.codes());
    let mut removed = BTreeSet::new();
    let mut dependents = BTreeSet::new();
    for task in after.visible().into_iter().filter(|event| event.event_type == "task") {
        let values = task.fields.get("depends_on").and_then(Value::as_array).into_iter().flatten();
        for value in values.filter(|value| task_ref(before, &codes_before, value).is_some() && task_ref(after, &codes_after, value).is_none()) {
            removed.insert(dependency_name(&codes_after, value));
            dependents.insert(codes_after.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string()));
        }
    }
    if dependents.is_empty() {
        return Ok(());
    }
    Err(Refusal::TaskHasDependents { removed: removed.into_iter().collect(), dependents: dependents.into_iter().collect() })
}
