//! A conferência de que a onda não devolve trabalho começado como não feito.
//! Cada passo de término — o passo que aponta uma tarefa com que a onda saiu
//! — guarda a impressão da cópia da onda naquela hora, e a entrega compara a
//! cópia de agora com a do último passo de término. A tarefa dada como feita
//! também pede o passo de término dela: é na resposta dele que o agente lê o
//! tamanho da conversa e a ordem de seguir ou entregar.

use std::collections::BTreeMap;
use std::path::Path;

use mustard_core::domain::spec_events::{SpecEvent, SpecLog, COPY_STATE_FIELD};
use mustard_core::io::spec_events as store;
use mustard_core::io::tree_state::tree_state;
use serde_json::{json, Map, Value};

use super::answer::{ref_shown, RoundRefusal};
use super::commit::copy_of;
use super::read_check::opened_at;
use super::report::WaveReport;
use super::sent_tasks::sent_tasks;

/// A impressão da cópia da onda `wave` agora: o resumo do que ela tem mudado
/// em relação ao commit dela, vazio com a cópia limpa. `None` sem cópia no
/// disco, ou sem git: aí nada se afirma sobre ela.
fn copy_state_now(log: &SpecLog, wave: u64) -> Option<String> {
    let copy = copy_of(log, wave)?;
    tree_state(&|args| mustard_core::platform::git::run(&copy, args).out()).map(|state| state.diff)
}

/// O código de cada tarefa com que a onda `wave` saiu ([`sent_tasks`]), com
/// a tarefa.
fn wave_tasks<'a>(log: &'a SpecLog, wave: u64, codes: &BTreeMap<u64, String>) -> Vec<(String, &'a SpecEvent)> {
    let code_of = |task: &SpecEvent| codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
    sent_tasks(log, wave).into_iter().map(|task| (code_of(task), task)).collect()
}

/// O código da tarefa que o passo `item` aponta, pelo código ou pelo número.
fn step_task(item: Option<&Value>, codes: &BTreeMap<u64, String>) -> String {
    ref_shown(item, codes).trim().to_string()
}

/// Põe no passo `draft` da spec `spec`, do projeto `root`, a impressão da
/// cópia da onda dele, quando ele é o passo de término de uma tarefa com que
/// a onda saiu. O que vier no campo de quem grava sai sempre: a impressão é
/// da gravação. A cópia limpa, a que não se lê e o passo de um critério ficam
/// sem o campo.
pub(crate) fn mark_step_copy(root: &Path, spec: &str, draft: &mut Map<String, Value>) {
    draft.remove(COPY_STATE_FIELD);
    let Some(wave) = draft.get("wave").and_then(Value::as_u64) else { return };
    let Some(log) = store::spec_file(root, spec).ok().and_then(|path| store::read(&path).ok().flatten()) else {
        return;
    };
    let codes = log.codes();
    let item = step_task(draft.get("item"), &codes);
    if !wave_tasks(&log, wave, &codes).iter().any(|(code, _)| *code == item) {
        return;
    }
    if let Some(state) = copy_state_now(&log, wave).filter(|state| !state.is_empty()) {
        draft.insert(COPY_STATE_FIELD.into(), json!(state));
    }
}

/// Confere a entrega `report` da onda, com o pedido `sent` aberto e os campos
/// `fields` que o agente gravou, contra o que ele começou. Com tarefa
/// devolvida em `undone` e sem mudança de plano, a cópia de agora tem de ser
/// a do último passo de término desde o pedido — sem passo nenhum, a cópia
/// limpa —, e a recusa diz para concluir a tarefa começada. Depois, cada
/// tarefa da onda que a entrega não devolve, e que nenhuma outra onda levou,
/// tem de ter o passo de término dela. A cópia que não se lê não recusa nada.
pub(super) fn unfinished_work(
    log: &SpecLog,
    sent: u64,
    report: &WaveReport,
    fields: &Map<String, Value>,
) -> Result<(), RoundRefusal> {
    let wave = report.wave;
    let codes = log.codes();
    let tasks = wave_tasks(log, wave, &codes);
    let since = opened_at(log, sent);
    let steps: Vec<(String, &SpecEvent)> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "step" && e.wave() == Some(wave) && e.id > since)
        .map(|step| (step_task(step.fields.get("item"), &codes), step))
        .filter(|(item, _)| tasks.iter().any(|(code, _)| code == item))
        .collect();
    // A cópia mudada é conferida antes da tarefa sem passo: o passo gravado
    // de uma tarefa depois de começar outra não esconde o trabalho começado.
    if !report.undone.is_empty()
        && report.replan.is_none()
        && let Some(now) = copy_state_now(log, wave)
    {
        let then = steps.last().and_then(|(_, step)| step.str_field(COPY_STATE_FIELD)).unwrap_or_default();
        if now != then {
            let undone = report.undone.iter().map(|(_, code)| code.clone()).collect();
            return Err(RoundRefusal::StartedWorkUndone { wave, tasks: undone });
        }
    }
    let cited: Vec<String> = fields
        .get("undone")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| value.as_str().map_or_else(|| value.to_string(), |code| code.trim().to_string()))
        .collect();
    let missing: Vec<String> = tasks
        .iter()
        .filter(|(code, task)| task.wave() == Some(wave) && !cited.contains(code))
        .filter(|(code, _)| !steps.iter().any(|(item, _)| item == code))
        .map(|(code, _)| code.clone())
        .collect();
    if !missing.is_empty() {
        return Err(RoundRefusal::DoneWithoutStep { wave, tasks: missing });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use mustard_core::domain::spec_events::COPY_STATE_FIELD;
    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{translate, Locale};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::super::answer::RoundRefusal;
    use super::super::queue::open_sends;
    use super::super::tests::{approved_with, read_request, returned_unread, round, waves_in, write};
    use crate::commands::spec_events::write::{write_at, WriteOpts};

    /// A onda 1 com duas tarefas, a primeira em `src/a.rs` e a segunda em
    /// `src/b.rs`, despachada numa cópia e com o pedido lido. Devolve a
    /// cópia.
    fn two_task_wave(root: &Path) -> PathBuf {
        approved_with(root, "x", &[(1, &["src/a.rs", "src/b.rs"], &[])], |said| {
            let task = json!({"wave": 1, "text": "Segunda tarefa da onda 1.", "files": [{"path": "src/b.rs"}],
                "depends_on": [], "origin": said});
            assert_eq!(write(root, "x", "task", task)["ok"], json!(true));
        });
        let sent = round(root, "x", None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        read_request(root, "x", 1);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let copy = log.get(open_sends(&log)[&1]).and_then(|send| send.str_field("copy")).expect("the copy");
        PathBuf::from(copy)
    }

    /// O passo de término da tarefa `item`, gravado pelo `run write step`,
    /// como o agente o grava, com os campos a mais de `extra`.
    fn step(root: &Path, item: &str, extra: Value) -> Value {
        let mut body = json!({"wave": 1, "item": item, "text": "A tarefa ficou pronta."});
        for (key, value) in extra.as_object().into_iter().flatten() {
            body[key] = value.clone();
        }
        let wrote = write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            event_type: "step".into(),
            json: body.to_string(),
        });
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        Value::Object(log.get(wrote["id"].as_u64().unwrap()).expect("the step").fields.clone())
    }

    /// Uma linha a mais no arquivo `file` da cópia.
    fn edit(copy: &Path, file: &str, line: &str) {
        let path = copy.join(file);
        let before = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(&path, format!("{before}// {line}\n")).unwrap();
    }

    /// O arquivo `file` da cópia de volta ao commit dela.
    fn restore(copy: &Path, file: &str) {
        let out = std::process::Command::new("git").args(["checkout", "--", file]).current_dir(copy).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// As linhas do arquivo da spec `x`: a entrega recusada não escreve nada.
    fn spec_lines(root: &Path) -> usize {
        std::fs::read_to_string(store::spec_file(root, "x").unwrap()).unwrap().lines().count()
    }

    /// O passo de término de uma tarefa da onda guarda a impressão da cópia
    /// de agora, e a que quem grava mandou é trocada; com a cópia limpa, e no
    /// passo de um critério, o campo fica de fora.
    #[test]
    fn the_task_step_keeps_the_copy_state_of_the_moment() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);

        let clean = step(root, "MSTD-TASK-0001", json!({"copy_state": "escrito-a-mao"}));
        assert_eq!(clean.get(COPY_STATE_FIELD), None, "the clean copy leaves no mark: {clean}");

        edit(&copy, "src/a.rs", "a soma saiu");
        let first = step(root, "MSTD-TASK-0001", json!({"copy_state": "escrito-a-mao"}));
        let mark = first[COPY_STATE_FIELD].as_str().unwrap_or_else(|| panic!("the step keeps the copy: {first}"));
        assert_ne!(mark, "escrito-a-mao", "{first}");
        assert_eq!(step(root, "MSTD-TASK-0001", json!({}))[COPY_STATE_FIELD], json!(mark), "the same copy, the same mark");

        edit(&copy, "src/b.rs", "o total saiu");
        let second = step(root, "MSTD-TASK-0002", json!({}));
        assert!(second[COPY_STATE_FIELD].as_str().is_some_and(|other| other != mark), "a changed copy, another mark: {second}");

        let criterion = step(root, "MSTD-CRIT-0001", json!({}));
        assert_eq!(criterion.get(COPY_STATE_FIELD), None, "a criterion step is no task end: {criterion}");
    }

    /// A tarefa devolvida como não feita, com a cópia mudada depois do último
    /// passo de término, recusa a entrega nos dois idiomas, sem gravar nada; a
    /// cópia de volta à do passo deixa a mesma entrega gravar.
    #[test]
    fn work_started_after_the_last_task_step_refuses_the_give_back_until_the_copy_matches_the_step() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);
        edit(&copy, "src/a.rs", "a soma saiu");
        step(root, "MSTD-TASK-0001", json!({}));
        edit(&copy, "src/b.rs", "o total começou");
        let body = json!({"wave": 1, "text": "A soma saiu; o total ficou.", "files": ["src/a.rs"], "commit": "a soma sai",
            "undone": ["MSTD-TASK-0002"]});

        let before = spec_lines(root);
        let refused = returned_unread(root, body.clone());
        assert_eq!(refused["reason"], json!("delivery-started-work-undone"), "{refused}");
        let expected = translate("round.started_work_undone", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{tasks}", "MSTD-TASK-0002");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let english = RoundRefusal::StartedWorkUndone { wave: 1, tasks: vec!["MSTD-TASK-0002".into()] }.message(Locale::EnUs);
        assert!(english.starts_with("Wave 1 gives back MSTD-TASK-0002 as not done"), "{english}");
        assert_eq!(spec_lines(root), before, "nothing was written: {refused}");

        restore(&copy, "src/b.rs");
        let wrote = returned_unread(root, body);
        assert_eq!(wrote["ok"], json!(true), "the copy as the step left it gives the task back: {wrote}");
    }

    /// Sem passo de término nenhum, a cópia mudada já é trabalho começado, e a
    /// cópia limpa não é: as duas tarefas devolvidas recusam com a cópia
    /// mudada e gravam com ela limpa.
    #[test]
    fn a_changed_copy_without_any_task_step_refuses_the_give_back() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);
        edit(&copy, "src/a.rs", "a soma começou");
        let body = json!({"wave": 1, "text": "Nada saiu.", "undone": ["MSTD-TASK-0001", "MSTD-TASK-0002"]});

        let refused = returned_unread(root, body.clone());
        assert_eq!(refused["reason"], json!("delivery-started-work-undone"), "{refused}");
        assert!(refused["hint"].as_str().is_some_and(|hint| hint.contains("MSTD-TASK-0001, MSTD-TASK-0002")), "{refused}");

        restore(&copy, "src/a.rs");
        let wrote = returned_unread(root, body);
        assert_eq!(wrote["ok"], json!(true), "the clean copy gives both tasks back: {wrote}");
    }

    /// A mudança de plano devolve a tarefa começada sem a conferência da
    /// cópia: o agente diz que o plano não funciona, e a tarefa volta.
    #[test]
    fn a_plan_change_gives_back_started_work() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);
        edit(&copy, "src/a.rs", "a soma saiu");
        step(root, "MSTD-TASK-0001", json!({}));
        edit(&copy, "src/b.rs", "o total não fecha");
        let body = json!({"wave": 1, "text": "O total não fecha com o plano.", "files": ["src/a.rs"], "commit": "a soma sai",
            "replan": "O total pede a tabela nova antes.", "undone": ["MSTD-TASK-0002"]});
        let wrote = returned_unread(root, body);
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
    }

    /// A tarefa dada como feita sem o passo de término dela recusa a entrega
    /// nos dois idiomas, citando só ela: o passo de um critério não conta, e a
    /// tarefa devolvida não pede passo. Com o passo gravado, a mesma entrega
    /// grava.
    #[test]
    fn a_task_given_as_done_without_its_step_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);
        edit(&copy, "src/a.rs", "a soma saiu");
        step(root, "MSTD-TASK-0001", json!({}));
        edit(&copy, "src/b.rs", "o total saiu");
        step(root, "MSTD-CRIT-0001", json!({}));
        let body = json!({"wave": 1, "text": "A soma e o total saíram.", "files": ["src/a.rs", "src/b.rs"],
            "commit": "a soma e o total saem"});

        let before = spec_lines(root);
        let refused = returned_unread(root, body.clone());
        assert_eq!(refused["reason"], json!("delivery-done-without-step"), "{refused}");
        let expected = translate("round.done_without_step", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{tasks}", "MSTD-TASK-0002");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        let english = RoundRefusal::DoneWithoutStep { wave: 1, tasks: vec!["MSTD-TASK-0002".into()] }.message(Locale::EnUs);
        assert!(english.starts_with("Wave 1 gives tasks MSTD-TASK-0002 as done without"), "{english}");
        assert_eq!(spec_lines(root), before, "nothing was written: {refused}");

        let given_back = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai",
            "undone": ["MSTD-TASK-0002"]});
        assert_eq!(returned_unread(root, given_back)["reason"], json!("delivery-started-work-undone"));

        step(root, "MSTD-TASK-0002", json!({}));
        let wrote = returned_unread(root, body);
        assert_eq!(wrote["ok"], json!(true), "with each task's step the delivery goes in: {wrote}");
    }
}
