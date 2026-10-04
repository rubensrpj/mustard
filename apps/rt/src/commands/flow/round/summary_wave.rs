//! As ondas que continuam um resumo: a cada resumo não usado
//! ([`SpecLog::unused_summaries`]), a montagem forma primeiro uma onda com as
//! tarefas que o resumo deixou por fazer (`undone`, na versão vigente de
//! cada uma) e as do backlog pronto que dividem arquivo com elas. Uma onda
//! leva no máximo um resumo, e as tarefas dela não entram em outra onda da
//! mesma montagem, saia ela agora ou espere a vez pela regra de conflito.

use std::collections::{BTreeMap, BTreeSet};

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use serde_json::Value;

use super::queue::backlog_task_ref;
use crate::shared::dag::{sets_cross, touches_whole_tree, BacklogTask, Batch};

/// Uma onda formada a partir de um resumo não usado.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SummaryWave {
    /// O número da entrega que é o resumo.
    pub summary: u64,
    /// As tarefas da onda, na ordem de prontidão, e os arquivos delas.
    pub batch: Batch<u64>,
}

/// As ondas dos resumos não usados, na ordem de número do resumo. `ready` é
/// o backlog pronto, na ordem de prontidão, e `population` traz os arquivos
/// de cada tarefa. De cada resumo entram as tarefas de `undone` que estão
/// prontas e as prontas que dividem arquivo com elas, direto; a tarefa com o
/// curinga da árvore inteira nunca entra de carona, e uma tarefa só vai a um
/// resumo. O resumo sem nenhuma tarefa pronta não forma onda.
pub(super) fn summary_waves(log: &SpecLog, ready: &[u64], population: &[BacklogTask<u64>]) -> Vec<SummaryWave> {
    let codes = log.codes();
    let files_of = |id: u64| population.iter().find(|task| task.id == id).map(|task| &task.files);
    let mut taken: BTreeSet<u64> = BTreeSet::new();
    let mut out = Vec::new();
    for summary in log.unused_summaries() {
        let left = left_by(log, summary, &codes);
        let base: Vec<u64> = ready.iter().copied().filter(|id| left.contains(id) && !taken.contains(id)).collect();
        if base.is_empty() {
            continue;
        }
        let mut batch = Batch { tasks: base.clone(), files: BTreeSet::new() };
        for id in &base {
            batch.files.extend(files_of(*id).into_iter().flatten().cloned());
        }
        if !touches_whole_tree(&batch.files) {
            let sharing = |id: &u64| {
                !taken.contains(id)
                    && !base.contains(id)
                    && files_of(*id).is_some_and(|files| !touches_whole_tree(files) && sets_cross(files, &batch.files))
            };
            let along: Vec<u64> = ready.iter().copied().filter(sharing).collect();
            for id in &along {
                batch.files.extend(files_of(*id).into_iter().flatten().cloned());
            }
            batch.tasks.extend(along);
            batch.tasks.sort_by_key(|id| ready.iter().position(|had| had == id));
        }
        taken.extend(batch.tasks.iter().copied());
        out.push(SummaryWave { summary: summary.id, batch });
    }
    out
}

/// As tarefas que o resumo `summary` deixou por fazer (`undone`), cada uma
/// pelo número da versão vigente dela.
fn left_by(log: &SpecLog, summary: &SpecEvent, codes: &BTreeMap<u64, String>) -> BTreeSet<u64> {
    summary
        .fields
        .get("undone")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|code| backlog_task_ref(log, codes, code))
        .collect()
}
