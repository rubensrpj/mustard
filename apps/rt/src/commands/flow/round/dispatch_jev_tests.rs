//! As perguntas ao Jev do despacho — a formação e a ordem da lista, a
//! montagem das ondas e a escolha dos itens do pedido (a das skills mora em
//! `skill_cycle_tests`) — nunca saem com a trava do passo do git ou a das
//! cópias presa: saem antes delas, e o despacho lê a decisão gravada. O Jev
//! que falha manda a pergunta à reserva, e só a onda ou a tarefa que depende
//! dela espera o recibo; as outras saem. Com o Jev desligado, vale a regra de
//! sempre, sem espera.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mustard_core::domain::map_filter::FilterError;
use mustard_core::io::spec_events as store;
use serde_json::{Map, Value, json};
use tempfile::tempdir;

use super::tests::{approved, approved_with, id_of, launched, round, waves_in, write};
use super::{RoundOpts, round_for};
use crate::shared::dag::{Judgement, TaskKind};
use crate::shared::judgement::{Board, ItemsBoard, ItemsJudged, Judged, WaveJudge};

/// How the Jev behaves in a scenario.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Answers,
    Fails,
    Off,
}

/// `true` when nobody holds the git-step lock nor the copies lock of `root`.
fn locks_free(root: &Path) -> bool {
    let free = |name: &str| mustard_core::io::fs::lock::LockedFile::exclusive_if_free(&root.join(".claude/spec").join(name)).unwrap().is_some();
    free("round-git.lock") && free("round-copies.lock")
}

/// The Jev of the assembly and of the items: each call keeps, in `seen`,
/// whether both locks of `root` were free. The assembly fails with
/// `assembly_fails`; the items of a board whose task says `failing` fail.
#[derive(Debug)]
struct FakeJudge {
    root: PathBuf,
    assembly_fails: bool,
    failing: Option<&'static str>,
    seen: Arc<Mutex<Vec<bool>>>,
}

impl WaveJudge for FakeJudge {
    fn judge_backlog(&self, board: &Board) -> Result<Judged, FilterError> {
        self.seen.lock().unwrap().push(locks_free(&self.root));
        if self.assembly_fails {
            return Err(FilterError::Refused { status: 529 });
        }
        let feature = Judgement { kind: TaskKind::Feature, confidence: 0.95, clash: 0.0 };
        Ok(Judged { tasks: board.backlog.iter().map(|task| (task.id, feature)).collect(), usage: Default::default() })
    }

    fn judge_items(&self, board: &ItemsBoard) -> Result<ItemsJudged, FilterError> {
        self.seen.lock().unwrap().push(locks_free(&self.root));
        if self.failing.is_some_and(|said| board.tasks.iter().any(|task| task.text.contains(said))) {
            return Err(FilterError::Refused { status: 529 });
        }
        Ok(ItemsJudged { chances: board.items.iter().map(|item| (item.id, 0.0)).collect(), usage: Default::default() })
    }
}

/// Installs the Jev of `mode` for the next rounds of `root`.
fn judge_for(root: &Path, mode: Mode, assembly_fails: bool, failing: Option<&'static str>) -> Arc<Mutex<Vec<bool>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let judge = FakeJudge { root: root.to_path_buf(), assembly_fails, failing, seen: Arc::clone(&seen) };
    crate::shared::jev::judge_in_test((mode != Mode::Off).then(|| Arc::new(judge) as Arc<dyn WaveJudge>));
    seen
}

/// Every answer the Jev gave since the last look, all with both locks free.
fn asked_with_locks_free(seen: &Arc<Mutex<Vec<bool>>>) -> usize {
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    assert!(seen.iter().all(|free| *free), "a Jev call with a lock held: {seen:?}");
    seen.len()
}

/// The decisions of the dispatch the spec `x` of `root` keeps, by call.
fn decisions(root: &Path, call: &str) -> usize {
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    log.events.iter().filter(|e| e.event_type == "stage_run" && e.str_field("stage") == Some(&format!("jev-dispatch:{call}"))).count()
}

/// The text of the item that the send of wave `wave` took out of its
/// request, by the analysis the send records.
fn removed_by_the_jev(root: &Path, wave: u64) -> Vec<Value> {
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let sent = log.last_by_wave("send")[&wave];
    log.send_field(sent, "analysis").and_then(|analysis| analysis["removed"].as_array().cloned()).unwrap_or_default()
}

/// The choice of the items of each wave asks the Jev with both locks free,
/// and the send uses the recorded answer. The Jev that fails on wave 1 sends
/// its question to the reserve: only wave 1 waits, and wave 2 leaves with the
/// Jev's choice. The receipt sends wave 1 in the next round, without asking
/// the Jev again. With the Jev off, both leave with the default request.
#[test]
fn the_item_choice_asks_outside_the_locks_and_only_the_failed_wave_waits_for_the_receipt() {
    for mode in [Mode::Answers, Mode::Fails, Mode::Off] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])], |said| {
            let rule = json!({"title": "A tabela tem chave.", "text": "A tabela tem chave.", "example": "e", "keys": ["k"],
                "applies_to": {"files": ["**"]}, "origin": said});
            write(root, "x", "rule", rule);
        });
        let seen = judge_for(root, mode, false, (mode == Mode::Fails).then_some("onda 1"));
        let first = round(root, "x", None);
        if mode == Mode::Off {
            crate::shared::jev::judge_in_test(None);
            assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
            assert!(first.get("reserves").is_none(), "with the Jev off nothing waits: {first}");
            assert!(removed_by_the_jev(root, 1).is_empty(), "today's default request: {first}");
            continue;
        }
        assert_eq!(asked_with_locks_free(&seen), 2, "one call per wave, both locks free: {first}");
        assert!(decisions(root, "wave items") >= 1, "the answer is recorded: {first}");
        assert_eq!(removed_by_the_jev(root, 2).len(), 1, "wave 2 leaves with the Jev's choice: {first}");
        if mode == Mode::Answers {
            crate::shared::jev::judge_in_test(None);
            assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
            assert_eq!(removed_by_the_jev(root, 1).len(), 1, "{first}");
            continue;
        }
        assert_eq!(waves_in(&first, "dispatch"), vec![2], "only the wave whose choice failed waits: {first}");
        let request = first["reserves"][0].clone();
        let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("the reserve request: {first}"));
        assert!(prompt.contains("govern something that `task` changes or tests"), "the same question goes to the reserve: {prompt}");
        let warned = first["warnings"].as_array().into_iter().flatten().filter(|w| w["reason"] == json!("items-jev-pending")).count();
        assert_eq!(warned, 1, "{first}");

        crate::shared::jev::reserve::answer_in_test(root, &request, |_| "no".into(), "sure");
        let sent = round(root, "x", None);
        crate::shared::jev::judge_in_test(None);
        assert_eq!(asked_with_locks_free(&seen), 0, "the receipt answers in place of the Jev: {sent}");
        assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
        assert_eq!(removed_by_the_jev(root, 1).len(), 1, "the receipt decides like the Jev: {sent}");
    }
}

/// A task loose in the backlog of spec `x` of `root`, on `file`, covering the
/// criterion of the plan.
pub(super) fn loose_task(root: &Path, text: &str, file: &str) -> u64 {
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let first = |kind: &str| log.visible().into_iter().find(|e| e.event_type == kind).map(|e| e.id).unwrap();
    let (said, crit) = (first("message"), first("criterion"));
    std::fs::write(root.join(file), "fn one() {}\n").unwrap();
    id_of(&write(root, "x", "task", json!({"text": text, "files": [{"path": file}], "depends_on": [], "covers": [crit], "origin": said})))
}

/// The tasks the round answer `out` holds, with the reason.
fn held(out: &Value) -> Vec<Value> {
    out["held"].as_array().cloned().unwrap_or_default()
}

/// The assembly of the backlog asks the Jev with both locks free, and the
/// round assembles by the recorded answer. The Jev that fails sends the
/// question to the reserve: the backlog tasks wait for the receipt, and the
/// planned wave leaves. The receipt assembles them in the next round, without
/// asking the Jev again. With the Jev off, the backlog leaves by the files.
#[test]
fn the_assembly_asks_outside_the_locks_and_only_the_backlog_waits_for_the_receipt() {
    for mode in [Mode::Answers, Mode::Fails, Mode::Off] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        loose_task(root, "Somar o total.", "src/b.rs");
        let seen = judge_for(root, mode, mode == Mode::Fails, None);
        let first = round(root, "x", None);
        if mode == Mode::Off {
            crate::shared::jev::judge_in_test(None);
            assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
            assert!(first.get("reserves").is_none(), "with the Jev off nothing waits: {first}");
            continue;
        }
        assert_eq!(asked_with_locks_free(&seen), 1, "one assembly call, both locks free: {first}");
        if mode == Mode::Answers {
            crate::shared::jev::judge_in_test(None);
            assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
            assert!(decisions(root, "wave assembly") >= 1, "the answer is recorded: {first}");
            continue;
        }
        assert_eq!(waves_in(&first, "dispatch"), vec![1], "the planned wave leaves; the backlog waits: {first}");
        let waiting = held(&first);
        assert!(waiting.iter().any(|row| row["reason"] == json!("jev-pending")), "{first}");
        let request = first["reserves"][0].clone();
        let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("the reserve request: {first}"));
        assert!(prompt.contains("What kind of work is"), "the same question goes to the reserve: {prompt}");

        let kind_or_no = |question: &str| if question.contains("What kind of work") { "feature".to_string() } else { "no".to_string() };
        crate::shared::jev::reserve::answer_in_test(root, &request, kind_or_no, "sure");
        let sent = round(root, "x", None);
        crate::shared::jev::judge_in_test(None);
        assert_eq!(asked_with_locks_free(&seen), 0, "the receipt answers in place of the Jev: {sent}");
        assert_eq!(waves_in(&sent, "dispatch"), vec![2], "{sent}");
    }
}

/// A onda que a montagem do backlog formou e cuja escolha dos itens espera o
/// recibo, porque o Jev falhou, sai na rodada seguinte ao recibo com o número
/// que ganhou: a mesma onda é enviada, e nenhuma onda fica gravada sem
/// tarefa.
#[test]
fn an_assembled_wave_waiting_for_the_item_choice_leaves_with_its_number() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved_with(root, "x", &[], |said| {
        let rule = json!({"title": "A tabela tem chave.", "text": "A tabela tem chave.", "example": "e", "keys": ["k"],
            "applies_to": {"files": ["**"]}, "origin": said});
        write(root, "x", "rule", rule);
    });
    loose_task(root, "Somar o total.", "src/b.rs");
    let seen = judge_for(root, Mode::Fails, false, Some("Somar o total"));
    let first = round(root, "x", None);
    assert_eq!(asked_with_locks_free(&seen), 1, "the item choice goes to the Jev once: {first}");
    assert!(waves_in(&first, "dispatch").is_empty(), "the assembled wave waits for the receipt: {first}");
    let waiting: Vec<u64> =
        first["warnings"].as_array().into_iter().flatten().filter(|w| w["reason"] == json!("items-jev-pending")).filter_map(|w| w["wave"].as_u64()).collect();
    assert_eq!(waiting.len(), 1, "{first}");
    let request = first["reserves"][0].clone();
    assert!(request["delegation"]["request"]["prompt"].as_str().is_some(), "the reserve request: {first}");

    crate::shared::jev::reserve::answer_in_test(root, &request, |_| "no".into(), "sure");
    let sent = round(root, "x", None);
    crate::shared::jev::judge_in_test(None);
    assert_eq!(asked_with_locks_free(&seen), 0, "the receipt answers in place of the Jev: {sent}");
    assert_eq!(waves_in(&sent, "dispatch"), waiting, "the wave that waited leaves with its number: {sent}");
    assert_eq!(removed_by_the_jev(root, waiting[0]).len(), 1, "the receipt decides like the Jev: {sent}");
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let recorded: BTreeSet<u64> = log.events.iter().filter(|e| e.event_type == "wave").filter_map(|e| e.wave()).collect();
    assert_eq!(recorded, log.planned_waves(), "no wave stays recorded without tasks: {sent}");
}

/// A doc answering every question of the request `payload`: a sure yes, the
/// first option, or the first level.
fn every_question_answered(payload: &str) -> Value {
    let body: Value = serde_json::from_str(payload).unwrap();
    let answers: Map<String, Value> = body["questions"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, question)| {
            let answer = match question["type"].as_str() {
                Some("noul") => json!({"type": "noul", "noul": 0.9}),
                Some("score") => json!({"type": "score", "score": 1, "confidence": 0.9, "probabilities": {"1": 0.9}}),
                _ => {
                    let first = question["criteria"].as_object().and_then(|c| c.keys().next().cloned()).unwrap_or_default();
                    json!({"type": "choice", "choice": first, "confidence": 0.9, "probabilities": {first: 0.9}})
                }
            };
            (key.clone(), answer)
        })
        .collect();
    json!({ "answers": answers })
}

/// The order of the formation asks the Jev with both locks free, before the
/// round records the formed waves, and the formation reads the recorded
/// answer: the formed wave leaves in the same round.
#[test]
fn the_order_of_the_formation_asks_outside_the_locks() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    approved(root, "x", &[]);
    loose_task(root, "A retomada lista as ondas no ar.", "src/resume.rs");
    loose_task(root, "A busca não espera a trava do mapa.", "src/search.rs");
    let session = || {
        super::review::test_conductor_review(root, "x");
        let out = round_for(&RoundOpts { root: root.to_path_buf(), spec: Some("x".into()), report: None }, Some("sessao"));
        launched(root, "x", out)
    };
    let pending = session();
    let request = pending["formation"]["request_id"].as_str().unwrap_or_else(|| panic!("the reading of the small model: {pending}")).to_string();
    let receipt = mustard_core::io::knowledge::enrichment::SessionReceipt {
        request_id: request,
        model: "inherit".into(),
        effort: None,
        result: json!({ "waves": [
            { "objective": "A retomada mostra as ondas", "tasks": ["MSTD-TASK-0001"], "merges": [] },
            { "objective": "A busca responde sem esperar", "tasks": ["MSTD-TASK-0002"], "merges": [] },
        ] }),
        input_tokens: None,
        output_tokens: None,
    };
    mustard_core::io::knowledge::enrichment::complete_session(root, root, &receipt).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (spy, at) = (Arc::clone(&seen), root.to_path_buf());
    crate::shared::jev::yes_no::ask_in_test(Some(std::rc::Rc::new(move |payloads: &[String]| {
        spy.lock().unwrap().push(locks_free(&at));
        Some(payloads.iter().map(|payload| every_question_answered(payload)).collect())
    })));
    let formed = session();
    crate::shared::jev::yes_no::ask_in_test(None);
    assert!(asked_with_locks_free(&seen) >= 1, "the order goes to the Jev: {formed}");
    assert!(decisions(root, "task triage") >= 1, "the answer is recorded: {formed}");
    assert!(!waves_in(&formed, "dispatch").is_empty(), "the formed waves leave in the same round: {formed}");
}
