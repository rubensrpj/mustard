//! O registro do que o agente lê do próprio pedido.
//!
//! Quem lê de dentro da cópia de um pedido aberto está lendo o pedido dele: o
//! envio grava a vaga (`copy`) e o agente roda cada comando de dentro dela,
//! então a pasta atual diz de qual pedido é a leitura, sem opção nenhuma para
//! o agente lembrar. A leitura que acha o item grava uma chamada `read` com o
//! pedido (`request-<onda>` ou `request-review`) e o item (o código, ou
//! `lesson-<número>`); é dessas chamadas que a entrega e o veredito conferem
//! que o agente leu tudo o que o pedido lista.
//!
//! Fora de uma cópia com pedido aberto — o condutor lendo a spec — nada é
//! gravado, e a gravação que falha nunca falha a leitura.

use std::path::{Path, PathBuf};
use std::time::Instant;

use mustard_core::domain::spec_events::SpecLog;
use serde_json::{json, Map};

use super::conversation::record_measured_call;
use crate::commands::flow::round::{open_review, open_sends, request_name};

/// O pedido aberto a que uma pasta pertence.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Request {
    /// `request-<onda>` ou `request-review`, como a chamada o grava.
    pub name: String,
    /// A onda do pedido; o pedido da revisão final não tem.
    pub wave: Option<u64>,
}

/// O pedido aberto cuja cópia contém `from`: a vaga ou uma pasta dentro
/// dela. O envio da onda aberto vale primeiro; sem ele, o da revisão final.
/// Pasta fora de toda cópia de pedido aberto não é de pedido nenhum.
pub(super) fn request_of(log: &SpecLog, from: &Path) -> Option<Request> {
    let here = settled(from);
    let inside = |sent: u64| {
        log.get(sent)
            .and_then(|event| event.str_field("copy"))
            .filter(|copy| !copy.trim().is_empty())
            .is_some_and(|copy| here.starts_with(settled(Path::new(copy))))
    };
    if let Some((wave, _)) = open_sends(log).into_iter().find(|(_, sent)| inside(*sent)) {
        return Some(Request { name: request_name(Some(wave)), wave: Some(wave) });
    }
    open_review(log).filter(|sent| inside(*sent)).map(|_| Request { name: request_name(None), wave: None })
}

/// A pasta como se compara com a vaga gravada: o caminho real, quando o disco
/// o resolve, e o absoluto, quando a vaga já não existe.
fn settled(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}

/// Grava uma chamada `read` por item lido, na spec `spec`. Um item que a
/// gravação recusa fica sem registro, e os outros seguem.
pub(super) fn record(
    root: &Path,
    spec: &str,
    session: Option<&str>,
    request: &Request,
    items: &[String],
    started: Instant,
) {
    for item in items {
        let mut measured = Map::new();
        measured.insert("request".into(), json!(request.name));
        measured.insert("item".into(), json!(item));
        record_measured_call(root, "read", Some(spec), session, started, &json!({"ok": true, "spec": spec}), measured);
    }
}
