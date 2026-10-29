//! A vida das skills do Mustard pela rodada: a conta de cada envio, a tarefa
//! de atualizar a skill cujo arquivo citado um commit mudou e a saída da que
//! não serve ([`crate::shared::skill_cycle`]).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use mustard_core::domain::spec_events::SpecEvent;
use mustard_core::domain::spec_state::PhaseWriter;
use mustard_core::io::mustard_skills::{self as registry, MustardSkill};
use mustard_core::io::spec_events as store;
use serde_json::{Map, Value, json};
use tempfile::tempdir;

use super::tests::{approved, approved_with, delivered, git_at, id_of, round, waves_in, write};

fn log_of(root: &Path) -> mustard_core::domain::spec_events::SpecLog {
    store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
}

/// Writes the skill `name` in the main project of `root`, scoped to `src`
/// when `scoped`.
fn write_skill(root: &Path, name: &str, scoped: bool) {
    let dir = root.join(".claude/skills").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = if scoped { "paths:\n  - src/**\n" } else { "" };
    std::fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\ndescription: Does {name}.\n{paths}---\n\nAlways sum in whole cents in {name}.\n")).unwrap();
}

/// The registry holding `skills`, each made by Mustard before this spec.
fn registered(root: &Path, skills: &[(&str, MustardSkill)]) {
    registry::change(root, |held| {
        for (name, skill) in skills {
            held.skills.insert((*name).to_string(), skill.clone());
        }
    })
    .unwrap();
}

/// A skill Mustard made in another spec, citing `files`.
fn made(files: &[&str]) -> MustardSkill {
    MustardSkill {
        state: registry::ACTIVE.into(),
        spec: "antiga".into(),
        wave: Some(3),
        since: "2026-01-01T00:00:00Z".into(),
        made: true,
        files: files.iter().map(|file| (*file).to_string()).collect(),
        gain: Some("Acertou 3 de 3 com a skill.".into()),
        ..MustardSkill::default()
    }
}

/// The Jev answering `chance` to every question, keeping each request.
fn jev(chance: f64, asked: &Rc<RefCell<Vec<Value>>>) -> crate::shared::jev::yes_no::FakeAsk {
    let asked = Rc::clone(asked);
    Rc::new(move |payloads: &[String]| {
        let docs = payloads.iter().map(|payload| {
            let body: Value = serde_json::from_str(payload).unwrap();
            let answers: Map<String, Value> = body["questions"].as_object().unwrap().keys().map(|key| (key.clone(), json!({"type":"noul","noul":chance}))).collect();
            asked.borrow_mut().push(body);
            json!({ "answers": answers })
        });
        Some(docs.collect())
    })
}

/// The tasks of spec `x` in `root` that update a skill.
fn update_tasks(root: &Path) -> Vec<SpecEvent> {
    log_of(root).visible().into_iter().filter(|e| e.event_type == "task" && e.str_field("title").is_some_and(|title| title.starts_with("Atualizar a skill"))).cloned().collect()
}

/// Wave 1 of spec `x` goes out, changes `src/a.rs` and is committed, with
/// the Jev answering `chance`; returns what the Jev was asked.
fn committed_with(root: &Path, chance: f64) -> Vec<Value> {
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    write_skill(root, "falta-teste", false);
    registered(root, &[("falta-teste", made(&["src/a.rs"]))]);
    assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
    let asked = Rc::new(RefCell::new(Vec::new()));
    crate::shared::jev::yes_no::ask_in_test(Some(jev(chance, &asked)));
    let report = delivered(root, 1, "Saiu.", &["src/a.rs"]);
    let done = round(root, "x", Some(&report));
    crate::shared::jev::yes_no::ask_in_test(None);
    assert!(log_of(root).visible().iter().any(|e| e.event_type == "commit"), "wave 1 was committed: {done}");
    asked.take()
}

#[test]
fn a_commit_changing_a_cited_file_with_a_yes_or_a_doubt_creates_the_update_task_once() {
    for chance in [0.9, 0.5] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let asked = committed_with(root, chance);
        let question = asked.iter().find(|body| body["state"].get("skill").is_some()).unwrap_or_else(|| panic!("the Jev saw the skill: {asked:?}"));
        assert_eq!(question["questions"]["c1"]["instructions"]["question"], "Does the change in `change.c1` contradict what `skill` teaches?");
        assert!(question["state"]["skill"].as_str().unwrap().contains("Always sum in whole cents"), "{question}");
        assert!(question["state"]["change"]["c1"].as_str().unwrap().contains("src/a.rs"), "the diff of the cited file: {question}");
        let tasks = update_tasks(root);
        assert_eq!(tasks.len(), 1, "chance {chance}: {tasks:?}");
        assert_eq!(tasks[0].str_field("title"), Some("Atualizar a skill falta-teste"));
        assert_eq!(tasks[0].str_field("skill"), Some("skill-creator"), "the block cites the skill-creator by its path");
        assert!(tasks[0].str_field("text").unwrap().contains("`src/a.rs`"), "{:?}", tasks[0].fields);
        let agent = tasks[0].str_field("agent").unwrap();
        assert!(agent.contains("```diff") && agent.contains("falta-teste/SKILL.md"), "{agent}");
        let marked = registry::read(root).skills["falta-teste"].update_task.clone().expect("the registry marks the task");
        assert_eq!((marked.spec.as_str(), marked.task), ("x", Some(tasks[0].id)));

        crate::shared::jev::yes_no::ask_in_test(Some(jev(chance, &Rc::new(RefCell::new(Vec::new())))));
        round(root, "x", None);
        crate::shared::jev::yes_no::ask_in_test(None);
        assert_eq!(update_tasks(root).len(), 1, "one task per skill while it waits");
    }
}

/// The Jev that fails on the stale-skill question sends it to the reserve:
/// the round's answer carries the request with the same question, and the
/// commit is not read until the receipt. The sure yes creates the update task
/// without the Jev, and so does the unsure answer, a doubt. With the Jev off,
/// the commit is read and nothing goes to the reserve.
#[test]
fn a_failed_jev_sends_the_stale_skill_question_to_the_reserve_and_the_receipt_applies() {
    for mode in ["sure", "unsure", "off"] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        write_skill(root, "falta-teste", false);
        registered(root, &[("falta-teste", made(&["src/a.rs"]))]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        if mode != "off" {
            crate::shared::jev::yes_no::ask_in_test(Some(Rc::new(|_: &[String]| None)));
        }
        let report = delivered(root, 1, "Saiu.", &["src/a.rs"]);
        let done = round(root, "x", Some(&report));
        crate::shared::jev::yes_no::ask_in_test(None);
        assert!(log_of(root).visible().iter().any(|e| e.event_type == "commit"), "wave 1 was committed: {done}");
        assert!(update_tasks(root).is_empty(), "{mode}");
        if mode == "off" {
            assert!(done.get("reserves").is_none(), "with the Jev off nothing goes to the reserve: {done}");
            assert!(registry::read(root).commits_read.contains_key("x"), "the commit is read");
            continue;
        }
        let request = done["reserves"][0].clone();
        let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("{done}"));
        assert!(prompt.contains("Does the change in `change.c1` contradict what `skill` teaches?") && prompt.contains("Always sum in whole cents"), "{prompt}");
        assert!(done["reserves_hint"].as_str().unwrap().contains("run round"), "{done}");
        assert!(!registry::read(root).commits_read.contains_key("x"), "the commit waits for the receipt");

        let choice = if mode == "sure" { "yes" } else { "no" };
        crate::shared::jev::reserve::answer_in_test(root, &request, |_| choice.into(), mode);
        crate::shared::jev::yes_no::ask_in_test(Some(Rc::new(|_: &[String]| panic!("the receipt answers in place of the Jev"))));
        let applied = round(root, "x", None);
        crate::shared::jev::yes_no::ask_in_test(None);
        assert!(applied.get("reserves").is_none(), "{applied}");
        assert_eq!(update_tasks(root).len(), 1, "{mode}: {applied}");
        assert!(registry::read(root).commits_read.contains_key("x"), "the receipt reads the commit");
    }
}

/// A spec opened by the user's messages alone has no request to fall back on.
/// The update task born from a wave commit that changes a file the skill cites
/// covers the items the tasks of that wave cover, and the next round puts it
/// in a wave. A skill task the program recorded without items gets the same
/// ones, in a new version, on the following round, and the round no longer
/// names it as a task without covers.
#[test]
fn a_skill_task_from_a_wave_commit_covers_the_wave_items() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let mut item = 0;
    approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
        let body = json!({"text": "A soma arredonda para baixo.", "why": "w", "waves": [1], "keys": ["k"], "origin": said});
        item = id_of(&write(root, "x", "decision", body));
        let task = json!({"wave": 1, "text": "Arredondar a soma.", "files": [{"path": "src/a.rs"}], "depends_on": [], "covers": [item], "origin": said});
        assert_eq!(write(root, "x", "task", task)["ok"], json!(true));
    });
    assert!(log_of(root).visible().iter().all(|e| e.event_type != "request"), "the spec holds only the user's messages");
    write_skill(root, "falta-teste", false);
    registered(root, &[("falta-teste", made(&["src/a.rs"]))]);
    assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
    crate::shared::jev::yes_no::ask_in_test(Some(jev(0.9, &Rc::new(RefCell::new(Vec::new())))));
    let report = delivered(root, 1, "Saiu.", &["src/a.rs"]);
    let done = round(root, "x", Some(&report));
    crate::shared::jev::yes_no::ask_in_test(None);
    let log = log_of(root);
    let commit = log.visible().into_iter().find(|e| e.event_type == "commit").cloned().unwrap_or_else(|| panic!("wave 1 was committed: {done}"));
    assert_eq!(commit.ints("waves"), vec![1]);

    let born = update_tasks(root);
    assert_eq!(born.len(), 1, "{done}");
    assert_eq!(born[0].ints("covers"), vec![item], "the task covers the items of the committed wave: {:?}", born[0].fields);
    let out = round(root, "x", None);
    assert_eq!(waves_in(&out, "dispatch"), vec![2], "the round puts it in a wave: {out}");
    assert_eq!(log_of(root).current(born[0].id).and_then(SpecEvent::wave), Some(2));

    let why = json!([{"kind": "change", "sha": commit.str_field("sha").unwrap(), "files": ["src/a.rs"], "chance": 0.9}]);
    let bare = json!({"title": "Atualizar a skill outra", "text": "Atualizar a skill outra.", "agent": "- atualizar", "skill": "skill-creator",
        "files": [{"path": "src/a.rs"}], "depends_on": [], "author": "binary", "skill_whys": why});
    let bare = crate::commands::spec_events::write::record(root, "x", "task", bare.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap().written.id;
    let next = round(root, "x", None);
    let covered = log_of(root).current(bare).cloned().expect("the skill task");
    assert_ne!(covered.id, bare, "a new version of the task: {next}");
    assert_eq!(covered.ints("covers"), vec![item], "{next}");
    let warned = next["warnings"].as_array().into_iter().flatten().any(|warning| warning["reason"] == "task-without-covers");
    assert!(!warned, "the round does not name it as a task without covers: {next}");
}

#[test]
fn a_commit_the_jev_finds_no_contradiction_in_creates_no_task() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let asked = committed_with(root, 0.1);
    assert!(asked.iter().any(|body| body["state"].get("skill").is_some()), "the change went to the Jev");
    assert!(update_tasks(root).is_empty());
}

#[test]
fn a_cited_file_deleted_by_a_commit_creates_the_update_task_without_the_jev() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs", "src/b.rs"], &[])]);
    write_skill(root, "falta-teste", false);
    registered(root, &[("falta-teste", made(&["src/b.rs"]))]);
    git_at(root, &["rm", "-q", "src/b.rs"]);
    git_at(root, &["commit", "-q", "-m", "apaga o b"]);
    let sha = super::commit::head(root);
    let draft = json!({"sha": sha, "title": "apaga o b", "waves": [1], "files": ["src/b.rs"], "repo": "x", "author": "binary"});
    crate::commands::spec_events::write::record(root, "x", "commit", draft.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap();
    let asked = Rc::new(RefCell::new(Vec::new()));
    crate::shared::jev::yes_no::ask_in_test(Some(jev(0.0, &asked)));
    let (recorded, _, _) = crate::shared::skill_cycle::after_round(root, "x", mustard_core::platform::i18n::Locale::PtBr);
    crate::shared::jev::yes_no::ask_in_test(None);
    assert!(asked.borrow().is_empty(), "a deleted file needs no question: {:?}", asked.borrow());
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    let tasks = update_tasks(root);
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].str_field("agent").unwrap().contains("apagou `src/b.rs`"), "{:?}", tasks[0].fields);
}

/// Acrescenta `line` a `src/a.rs`, comita e grava o commit da onda 1 como a
/// rodada o grava. Devolve o commit.
fn cited_change(root: &Path, line: &str) -> String {
    let path = root.join("src/a.rs");
    let before = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{before}{line}\n")).unwrap();
    git_at(root, &["commit", "-q", "-am", line]);
    let sha = super::commit::head(root);
    let draft = json!({"sha": sha, "title": line, "waves": [1], "files": ["src/a.rs"], "repo": "x", "author": "binary"});
    crate::commands::spec_events::write::record(root, "x", "commit", draft.as_object().cloned().unwrap(), PhaseWriter::Binary).unwrap();
    sha
}

/// Dois commits mudam o arquivo que a skill cita, cada um lido depois da sua
/// própria rodada: a avaliação que o primeiro criou ganha uma versão nova com
/// as duas mudanças, cada uma com o seu diff e a chance que o Jev deu, e o
/// registro continua apontando para ela.
#[test]
fn two_changes_to_a_skill_file_make_one_evaluation_with_both() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    write_skill(root, "falta-teste", false);
    registered(root, &[("falta-teste", made(&["src/a.rs"]))]);
    let asked = Rc::new(RefCell::new(Vec::new()));
    crate::shared::jev::yes_no::ask_in_test(Some(jev(0.9, &asked)));
    let first = cited_change(root, "// soma em centavos");
    let (created, _, _) = crate::shared::skill_cycle::after_round(root, "x", mustard_core::platform::i18n::Locale::PtBr);
    let second = cited_change(root, "// arredonda no fim");
    let (revised, _, _) = crate::shared::skill_cycle::after_round(root, "x", mustard_core::platform::i18n::Locale::PtBr);
    crate::shared::jev::yes_no::ask_in_test(None);
    assert_eq!((created.len(), revised.len()), (1, 1), "{created:?} {revised:?}");
    let tasks = update_tasks(root);
    assert_eq!(tasks.len(), 1, "one evaluation for the two changes: {tasks:?}");
    let agent = tasks[0].str_field("agent").unwrap();
    for (sha, added) in [(&first, "+// soma em centavos"), (&second, "+// arredonda no fim")] {
        assert!(agent.contains(&sha[..12]) && agent.contains(added), "each change with its diff: {agent}");
    }
    assert_eq!(agent.matches("chance de 0.90").count(), 2, "the chance of each change: {agent}");
    let marked = registry::read(root).skills["falta-teste"].update_task.clone().expect("the registry marks the task");
    assert_eq!(marked.task, Some(tasks[0].id), "{marked:?}");
}

/// A avaliação retirada da spec deixa a mudança dela para a próxima: o
/// próximo commit no arquivo citado cria uma tarefa nova, que lista todos os
/// commits desde a última avaliação da skill, inclusive o da retirada.
#[test]
fn a_removed_evaluation_leaves_its_change_to_the_next_one() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    write_skill(root, "falta-teste", false);
    registered(root, &[("falta-teste", made(&["src/a.rs"]))]);
    crate::shared::jev::yes_no::ask_in_test(Some(jev(0.9, &Rc::new(RefCell::new(Vec::new())))));
    let first = cited_change(root, "// soma em centavos");
    crate::shared::skill_cycle::after_round(root, "x", mustard_core::platform::i18n::Locale::PtBr);
    let removed = update_tasks(root)[0].id;
    assert_eq!(write(root, "x", "remove", json!({"targets": [removed], "reason": "Não é agora."}))["ok"], json!(true));
    let second = cited_change(root, "// arredonda no fim");
    let (recorded, _, _) = crate::shared::skill_cycle::after_round(root, "x", mustard_core::platform::i18n::Locale::PtBr);
    crate::shared::jev::yes_no::ask_in_test(None);
    assert_eq!(recorded.len(), 1, "the next change makes a new evaluation: {recorded:?}");
    let tasks = update_tasks(root);
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    let agent = tasks[0].str_field("agent").unwrap();
    for (sha, added) in [(&first, "+// soma em centavos"), (&second, "+// arredonda no fim")] {
        assert!(agent.contains(&sha[..12]) && agent.contains(added), "every change since the last evaluation: {agent}");
    }
}

/// The skill-folder and backup of `name` in `root`.
fn folders(root: &Path, name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    (root.join(".claude/skills").join(name), mustard_core::io::wave_prompt::skills_backup_dir(root).join(name))
}

#[test]
fn thirty_candidacies_without_a_delivery_retire_the_skill() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    for name in ["a1", "a2", "a3", "z-parada"] {
        write_skill(root, name, true);
    }
    registered(root, &[("z-parada", MustardSkill { candidacies: 29, ..made(&["src/a.rs"]) }), ("a1", MustardSkill { candidacies: 29, ..made(&["src/a.rs"]) })]);
    let (main, backup) = folders(root, "z-parada");
    let _ = std::fs::remove_dir_all(&backup);
    let out = round(root, "x", None);
    assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
    let log = log_of(root);
    let sent = log.last_by_wave("send")[&1];
    let blocks = log.send_field(sent, "tasks").cloned().unwrap_or_default();
    assert_eq!(blocks[0]["skills"], json!(["a1", "a2", "a3"]), "the send records the skills of each block: {blocks}");
    let skills = registry::read(root).skills;
    assert_eq!((skills["a1"].candidacies, skills["a1"].deliveries, skills["a1"].state.as_str()), (30, 1, registry::ACTIVE));
    assert_eq!((skills["z-parada"].candidacies, skills["z-parada"].deliveries, skills["z-parada"].state.as_str()), (30, 0, registry::RETIRED));
    assert!(!main.exists(), "the skill left .claude/skills");
    assert!(backup.join("SKILL.md").is_file(), "the backup copy stays");
    let retired = log.visible().into_iter().rfind(|e| e.event_type == "skill").cloned().expect("the retired event");
    assert_eq!((retired.str_field("name"), retired.str_field("action")), (Some("z-parada"), Some("retired")));
    let warning = out["warnings"].as_array().into_iter().flatten().find(|w| w["reason"] == "skill-retired").cloned().unwrap_or_else(|| panic!("{out}"));
    assert!(warning["hint"].as_str().unwrap().contains("z-parada"), "{warning}");
    let _ = std::fs::remove_dir_all(&backup);
}

#[test]
fn two_discards_in_a_row_retire_the_skill() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    write_skill(root, "falta-teste", false);
    registered(root, &[("falta-teste", made(&["src/a.rs"]))]);
    let (main, backup) = folders(root, "falta-teste");
    let _ = std::fs::remove_dir_all(&backup);
    let lang = mustard_core::platform::i18n::Locale::PtBr;
    for at in 0..2 {
        let body = json!({"name":"falta-teste","action":"discarded","text":"Sem ganho no teste com e sem."});
        let opts = crate::commands::spec_events::write::WriteOpts { root: root.to_path_buf(), spec: Some("x".into()), event_type: "skill".into(), json: body.to_string() };
        let wrote = crate::commands::spec_events::write::write_at(&opts);
        assert_eq!(wrote["ok"], true, "{wrote}");
        let (recorded, _, _) = crate::shared::skill_cycle::after_round(root, "x", lang);
        assert_eq!(recorded.is_empty(), at == 0, "one discard keeps it, the second retires it: {recorded:?}");
    }
    assert_eq!(registry::read(root).skills["falta-teste"].state, registry::RETIRED);
    assert!(!main.exists() && backup.join("SKILL.md").is_file());
    let _ = std::fs::remove_dir_all(&backup);
}

/// A onda da tarefa que atualiza uma skill escreve a versão nova na pasta
/// reservada da própria cópia e registra a atualização com um ganho: a rodada
/// que recebe o retorno a escreve no lugar da skill, o backup, o registro e a
/// impressão digital gravada acompanham a versão nova, e a pasta reservada
/// sai da cópia. Nenhum commit a leva, e a tarefa manda a onda escrevê-la na
/// cópia dela, não na pasta da skill.
#[test]
fn a_skill_update_the_wave_delivers_is_installed_by_the_round() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    committed_with(root, 0.9);
    let reserved = ".mustard-skill/falta-teste/SKILL.md";
    let task = update_tasks(root)[0].clone();
    let agent = task.str_field("agent").unwrap();
    assert!(agent.contains(reserved), "the task names the reserved file: {agent}");
    let log = log_of(root);
    let criterion = log.visible().into_iter().find(|e| e.event_type == "criterion").map(|e| e.id).expect("the criterion");
    let covering = super::queue::task_revision(&log, task.id, json!({"covers": [criterion]}).as_object().cloned().unwrap()).unwrap();
    crate::commands::spec_events::write::record(root, "x", "task", covering, PhaseWriter::Binary).unwrap();
    let out = round(root, "x", None);
    assert_eq!(waves_in(&out, "dispatch"), vec![2], "the update task goes out: {out}");
    let copy = super::commit::copy_of(&log_of(root), 2).expect("the copy of wave 2");
    let new = "---\nname: falta-teste\ndescription: Does falta-teste.\n---\n\nAlways sum in whole cents in `src/b.rs`.\n";
    std::fs::create_dir_all(copy.join(".mustard-skill/falta-teste")).unwrap();
    std::fs::write(copy.join(reserved), new).unwrap();
    let (main, backup) = folders(root, "falta-teste");
    let body = json!({"name": "falta-teste", "action": "updated", "text": "Acertou 3 de 3 com a versão nova."});
    let opts = crate::commands::spec_events::write::WriteOpts { root: root.to_path_buf(), spec: Some("x".into()), event_type: "skill".into(), json: body.to_string() };
    let wrote = crate::commands::spec_events::write::write_at(&opts);
    assert_eq!(wrote["ok"], true, "{wrote}");
    let head = super::commit::head(root);

    let report = delivered(root, 2, "A skill saiu.", &[reserved]);
    let taken = round(root, "x", Some(&report));

    assert!(log_of(root).last_by_wave("delivered").contains_key(&2), "the round took the return: {taken}");
    assert_eq!(std::fs::read_to_string(main.join("SKILL.md")).unwrap(), new, "the new version is in the place of the skill: {taken}");
    assert_eq!(std::fs::read_to_string(backup.join("SKILL.md")).unwrap(), new, "the backup follows it");
    assert!(!copy.join(".mustard-skill").exists(), "the reserved folder leaves the copy");
    assert!(!root.join(".mustard-skill").exists(), "nothing of it reaches the main checkout");
    assert_eq!(super::commit::head(root), head, "no commit carries the reserved file: {taken}");
    assert_eq!(registry::read(root).skills["falta-teste"].files, vec!["src/b.rs".to_string()], "the registry reads the new version");
    let mut hash = mustard_core::io::sha256::Sha256::new();
    hash.update(new.as_bytes());
    let event = log_of(root).visible().into_iter().rfind(|e| e.event_type == "skill").cloned().expect("the skill event");
    assert_eq!(event.str_field("sha"), Some(hash.hex_digest().as_str()), "the event fingerprints the new version");
    let _ = std::fs::remove_dir_all(&backup);
}

/// A onda da tarefa que cria uma skill a escreve na pasta reservada da própria
/// cópia, como faz uma atualização, e registra a criação com um ganho enquanto
/// o projeto ainda não tem pasta para ela: o registro espera sem aviso de
/// backup, a rodada que recebe o retorno escreve a skill no projeto com o
/// backup dela, a pasta reservada sai da cópia, e nenhum commit a leva.
#[test]
fn a_skill_creation_the_wave_delivers_is_installed_by_the_round() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[(1, &["src/a.rs"], &[])]);
    let log = log_of(root);
    let task = log.visible().into_iter().find(|e| e.event_type == "task").map(|e| e.id).expect("the task of wave 1");
    let creating = super::queue::task_revision(&log, task, json!({"skill": "skill-creator"}).as_object().cloned().unwrap()).unwrap();
    crate::commands::spec_events::write::record(root, "x", "task", creating, PhaseWriter::Binary).unwrap();
    assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1], "the creation task goes out");
    let copy = super::commit::copy_of(&log_of(root), 1).expect("the copy of wave 1");
    let reserved = ".mustard-skill/falta-teste/SKILL.md";
    let new = "---\nname: falta-teste\ndescription: Does falta-teste.\n---\n\nAlways test the empty case in `src/a.rs`.\n";
    std::fs::create_dir_all(copy.join(".mustard-skill/falta-teste")).unwrap();
    std::fs::write(copy.join(reserved), new).unwrap();
    let (main, backup) = folders(root, "falta-teste");
    let _ = std::fs::remove_dir_all(&backup);
    let body = json!({"name": "falta-teste", "action": "created", "text": "Acertou 3 de 3 com a skill."});
    let opts = crate::commands::spec_events::write::WriteOpts { root: root.to_path_buf(), spec: Some("x".into()), event_type: "skill".into(), json: body.to_string() };
    let wrote = crate::commands::spec_events::write::write_at(&opts);
    assert_eq!(wrote["ok"], true, "{wrote}");
    assert!(wrote.get("warnings").is_none(), "no backup is due before the skill has a folder: {wrote}");
    assert!(!main.exists(), "the skill waits for the round");
    let head = super::commit::head(root);

    let report = delivered(root, 1, "A skill saiu.", &[reserved]);
    let taken = round(root, "x", Some(&report));

    assert!(log_of(root).last_by_wave("delivered").contains_key(&1), "the round took the return: {taken}");
    assert_eq!(std::fs::read_to_string(main.join("SKILL.md")).unwrap(), new, "the skill is in the project: {taken}");
    assert_eq!(std::fs::read_to_string(backup.join("SKILL.md")).unwrap(), new, "the backup follows it");
    assert!(!copy.join(".mustard-skill").exists(), "the reserved folder leaves the copy");
    assert_eq!(super::commit::head(root), head, "no commit carries the reserved file: {taken}");
    assert_eq!(registry::read(root).skills["falta-teste"].files, vec!["src/a.rs".to_string()], "the registry reads the delivered version");
    let _ = std::fs::remove_dir_all(&backup);
}

/// The skills recorded in the first block of the last send of wave `wave` of
/// spec `x` in `root`; nothing when the block went without skills.
fn sent_skills(root: &Path, wave: u64) -> Value {
    let log = log_of(root);
    let sent = log.last_by_wave("send")[&wave];
    log.send_field(sent, "tasks").cloned().unwrap_or_default()[0]["skills"].clone()
}

/// The skill pick the Jev fails holds only the wave with the tie: the round's
/// answer carries the reserve with the same questions, and the other wave
/// goes out. Until the receipt, the next round asks the Jev again and holds
/// the wave with the same request. The receipt sends the held wave with the
/// skills it chose, and an unsure answer is a doubt, below the cut. With the
/// Jev off, the first three go out without waiting. The Jev is never asked
/// with the git-step lock or the copies lock held.
#[test]
fn a_failed_skill_pick_holds_only_its_wave_and_the_receipt_sends_it_with_the_chosen_skills() {
    for mode in ["sure", "unsure", "off"] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["lib/b.rs"], &[])]);
        for name in ["a1", "a2", "a3", "a4", "a5"] {
            write_skill(root, name, true);
        }
        let seen = Rc::new(RefCell::new(Vec::new()));
        if mode != "off" {
            let (spy, at) = (Rc::clone(&seen), root.to_path_buf());
            let free = move || {
                let free = |name: &str| mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&at.join(".claude/spec").join(name)).unwrap().is_some();
                free("round-git.lock") && free("round-copies.lock")
            };
            crate::shared::jev::yes_no::ask_in_test(Some(Rc::new(move |_: &[String]| {
                spy.borrow_mut().push(free());
                None
            })));
        }
        let first = round(root, "x", None);
        let calls = seen.take();
        assert!(calls.iter().all(|free| *free), "a skill pick asked with a lock held: {calls:?}");
        assert_eq!(calls.is_empty(), mode == "off", "{first}");
        if mode == "off" {
            assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
            assert!(first.get("reserves").is_none(), "with the Jev off nothing goes to the reserve: {first}");
            assert_eq!(sent_skills(root, 1), json!(["a1", "a2", "a3"]), "{mode}");
            continue;
        }
        assert_eq!(waves_in(&first, "dispatch"), vec![2], "only the wave with the tie waits: {first}");
        let request = first["reserves"][0].clone();
        let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("{first}"));
        assert!(prompt.contains("Does `skills.s1` do the specific thing `task` asks for?") && prompt.contains("Does a5."), "{prompt}");
        assert!(first["reserves_hint"].as_str().unwrap().contains("run round"), "{first}");

        let again = round(root, "x", None);
        crate::shared::jev::yes_no::ask_in_test(None);
        assert!(waves_in(&again, "dispatch").is_empty(), "the wave still waits for the receipt: {again}");
        assert_eq!(again["reserves"][0]["request_id"], request["request_id"], "the same decision, the same request: {again}");

        let wanted = |text: &str| if text.contains("skills.s4") || text.contains("skills.s5") { "yes".to_string() } else { "no".to_string() };
        crate::shared::jev::reserve::answer_in_test(root, &request, wanted, mode);
        crate::shared::jev::yes_no::ask_in_test(Some(Rc::new(|_: &[String]| panic!("the receipt answers in place of the Jev"))));
        let sent = round(root, "x", None);
        crate::shared::jev::yes_no::ask_in_test(None);
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{mode}: {sent}");
        assert!(sent.get("reserves").is_none(), "{sent}");
        let chosen = if mode == "sure" { json!(["a4", "a5"]) } else { Value::Null };
        assert_eq!(sent_skills(root, 1), chosen, "{mode}: an unsure answer stays below the cut");
    }
}
