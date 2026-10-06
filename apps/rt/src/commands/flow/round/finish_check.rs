//! A conferência de que a onda não devolve trabalho começado como não feito.
//! Cada passo de término — o passo que aponta uma tarefa com que a onda saiu
//! — guarda a impressão da cópia da onda naquela hora, a da cópia inteira e
//! a de cada arquivo das tarefas, e a entrega compara a cópia de agora com a
//! do último passo de término. A tarefa dada como feita também pede o passo
//! de término dela: é na resposta dele que o agente lê o tamanho da conversa
//! e a ordem de seguir ou entregar.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::spec_events::{Hidden, SpecEvent, SpecLog, COPY_FILES_FIELD, COPY_STATE_FIELD};
use mustard_core::io::sha256::Sha256;
use mustard_core::io::spec_events as store;
use mustard_core::io::tree_state::tree_state;
use mustard_core::platform::git;
use serde_json::{json, Map, Value};

use super::answer::{ref_shown, RoundRefusal};
use super::commit::{changed_paths, copy_of};
use super::queue::task_files;
use super::read_check::opened_at;
use super::report::WaveReport;
use super::sent_tasks::sent_tasks;

/// Quantos caracteres do SHA-256 do conteúdo a impressão de um arquivo guarda.
const FILE_DIGEST_LEN: usize = 12;

/// A impressão da cópia da onda `wave` agora: o resumo do que ela tem mudado
/// em relação ao commit dela, vazio com a cópia limpa. `None` sem cópia no
/// disco, ou sem git: aí nada se afirma sobre ela.
fn copy_state_now(log: &SpecLog, wave: u64) -> Option<String> {
    let copy = copy_of(log, wave)?;
    tree_state(&|args| git::run(&copy, args).out()).map(|state| state.diff)
}

/// A impressão por arquivo da cópia da onda `wave` agora, só dos arquivos que
/// as tarefas `tasks` listam: cada um que a cópia tem mudado em relação ao
/// commit dela, com o começo do SHA-256 do conteúdo, vazio no arquivo
/// apagado. Vazia com esses arquivos como no commit; `None` sem cópia no
/// disco, ou sem git.
fn copy_files_now(log: &SpecLog, wave: u64, tasks: &[(String, &SpecEvent)]) -> Option<BTreeMap<String, String>> {
    let copy = copy_of(log, wave)?;
    let status = git::run(&copy, &["status", "--porcelain", "-z", "--untracked-files=all", "--ignore-submodules=all"]);
    if !status.ok {
        return None;
    }
    let listed: BTreeSet<String> = tasks.iter().flat_map(|(_, task)| task_files(task)).collect();
    let digest = |path: &str| {
        let Ok(body) = std::fs::read(copy.join(path)) else { return String::new() };
        let mut hasher = Sha256::new();
        hasher.update(&body);
        hasher.hex_digest()[..FILE_DIGEST_LEN].to_string()
    };
    let changed = changed_paths(&status.stdout).into_iter().filter(|path| listed.contains(path));
    Some(changed.map(|path| (path.clone(), digest(&path))).collect())
}

/// As tarefas de `again` — as que a entrega devolve de novo, depois de uma
/// entrega conferida que já as devolveu — que têm arquivo só delas mudado
/// desde o passo de término `last` (sem passo, desde o commit da cópia).
/// Arquivo só dela é o que ela lista e nenhuma tarefa feita da onda — fora de
/// `undone` — lista: o conserto mexe nos arquivos das tarefas feitas, e o
/// arquivo que uma delas também lista fica com o conserto. A cópia que não se
/// lê não aponta nada.
fn started_again(
    log: &SpecLog,
    wave: u64,
    tasks: &[(String, &SpecEvent)],
    undone: &[String],
    again: &[String],
    last: Option<&SpecEvent>,
) -> Vec<String> {
    if again.is_empty() {
        return Vec::new();
    }
    let Some(now) = copy_files_now(log, wave, tasks) else { return Vec::new() };
    let then = last.and_then(|step| step.fields.get(COPY_FILES_FIELD)).and_then(Value::as_object);
    let changed = |path: &String| now.get(path).map(String::as_str) != then.and_then(|then| then.get(path)?.as_str());
    let done: BTreeSet<String> = tasks
        .iter()
        .filter(|(code, task)| task.wave() == Some(wave) && !undone.contains(code))
        .flat_map(|(_, task)| task_files(task))
        .collect();
    tasks
        .iter()
        .filter(|(code, task)| again.contains(code) && task_files(task).iter().any(|f| !done.contains(f) && changed(f)))
        .map(|(code, _)| code.clone())
        .collect()
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

/// Põe no passo `draft` da spec `spec`, do projeto `root`, as impressões da
/// cópia da onda dele — a da cópia inteira e a de cada arquivo das tarefas
/// —, quando ele é o passo de término de uma tarefa com que a onda saiu. O
/// que vier nos campos de quem grava sai sempre: a impressão é da gravação.
/// A cópia limpa, a que não se lê e o passo de um critério ficam sem os
/// campos; a impressão por arquivo fica de fora também quando nenhum arquivo
/// das tarefas mudou.
pub(crate) fn mark_step_copy(root: &Path, spec: &str, draft: &mut Map<String, Value>) {
    draft.remove(COPY_STATE_FIELD);
    draft.remove(COPY_FILES_FIELD);
    let Some(wave) = draft.get("wave").and_then(Value::as_u64) else { return };
    let Some(log) = store::spec_file(root, spec).ok().and_then(|path| store::read(&path).ok().flatten()) else {
        return;
    };
    let codes = log.codes();
    let item = step_task(draft.get("item"), &codes);
    let tasks = wave_tasks(&log, wave, &codes);
    if !tasks.iter().any(|(code, _)| *code == item) {
        return;
    }
    if let Some(state) = copy_state_now(&log, wave).filter(|state| !state.is_empty()) {
        draft.insert(COPY_STATE_FIELD.into(), json!(state));
    }
    if let Some(files) = copy_files_now(&log, wave, &tasks).filter(|files| !files.is_empty()) {
        draft.insert(COPY_FILES_FIELD.into(), json!(files));
    }
}

/// Os códigos que os campos `fields` de uma entrega citam em `undone`, como o
/// agente os escreveu, sem as bordas.
fn undone_cited(fields: &Map<String, Value>) -> Vec<String> {
    let cited = fields.get("undone").and_then(Value::as_array).into_iter().flatten();
    cited.map(|value| value.as_str().map_or_else(|| value.to_string(), |code| code.trim().to_string())).collect()
}

/// As tarefas que uma entrega anterior da onda `wave`, gravada depois da
/// posição `since`, já devolveu sem mudar o plano: essa entrega passou pela
/// conferência da cópia com elas devolvidas. A entrega com mudança de plano
/// não conta, porque devolve a tarefa começada sem a conferência.
fn given_back_before(log: &SpecLog, wave: u64, since: u64) -> BTreeSet<String> {
    let hidden = log.hidden();
    let replanned = |e: &SpecEvent| e.str_field("replan").is_some_and(|change| !change.trim().is_empty());
    log.events
        .iter()
        .filter(|e| e.event_type == "delivered" && e.wave() == Some(wave) && e.id > since && e.returned())
        .filter(|e| hidden.get(&e.id) == Some(&Hidden::Returned) && !replanned(e))
        .flat_map(|e| undone_cited(&e.fields))
        .collect()
}

/// Confere a entrega `report` da onda, com o pedido `sent` aberto e os campos
/// `fields` que o agente gravou, contra o que ele começou. Com tarefa
/// devolvida em `undone` e sem mudança de plano, a cópia de agora tem de ser
/// a do último passo de término desde o pedido — sem passo nenhum, a cópia
/// limpa —, e a recusa diz para concluir a tarefa começada. A entrega que
/// conserta uma volta recusada não confere a cópia inteira pelas tarefas que
/// uma entrega anterior desde o pedido já devolveu ([`given_back_before`]): a
/// cópia mudou pelo conserto, que é parte da tarefa feita. Delas, confere só
/// os arquivos de cada uma, contra a impressão por arquivo do mesmo passo
/// ([`started_again`]), e o arquivo só dela mudado recusa como trabalho
/// começado. A tarefa devolvida pela primeira vez é conferida como sempre.
/// Depois, cada tarefa da onda que a entrega não devolve, e que nenhuma outra
/// onda levou, tem de ter o passo de término dela. A cópia que não se lê não
/// recusa nada.
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
    let last = steps.last().map(|(_, step)| *step);
    // A cópia mudada é conferida antes da tarefa sem passo: o passo gravado
    // de uma tarefa depois de começar outra não esconde o trabalho começado.
    let given_back = given_back_before(log, wave, since);
    let undone: Vec<String> = report.undone.iter().map(|(_, code)| code.clone()).collect();
    let (again, fresh): (Vec<String>, Vec<String>) = undone.iter().cloned().partition(|code| given_back.contains(code));
    if !fresh.is_empty()
        && report.replan.is_none()
        && let Some(now) = copy_state_now(log, wave)
    {
        let then = last.and_then(|step| step.str_field(COPY_STATE_FIELD)).unwrap_or_default();
        if now != then {
            return Err(RoundRefusal::StartedWorkUndone { wave, tasks: fresh });
        }
    }
    if report.replan.is_none() {
        let started = started_again(log, wave, &tasks, &undone, &again, last);
        if !started.is_empty() {
            return Err(RoundRefusal::StartedWorkUndone { wave, tasks: started });
        }
    }
    let cited = undone_cited(fields);
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

    use mustard_core::domain::spec_events::{COPY_FILES_FIELD, COPY_STATE_FIELD};
    use mustard_core::io::spec_events as store;
    use mustard_core::platform::i18n::{translate, Locale};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::super::answer::RoundRefusal;
    use super::super::queue::open_sends;
    use super::super::tests::{approved_with, read_request, returned_unread, round, waves_in, write};
    use crate::commands::spec_events::write::{write_at, WriteOpts};

    /// A onda 1 com duas tarefas, a primeira em `src/a.rs` e `src/b.rs` e a
    /// segunda em `src/b.rs`, despachada numa cópia e com o pedido lido.
    /// Devolve a cópia.
    fn two_task_wave(root: &Path) -> PathBuf {
        wave_of(root, &["src/a.rs", "src/b.rs"], &["src/b.rs"])
    }

    /// A onda 1 com duas tarefas, a primeira listando os arquivos `first`,
    /// que o commit traz, e a segunda os arquivos `second`, despachada numa
    /// cópia e com o pedido lido. Devolve a cópia.
    fn wave_of(root: &Path, first: &[&str], second: &[&str]) -> PathBuf {
        approved_with(root, "x", &[(1, first, &[])], |said| {
            let files: Vec<Value> = second.iter().map(|path| json!({"path": path})).collect();
            let task = json!({"wave": 1, "text": "Segunda tarefa da onda 1.", "files": files, "depends_on": [],
                "origin": said});
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
    /// de agora e a de cada arquivo que as tarefas listam, e as que quem grava
    /// mandou são trocadas; com a cópia limpa, e no passo de um critério, os
    /// campos ficam de fora. O arquivo que nenhuma tarefa lista muda a
    /// impressão da cópia, mas não entra na impressão por arquivo.
    #[test]
    fn the_task_step_keeps_the_copy_state_of_the_moment() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);
        let by_hand = json!({"copy_state": "escrito-a-mao", "copy_files": {"src/a.rs": "escrito-a-mao"}});

        let clean = step(root, "MSTD-TASK-0001", by_hand.clone());
        assert_eq!((clean.get(COPY_STATE_FIELD), clean.get(COPY_FILES_FIELD)), (None, None), "the clean copy leaves no mark: {clean}");

        edit(&copy, "src/a.rs", "a soma saiu");
        let first = step(root, "MSTD-TASK-0001", by_hand);
        let mark = first[COPY_STATE_FIELD].as_str().unwrap_or_else(|| panic!("the step keeps the copy: {first}"));
        assert_ne!(mark, "escrito-a-mao", "{first}");
        let files = first[COPY_FILES_FIELD].as_object().unwrap_or_else(|| panic!("the step keeps each file: {first}"));
        let sum = files["src/a.rs"].as_str().unwrap_or_default();
        assert!(files.len() == 1 && sum.len() == 12 && sum != "escrito-a-mao", "{first}");
        assert_eq!(step(root, "MSTD-TASK-0001", json!({}))[COPY_STATE_FIELD], json!(mark), "the same copy, the same mark");

        edit(&copy, "src/b.rs", "o total saiu");
        edit(&copy, "src/fora.rs", "o rascunho de fora");
        let second = step(root, "MSTD-TASK-0002", json!({}));
        assert!(second[COPY_STATE_FIELD].as_str().is_some_and(|other| other != mark), "a changed copy, another mark: {second}");
        let files = second[COPY_FILES_FIELD].as_object().unwrap_or_else(|| panic!("the step keeps each file: {second}"));
        assert_eq!(files.keys().collect::<Vec<_>>(), ["src/a.rs", "src/b.rs"], "only the files of the tasks: {second}");
        assert_eq!(files["src/a.rs"], json!(sum), "the same file, the same mark: {second}");

        let criterion = step(root, "MSTD-CRIT-0001", json!({}));
        assert_eq!((criterion.get(COPY_STATE_FIELD), criterion.get(COPY_FILES_FIELD)), (None, None), "a criterion step is no task end: {criterion}");
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

    /// A onda entrega a primeira tarefa e devolve a segunda; a rodada recusa
    /// a volta, e o agente conserta o arquivo da primeira e entrega de novo,
    /// com a mesma tarefa devolvida e sem passo novo. O conserto é parte da
    /// tarefa feita: a entrega grava, e um segundo conserto também.
    #[test]
    fn the_fix_of_a_refused_return_gives_back_the_same_task_without_a_new_step() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);
        edit(&copy, "src/a.rs", "a soma saiu");
        step(root, "MSTD-TASK-0001", json!({}));
        let body = json!({"wave": 1, "text": "A soma saiu; o total ficou.", "files": ["src/a.rs"], "commit": "a soma sai",
            "undone": ["MSTD-TASK-0002"]});
        assert_eq!(returned_unread(root, body.clone())["ok"], json!(true));

        for fix in ["a soma conserta o arredondamento", "a soma conserta o sinal"] {
            edit(&copy, "src/a.rs", fix);
            let fixed = returned_unread(root, body.clone());
            assert_eq!(fixed["ok"], json!(true), "{fix}: {fixed}");
        }
    }

    /// A primeira tarefa lista `src/a.rs`; a segunda, devolvida, lista o
    /// mesmo arquivo e `src/b.rs`. A volta que devolve a segunda grava, e o
    /// conserto seguinte muda só `src/a.rs`, com o que pode ser trabalho
    /// começado na tarefa devolvida: o arquivo que a tarefa feita também
    /// lista fica com o conserto, e a entrega grava.
    #[test]
    fn a_fix_with_a_new_change_only_in_a_file_the_done_task_also_lists_goes_in() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = wave_of(root, &["src/a.rs"], &["src/a.rs", "src/b.rs"]);
        edit(&copy, "src/a.rs", "a soma saiu");
        step(root, "MSTD-TASK-0001", json!({}));
        let body = json!({"wave": 1, "text": "A soma saiu; o total ficou.", "files": ["src/a.rs"], "commit": "a soma sai",
            "undone": ["MSTD-TASK-0002"]});
        assert_eq!(returned_unread(root, body.clone())["ok"], json!(true));

        edit(&copy, "src/a.rs", "o total começou");
        let fixed = returned_unread(root, body);
        assert_eq!(fixed["ok"], json!(true), "the file the done task also lists stays with the fix: {fixed}");
    }

    /// A primeira tarefa lista `src/a.rs`; a segunda, devolvida, lista o
    /// mesmo arquivo e `src/b.rs`, que a primeira também mudou antes do passo
    /// dela. Depois da volta que devolve a segunda, a mudança nova em
    /// `src/b.rs`, só da tarefa devolvida, é trabalho começado nela e recusa
    /// o conserto citando só ela, sem gravar nada; com `src/b.rs` de volta ao
    /// que o passo guardou, a mesma entrega grava.
    #[test]
    fn work_started_on_a_given_back_task_during_the_fix_refuses_the_fix() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = wave_of(root, &["src/a.rs"], &["src/a.rs", "src/b.rs"]);
        edit(&copy, "src/a.rs", "a soma saiu");
        edit(&copy, "src/b.rs", "a soma pede o total");
        step(root, "MSTD-TASK-0001", json!({}));
        let body = json!({"wave": 1, "text": "A soma saiu; o total ficou.", "files": ["src/a.rs", "src/b.rs"],
            "commit": "a soma sai", "undone": ["MSTD-TASK-0002"]});
        assert_eq!(returned_unread(root, body.clone())["ok"], json!(true));

        let stepped = std::fs::read_to_string(copy.join("src/b.rs")).unwrap();
        edit(&copy, "src/b.rs", "o total começou");
        let before = spec_lines(root);
        let refused = returned_unread(root, body.clone());
        assert_eq!(refused["reason"], json!("delivery-started-work-undone"), "{refused}");
        let expected = translate("round.started_work_undone", Locale::PtBr).replace("{wave}", "1").replace("{tasks}", "MSTD-TASK-0002");
        assert_eq!(refused["hint"], json!(expected), "{refused}");
        assert_eq!(spec_lines(root), before, "nothing was written: {refused}");

        std::fs::write(copy.join("src/b.rs"), stepped).unwrap();
        let wrote = returned_unread(root, body);
        assert_eq!(wrote["ok"], json!(true), "the given-back file as the step left it goes in: {wrote}");
    }

    /// O conserto só dispensa a conferência das tarefas que uma entrega
    /// anterior devolveu passando por ela. A tarefa dada como feita antes e
    /// devolvida agora, com a cópia mudada, recusa citando só ela. E a
    /// tarefa devolvida antes por uma mudança de plano, que pulou a
    /// conferência, recusa na entrega seguinte sem a mudança, com o trabalho
    /// começado na cópia.
    #[test]
    fn only_a_task_given_back_by_a_checked_delivery_skips_the_copy_check_of_the_fix() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);
        edit(&copy, "src/a.rs", "a soma saiu");
        step(root, "MSTD-TASK-0001", json!({}));
        let first = json!({"wave": 1, "text": "A soma saiu; o total ficou.", "files": ["src/a.rs"], "commit": "a soma sai",
            "undone": ["MSTD-TASK-0002"]});
        assert_eq!(returned_unread(root, first)["ok"], json!(true));
        edit(&copy, "src/a.rs", "a soma conserta o sinal");
        let both = json!({"wave": 1, "text": "Nada saiu.", "undone": ["MSTD-TASK-0001", "MSTD-TASK-0002"]});
        let refused = returned_unread(root, both);
        assert_eq!(refused["reason"], json!("delivery-started-work-undone"), "{refused}");
        let expected = translate("round.started_work_undone", Locale::PtBr).replace("{wave}", "1").replace("{tasks}", "MSTD-TASK-0001");
        assert_eq!(refused["hint"], json!(expected), "{refused}");

        let dir = tempdir().unwrap();
        let root = dir.path();
        let copy = two_task_wave(root);
        edit(&copy, "src/a.rs", "a soma saiu");
        step(root, "MSTD-TASK-0001", json!({}));
        edit(&copy, "src/b.rs", "o total não fecha");
        let replanned = json!({"wave": 1, "text": "O total não fecha com o plano.", "files": ["src/a.rs"],
            "commit": "a soma sai", "replan": "O total pede a tabela nova antes.", "undone": ["MSTD-TASK-0002"]});
        assert_eq!(returned_unread(root, replanned)["ok"], json!(true));
        let given_back = json!({"wave": 1, "text": "A soma saiu; o total ficou.", "files": ["src/a.rs"],
            "commit": "a soma sai", "undone": ["MSTD-TASK-0002"]});
        let refused = returned_unread(root, given_back);
        assert_eq!(refused["reason"], json!("delivery-started-work-undone"), "{refused}");
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
