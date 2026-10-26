//! A tarefa de um achado do veredito final aprovado depois que a rodada a
//! regrava: as versões que a rodada grava (a da onda e a da volta ao
//! backlog) trazem o autor `binary`, e o `remove` segue recusado em todas.

use std::path::Path;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::{PhaseWriter, final_approval};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::Locale;
use serde_json::{Map, Value, json};
use tempfile::tempdir;

use super::queue::task_revision;
use super::tests::{without_measurements, write};
use super::{backlog_return, record_final_leftovers};
use crate::commands::spec_events::write::record;
use crate::shared::spec_state::seed_event;

const SPEC: &str = "teste";

/// A spec como está gravada agora.
fn log_of(root: &Path) -> SpecLog {
    store::read(&store::spec_file(root, SPEC).unwrap()).unwrap().unwrap()
}

/// A spec com o veredito final aprovado que traz um achado em `src/main.rs`,
/// que o fechamento grava como pedido e tarefa. Devolve a tarefa do achado.
fn spec_with_a_final_finding(root: &Path) -> u64 {
    std::fs::write(root.join("mustard.json"), "{}").unwrap();
    seed_event(root, SPEC, "send", json!({"role": "review", "text": "revise", "lines": 1, "chars": 6, "mustard": "0", "author": "binary"}));
    let findings = json!([{"title": "A soma não trata estouro", "detail": "A soma estoura em src/main.rs.", "files": ["src/main.rs"], "rule": "A soma trata estouro", "places": ["src/main.rs"]}]);
    seed_event(root, SPEC, "verdict", json!({"final": true, "result": "approved", "text": "Pronta, com um achado.", "author": "review", "leftovers": findings}));
    let log = log_of(root);
    let approval = final_approval(&log).expect("o veredito final aprova");
    let recorded = record_final_leftovers(root, root, SPEC, (&log, approval), Locale::PtBr).expect("o fechamento grava o achado");
    recorded.iter().find(|entry| entry["type"] == json!("task")).and_then(|entry| entry["id"].as_u64()).expect("o achado virou tarefa")
}

/// A versão `draft` da tarefa, gravada como a rodada a grava: pela
/// gravação da spec, com a fase do programa. Devolve o número dela.
fn recorded_by_the_round(root: &Path, draft: Map<String, Value>) -> u64 {
    record(root, SPEC, "task", draft, PhaseWriter::Binary).expect("a rodada grava a versão dela da tarefa").written.id
}

/// Depois de a rodada pôr a tarefa do achado numa onda e devolvê-la ao
/// backlog, a versão vigente já não é a de autor `review`, mas o `remove` de
/// qualquer versão dela, pelo número, pelo código ou por filtro de horário,
/// segue recusado, e o arquivo não muda: o fechamento recriaria a tarefa, e o
/// achado tem de ser feito. As versões vêm da rodada (`task_revision` e
/// `backlog_return`), e não de um rascunho montado à mão.
#[test]
fn the_task_of_a_final_finding_stays_guarded_in_every_version_the_round_writes() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let finding = spec_with_a_final_finding(root);
    let code = log_of(root).codes()[&finding].clone();

    let revision = task_revision(&log_of(root), finding, Map::from_iter([("wave".to_string(), json!(1))])).expect("a tarefa do achado está na spec");
    let in_wave = recorded_by_the_round(root, revision);
    let back = recorded_by_the_round(root, backlog_return(log_of(root).get(in_wave).expect("a versão na onda")));

    let log = log_of(root);
    assert_eq!(log.get(in_wave).and_then(|task| task.wave()), Some(1), "a versão da rodada põe a tarefa na onda");
    assert_eq!(log.get(back).and_then(|task| task.wave()), None, "a versão da volta devolve a tarefa ao backlog");
    let current = log.current(finding).expect("a tarefa do achado segue vigente");
    assert_eq!(current.id, back, "a versão da rodada é a vigente");
    assert_eq!(current.str_field("author"), Some("binary"), "a versão vigente já não é a de autor `review`");

    let file = store::spec_file(root, SPEC).unwrap();
    let everything = json!({"type": "task", "from": "2000-01-01T00:00:00", "to": "2999-12-31T23:59:59"});
    let by_number = [finding, in_wave, back].into_iter().map(|id| json!({"targets": [id]}));
    for removal in by_number.chain([json!({"targets": [code.clone()]}), json!({"filter": everything})]) {
        let before = without_measurements(&std::fs::read_to_string(&file).unwrap());
        let mut draft = removal.clone();
        draft["reason"] = json!("não vale");
        let refused = write(root, SPEC, "remove", draft);
        assert_eq!(refused["reason"], json!("final-finding-removed"), "a remoção {removal} devia ser recusada: {refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(&code), "a recusa da remoção {removal} devia citar a tarefa {code}: {hint}");
        assert_eq!(without_measurements(&std::fs::read_to_string(&file).unwrap()), before, "a remoção {removal} recusada não grava nada");
    }
    let log = log_of(root);
    let tasks: Vec<u64> = log.visible().into_iter().filter(|event| event.event_type == "task").map(|event| event.id).collect();
    assert_eq!(tasks, vec![back], "a tarefa do achado segue na spec, só com a versão da volta");
}
