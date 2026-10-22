//! A conferência de que o agente leu o que o pedido dele lista: o envio guarda
//! a lista (`read_items`), a leitura de dentro da cópia grava um recibo — uma
//! chamada `read` com os itens achados —, e a entrega da onda e o veredito da
//! revisão final se recusam quando algum item da lista ficou sem recibo.

use std::collections::{BTreeMap, BTreeSet};

use mustard_core::domain::spec_events::{BlockQuery, SpecLog};
use serde_json::Value;

/// O nome com que a leitura registra o pedido de uma onda (`request-<onda>`)
/// ou o da revisão final (`request-review`, sem onda): o mesmo que a
/// conferência da entrega e a do veredito procuram nas chamadas `read`.
pub(crate) fn request_name(wave: Option<u64>) -> String {
    wave.map_or_else(|| String::from("request-review"), |wave| format!("request-{wave}"))
}

/// A posição a partir da qual as leituras e os passos valem para o pedido
/// `sent`: a em que a onda abriu esta rodada dela.
///
/// Recua por dois caminhos, até o primeiro pedido da rodada. Pelo `resends`,
/// quando o pedido é um reenvio: o reenvio não zera as leituras nem os passos
/// da onda. E pelo pedido anterior da mesma onda quando nada voltou entre os
/// dois ([`same_round_before`]): um pedido novo gravado enquanto o agente
/// ainda trabalha não apaga o que ele já leu pelo pedido de antes. Em campo,
/// essa troca de pedido no meio do trabalho recusou a entrega do agente, e é a
/// mesma causa de ondas que nunca saíram (93 recusas até 21/10/2026).
///
/// Ampliar a janela não afrouxa a conferência: [`unread_items`] cobra recibo
/// por CÓDIGO de item, então o item que o pedido novo trouxe segue sem recibo
/// e segue cobrado. O que a janela maior salva é só o que o agente leu de
/// verdade.
pub(super) fn opened_at(log: &SpecLog, sent: u64) -> u64 {
    let mut first = sent;
    // As duas cadeias são finitas, mas a spec é um arquivo de texto: o passo
    // conta os saltos para um círculo gravado à mão não prender a volta.
    for _ in 0..log.events.len() {
        let resent = log
            .get(first)
            .and_then(|e| e.int("resends"))
            .filter(|n| log.get(*n).is_some_and(|e| e.event_type == "send"));
        let Some(earlier) = resent.or_else(|| same_round_before(log, first)) else {
            break;
        };
        first = earlier;
    }
    log.dispatch_position(first)
}

/// O pedido anterior da mesma onda, quando nada da onda voltou entre os dois:
/// nem entrega, nem veredito. `None` quando `sent` é o primeiro pedido da
/// onda, ou quando a onda voltou antes dele — aí ele abre uma rodada nova, e
/// as leituras da rodada anterior não valem.
fn same_round_before(log: &SpecLog, sent: u64) -> Option<u64> {
    let wave = log.get(sent)?.wave()?;
    let of_wave = |kind: &str| -> Vec<u64> {
        log.events
            .iter()
            .filter(|e| e.event_type == kind && e.wave() == Some(wave))
            .map(|e| e.id)
            .collect()
    };
    let earlier = of_wave("send").into_iter().rfind(|id| *id < sent)?;
    let answered = of_wave("delivered").into_iter().chain(of_wave("verdict"));
    let between = answered.filter(|id| *id > earlier && *id < sent).count() > 0;
    (!between).then_some(earlier)
}

/// Os itens que o pedido `sent` lista e que ninguém leu desde que ele saiu
/// ([`opened_at`]), na ordem em que o pedido os lista: o código do item, ou
/// `lesson-<número>`. Lido é o que uma chamada `read` gravou para o pedido
/// `request` ([`request_name`]). O pedido sem a lista — gravado antes de o
/// envio guardá-la — não cobra leitura nenhuma.
pub(crate) fn unread_items(log: &SpecLog, sent: u64, request: &str) -> Vec<String> {
    let Some(listed) = log.send_field(sent, "read_items").and_then(Value::as_array) else {
        return Vec::new();
    };
    let since = opened_at(log, sent);
    // O recibo de agora traz todos os itens de uma leitura em `items`; o de
    // antes, um por item, em `item`. Os dois contam. Guarda o recibo MAIS NOVO
    // de cada item: é ele que se compara com a versão do item.
    let mut read: BTreeMap<String, u64> = BTreeMap::new();
    for event in log.events.iter().filter(|e| e.id > since && e.event_type == "call") {
        if event.str_field("command") != Some("read") || event.str_field("request") != Some(request) {
            continue;
        }
        let items = event
            .str_field("item")
            .into_iter()
            .chain(event.fields.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str));
        for item in items {
            let at = read.entry(item.to_string()).or_insert(event.id);
            *at = (*at).max(event.id);
        }
    }
    // A versão mais nova de cada código. O item que ganhou versão depois de
    // ser lido volta a ser cobrado: o recibo é de um texto que não existe
    // mais. A lição não tem código de item e nunca entra aqui.
    let mut newest: BTreeMap<String, u64> = BTreeMap::new();
    for (id, code) in log.codes() {
        let at = newest.entry(code).or_insert(id);
        *at = (*at).max(id);
    }
    listed
        .iter()
        .filter_map(Value::as_str)
        .filter(|item| match read.get(*item) {
            None => true,
            Some(at) => newest.get(*item).is_some_and(|version| version > at),
        })
        .map(str::to_string)
        .collect()
}

/// Tira de `unread` o que só as tarefas devolvidas em `undone` pedem: o código
/// da tarefa e os itens que só ela atende, o que ela cobre e a mensagem de onde
/// nasceu. A leitura é das tarefas feitas: o que uma tarefa feita também
/// atende, e o que vale para a onda toda (regras, decisões e lições), segue
/// cobrado.
pub(super) fn without_undone(log: &SpecLog, wave: u64, undone: &[String], unread: Vec<String>) -> Vec<String> {
    if undone.is_empty() {
        return unread;
    }
    let codes = log.codes();
    let code_of = |id: u64| codes.get(&id).cloned();
    let mut returned: BTreeSet<String> = undone.iter().cloned().collect();
    let mut kept: BTreeSet<String> = BTreeSet::new();
    for task in log.block(BlockQuery::Wave(wave)).iter().filter(|e| e.event_type == "task") {
        let attended = task.ints("covers").into_iter().chain(task.int("origin")).filter_map(code_of);
        if code_of(task.id).is_some_and(|code| undone.contains(&code)) {
            returned.extend(attended);
        } else {
            kept.extend(attended);
        }
    }
    unread.into_iter().filter(|item| !returned.contains(item) || kept.contains(item)).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mustard_core::io::spec_events as store;
    use serde_json::json;

    use super::*;
    use crate::commands::flow::round::seed_read;
    use crate::shared::spec_state::seed_event;

    /// Um envio da onda 1 com a lista de leitura de `items`, e o número dele.
    fn send(root: &Path, items: &[&str], resends: Option<u64>) -> u64 {
        let mut fields = json!({"wave": 1, "role": "wave", "text": "pedido", "lines": 1, "chars": 6, "items": [1],
            "read_items": items, "mustard": "0", "author": "binary"});
        if let Some(previous) = resends {
            fields["resends"] = json!(previous);
        }
        seed_event(root, "x", "send", fields)
    }

    fn unread(root: &Path, sent: u64) -> Vec<String> {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        unread_items(&log, sent, &request_name(Some(1)))
    }

    /// O reenvio não zera as leituras: o que o agente leu depois do primeiro
    /// envio vale para o reenvio, que é o mesmo pedido. O pedido novo gravado
    /// enquanto nada voltou também não zera, porque o agente leu pelo pedido
    /// de antes dele. E a leitura de antes do primeiro envio nunca conta.
    ///
    /// O pedido novo zerava a leitura, e era isso que recusava a entrega de um
    /// agente cujo pedido o condutor trocou no meio do trabalho: a mesma causa
    /// de ondas que nunca saíram (93 recusas até 21/10/2026). O que passou a
    /// cobrar releitura é o item mudar de versão, não chegar pedido novo.
    #[test]
    fn a_resend_keeps_the_reading_made_since_the_first_send() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let items = ["MSTD-TASK-0001", "lesson-3"];
        seed_read(root, "x", &request_name(Some(1)), "MSTD-TASK-0001");
        let first = send(root, &items, None);
        assert_eq!(unread(root, first), items, "the reading made before the send does not count");
        seed_read(root, "x", &request_name(Some(1)), "MSTD-TASK-0001");
        assert_eq!(unread(root, first), ["lesson-3"]);

        let resent = send(root, &items, Some(first));
        assert_eq!(unread(root, resent), ["lesson-3"], "the resend keeps the reading of the first send");
        let again = send(root, &items, Some(resent));
        assert_eq!(unread(root, again), ["lesson-3"], "and so does the resend of the resend");

        let fresh = send(root, &items, None);
        assert_eq!(unread(root, fresh), ["lesson-3"], "a new request with nothing returned keeps the reading");
    }

    /// O item que ganhou versão nova depois de ser lido volta a ser cobrado: o
    /// recibo é de um texto que não existe mais. É isto que cobra a releitura,
    /// no lugar de um pedido novo zerar tudo.
    #[test]
    fn an_item_that_got_a_new_version_after_the_reading_is_charged_again() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let items = ["MSTD-TASK-0001"];
        let said = seed_event(root, "x", "message", json!({"author": "user", "text": "o plano"}));
        let task = seed_event(
            root,
            "x",
            "task",
            json!({"text": "a tarefa", "origin": said, "files": [{"path": "src/a.rs"}], "depends_on": [], "wave": 1}),
        );
        let first = send(root, &items, None);
        seed_read(root, "x", &request_name(Some(1)), "MSTD-TASK-0001");
        assert!(unread(root, first).is_empty(), "o item lido não é cobrado");

        // A versão nova da tarefa herda o código da que ela substitui.
        seed_event(
            root,
            "x",
            "task",
            json!({"text": "a tarefa, de novo", "origin": said, "files": [{"path": "src/a.rs"}], "depends_on": [], "wave": 1,
                "replaces": [task]}),
        );
        assert_eq!(unread(root, first), items, "a versão nova volta a cobrar a leitura");
        seed_read(root, "x", &request_name(Some(1)), "MSTD-TASK-0001");
        assert!(unread(root, first).is_empty(), "e a releitura fecha a conta");
    }

    /// A tarefa devolvida em `undone` não cobra leitura: nem o código dela, nem
    /// o que só ela atende. O que a tarefa feita também atende e o que vale
    /// para a onda toda seguem cobrados.
    #[test]
    fn the_reading_of_a_returned_task_is_not_charged() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let seed = |event_type: &str, body: serde_json::Value| seed_event(root, "x", event_type, body);
        let shared = seed("message", json!({"author": "user", "text": "o plano"}));
        let own_done = seed("criterion", json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": shared}));
        let own_back = seed("criterion", json!({"when": "c", "then": "d", "proof": "p", "form": "ubiquitous", "origin": shared}));
        let only_back = seed("message", json!({"author": "user", "text": "outro pedido"}));
        seed("wave", json!({"n": 1, "text": "Onda.", "criteria": [own_done, own_back], "done_when": "x", "origin": shared}));
        let task = |title: &str, covers: u64, origin: u64| {
            seed("task", json!({"wave": 1, "title": title, "text": "Fazer.", "agent": "Fazer.", "covers": [covers],
                "files": [{"path": format!("src/{title}.ts")}], "origin": origin}))
        };
        let done = task("feita", own_done, shared);
        let back = task("devolvida", own_back, only_back);
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        let codes = log.codes();
        let code = |id: u64| codes.get(&id).cloned().unwrap();
        let unread: Vec<String> = [done, back, own_done, own_back, shared, only_back].iter().map(|id| code(*id)).chain(["MSTD-RULE-0001".to_string()]).collect();

        let left = without_undone(&log, 1, &[code(back)], unread.clone());
        let expected: Vec<String> = [done, own_done, shared].iter().map(|id| code(*id)).chain(["MSTD-RULE-0001".to_string()]).collect();
        assert_eq!(left, expected, "only the reading of the done task and of the whole wave stays");
        assert_eq!(without_undone(&log, 1, &[], unread.clone()), unread, "nothing returned, everything charged");
    }

    /// O recibo de antes, um evento por item (`item`), e o de agora, um
    /// evento por leitura com a lista dos itens (`items`), contam igual: a
    /// spec gravada antes da mudança segue cobrando só o que ninguém leu.
    #[test]
    fn unread_items_counts_single_item_and_item_list_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sent = send(root, &["MSTD-TASK-0001", "MSTD-RULE-0001", "lesson-3", "MSTD-RULE-0002"], None);
        let receipt = |extra: serde_json::Value| {
            let mut call = json!({"author": "binary", "command": "read", "ms": 0, "result": "ok", "request": request_name(Some(1))});
            call.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            seed_event(root, "x", "call", call)
        };
        receipt(json!({"item": "MSTD-TASK-0001"}));
        receipt(json!({"items": ["MSTD-RULE-0001", "lesson-3"]}));
        assert_eq!(unread(root, sent), ["MSTD-RULE-0002"]);
    }

    /// O pedido da revisão final tem o nome dele, distinto do de toda onda.
    #[test]
    fn the_final_review_request_has_a_name_of_its_own() {
        assert_eq!(request_name(None), "request-review");
        assert_eq!(request_name(Some(7)), "request-7");
    }
}
