//! As ondas que continuam um resumo: a cada resumo que ainda vale
//! ([`SpecLog::live_summaries`]), a montagem forma primeiro uma onda com as
//! tarefas dele que estão prontas no backlog e as do backlog pronto que
//! dividem arquivo com elas ou que o classificador pôs no grupo de assunto
//! delas. O resumo acompanha cada tarefa que deixou até
//! ela ser entregue, em quantas ondas ela sair. Uma onda leva no máximo um
//! resumo, e as tarefas dela não entram em outra onda da mesma montagem,
//! saia ela agora ou espere uma vaga.

use std::collections::BTreeSet;

use mustard_core::domain::spec_events::SpecLog;

use super::leftovers::{GROUP, group_name};
use crate::shared::dag::{BacklogTask, Batch, sets_cross, touches_whole_tree};

/// Uma onda formada a partir de um resumo que ainda vale.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SummaryWave {
    /// O número da entrega que é o resumo.
    pub summary: u64,
    /// As tarefas da onda, na ordem de prontidão, e os arquivos delas.
    pub batch: Batch<u64>,
}

/// As ondas dos resumos que ainda valem, na ordem de número do resumo.
/// `ready` é o backlog pronto, na ordem de prontidão, e `population` traz os
/// arquivos de cada tarefa. De cada resumo entram as tarefas que ele
/// acompanha e que estão prontas, e as prontas que dividem arquivo com elas,
/// direto, ou que gravam o grupo de assunto de uma delas ([`group_name`]),
/// mesmo sem arquivo em comum; a tarefa com o curinga da árvore inteira nunca entra de carona, nem
/// a que outro resumo acompanha, que sai na onda dele. Uma tarefa só vai a um
/// resumo, e o resumo sem nenhuma tarefa pronta não forma onda.
pub(super) fn summary_waves(log: &SpecLog, ready: &[u64], population: &[BacklogTask<u64>]) -> Vec<SummaryWave> {
    let files_of = |id: u64| population.iter().find(|task| task.id == id).map(|task| &task.files);
    let live = log.live_summaries();
    let carried: BTreeSet<u64> = live.iter().flat_map(|(_, tasks)| tasks.iter().copied()).collect();
    let mut taken: BTreeSet<u64> = BTreeSet::new();
    let mut out = Vec::new();
    for (summary, left) in live {
        let base: Vec<u64> = ready.iter().copied().filter(|id| left.contains(id) && !taken.contains(id)).collect();
        if base.is_empty() {
            continue;
        }
        let mut batch = Batch { tasks: base.clone(), files: BTreeSet::new() };
        for id in &base {
            batch.files.extend(files_of(*id).into_iter().flatten().cloned());
        }
        let groups: BTreeSet<String> = base.iter().filter_map(|id| log.get(*id)).map(|task| group_name(log, task)).collect();
        let in_group = |id: u64| {
            log.get(id).is_some_and(|task| task.fields.contains_key(GROUP) && groups.contains(&group_name(log, task)))
        };
        if !touches_whole_tree(&batch.files) {
            let sharing = |id: &u64| {
                !taken.contains(id)
                    && !carried.contains(id)
                    && files_of(*id).is_some_and(|files| !touches_whole_tree(files) && (sets_cross(files, &batch.files) || in_group(*id)))
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

/// As ondas que o classificador formou: as tarefas prontas de `ready` que
/// gravam o mesmo grupo ([`GROUP`], pelo nome de [`group_name`]) saem juntas,
/// na posição que ele deu a cada uma, com os arquivos de `population`; os
/// grupos, na ordem da primeira tarefa pronta de cada um. Cada onda leva como
/// base o resumo que ainda vale e que deixou alguma tarefa dela: um resumo
/// que deixou tarefas de vários objetivos vai com cada uma dessas ondas. A
/// tarefa com o curinga da árvore inteira não entra: ela sai sozinha.
pub(super) fn group_waves(log: &SpecLog, ready: &[u64], population: &[BacklogTask<u64>]) -> Vec<(Batch<u64>, Option<u64>)> {
    let live = log.live_summaries();
    let files_of = |id: u64| population.iter().find(|task| task.id == id).map(|task| &task.files);
    let mut groups: Vec<(String, Vec<(u64, u64)>)> = Vec::new();
    for &id in ready {
        let Some(task) = log.get(id) else { continue };
        let Some(group) = task.fields.get(GROUP) else { continue };
        if files_of(id).is_some_and(touches_whole_tree) {
            continue;
        }
        let name = group_name(log, task);
        let position = group.get("position").and_then(serde_json::Value::as_u64).unwrap_or(u64::MAX);
        match groups.iter_mut().find(|(known, _)| *known == name) {
            Some((_, tasks)) => tasks.push((position, id)),
            None => groups.push((name, vec![(position, id)])),
        }
    }
    // O grupo que leva o código de uma tarefa é o grupo dela: a tarefa, que
    // não grava grupo, vai na mesma onda, na frente.
    for &id in ready {
        let Some(task) = log.get(id).filter(|task| !task.fields.contains_key(GROUP)) else { continue };
        let name = group_name(log, task);
        if let Some((_, tasks)) = groups.iter_mut().find(|(known, _)| *known == name) {
            tasks.push((0, id));
        }
    }
    groups
        .into_iter()
        .map(|(_, mut tasks)| {
            tasks.sort_unstable();
            let tasks: Vec<u64> = tasks.into_iter().map(|(_, id)| id).collect();
            let files = tasks.iter().flat_map(|id| files_of(*id).into_iter().flatten().cloned()).collect();
            let base = live.iter().find(|(_, left)| tasks.iter().any(|id| left.contains(id))).map(|(summary, _)| summary.id);
            (Batch { tasks, files }, base)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::platform::i18n::{Locale, translate};
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::agreed::request_agreed;
    use super::super::backlog::dispatch_backlog;
    use super::super::queue::{SIX_FILES, backlog_project, backlog_task_on, max_parallel, open_sends, seed_running, spec_now, wave_order};
    use super::super::read_check::{request_name, unread_items};
    use super::super::tests::{delivered, id_of, request_at, returned_unread, round, seed_read, seed_send, waves_in, write};
    use crate::commands::spec_events::read::{ReadOpts, read_for};
    use crate::shared::spec_state::seed_event;

    /// A entrega de uma onda que parou e deixou por fazer as tarefas `left`,
    /// pelo código, gravada como o agente a grava. Devolve o número dela.
    fn stopped_with(root: &Path, left: &[u64]) -> u64 {
        stopped_in(root, 7, left)
    }

    /// [`stopped_with`] da onda `wave`.
    fn stopped_in(root: &Path, wave: u64, left: &[u64]) -> u64 {
        let codes = spec_now(root).codes();
        let undone: Vec<&String> = left.iter().map(|id| &codes[id]).collect();
        seed_event(root, "x", "delivered", json!({"wave": wave, "text": "Parei no limite.", "files": [], "undone": undone}))
    }

    /// O envio da onda `wave`, que cita o resumo `summary` quando ela o leva.
    fn sent(root: &Path, wave: u64, summary: Option<u64>) {
        seed_send(root, wave);
        if let Some(summary) = summary {
            seed_event(
                root,
                "x",
                "send",
                json!({"wave": wave, "role": "wave", "text": "pedido", "lines": 1, "chars": 6,
                "items": [1], "mustard": "0", "author": "binary", "summary": summary}),
            );
        }
    }

    /// O resumo que vale é a primeira onda da montagem, mesmo com tarefa de
    /// código mais baixo pronta no backlog: a onda leva só as tarefas que ele
    /// deixou, e o evento dela grava o resumo. A outra tarefa sai na onda
    /// seguinte, sem resumo. Com a tarefa dele já numa onda enviada, nenhuma
    /// outra montagem forma onda com ele.
    #[test]
    fn a_summary_is_the_first_wave_even_with_a_lower_numbered_task_ready() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let lower = backlog_task_on(root, said, crit, "Mexer no código de um.", &["src/a.rs"]);
        let left = backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs"]);
        let summary = stopped_with(root, &[left]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![1, 2]));
        assert_eq!(wave_order(root, 1), vec![left], "o resumo vem antes, com o que ele deixou");
        assert_eq!(wave_order(root, 2), vec![lower], "a de código mais baixo sai depois");
        let log = spec_now(root);
        let waves: Vec<_> = log.visible().into_iter().filter(|e| e.event_type == "wave").collect();
        assert_eq!(waves[0].int("summary"), Some(summary), "a onda grava o resumo que leva: {:?}", waves[0].fields);
        assert_eq!(waves[1].int("summary"), None, "a outra onda não leva resumo");

        sent(root, 1, Some(summary));
        sent(root, 2, None);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![]), "nenhuma onda nova");
    }

    /// A tarefa pronta que divide arquivo, direto, com uma das que o resumo
    /// deixou vai na onda dele; a que só divide arquivo com essa, por
    /// corrente, não vai, e sai na sua onda, como a sem relação.
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
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![1, 2, 3]));
        let id = |task: u64| log.current(task).unwrap().id;
        assert_eq!(wave_order(root, 1), vec![id(left), id(along)], "a que divide arquivo vai de carona");
        assert_eq!(wave_order(root, 2), vec![id(chained)], "a da corrente sai na sua onda");
        assert_eq!(wave_order(root, 3), vec![id(apart)], "a sem relação sai à parte");
    }

    /// A tarefa pronta que o classificador pôs no grupo de assunto de uma das
    /// que o resumo deixou vai na onda dele, sem arquivo em comum; a pronta
    /// sem grupo e sem arquivo em comum sai à parte.
    #[test]
    fn task_triage_group_rides_along_with_the_summary_without_a_shared_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let left = backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs"]);
        let code = spec_now(root).codes()[&left].clone();
        let grouped = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Mexer no cinco, do mesmo assunto.", "files": [{"path": "src/e.rs"}], "depends_on": [],
            "covers": [crit], "origin": said, "group": {"name": code, "position": 2}}),
        ));
        let apart = backlog_task_on(root, said, crit, "Mexer no quatro.", &["src/d.rs"]);
        stopped_with(root, &[left]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![1, 2]));
        let id = |task: u64| log.current(task).unwrap().id;
        assert_eq!(wave_order(root, 1), vec![id(left), id(grouped)], "a do mesmo grupo vai de carona");
        assert_eq!(wave_order(root, 2), vec![id(apart)], "a sem grupo sai à parte");
    }

    /// O resumo que divide arquivo com uma onda em andamento sai ao lado
    /// dela, com a tarefa que divide arquivo direto com a dele; a que só
    /// divide por corrente e a que não divide nada saem cada uma na sua vaga.
    #[test]
    fn a_summary_sharing_a_file_with_a_running_wave_leaves_beside_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs"]);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![1]));
        seed_running(root, 1);

        let left = backlog_task_on(root, said, crit, "Mexer nos dois.", &["src/b.rs", "src/a.rs"]);
        let along = backlog_task_on(root, said, crit, "Mexer no um.", &["src/a.rs", "src/d.rs"]);
        let chained = backlog_task_on(root, said, crit, "Mexer no quatro.", &["src/d.rs"]);
        let apart = backlog_task_on(root, said, crit, "Mexer no cinco.", &SIX_FILES);
        stopped_with(root, &[left]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![2, 3, 4]));
        let id = |task: u64| log.current(task).unwrap().id;
        assert_eq!(wave_order(root, 2), vec![id(left), id(along)], "o resumo sai ao lado da onda em andamento");
        assert_eq!(wave_order(root, 3), vec![id(chained)], "a da corrente sai na sua vaga");
        assert_eq!(wave_order(root, 4), vec![id(apart)], "a que não divide nada também");
    }

    /// Summary continuation keeps its own wave; independent small work can
    /// use another free slot.
    #[test]
    fn summary_continuation_and_independent_small_work_use_free_slots() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        backlog_task_on(root, said, crit, "Mexer na parte grande.", &SIX_FILES);
        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![1]));
        seed_running(root, 1);
        let left = backlog_task_on(root, said, crit, "Mexer no código de dois.", &["src/b.rs"]);
        let other = backlog_task_on(root, said, crit, "Mexer no código de três.", &["src/c.rs"]);
        stopped_with(root, &[left]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![2, 3]));
        assert_eq!(wave_order(root, 2), vec![left], "summary stays attached");
        assert_eq!(wave_order(root, 3), vec![other], "independent small work uses a slot");
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
        let expected = translate("spec_events.delivery_read_missing", Locale::PtBr).replace("{wave}", "1").replace("{missing}", &code);
        assert_eq!(refused["hint"], json!(expected), "only the summary is missing: {refused}");

        let copy = log.get(open).and_then(|send| send.str_field("copy")).expect("the copy of the wave").to_string();
        let opts = ReadOpts { root: root.to_path_buf(), spec: Some("x".into()), block: format!("item-{code}"), term: None };
        read_for(&opts, None, Path::new(&copy)).unwrap_or_else(|refused| panic!("{code}: {refused}"));
        crate::commands::flow::round::finish_tasks(root, "x", 1);
        let wrote = returned_unread(root, delivery);
        assert_eq!(wrote["ok"], json!(true), "the same delivery passes once the summary is read: {wrote}");
    }

    /// Duas tarefas voltam com o mesmo resumo, e a segunda depende da
    /// primeira: a primeira sai sozinha, e a segunda só depois que a rodada
    /// assume a entrega da onda dela. O pedido de cada onda, lido pelo comando
    /// que a rodada devolve, abre com a linha do resumo.
    #[test]
    fn two_returned_tasks_leaving_in_different_waves_both_carry_the_summary() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let first = backlog_task_on(root, said, crit, "Mexer no código de um.", &["src/a.rs"]);
        let second = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Mexer no código de dois.",
            "files": [{"path": "src/b.rs"}], "depends_on": [first], "covers": [crit], "origin": said}),
        ));
        let summary = stopped_with(root, &[first, second]);
        let code = spec_now(root).codes()[&summary].clone();
        let read = translate("wave_prompt.summary.read", Locale::PtBr).replace("{code}", &code);
        let opening = read.split("{root}").next().unwrap_or_default();

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
        assert_eq!(wave_order(root, 1), vec![first], "the second waits for the first");
        let request = request_at(&out, 0);
        assert!(request.contains(opening), "the first request carries the summary: {request}");

        let took = round(root, "x", Some(&delivered(root, 1, "Saiu.", &["src/a.rs"])));
        assert_eq!(took["ok"], json!(true), "{took}");
        let out = took;
        assert_eq!(waves_in(&out, "dispatch"), vec![2], "{out}");
        assert!(waves_in(&round(root, "x", None), "dispatch").is_empty(), "already dispatched");
        assert_eq!(wave_order(root, 2), vec![second]);
        let request = request_at(&out, 0);
        assert!(request.contains(opening), "the second request carries the summary too: {request}");
    }

    /// A tarefa que outro resumo acompanha não vai de carona na onda de um
    /// resumo, mesmo dividindo arquivo com ela: sai na onda do resumo dela,
    /// na mesma rodada.
    #[test]
    fn a_task_another_summary_carries_does_not_ride_along() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let one = backlog_task_on(root, said, crit, "Mexer no código de um.", &["src/a.rs"]);
        let two = backlog_task_on(root, said, crit, "Mexer nos dois.", &["src/a.rs", "src/b.rs"]);
        stopped_in(root, 7, &[one]);
        stopped_in(root, 8, &[two]);

        let log = spec_now(root);
        assert_eq!(dispatch_backlog(root, "x", &log, &log, max_parallel(root), None).map(|(formed, _)| formed), Ok(vec![1, 2]));
        assert_eq!(wave_order(root, 1), vec![one], "only the task of its own summary");
        assert_eq!(wave_order(root, 2), vec![two], "it leaves with its own summary");
    }

    /// As tarefas prontas que gravam a mesma onda do classificador saem
    /// juntas, na posição que ele deu; a sem grupo fica para a montagem de
    /// sempre, e cada onda leva os arquivos das tarefas dela.
    #[test]
    fn the_waves_the_classifier_formed_leave_whole_in_its_order() {
        let task = |id: u64, file: &str, group: Option<(&str, u64)>| {
            let mut line = json!({ "v": 1, "id": id, "type": "task", "title": format!("t{id}"), "files": [{ "path": file }], "depends_on": [] });
            if let Some((name, position)) = group {
                line["group"] = json!({ "name": name, "position": position });
            }
            line.to_string()
        };
        let lines = [
            task(1, "web/auth/token.ts", Some(("A sessão renova o token", 2))),
            task(2, "web/cart/total.ts", None),
            task(3, "web/auth/session.ts", Some(("A sessão renova o token", 1))),
            task(4, "web/report/export.ts", Some(("A exportação grava um arquivo limpo", 1))),
        ];
        let log = mustard_core::domain::spec_events::parse_log(&lines.join("\n"));
        let population: Vec<crate::shared::dag::BacklogTask<u64>> = log
            .visible()
            .into_iter()
            .map(|event| crate::shared::dag::BacklogTask {
                id: event.id,
                depends_on: std::collections::BTreeSet::new(),
                files: super::super::queue::task_files(event),
                done: false,
                blocked: false,
                priority: false,
            })
            .collect();
        let waves = super::group_waves(&log, &[1, 2, 3, 4], &population);
        let tasks: Vec<Vec<u64>> = waves.iter().map(|(wave, _)| wave.tasks.clone()).collect();
        assert_eq!(tasks, [vec![3, 1], vec![4]]);
        assert!(waves[0].0.files.contains("web/auth/token.ts") && waves[0].0.files.contains("web/auth/session.ts"), "{:?}", waves[0]);
        assert_eq!((waves[0].1, waves[1].1), (None, None), "no summary is live in this log");
    }
}
