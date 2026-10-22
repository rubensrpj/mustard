//! A onda da versão nova de uma tarefa que o modelo grava pelo `run write`.
//!
//! A onda nasce do backlog e só o programa a monta; o modelo grava a tarefa
//! sem `wave`. A versão nova de uma tarefa cuja onda tem o agente trabalhando
//! (envio aberto), gravada assim, não pode tirá-la de lá: a onda ficaria sem a
//! tarefa que o agente entrega, e a entrega dele seria recusada. A tarefa de
//! um lote que ainda não saiu, ou de uma onda que já voltou, regravada sem
//! `wave` volta ao backlog, como sempre voltou.

use mustard_core::domain::spec_events::{SpecEvent, SpecLog, resolve_codes};
use serde_json::{Map, Value, json};

use crate::commands::flow::round::queue::{unanswered_sends, waves_returned};

/// A versão nova de uma tarefa que não traz `wave` herda a da versão que ela
/// substitui, quando o agente dessa onda trabalha: a onda tem envio sem volta
/// ([`unanswered_sends`]) e o agente ainda não gravou a entrega
/// ([`waves_returned`]). Com um número de onda no rascunho, ele fica como
/// veio: a conferência da gravação recusa o que muda a onda da tarefa. A
/// versão que não substitui nada, ou que substitui uma tarefa do backlog, de
/// lote que não saiu ou de onda que já voltou, segue sem onda.
pub(super) fn inherit_wave(log: &SpecLog, draft: &mut Map<String, Value>) {
    if draft.get("wave").is_some_and(|wave| !wave.is_null()) {
        return;
    }
    let replaced_wave = replaced_id(log, draft).and_then(|old| log.get(old)).and_then(SpecEvent::wave);
    if let Some(wave) = replaced_wave.filter(|wave| agent_working(log, *wave)) {
        draft.insert("wave".into(), json!(wave));
    }
}

/// O número da versão que o rascunho substitui. O `run write` recebe o
/// `replaces` pelo código do item (`MSTD-TASK-NNNN`), e a gravação só o troca
/// pelo número depois desta leitura: o código vira aqui o número da versão
/// mais nova dele, numa cópia, sem mexer no rascunho. O código que não se
/// acha fica para a conferência da gravação, que o recusa.
fn replaced_id(log: &SpecLog, draft: &Map<String, Value>) -> Option<u64> {
    let replaces = draft.get("replaces").filter(|value| !value.is_array())?;
    let mut probe = Map::new();
    probe.insert("replaces".into(), replaces.clone());
    resolve_codes(log, &mut probe).ok()?;
    probe.get("replaces").and_then(Value::as_u64)
}

/// O agente da onda `wave` trabalha: ela saiu e ele não voltou.
fn agent_working(log: &SpecLog, wave: u64) -> bool {
    unanswered_sends(log).contains_key(&wave) && !waves_returned(log).contains(&wave)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    /// Uma spec com uma tarefa do backlog (id 5), uma da onda 3 (id 4), que
    /// tem envio sem volta (id 6), e uma da onda 7 (id 8), que ainda não saiu.
    /// Com `returned`, o agente da onda 3 gravou a entrega dele.
    fn log_with_tasks(returned: bool) -> SpecLog {
        let dir = tempdir().unwrap();
        let path = store::spec_file(dir.path(), "x").unwrap();
        let fields = |value: Value| value.as_object().cloned().unwrap();
        let write = |event_type: &str, value: Value| store::write(&path, event_type, fields(value), &[]).unwrap().id;
        let said = write("message", json!({"author": "user", "text": "O pedido."}));
        let criterion = write("criterion", json!({"when": "a", "then": "b", "proof": "git --version", "form": "ubiquitous", "origin": said}));
        let wave = |n: u64| json!({"author": "binary", "n": n, "text": "Onda.", "criteria": [criterion], "done_when": "passa", "origin": said});
        let task = |wave: Option<u64>| {
            let mut task = json!({"text": "Tarefa.", "files": [], "depends_on": [], "origin": said});
            if let Some(wave) = wave {
                task["wave"] = json!(wave);
            }
            task
        };
        write("wave", wave(3));
        write("task", task(Some(3)));
        write("task", task(None));
        write("send", json!({"author": "binary", "wave": 3, "role": "wave", "text": "pedido", "lines": 1, "chars": 6, "items": [4], "mustard": "0"}));
        write("wave", wave(7));
        write("task", task(Some(7)));
        if returned {
            write("delivered", json!({"wave": 3, "text": "Pronta.", "files": [], "returned": true}));
        }
        store::read(&path).unwrap().unwrap()
    }

    fn draft(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn the_version_without_the_wave_takes_the_wave_that_has_an_open_send() {
        let log = log_with_tasks(false);
        let mut absent = draft(json!({"replaces": 4, "text": "Revista."}));
        inherit_wave(&log, &mut absent);
        assert_eq!(absent.get("wave"), Some(&json!(3)));
        let mut null = draft(json!({"replaces": 4, "wave": null}));
        inherit_wave(&log, &mut null);
        assert_eq!(null.get("wave"), Some(&json!(3)));
        let code = log.codes().get(&4).cloned().unwrap();
        let mut by_code = draft(json!({"replaces": code, "text": "Revista."}));
        inherit_wave(&log, &mut by_code);
        assert_eq!(by_code.get("wave"), Some(&json!(3)));
        assert_eq!(by_code.get("replaces"), Some(&json!(code)), "o rascunho segue com o código");
    }

    #[test]
    fn a_wave_number_in_the_draft_stays_as_it_came() {
        let log = log_with_tasks(false);
        let mut other = draft(json!({"replaces": 4, "wave": 7}));
        inherit_wave(&log, &mut other);
        assert_eq!(other.get("wave"), Some(&json!(7)));
    }

    #[test]
    fn a_version_of_a_backlog_task_or_of_a_batch_not_sent_or_of_a_wave_that_returned_stays_without_a_wave() {
        let log = log_with_tasks(false);
        let mut backlog = draft(json!({"replaces": 5}));
        inherit_wave(&log, &mut backlog);
        let mut unsent = draft(json!({"replaces": 8}));
        inherit_wave(&log, &mut unsent);
        let mut returned = draft(json!({"replaces": 4}));
        inherit_wave(&log_with_tasks(true), &mut returned);
        let mut fresh = draft(json!({"text": "Nova."}));
        inherit_wave(&log, &mut fresh);
        let mut unknown = draft(json!({"replaces": 99}));
        inherit_wave(&log, &mut unknown);
        for stays in [backlog, unsent, returned, fresh, unknown] {
            assert!(!stays.contains_key("wave"), "{stays:?}");
        }
    }
}
