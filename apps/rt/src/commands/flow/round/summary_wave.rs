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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::platform::i18n::{translate, Locale};
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::agreed::request_agreed;
    use super::super::backlog::dispatch_backlog;
    use super::super::queue::{backlog_project, backlog_task_on, max_parallel, open_sends, spec_now, wave_order};
    use super::super::read_check::{request_name, unread_items};
    use super::super::tests::{returned_unread, round, seed_read, seed_send, waves_in};
    use crate::commands::spec_events::read::{read_for, ReadOpts};
    use crate::shared::spec_state::seed_event;

    /// A entrega de uma onda que parou e deixou por fazer as tarefas `left`,
    /// pelo código, gravada como o agente a grava. Devolve o número dela.
    fn stopped_with(root: &Path, left: &[u64]) -> u64 {
        let codes = spec_now(root).codes();
        let undone: Vec<&String> = left.iter().map(|id| &codes[id]).collect();
        seed_event(root, "x", "delivered", json!({"wave": 7, "text": "Parei no limite.", "files": [], "undone": undone}))
    }

    /// O envio da onda `wave`, que cita o resumo `summary` quando ela o leva.
    fn sent(root: &Path, wave: u64, summary: Option<u64>) {
        seed_send(root, wave);
        if let Some(summary) = summary {
            seed_event(root, "x", "send", json!({"wave": wave, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [1], "mustard": "0", "author": "binary", "summary": summary}));
        }
    }

    /// O resumo não usado é a primeira onda da montagem, mesmo com tarefa de
    /// código mais baixo pronta no backlog: a onda leva só as tarefas que ele
    /// deixou, e o evento dela grava o resumo. A outra tarefa sai na onda
    /// seguinte, sem resumo. Com o envio citando o resumo ele está em uso, e
    /// nenhuma outra montagem forma onda com ele.
    #[test]
    fn an_unused_summary_is_the_first_wave_even_with_a_lower_numbered_task_ready() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let lower = backlog_task_on(root, said, crit, "Mexer no código de um.", &["src/a.rs"]);
        let left = backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs"]);
        let summary = stopped_with(root, &[left]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None), Ok(vec![1, 2]));
        assert_eq!(wave_order(root, 1), vec![left], "o resumo vem antes, com o que ele deixou");
        assert_eq!(wave_order(root, 2), vec![lower], "a de código mais baixo sai depois");
        let log = spec_now(root);
        let waves: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "wave").collect();
        assert_eq!(waves[0].int("summary"), Some(summary), "a onda grava o resumo que leva: {:?}", waves[0].fields);
        assert_eq!(waves[1].int("summary"), None, "a outra onda não leva resumo");

        sent(root, 1, Some(summary));
        sent(root, 2, None);
        let log = spec_now(root);
        assert!(log.unused_summaries().is_empty(), "o resumo citado pelo envio está em uso");
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None), Ok(vec![]), "nenhuma onda nova");
    }

    /// A tarefa pronta que divide arquivo, direto, com uma das que o resumo
    /// deixou vai na onda dele; a que só divide arquivo com essa, por
    /// corrente, não vai, e espera a vez.
    #[test]
    fn a_ready_task_sharing_a_file_with_the_left_ones_rides_along_but_a_chain_does_not() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let left = backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs"]);
        let along = backlog_task_on(root, said, crit, "Mexer nos dois.", &["src/b.rs", "src/c.rs"]);
        let chained = backlog_task_on(root, said, crit, "Mexer no três.", &["src/c.rs"]);
        let apart = backlog_task_on(root, said, crit, "Mexer no quatro.", &["src/d.rs"]);
        stopped_with(root, &[left]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None), Ok(vec![1, 2]));
        assert_eq!(wave_order(root, 1), vec![left, along], "a que divide arquivo vai de carona");
        assert_eq!(wave_order(root, 2), vec![apart], "a da corrente espera, e a sem relação sai à parte");
        assert_eq!(spec_now(root).current(chained).and_then(|task| task.wave()), None, "a da corrente segue no backlog");
    }

    /// O resumo preso por uma onda em andamento reserva os arquivos dele: a
    /// tarefa que divide arquivo com a onda dele, mesmo só por corrente, não
    /// toma a vaga que ele deixa livre, e a que não divide sai.
    #[test]
    fn a_summary_held_by_a_running_wave_keeps_its_files_from_a_later_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs"]);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None), Ok(vec![1]));
        seed_send(root, 1);

        let left = backlog_task_on(root, said, crit, "Mexer nos dois.", &["src/b.rs", "src/a.rs"]);
        backlog_task_on(root, said, crit, "Mexer no um.", &["src/a.rs", "src/d.rs"]);
        let chained = backlog_task_on(root, said, crit, "Mexer no quatro.", &["src/d.rs"]);
        let apart = backlog_task_on(root, said, crit, "Mexer no cinco.", &["src/e.rs"]);
        stopped_with(root, &[left]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None), Ok(vec![2]));
        assert_eq!(wave_order(root, 2), vec![apart], "só a que não divide arquivo sai");
        assert_eq!(spec_now(root).current(chained).and_then(|task| task.wave()), None, "a que divide espera a vez");
        assert_eq!(spec_now(root).current(left).and_then(|task| task.wave()), None, "o resumo espera a onda em andamento");
    }

    /// A entrega da onda que continua um resumo é recusada, com o código dele,
    /// enquanto ele não foi lido de dentro da cópia, ainda que todo o resto do
    /// pedido tenha sido lido; lido o resumo pelo comando de verdade, a mesma
    /// entrega grava.
    #[test]
    fn a_delivery_is_refused_until_the_summary_the_wave_continues_is_read() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let left = backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs"]);
        let summary = stopped_with(root, &[left]);
        let code = spec_now(root).codes()[&summary].clone();

        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        let log = spec_now(root);
        let open = open_sends(&log)[&1];
        let request = request_name(Some(1));
        let listed = unread_items(&log, open, &request);
        assert_eq!(listed.first(), Some(&code), "the request lists the summary first: {listed:?}");
        for item in listed.iter().filter(|item| **item != code) {
            seed_read(root, "x", &request, item);
        }
        let agreed: Vec<_> = request_agreed(&log, 1).iter().map(|item| json!({"item": item.id, "met": true})).collect();
        let delivery = json!({"wave": 1, "text": "A onda 1 saiu.", "agreed": agreed});

        let refused = returned_unread(root, delivery.clone());
        assert_eq!(refused["reason"], json!("delivery-read-missing"), "{refused}");
        let expected = translate("spec_events.delivery_read_missing", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{missing}", &code);
        assert_eq!(refused["hint"], json!(expected), "only the summary is missing: {refused}");

        let copy = log.get(open).and_then(|send| send.str_field("copy")).expect("the copy of the wave").to_string();
        let opts = ReadOpts { root: root.to_path_buf(), spec: Some("x".into()), block: format!("item-{code}"), term: None };
        read_for(&opts, None, Path::new(&copy)).unwrap_or_else(|refused| panic!("{code}: {refused}"));
        let wrote = returned_unread(root, delivery);
        assert_eq!(wrote["ok"], json!(true), "the same delivery passes once the summary is read: {wrote}");
    }
}
