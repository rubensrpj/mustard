//! A dependência entre tarefas da spec.
//!
//! A tarefa que a gravação acrescenta só aponta, em `depends_on`, tarefas da
//! spec, e nenhum círculo se fecha entre elas ([`task_dependency_rule`]). A
//! remoção de uma tarefa de que outra, ainda viva, depende deixaria a
//! dependente apontando o vazio: a próxima versão que a rodada gravasse dela
//! seria recusada e a rodada inteira parava. O `remove` do modelo que faria
//! isso é recusado, nomeando as dependentes ([`removal_rule`]); a dependência
//! que já está quebrada na spec é lida por [`broken_dependencies`], e a
//! rodada segura só a tarefa que a traz. A versão nova dessa tarefa, com a
//! dependência que a anterior já trazia, grava sem recusa: a regra de
//! gravação julga só a dependência que a gravação acrescenta.

use std::collections::{BTreeMap, BTreeSet};

use mustard_core::domain::spec_events::{EventRef, Refusal, SpecEvent, SpecLog};
use serde_json::Value;

/// O texto que nomeia, na recusa, uma dependência que não bateu com tarefa
/// nenhuma: exatamente o que `value` trazia, código ou número.
fn dependency_label(value: &Value) -> String {
    value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string())
}

/// O nome, na mensagem, do que `value` aponta: o código do item, também o da
/// tarefa que já saiu da leitura, ou exatamente o que a tarefa trazia.
fn dependency_name(codes: &BTreeMap<u64, String>, value: &Value) -> String {
    match EventRef::from_value(value) {
        Some(EventRef::Id(id)) => codes.get(&id).cloned().unwrap_or_else(|| id.to_string()),
        Some(EventRef::Code(code)) => code,
        None => dependency_label(value),
    }
}

/// O número da versão vigente da tarefa desta spec que `value` aponta (pelo
/// número do evento ou pelo código), ou `None` quando não existe tarefa
/// nenhuma com esse número ou código. Um número ou código que aponta uma
/// versão já substituída resolve para a versão nova, do mesmo jeito que o
/// resto do arquivo trata a identidade de um item.
fn task_ref(after: &SpecLog, codes: &BTreeMap<u64, String>, value: &Value) -> Option<u64> {
    let raw = match EventRef::from_value(value)? {
        EventRef::Id(id) => id,
        EventRef::Code(code) => {
            after.events.iter().filter(|event| event.event_type == "task" && codes.get(&event.id) == Some(&code)).map(|event| event.id).next_back()?
        }
    };
    after.current(raw).filter(|event| event.event_type == "task").map(|event| event.id)
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

/// A tarefa que uma gravação acrescenta a `after` e não estava em `before`;
/// nenhuma para uma gravação que não é de uma tarefa nova.
fn new_task<'a>(before: &SpecLog, after: &'a SpecLog) -> Option<&'a SpecEvent> {
    let had: BTreeSet<u64> = before.events.iter().map(|event| event.id).collect();
    after.events.iter().find(|event| event.event_type == "task" && !had.contains(&event.id))
}

/// As tarefas de que `task` depende, já resolvidas a números; uma referência
/// que não aponta tarefa nenhuma desta spec some da lista — quem confere a
/// existência da dependência é [`task_dependency_rule`], não este helper.
fn task_depends_on(task: &SpecEvent, after: &SpecLog, codes: &BTreeMap<u64, String>) -> Vec<u64> {
    task.fields.get("depends_on").and_then(Value::as_array).into_iter().flatten().filter_map(|value| task_ref(after, codes, value)).collect()
}

/// Os nomes das dependências que `task` herda da versão anterior dela, a que
/// o `replaces` aponta em `before`: o que essa versão já trazia em
/// `depends_on`, aponte ou não uma tarefa da spec. Uma tarefa que não revê
/// versão nenhuma não herda nada.
fn inherited_dependencies(task: &SpecEvent, before: &SpecLog, codes: &BTreeMap<u64, String>) -> BTreeSet<String> {
    task.int("replaces")
        .and_then(|id| before.get(id))
        .filter(|previous| previous.event_type == "task")
        .and_then(|previous| previous.fields.get("depends_on").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .map(|value| dependency_name(codes, value))
        .collect()
}

/// O primeiro círculo que se fecha partindo de `start`, pelo grafo de
/// dependências `graph`: a lista de números, na ordem, terminando de volta em
/// `start`. `None` quando nenhum círculo se fecha.
fn find_cycle(start: u64, graph: &BTreeMap<u64, Vec<u64>>) -> Option<Vec<u64>> {
    fn walk(node: u64, start: u64, graph: &BTreeMap<u64, Vec<u64>>, path: &mut Vec<u64>, seen: &mut BTreeSet<u64>) -> Option<Vec<u64>> {
        for &next in graph.get(&node).map(Vec::as_slice).unwrap_or_default() {
            if next == start {
                let mut cycle = path.clone();
                cycle.push(next);
                return Some(cycle);
            }
            if seen.insert(next) {
                path.push(next);
                if let Some(found) = walk(next, start, graph, path, seen) {
                    return Some(found);
                }
                path.pop();
            }
        }
        None
    }
    let mut path = vec![start];
    let mut seen = BTreeSet::from([start]);
    walk(start, start, graph, &mut path, &mut seen)
}

/// A tarefa recém-gravada só aponta, em `depends_on`, tarefas desta spec, e
/// nenhum círculo se fecha entre elas: a recusa nomeia a dependência que não
/// existe, ou o círculo inteiro, na ordem, voltando ao começo. Só a
/// dependência que a gravação acrescenta é julgada: a que a versão anterior da
/// tarefa já trazia passa como estava, quebrada ou não, porque quem a segura é
/// a rodada ([`broken_dependencies`]) — a volta da tarefa ao backlog, que
/// repete as dependências dela, não pode ser recusada por um defeito que não é
/// seu.
///
/// # Errors
///
/// [`Refusal::TaskDependsOnUnknown`] e [`Refusal::TaskDependencyCycle`].
pub(super) fn task_dependency_rule(before: &SpecLog, after: &SpecLog) -> Result<(), Refusal> {
    let Some(task) = new_task(before, after) else {
        return Ok(());
    };
    let codes = after.codes();
    let task_label = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
    let inherited = inherited_dependencies(task, before, &codes);
    let raw: Vec<Value> = task.fields.get("depends_on").and_then(Value::as_array).cloned().unwrap_or_default();
    for value in &raw {
        if task_ref(after, &codes, value).is_none() && !inherited.contains(&dependency_name(&codes, value)) {
            return Err(Refusal::TaskDependsOnUnknown { task: task_label, depends_on: dependency_label(value) });
        }
    }
    let graph: BTreeMap<u64, Vec<u64>> =
        after.visible().into_iter().filter(|event| event.event_type == "task").map(|event| (event.id, task_depends_on(event, after, &codes))).collect();
    if let Some(cycle) = find_cycle(task.id, &graph) {
        let names = cycle.into_iter().map(|id| codes.get(&id).cloned().unwrap_or_else(|| id.to_string())).collect();
        return Err(Refusal::TaskDependencyCycle { cycle: names });
    }
    Ok(())
}
