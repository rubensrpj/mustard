//! As escolhas da orquestração — o procedimento de cada onda e a triagem da
//! volta sem mudança — fazem, cada uma, uma pergunta ao Jev com teto de
//! tamanho. Toda pergunta que sai, e a que o teto segura, vira um evento
//! `call` na spec pelo mesmo registro das outras chamadas da rodada
//! ([`record_jev_call`]). Sem resposta, a escolha fica a nativa.

use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::{FilterError, FilterUsage};
use serde_json::Value;

use super::call_record::record_jev_call;
use super::{JevFilter, cost_micro_usd, model_of, request_count, tokens_of, unknown_usage};

/// O maior pedido de uma escolha da orquestração, em bytes: acima dele a
/// pergunta não sai, e a escolha fica a nativa.
const MAX_PAYLOAD_BYTES: usize = 48_000;

impl JevFilter {
    /// A resposta do Jev à pergunta `payload`, com a chamada gravada como
    /// `command` na spec `spec` de `root`. O pedido acima do teto não sai e é
    /// gravado como grande demais; a falha do serviço é gravada com o motivo.
    /// `None` em toda falha: quem chama segue pela escolha nativa.
    pub(crate) fn choose_recorded(&self, root: &Path, spec: &str, command: &str, payload: &str) -> Option<Value> {
        let called = Instant::now();
        let answer = if payload.len() > MAX_PAYLOAD_BYTES {
            Err(FilterError::TooLarge { estimated_tokens: tokens_of(payload.chars().count()) })
        } else {
            self.choose_orchestration(payload)
        };
        let usage = answer.as_ref().map(|doc| usage_of(doc, called));
        record_jev_call(root, spec, command, called, None, usage.as_ref().map_err(|error| *error));
        answer.ok()
    }
}

/// O uso da resposta `doc`, pedida em `called`.
fn usage_of(doc: &Value, called: Instant) -> FilterUsage {
    let input_tokens = doc.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0);
    FilterUsage {
        incomplete: unknown_usage(doc),
        input_tokens,
        millis: u64::try_from(called.elapsed().as_millis()).unwrap_or(u64::MAX),
        cost_micro_usd: cost_micro_usd(input_tokens),
        requests: request_count(doc),
        model: model_of(doc),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use mustard_core::io::spec_events as store;
    use serde_json::{Map, json};

    use super::*;
    use crate::shared::jev::{EXPERTISE_CALL, JevKey, TRIAGE_CALL};

    /// Um serviço em 127.0.0.1 que responde `status` a todo pedido e conta
    /// quantos chegaram.
    fn answering(status: u16) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = std::sync::Arc::clone(&count);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut reader = BufReader::new(&stream);
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; length];
                let _ = std::io::Read::read_exact(&mut reader, &mut body);
                let mut stream = &stream;
                let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        (url, count)
    }

    fn calls(root: &Path, command: &str) -> Vec<Map<String, Value>> {
        let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
        log.visible().into_iter().filter(|e| e.event_type == "call" && e.str_field("command") == Some(command)).map(|e| e.fields.clone()).collect()
    }

    /// A escolha que o serviço recusa por falta de crédito volta vazia e deixa
    /// a chamada gravada com o motivo; a pergunta acima do teto nem chega ao
    /// serviço, e a chamada fica gravada como grande demais.
    #[test]
    fn an_orchestration_choice_records_the_refusal_and_the_oversized_question() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("mustard.json", "{}")]);
        assert_eq!(crate::commands::spec_events::write::record_open(root, "x", "feature/x", "dev"), Ok(true));
        let (url, count) = answering(402);
        let filter = JevFilter::at(JevKey("sk-test".to_string()), &url);

        assert_eq!(filter.choose_recorded(root, "x", EXPERTISE_CALL, r#"{"model":"jev"}"#), None);
        let recorded = calls(root, EXPERTISE_CALL);
        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0]["filter"], json!("jev:no_credit"), "{recorded:?}");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);

        let oversized = format!(r#"{{"state":"{}"}}"#, "a".repeat(MAX_PAYLOAD_BYTES));
        assert_eq!(filter.choose_recorded(root, "x", TRIAGE_CALL, &oversized), None);
        let recorded = calls(root, TRIAGE_CALL);
        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0]["filter"], json!("jev:too_large"), "{recorded:?}");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1, "the oversized question never leaves");
    }
}
