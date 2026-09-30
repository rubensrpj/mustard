//! A conferência de que o agente leu o que o pedido dele lista: o envio guarda
//! a lista (`read_items`), a leitura de dentro da cópia grava uma chamada
//! `read` por item, e a entrega da onda e o veredito da revisão final se
//! recusam quando algum item da lista ficou sem chamada.

use std::collections::BTreeSet;

use mustard_core::domain::spec_events::SpecLog;
use serde_json::Value;

/// O nome com que a leitura registra o pedido de uma onda (`request-<onda>`)
/// ou o da revisão final (`request-review`, sem onda): o mesmo que a
/// conferência da entrega e a do veredito procuram nas chamadas `read`.
pub(crate) fn request_name(wave: Option<u64>) -> String {
    wave.map_or_else(|| String::from("request-review"), |wave| format!("request-{wave}"))
}

/// A posição a partir da qual as leituras valem para o pedido `sent`: a em que
/// o pedido despachou ([`SpecLog::dispatch_position`]) e, num reenvio, a do
/// primeiro pedido da cadeia dele — o reenvio não zera as leituras da onda.
fn opened_at(log: &SpecLog, sent: u64) -> u64 {
    let mut first = sent;
    // A cadeia de reenvios é finita, mas a spec é um arquivo de texto: o
    // passo conta os saltos para um círculo gravado à mão não prender a volta.
    for _ in 0..log.events.len() {
        let Some(earlier) = log.get(first).and_then(|e| e.int("resends")).filter(|n| log.get(*n).is_some_and(|e| e.event_type == "send"))
        else {
            break;
        };
        first = earlier;
    }
    log.dispatch_position(first)
}

/// Os itens que o pedido `sent` lista e que ninguém leu desde que ele saiu
/// ([`opened_at`]), na ordem em que o pedido os lista: o código do item, ou
/// `lesson-<número>`. Lido é o que uma chamada `read` gravou para o pedido
/// `request` ([`request_name`]). O pedido sem a lista — gravado antes de o
/// envio guardá-la — não cobra leitura nenhuma.
pub(crate) fn unread_items(log: &SpecLog, sent: u64, request: &str) -> Vec<String> {
    let Some(listed) = log.get(sent).and_then(|e| e.fields.get("read_items")).and_then(Value::as_array) else {
        return Vec::new();
    };
    let since = opened_at(log, sent);
    let read: BTreeSet<&str> = log
        .events
        .iter()
        .filter(|e| e.id > since && e.event_type == "call")
        .filter(|e| e.str_field("command") == Some("read") && e.str_field("request") == Some(request))
        .filter_map(|e| e.str_field("item"))
        .collect();
    listed.iter().filter_map(Value::as_str).filter(|item| !read.contains(item)).map(str::to_string).collect()
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
    /// envio vale para o reenvio, que é o mesmo pedido. O envio novo de
    /// verdade, sem `resends`, começa do zero, e a leitura de antes do envio
    /// nunca conta.
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
        assert_eq!(unread(root, fresh), items, "a new request starts the reading over");
    }

    /// O pedido da revisão final tem o nome dele, distinto do de toda onda.
    #[test]
    fn the_final_review_request_has_a_name_of_its_own() {
        assert_eq!(request_name(None), "request-review");
        assert_eq!(request_name(Some(7)), "request-7");
    }
}
