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

/// A leitura única das referências a tarefas de uma spec, a mesma para a
/// gravação e para a rodada: cada valor de `depends_on`, número do evento ou
/// código, vira o número da versão vigente da tarefa que ele aponta. O código
/// aponta a versão mais nova que a leitura mostra; o número segue as
/// substituições até a versão vigente. O que não aponta tarefa nenhuma da
/// leitura some. As tabelas saem uma vez por leitura do log, e cada
/// referência resolve sem varrer os eventos de novo.
pub(crate) struct TaskRefs<'a> {
    /// A versão mais nova que a leitura mostra de cada código de tarefa.
    by_code: BTreeMap<&'a str, u64>,
    /// A versão vigente de cada número de evento.
    current: BTreeMap<u64, &'a SpecEvent>,
}

impl<'a> TaskRefs<'a> {
    /// As referências a tarefas de `log`, com os códigos `codes` dele.
    pub(crate) fn new(log: &'a SpecLog, codes: &'a BTreeMap<u64, String>) -> Self {
        let by_code = log
            .visible()
            .into_iter()
            .filter(|event| event.event_type == "task")
            .filter_map(|event| codes.get(&event.id).map(|code| (code.as_str(), event.id)))
            .collect();
        Self { by_code, current: log.current_versions() }
    }

    /// O número da versão vigente da tarefa que `value` aponta, ou `None`
    /// quando o número ou o código não aponta tarefa nenhuma da leitura.
    pub(crate) fn resolve(&self, value: &Value) -> Option<u64> {
        let raw = match EventRef::from_value(value)? {
            EventRef::Id(id) => id,
            EventRef::Code(code) => *self.by_code.get(code.as_str())?,
        };
        self.current.get(&raw).filter(|event| event.event_type == "task").map(|event| event.id)
    }

    /// As tarefas que `task` aponta em `depends_on`, já resolvidas
    /// ([`Self::resolve`]); a referência que não aponta tarefa nenhuma some.
    pub(crate) fn depends_on<'b>(&'b self, task: &'b SpecEvent) -> impl Iterator<Item = u64> + 'b {
        task.fields.get("depends_on").and_then(Value::as_array).into_iter().flatten().filter_map(|value| self.resolve(value))
    }
}

/// As dependências de cada tarefa vigente que não apontam tarefa nenhuma
/// desta spec, pelo número da tarefa: a mesma leitura que a gravação de uma
/// tarefa faz para recusar o `depends_on` desconhecido. Só entram as
/// tarefas com alguma dependência quebrada.
pub(crate) fn broken_dependencies(log: &SpecLog) -> BTreeMap<u64, Vec<String>> {
    let codes = log.codes();
    let refs = TaskRefs::new(log, &codes);
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
                .filter(|value| refs.resolve(value).is_none())
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
    let (refs_before, refs_after) = (TaskRefs::new(before, &codes_before), TaskRefs::new(after, &codes_after));
    let mut removed = BTreeSet::new();
    let mut dependents = BTreeSet::new();
    for task in after.visible().into_iter().filter(|event| event.event_type == "task") {
        let values = task.fields.get("depends_on").and_then(Value::as_array).into_iter().flatten();
        for value in values.filter(|value| refs_before.resolve(value).is_some() && refs_after.resolve(value).is_none()) {
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
    let refs = TaskRefs::new(after, &codes);
    let task_label = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
    let inherited = inherited_dependencies(task, before, &codes);
    let raw: Vec<Value> = task.fields.get("depends_on").and_then(Value::as_array).cloned().unwrap_or_default();
    for value in &raw {
        if refs.resolve(value).is_none() && !inherited.contains(&dependency_name(&codes, value)) {
            return Err(Refusal::TaskDependsOnUnknown { task: task_label, depends_on: dependency_label(value) });
        }
    }
    // A referência que não aponta tarefa nenhuma some do grafo: quem confere
    // a existência da dependência é o laço acima.
    let graph: BTreeMap<u64, Vec<u64>> =
        after.visible().into_iter().filter(|event| event.event_type == "task").map(|event| (event.id, refs.depends_on(event).collect())).collect();
    if let Some(cycle) = find_cycle(task.id, &graph) {
        let names = cycle.into_iter().map(|id| codes.get(&id).cloned().unwrap_or_else(|| id.to_string())).collect();
        return Err(Refusal::TaskDependencyCycle { cycle: names });
    }
    Ok(())
}
