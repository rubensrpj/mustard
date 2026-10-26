//! A triagem das atividades que a rodada grava — o alvo da mudança de plano,
//! o item que a onda não cumpriu e o item que a revisão final não cumpriu —
//! nunca pergunta ao Jev com a trava do passo do git ou a das
//! cópias presa: a volta espera fora do commit, a pergunta sai depois de as
//! travas soltarem, e a rodada seguinte aplica a resposta gravada. O Jev que
//! falha manda a pergunta à reserva, e a volta espera o recibo. Com o Jev
//! desligado, vale a regra de sempre, na mesma rodada.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::io::spec_events as store;
use serde_json::{Map, Value, json};
use tempfile::tempdir;

use super::queue::{UndoneReturn, return_with_an_undone_task, task_now};
use super::tests::{approved_with, git_text, id_of, judged, returned, round, seed_review, waves_in, write};
use crate::shared::jev::yes_no::{FakeAsk, ask_in_test};

/// How the Jev behaves in a scenario.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Answers,
    Fails,
    Off,
}

const MODES: [Mode; 3] = [Mode::Answers, Mode::Fails, Mode::Off];

/// The text of the open task the Jev says covers each activity.
const OPEN_TASK: &str = "A tarefa aberta que já cobre o trabalho.";

fn log_of(root: &Path) -> SpecLog {
    store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
}

/// `true` when nobody holds the git-step lock nor the copies lock of `root`.
fn locks_free(root: &Path) -> bool {
    let free = |name: &str| mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&root.join(".claude/spec").join(name)).unwrap().is_some();
    free("round-git.lock") && free("round-copies.lock")
}

/// A sure answer choosing `label`.
fn sure(label: &str) -> Value {
    json!({"type": "choice", "choice": label, "confidence": 0.95, "probabilities": {label: 0.95}})
}

/// The answer to a question the scenario does not look at: doubt, below
/// every threshold.
fn doubt(question: &Value) -> Value {
    match question["type"].as_str() {
        Some("noul") => json!({"type": "noul", "noul": 0.5}),
        Some("score") => json!({"type": "score", "score": 0, "confidence": 0.3, "probabilities": {"0": 0.3}}),
        _ => {
            let first = question["criteria"].as_object().and_then(|c| c.keys().next().cloned()).unwrap_or_default();
            json!({"type": "choice", "choice": first, "confidence": 0.3, "probabilities": {first: 0.3}})
        }
    }
}

/// The key of the open task [`OPEN_TASK`] in the state of the triage
/// request `body`.
fn open_task_key(body: &Value) -> Option<String> {
    body["state"]["tasks"].as_object()?.iter().find(|(_, task)| task["text"] == json!(OPEN_TASK)).map(|(key, _)| key.clone())
}

/// The Jev of the round: every task-triage item goes to the open task
/// [`OPEN_TASK`] — covered by it —, sure, and every other question gets
/// doubt. Each task-triage call keeps, in `seen`, whether both locks of
/// `root` were free; with `failing`, the task-triage calls fail.
fn triage_jev(root: &Path, failing: bool, seen: &Rc<RefCell<Vec<bool>>>) -> FakeAsk {
    let (root, seen) = (root.to_path_buf(), Rc::clone(seen));
    Rc::new(move |payloads: &[String]| {
        let bodies: Vec<Value> = payloads.iter().map(|payload| serde_json::from_str(payload).unwrap()).collect();
        let triage = bodies.iter().any(|body| body["state"]["revision"] == json!("task-triage-v1"));
        if triage {
            seen.borrow_mut().push(locks_free(&root));
            if failing {
                return None;
            }
        }
        let docs = bodies.iter().map(|body| {
            let task = open_task_key(body);
            let answers: Map<String, Value> = body["questions"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, question)| {
                    let answer = match (&task, body["state"]["revision"] == json!("task-triage-v1")) {
                        (Some(task), true) if !key.ends_with("_near") => sure(&format!("covered:{task}")),
                        (_, true) if key.ends_with("_near") => sure("none"),
                        (None, true) => sure("new"),
                        _ => doubt(question),
                    };
                    (key.clone(), answer)
                })
                .collect();
            json!({ "answers": answers })
        });
        Some(docs.collect())
    })
}

/// Installs the Jev of `mode` for the next rounds of `root`.
fn jev_for(root: &Path, mode: Mode, seen: &Rc<RefCell<Vec<bool>>>) {
    ask_in_test((mode != Mode::Off).then(|| triage_jev(root, mode == Mode::Fails, seen)));
}

/// The wave the round answer `out` leaves out waiting for the Jev.
fn waiting(out: &Value) -> Vec<Value> {
    out["warnings"].as_array().into_iter().flatten().filter(|w| w["reason"] == json!("round-jev-decision-pending")).cloned().collect()
}

/// Answers the reserve `request` of `root` as the analysis agent would:
/// each item covered by the open task, sure.
fn answer_reserve(root: &Path, request: &Value) {
    let key = format!("covered:t{}", open_task(root).id);
    crate::shared::jev::reserve::answer_in_test(root, request, |question| if question.contains("closest subject") { "none".into() } else { key.clone() }, "sure");
}

/// The current version of the open task [`OPEN_TASK`].
fn open_task(root: &Path) -> SpecEvent {
    let log = log_of(root);
    log.visible().into_iter().rfind(|e| e.event_type == "task" && e.str_field("text") == Some(OPEN_TASK)).cloned().expect("the open task")
}

/// The current tasks of `root` whose text or title is `text`.
fn tasks_saying(root: &Path, text: &str) -> usize {
    let log = log_of(root);
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "task" && (e.str_field("text") == Some(text) || e.str_field("title") == Some(text)))
        .count()
}

/// The notes of `root` that point at the open task [`OPEN_TASK`].
fn notes_on_the_open_task(root: &Path) -> usize {
    let log = log_of(root);
    let code = log.codes().get(&open_task(root).id).cloned().unwrap_or_default();
    log.visible().into_iter().filter(|e| e.event_type == "note" && e.str_field("label") == Some(code.as_str())).count()
}

/// A spec `x` with wave 1 on `src/a.rs`, the first decision, which the
/// wave carries, and the open backlog task [`OPEN_TASK`] on `src/b.rs`,
/// covering a second decision; wave 1 already sent.
fn sent_with_an_open_task(root: &Path) {
    approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
        let decision = json!({"text": "A soma arredonda para baixo.", "why": "w", "waves": [1], "keys": ["k"], "origin": said});
        assert_eq!(write(root, "x", "decision", decision)["ok"], json!(true));
        let other = id_of(&write(root, "x", "decision", json!({"text": "O log gira por dia.", "why": "w", "keys": ["log"], "origin": said})));
        let task = json!({"text": OPEN_TASK, "files": [{"path": "src/b.rs"}], "depends_on": [], "covers": [other, "MSTD-CRIT-0001"], "origin": said});
        assert_eq!(write(root, "x", "task", task)["ok"], json!(true));
    });
    std::fs::write(root.join("src/b.rs"), "fn two() {}\n").unwrap();
    let sent = round(root, "x", None);
    assert!(waves_in(&sent, "dispatch").contains(&1), "{sent}");
}

/// The return of wave 1, with `src/a.rs` changed, answering every agreed
/// item of its request as met except the first decision when `unmet`, and with
/// `extra` fields.
fn return_wave_one(root: &Path, unmet: bool, extra: Value) {
    std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
    super::tests::read_request(root, "x", 1);
    super::tests::finish_tasks(root, "x", 1);
    let log = log_of(root);
    let codes = log.codes();
    let agreed: Vec<Value> = super::agreed::request_agreed(&log, 1)
        .iter()
        .map(|item| {
            let code = codes[&item.id].clone();
            if unmet && code == "MSTD-DEC-0001" { json!({"item": code, "met": false, "text": "Falta arredondar."}) } else { json!({"item": code, "met": true}) }
        })
        .collect();
    let mut body = json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai", "agreed": agreed});
    for (key, value) in extra.as_object().into_iter().flatten() {
        body[key] = value.clone();
    }
    let wrote = returned(root, body);
    assert_eq!(wrote["ok"], json!(true), "{wrote}");
}

/// The scenario of one activity under `mode`: `take` runs the round that
/// takes the return, `applied` says whether the Jev's answer (covered by
/// the open task) took effect, and `rule` whether today's rule did.
fn scenario(root: &Path, mode: Mode, take: &dyn Fn() -> Value, applied: &dyn Fn() -> bool, rule: &dyn Fn() -> bool) {
    let head = git_text(root, &["rev-parse", "HEAD"]);
    let seen = Rc::new(RefCell::new(Vec::new()));
    jev_for(root, mode, &seen);
    let first = take();
    if mode == Mode::Off {
        ask_in_test(None);
        assert!(waiting(&first).is_empty() && first.get("reserves").is_none(), "with the Jev off nothing waits: {first}");
        assert!(rule(), "today's rule in the same round: {first}");
        return;
    }
    assert_eq!(waiting(&first).len(), 1, "the return waits for the decision: {first}");
    assert!(!applied() && !rule(), "nothing recorded while the decision waits: {first}");
    assert_eq!(seen.take(), vec![true], "one triage call, with both locks free: {first}");
    if mode == Mode::Answers {
        let second = round(root, "x", None);
        ask_in_test(None);
        assert!(seen.take().is_empty(), "the recorded decision answers in place of the Jev: {second}");
        assert!(waiting(&second).is_empty(), "{second}");
        assert!(applied() && !rule(), "the next round applies the Jev's answer: {second}");
        assert_ne!(git_text(root, &["rev-parse", "HEAD"]), head, "{second}");
        return;
    }
    let request = first["reserves"][0].clone();
    let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("the reserve request: {first}"));
    assert!(prompt.contains("belong?"), "the same question goes to the reserve: {prompt}");
    let again = round(root, "x", None);
    assert_eq!(waiting(&again).len(), 1, "the return waits for the receipt: {again}");
    assert_eq!(again["reserves"][0]["request_id"], request["request_id"], "{again}");
    assert_eq!(seen.take(), vec![true], "the Jev is asked again, with both locks free: {again}");
    answer_reserve(root, &request);
    let applied_now = round(root, "x", None);
    ask_in_test(None);
    assert!(seen.take().is_empty(), "the receipt answers in place of the Jev: {applied_now}");
    assert!(waiting(&applied_now).is_empty(), "{applied_now}");
    assert!(applied() && !rule(), "the receipt decides like the Jev: {applied_now}");
}

/// An agreed item the return did not meet: the Jev says the open task
/// covers it; today's rule makes a task for it.
#[test]
fn an_unmet_item_of_a_wave_is_triaged_outside_the_locks_and_the_next_round_applies_the_decision() {
    for mode in MODES {
        let dir = tempdir().unwrap();
        let root = dir.path();
        sent_with_an_open_task(root);
        return_wave_one(root, true, json!({}));
        scenario(root, mode, &|| round(root, "x", None), &|| notes_on_the_open_task(root) == 1, &|| tasks_saying(root, "Falta arredondar.") == 1);
    }
}

/// An agreed item the final review did not meet: the Jev says the open task
/// covers it; today's rule makes a task for it.
#[test]
fn an_unmet_item_of_the_final_review_is_triaged_outside_the_locks_and_the_next_round_applies_the_decision() {
    for mode in MODES {
        let dir = tempdir().unwrap();
        let root = dir.path();
        sent_with_an_open_task(root);
        return_wave_one(root, false, json!({}));
        let took = round(root, "x", None);
        assert!(waiting(&took).is_empty(), "{took}");
        seed_review(root);
        let agreed = json!([{"item": "MSTD-DEC-0001", "met": false, "text": "Falta arredondar."}, {"item": "MSTD-DEC-0002", "met": true}]);
        let verdict = json!({"result": "approved", "final": true, "text": "passou", "agreed": agreed,
            "criteria": [{"criterion": "MSTD-CRIT-0001", "tests_rule": true}]});
        let wrote = judged(root, verdict);
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let official = || log_of(root).visible().into_iter().any(|e| e.event_type == "verdict" && e.str_field("author") == Some("review"));
        // The final verdict carries no commit: the scenario looks at the
        // verdict itself instead of the head.
        let head = git_text(root, &["rev-parse", "HEAD"]);
        let seen = Rc::new(RefCell::new(Vec::new()));
        jev_for(root, mode, &seen);
        let first = round(root, "x", None);
        if mode == Mode::Off {
            ask_in_test(None);
            assert!(waiting(&first).is_empty() && first.get("reserves").is_none(), "{first}");
            assert!(official() && tasks_saying(root, "Falta arredondar.") == 1, "today's rule in the same round: {first}");
            continue;
        }
        assert_eq!(waiting(&first).len(), 1, "the verdict waits for the decision: {first}");
        assert!(!official() && tasks_saying(root, "Falta arredondar.") == 0, "{first}");
        assert_eq!(seen.take(), vec![true], "{first}");
        if mode == Mode::Fails {
            let request = first["reserves"][0].clone();
            assert!(request["request_id"].is_string(), "{first}");
            let again = round(root, "x", None);
            assert_eq!(waiting(&again).len(), 1, "{again}");
            assert_eq!(seen.take(), vec![true], "{again}");
            answer_reserve(root, &request);
        }
        let applied = round(root, "x", None);
        ask_in_test(None);
        assert!(seen.take().is_empty(), "{applied}");
        assert!(waiting(&applied).is_empty(), "{applied}");
        assert!(official() && notes_on_the_open_task(root) == 1 && tasks_saying(root, "Falta arredondar.") == 0, "{applied}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head);
    }
}

/// The target of an accepted plan change that cites no task: the round
/// that takes the accepted return asks the classifier outside the locks,
/// and the next one returns the task to the backlog, held by the change.
#[test]
fn the_target_of_a_plan_change_is_triaged_outside_the_locks_and_the_next_round_applies_the_decision() {
    if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
        return;
    }
    for mode in MODES {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let seen = Rc::new(RefCell::new(Vec::new()));
        jev_for(root, mode, &seen);
        let UndoneReturn { b, change, accepted, .. } = return_with_an_undone_task(root);
        let held = || task_now(root, &b).wave().is_none() && task_now(root, &b).str_field("blocked_by") == Some(change.as_str());
        if mode == Mode::Off {
            assert!(waiting(&accepted).is_empty() && held(), "today's rule in the same round: {accepted}");
            continue;
        }
        assert_eq!(waiting(&accepted).len(), 1, "the return waits for the decision: {accepted}");
        assert!(!held(), "{accepted}");
        assert_eq!(seen.take(), vec![true], "{accepted}");
        if mode == Mode::Fails {
            let request = accepted["reserves"][0].clone();
            let again = round(root, "x", None);
            assert_eq!(waiting(&again).len(), 1, "{again}");
            assert_eq!(seen.take(), vec![true], "{again}");
            let key = format!("covered:t{}", task_now(root, &b).id);
            crate::shared::jev::reserve::answer_in_test(root, &request, |q| if q.contains("closest subject") { "none".into() } else { key.clone() }, "sure");
        }
        let applied = round(root, "x", None);
        ask_in_test(None);
        assert!(seen.take().is_empty(), "{applied}");
        assert!(waiting(&applied).is_empty() && held(), "{applied}");
    }
}
