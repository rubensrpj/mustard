//! A onda tarefa por tarefa. O pedido traz inteiro só o bloco da primeira
//! tarefa; o envio guarda o de todas (`tasks`), e a resposta do passo que
//! termina uma tarefa traz o bloco da seguinte e a libera. A liberação é um
//! recibo de leitura com a tarefa em `released`, gravado junto dos outros
//! recibos: entregar o texto conta como lido, e o recibo vale entre
//! processos. De dentro da cópia, ler adiante é recusado; a entrega cobra a
//! leitura só do que já foi liberado. O envio de antes, sem `tasks`, libera
//! tudo de uma vez, como sempre.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::wave_prompt::TaskBlock;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

use super::answer::ref_shown;
use super::queue::unanswered_sends;
use super::read_check::{opened_at, request_name};

/// O bloco de uma tarefa como o envio o guardou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SentBlock {
    /// O código da tarefa.
    pub task: String,
    /// O texto do bloco.
    pub text: String,
    /// O que o bloco entrega inteiro: o código da tarefa e o dos itens que só
    /// ele traz.
    pub items: Vec<String>,
}

/// O campo `tasks` do envio: o bloco de cada tarefa, na ordem de execução,
/// com as skills que entraram inteiras nele, quando entrou alguma. Sem
/// tarefa, nada.
pub(crate) fn blocks_field(tasks: &[TaskBlock]) -> Option<(String, Value)> {
    let blocks: Vec<Value> = tasks
        .iter()
        .map(|block| {
            let mut shown = json!({ "task": block.task, "text": block.text, "items": block.items });
            if !block.skills.is_empty() {
                shown["skills"] = json!(block.skills);
            }
            shown
        })
        .collect();
    (!blocks.is_empty()).then(|| ("tasks".to_string(), json!(blocks)))
}

/// Grava como lido o bloco da primeira tarefa da onda `wave`, que o pedido
/// entrega inteiro: pelo mesmo recibo da leitura de dentro da cópia.
pub(crate) fn record_first_block(root: &Path, spec: &str, wave: u64, tasks: &[TaskBlock]) {
    if let Some(first) = tasks.first() {
        let _ = crate::commands::spec_events::conversation::record_receipt(root, spec, &request_name(Some(wave)), &first.items, Instant::now());
    }
}

/// Os blocos que o envio `sent` guardou, na ordem de execução. `None` no
/// envio de antes deles, que libera a onda inteira.
pub(crate) fn sent_blocks(log: &SpecLog, sent: u64) -> Option<Vec<SentBlock>> {
    let tasks = log.send_field(sent, "tasks")?.as_array()?;
    Some(
        tasks
            .iter()
            .filter_map(|block| {
                let task = block.get("task")?.as_str()?.to_string();
                let text = block.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
                let items = block.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                Some(SentBlock { task, text, items })
            })
            .collect(),
    )
}

/// As tarefas que o pedido `sent` da onda `wave` já liberou: a primeira,
/// que o pedido traz, e cada uma que um recibo liberou depois dele — também
/// num reenvio, que não zera as liberações ([`opened_at`]).
pub(crate) fn released(log: &SpecLog, sent: u64, wave: u64) -> BTreeSet<String> {
    let Some(blocks) = sent_blocks(log, sent) else { return BTreeSet::new() };
    let request = request_name(Some(wave));
    let since = opened_at(log, sent);
    let receipts = log
        .events
        .iter()
        .filter(|e| e.id > since && e.event_type == "call")
        .filter(|e| e.str_field("command") == Some("read") && e.str_field("request") == Some(request.as_str()))
        .filter_map(|e| e.str_field("released").map(str::to_string));
    blocks.first().map(|first| first.task.clone()).into_iter().chain(receipts).collect()
}

/// O que o pedido `sent` da onda `wave` ainda segura: cada item que só os
/// blocos das tarefas não liberadas entregam, com a tarefa que o traz. Vazio
/// no envio de antes dos blocos.
pub(crate) fn held_back(log: &SpecLog, sent: u64, wave: u64) -> BTreeMap<String, String> {
    let Some(blocks) = sent_blocks(log, sent) else { return BTreeMap::new() };
    let free = released(log, sent, wave);
    let mut held = BTreeMap::new();
    for block in blocks.iter().filter(|block| !free.contains(&block.task)) {
        for item in &block.items {
            held.entry(item.clone()).or_insert_with(|| block.task.clone());
        }
    }
    held
}

/// [`held_back`] da onda `wave` pelo pedido dela que ainda não voltou: o
/// mesmo que a leitura de dentro da cópia acha.
pub(crate) fn held_in_wave(log: &SpecLog, wave: u64) -> BTreeMap<String, String> {
    unanswered_sends(log).get(&wave).map(|sent| held_back(log, *sent, wave)).unwrap_or_default()
}

/// O que a resposta do passo traz depois de gravá-lo.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// O bloco da próxima tarefa, liberada agora.
    Task(String),
    /// Nenhuma tarefa da onda ficou por terminar.
    Finished,
}

/// A próxima tarefa depois do passo `json` gravado na spec `spec` de `root`:
/// o passo que aponta uma tarefa com que o pedido aberto da onda saiu libera a
/// primeira tarefa da ordem que ainda não tem passo de término, e devolve o
/// bloco dela; sem nenhuma, avisa que todas terminaram. O passo de um
/// critério, o de uma tarefa que o pedido não leva e o pedido de antes dos
/// blocos não liberam nada.
pub(crate) fn after_step(root: &Path, spec: &str, json: &str, started: Instant) -> Option<Next> {
    let step: Value = serde_json::from_str(json).ok()?;
    let wave = step.get("wave").and_then(Value::as_u64)?;
    let log = store::read(&store::spec_file(root, spec).ok()?).ok()??;
    let sent = unanswered_sends(&log).get(&wave).copied()?;
    let blocks = sent_blocks(&log, sent)?;
    let item = ref_shown(step.get("item"), &log.codes()).trim().to_string();
    if !blocks.iter().any(|block| block.task == item) {
        return None;
    }
    let finished = finished_tasks(&log, sent, wave);
    let Some(next) = blocks.into_iter().find(|block| !finished.contains(&block.task)) else {
        return Some(Next::Finished);
    };
    let _ = crate::commands::spec_events::conversation::record_release(root, spec, &request_name(Some(wave)), &next.task, &next.items, started);
    Some(Next::Task(next.text))
}

/// As tarefas da onda `wave` com passo de término desde o pedido `sent`, pelo
/// código.
fn finished_tasks(log: &SpecLog, sent: u64, wave: u64) -> BTreeSet<String> {
    let since = opened_at(log, sent);
    let codes = log.codes();
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "step" && e.wave() == Some(wave) && e.id > since)
        .map(|e| ref_shown(e.fields.get("item"), &codes).trim().to_string())
        .collect()
}

/// O que o reenvio da onda `wave`, que repete o pedido `previous`, acrescenta
/// a ele: o bloco da tarefa em curso — a primeira da ordem sem passo de
/// término —, que fica liberada, com a frase que diz que o trabalho continua
/// dela. A primeira tarefa o pedido já traz, e com todas terminadas não há o
/// que acrescentar.
pub(crate) fn resend_block(root: &Path, spec: &str, log: &SpecLog, previous: u64, wave: u64, lang: Locale) -> Option<String> {
    let blocks = sent_blocks(log, previous)?;
    let finished = finished_tasks(log, previous, wave);
    let (at, current) = blocks.iter().enumerate().find(|(_, block)| !finished.contains(&block.task))?;
    if at == 0 {
        return None;
    }
    if !released(log, previous, wave).contains(&current.task) {
        let _ = crate::commands::spec_events::conversation::record_release(root, spec, &request_name(Some(wave)), &current.task, &current.items, Instant::now());
    }
    Some(format!("\n\n{}\n\n{}", translate("round.resume.task", lang), current.text))
}

// The one test reads processes the Linux way.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use mustard_core::io::spec_events as store;
    use serde_json::json;
    use tempfile::tempdir;

    use super::super::tests::{approved_with, orphan_the_send, request_at, round, waves_in, write};
    use super::*;
    use crate::commands::spec_events::write::{WriteOpts, write_at};

    /// O reenvio da onda cujo agente parou no meio traz, além do pedido, o
    /// bloco inteiro da tarefa em curso — a primeira sem passo de término —,
    /// e a deixa liberada: o agente novo continua dela sem ler adiante.
    #[test]
    fn the_resend_of_an_orphan_wave_brings_the_task_in_progress_whole_and_releases_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved_with(root, "x", &[(1, &["src/a.rs"], &[])], |said| {
            let task = json!({"wave": 1, "title": "Escrever o total", "text": "Segunda tarefa.", "agent": "- somar em centavos inteiros",
                "files": [{"path": "src/a.rs"}], "depends_on": [], "origin": said});
            assert_eq!(write(root, "x", "task", task)["ok"], json!(true));
        });
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1]);
        let step = json!({"wave": 1, "item": "MSTD-TASK-0001", "text": "A primeira ficou pronta."});
        let wrote = write_at(&WriteOpts { root: root.to_path_buf(), spec: Some("x".into()), event_type: "step".into(), json: step.to_string() });
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        orphan_the_send(root, 1);

        let out = round(root, "x", None);
        assert_eq!(waves_in(&out, "dispatch"), vec![1], "the orphan goes out again: {out}");
        let text = request_at(&out, 0);
        assert!(text.contains("- somar em centavos inteiros"), "the task in progress comes whole: {text}");
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let sent = unanswered_sends(&log)[&1];
        assert!(released(&log, sent, 1).contains("MSTD-TASK-0002"), "the task in progress is released");
    }
}
