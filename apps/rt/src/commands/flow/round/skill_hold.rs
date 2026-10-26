//! A espera da avaliação de uma skill do Mustard: a tarefa de atualizar uma
//! skill, a que o registro marca ([`crate::shared::skill_cycle`]), não sai
//! enquanto alguma tarefa aberta da obra, no backlog ou numa onda que roda,
//! cita em `files` um arquivo que a skill cita. Assim a avaliação sai uma vez,
//! depois de todas as mudanças da obra nesses arquivos, que se somam nela. A
//! tarefa do skill-creator escreve skill, e não os arquivos que cita: não
//! segura avaliação nenhuma, e duas avaliações de skills que citam o mesmo
//! arquivo não se esperam. A tarefa que depende da avaliação, direta ou por
//! outra, só sai depois dela e também não a segura.
//!
//! A espera vale nas duas saídas. Na montagem do backlog
//! ([`waiting_for_tasks`]), a avaliação só entra na onda da tarefa de que
//! depende, direta ou por outra, e a tarefa que depende dela e cita um desses
//! arquivos não vai de carona na onda dela. Na saída de toda onda, planejada
//! ou formada ([`waves_waiting`]), a onda que leva a avaliação não sai
//! enquanto alguma tarefa aberta fora dela cita um desses arquivos; a tarefa
//! que só sai depois da onda — a que depende de uma tarefa dela, ou a de uma
//! onda que depende dela — não a segura.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecEvent, SpecLog};
use mustard_core::io::mustard_skills as registry;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

use super::queue::{backlog_population, task_files, tasks_not_delivered, waves_done, waves_in_progress};
use crate::shared::dag::BacklogTask;
use crate::shared::skill_cycle::cites;

/// O que as avaliações de skill seguram na montagem do backlog.
#[derive(Debug, Default)]
pub(super) struct Evaluations {
    /// Cada avaliação que espera tarefas abertas, com as tarefas que ela
    /// espera, em ordem de número.
    pub(super) held: BTreeMap<u64, Vec<u64>>,
    /// As tarefas que dependem de uma avaliação, direta ou por outra, e citam
    /// um arquivo que a skill cita: nesta rodada não entram na onda dela.
    pub(super) after: BTreeSet<u64>,
}

/// As avaliações de skill que o registro `skills` marca na spec `spec`, lida
/// em `log`: a versão vigente de cada uma, com os arquivos que a skill cita.
fn marked_in<'a>(skills: &'a registry::Registry, spec: &str, log: &'a SpecLog) -> Vec<(&'a SpecEvent, &'a [String])> {
    skills
        .skills
        .values()
        .filter_map(|skill| {
            let mark = skill.update_task.as_ref().filter(|mark| mark.spec == spec)?;
            Some((log.current(mark.task?)?, skill.files.as_slice()))
        })
        .collect()
}

/// As tarefas de `open`, fora as de `apart`, que citam em `files` um arquivo
/// de `cited`, em ordem de número. A do skill-creator escreve skill, e não os
/// arquivos que ela cita: não entra.
fn holders(log: &SpecLog, open: &BTreeSet<u64>, cited: &[String], apart: &BTreeSet<u64>) -> Vec<u64> {
    open.iter()
        .filter(|id| !apart.contains(id))
        .filter_map(|id| log.get(*id))
        .filter(|task| task.str_field("skill") != Some(mustard_core::io::skill_creator::NAME))
        .filter(|task| task_files(task).iter().any(|file| cited.iter().any(|cited| cites(cited, file))))
        .map(|task| task.id)
        .collect()
}

/// As avaliações de skill entre `candidates`, na spec `spec` do projeto
/// `root` lida em `log`, que esperam tarefas abertas, e as tarefas que não
/// vão de carona na onda delas ([`Evaluations`]). Não segura a avaliação a
/// tarefa de que ela depende, direta ou por outra: ela só entra na onda
/// dessa tarefa, ou sai depois dela. `population` traz as dependências de
/// cada tarefa, pela versão vigente.
pub(super) fn waiting_for_tasks(root: &Path, spec: &str, log: &SpecLog, candidates: &[u64], population: &[BacklogTask<u64>]) -> Evaluations {
    let skills = registry::read(root);
    let marked: Vec<(u64, &[String])> =
        marked_in(&skills, spec, log).into_iter().filter(|(task, _)| candidates.contains(&task.id)).map(|(task, cited)| (task.id, cited)).collect();
    let mut out = Evaluations::default();
    if marked.is_empty() {
        return out;
    }
    let open = tasks_not_delivered(log, &BTreeSet::new());
    for (evaluation, cited) in marked {
        let after = dependents(BTreeSet::from([evaluation]), population);
        let apart: BTreeSet<u64> = after.union(&dependencies(evaluation, population)).copied().collect();
        let held = holders(log, &open, cited, &apart);
        if !held.is_empty() {
            out.held.insert(evaluation, held);
        }
        out.after.extend(holders(log, &after, cited, &BTreeSet::from([evaluation])));
    }
    out
}

/// As ondas do plano, ainda por entregar, que levam uma avaliação de skill
/// marcada no registro do projeto `root`, na spec `spec` lida em `log`, e
/// esperam tarefas abertas fora delas que citam um arquivo que a skill cita:
/// cada uma com as tarefas que ela espera, em ordem de número. A tarefa que
/// só sai depois da onda ([`after_wave`]) não a segura. É a espera que a
/// escolha das ondas que saem ([`super::queue::next_waves`]) aplica.
pub(crate) fn waves_waiting(root: &Path, spec: &str, log: &SpecLog) -> BTreeMap<u64, Vec<u64>> {
    let skills = registry::read(root);
    let planned = log.planned_waves();
    let done = waves_done(log, &waves_in_progress(log));
    let marked: Vec<(u64, &[String])> = marked_in(&skills, spec, log)
        .into_iter()
        .filter_map(|(task, cited)| Some((task.wave().filter(|wave| planned.contains(wave) && !done.contains(wave))?, cited)))
        .collect();
    let mut out: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    if marked.is_empty() {
        return out;
    }
    let open = tasks_not_delivered(log, &BTreeSet::new());
    let population = backlog_population(log, &done);
    for (wave, cited) in marked {
        let held = holders(log, &open, cited, &after_wave(log, wave, &population));
        if !held.is_empty() {
            let waits = out.entry(wave).or_default();
            waits.extend(held);
            waits.sort_unstable();
            waits.dedup();
        }
    }
    out
}

/// O aviso e a linha do próximo passo de cada onda de `kept` — pronta e com
/// vaga, segurada nesta rodada pela avaliação de skill que leva —, com as
/// tarefas que ela espera (`evaluating`) pelo código lido em `log`: os
/// avisos vão a `warnings`, e as linhas voltam, no idioma `lang`.
pub(super) fn waiting_notes(log: &SpecLog, evaluating: &BTreeMap<u64, Vec<u64>>, kept: &[u64], warnings: &mut Vec<Value>, lang: Locale) -> Vec<String> {
    let codes = log.codes();
    let code = |id: &u64| codes.get(id).cloned().unwrap_or_else(|| id.to_string());
    let mut lines = Vec::new();
    for wave in kept {
        let tasks: Vec<String> = evaluating.get(wave).into_iter().flatten().map(code).collect();
        let hint = translate("round.skill_waits_open_tasks", lang).replace("{wave}", &wave.to_string()).replace("{tasks}", &tasks.join(", "));
        warnings.push(json!({ "reason": "skill-waits-open-tasks", "wave": wave, "tasks": tasks, "hint": hint }));
        lines.push(hint);
    }
    lines
}

/// As tarefas da onda `wave` da spec lida em `log` e as que só saem depois
/// dela: as das ondas que dependem dela, direta ou por outra onda, e as que
/// dependem de uma dessas, direta ou por outra tarefa, em `population`.
fn after_wave(log: &SpecLog, wave: u64, population: &[BacklogTask<u64>]) -> BTreeSet<u64> {
    let mut depends: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for event in log.block(BlockQuery::Block(Block::Waves)).into_iter().filter(|e| e.event_type == "wave") {
        if let Some(n) = event.wave() {
            depends.insert(n, event.ints("depends_on"));
        }
    }
    let mut waves = BTreeSet::from([wave]);
    loop {
        let next: Vec<u64> = depends.iter().filter(|(n, on)| !waves.contains(*n) && on.iter().any(|on| waves.contains(on))).map(|(n, _)| *n).collect();
        if next.is_empty() {
            break;
        }
        waves.extend(next);
    }
    let start = population.iter().filter(|task| log.get(task.id).and_then(SpecEvent::wave).is_some_and(|n| waves.contains(&n))).map(|task| task.id).collect();
    dependents(start, population)
}

/// As tarefas de `start` e as que dependem de uma delas, direta ou por
/// outra, em `population`.
fn dependents(start: BTreeSet<u64>, population: &[BacklogTask<u64>]) -> BTreeSet<u64> {
    let mut reached = start;
    loop {
        let next: Vec<u64> =
            population.iter().filter(|task| !reached.contains(&task.id) && task.depends_on.iter().any(|on| reached.contains(on))).map(|task| task.id).collect();
        if next.is_empty() {
            return reached;
        }
        reached.extend(next);
    }
}

/// As tarefas de que a tarefa `id` depende, direta ou por outra, em
/// `population`.
fn dependencies(id: u64, population: &[BacklogTask<u64>]) -> BTreeSet<u64> {
    let mut reached = BTreeSet::new();
    let mut stack = vec![id];
    while let Some(at) = stack.pop() {
        for on in population.iter().filter(|task| task.id == at).flat_map(|task| task.depends_on.iter()) {
            if reached.insert(*on) {
                stack.push(*on);
            }
        }
    }
    reached
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::SpecEvent;
    use mustard_core::io::failures::SkillTask;
    use mustard_core::io::mustard_skills::{self as registry, MustardSkill};
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::super::queue::{backlog_population, backlog_project, backlog_task_on, spec_now, waves_done, waves_in_progress};
    use super::super::tests::{approved, delivered, id_of, round, warning_of, waves_in, write};
    use super::*;

    /// Marks the task `id` of spec `x` in `root` as the evaluation of the
    /// skill `name`, which cites `src/a.rs`.
    fn mark(root: &Path, name: &str, id: u64) {
        registry::change(root, |skills| {
            let skill = MustardSkill {
                state: registry::ACTIVE.into(),
                made: true,
                files: vec!["src/a.rs".into()],
                update_task: Some(SkillTask { spec: "x".into(), task: Some(id) }),
                ..MustardSkill::default()
            };
            skills.skills.insert(name.into(), skill);
        })
        .unwrap();
    }

    /// The evaluation of the skill `name`, citing `src/a.rs`, in the backlog
    /// of spec `x` in `root` and marked in the registry, depending on
    /// `depends_on`. Returns its number.
    fn evaluation(root: &Path, (said, crit): (u64, u64), name: &str, depends_on: &[u64]) -> u64 {
        let id = id_of(&write(
            root,
            "x",
            "task",
            json!({"title": format!("Atualizar a skill {name}"), "text": format!("A skill {name} pode ter ficado desatualizada."),
            "skill": mustard_core::io::skill_creator::NAME, "files": [{"path": "src/a.rs"}], "depends_on": depends_on,
            "covers": [crit], "origin": said}),
        ));
        mark(root, name, id);
        id
    }

    /// Wave 1 changes `src/a.rs`, wave 2 carries the evaluation of a skill
    /// that cites it, and wave 3, which depends on wave 2, changes the same
    /// file. Wave 2 does not go out with wave 1: it waits for the task of
    /// wave 1 only, named in the warning, since the task of wave 3 can only
    /// go after it. Once wave 1 delivers, wave 2 goes out.
    #[test]
    fn a_skill_evaluation_in_a_planned_wave_waits_for_the_open_tasks_on_its_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/a.rs"], &[]), (3, &["src/a.rs"], &[2])]);
        let log = spec_now(root);
        let task_of = |wave: u64| log.visible().into_iter().find(|e| e.event_type == "task" && e.wave() == Some(wave)).map(|task| task.id).unwrap();
        let (first, evaluation) = (task_of(1), task_of(2));
        mark(root, "falta-teste", evaluation);

        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        let warning = warning_of(&sent, "skill-waits-open-tasks");
        assert_eq!(warning["wave"], json!(2), "{sent}");
        assert_eq!(warning["tasks"], json!([log.codes()[&first]]), "only the task of wave 1: {sent}");

        let back = round(root, "x", Some(&delivered(root, 1, "A onda saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&back, "dispatch"), vec![2], "{back}");
    }

    /// A task on `src/a.rs` and the evaluation of a skill that cites it are
    /// both in the backlog: the task goes out alone, and the evaluation stays
    /// with the reason and the task it waits for. When the task delivers, the
    /// evaluation goes out.
    #[test]
    fn a_skill_evaluation_waits_for_the_open_tasks_on_its_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let code = backlog_task_on(root, said, crit, "Mexer no código de um.", &["src/a.rs"]);
        let evaluation = evaluation(root, (said, crit), "falta-teste", &[]);

        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        let log = spec_now(root);
        assert_eq!(log.current(code).and_then(SpecEvent::wave), Some(1), "the task goes out");
        assert_eq!(log.current(evaluation).and_then(SpecEvent::wave), None, "the evaluation stays in the backlog");
        let codes = log.codes();
        let held: Vec<&Value> = sent["held"].as_array().into_iter().flatten().filter(|held| held["reason"] == "skill-waits-open-tasks").collect();
        assert_eq!(held.len(), 1, "{sent}");
        assert_eq!(held[0]["task"], json!(codes[&evaluation]), "{sent}");
        assert!(held[0]["detail"].as_str().unwrap_or_default().contains(&codes[&code]), "the task it waits for: {sent}");

        let back = round(root, "x", Some(&delivered(root, 1, "A onda saiu.", &["src/a.rs"])));
        assert_eq!(waves_in(&back, "dispatch"), vec![2], "{back}");
        assert_eq!(spec_now(root).current(evaluation).and_then(SpecEvent::wave), Some(2), "the evaluation goes out after the task delivers");
    }

    /// Two evaluations of skills that cite the same file, and a task on that
    /// file that depends on the first: neither evaluation holds the other, and
    /// the task that waits for the first does not hold it, only the second.
    #[test]
    fn an_evaluation_waits_for_no_other_evaluation_nor_for_a_task_that_waits_for_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let project = backlog_project(root);
        let first = evaluation(root, project, "falta-teste", &[]);
        let second = evaluation(root, project, "soma-centavos", &[]);
        let after = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Mexer no código de um depois da avaliação.", "files": [{"path": "src/a.rs"}], "depends_on": [first],
            "covers": [project.1], "origin": project.0}),
        ));
        let log = spec_now(root);
        let population = backlog_population(&log, &waves_done(&log, &waves_in_progress(&log)));
        assert!(population.iter().any(|task| task.id == after && task.depends_on.contains(&first)), "{population:?}");
        let waiting = waiting_for_tasks(root, "x", &log, &[first, second], &population);
        assert_eq!(waiting.held, BTreeMap::from([(second, vec![after])]), "the second waits only for the task, and the first for nothing");
    }

    /// In the backlog, a task on `src/a.rs`, the evaluation of a skill that
    /// cites it, which depends on that task, and a task on the same file that
    /// depends on the evaluation. The evaluation goes out in the wave of the
    /// task it depends on, and the task after it does not ride along: it goes
    /// out once that wave delivers.
    #[test]
    fn an_evaluation_goes_with_the_task_it_depends_on_and_without_the_tasks_after_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, crit) = backlog_project(root);
        let code = backlog_task_on(root, said, crit, "Mexer no código de um.", &["src/a.rs"]);
        let evaluation = evaluation(root, (said, crit), "falta-teste", &[code]);
        let after = id_of(&write(
            root,
            "x",
            "task",
            json!({"text": "Mexer no código de um depois da avaliação.", "files": [{"path": "src/a.rs"}], "depends_on": [evaluation],
            "covers": [crit], "origin": said}),
        ));

        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        let log = spec_now(root);
        let wave = |id: u64| log.current(id).and_then(SpecEvent::wave);
        assert_eq!((wave(code), wave(evaluation), wave(after)), (Some(1), Some(1), None), "{sent}");

        let back = round(root, "x", Some(&delivered(root, 1, "A onda saiu.", &["src/a.rs"])));
        assert_eq!(spec_now(root).current(after).and_then(SpecEvent::wave), Some(2), "{back}");
    }
}
