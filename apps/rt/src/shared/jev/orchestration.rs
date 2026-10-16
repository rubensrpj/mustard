//! As escolhas da orquestração — o procedimento de cada onda e a triagem da
//! volta sem mudança — fazem, cada uma, uma pergunta ao Jev com teto de
//! tamanho. Toda pergunta que sai, e a que o teto segura, vira um evento
//! `call` na spec pelo mesmo registro das outras chamadas da rodada
//! ([`record_jev_calls`]). Sem resposta, a escolha fica a nativa.

use std::path::Path;
use std::time::{Duration, Instant};

use mustard_core::domain::map_filter::FilterUsage;
use serde_json::Value;

use super::call_record::record_jev_calls;
use super::{JevFilter, cost_micro_usd, model_of, request_count, unknown_usage};

impl JevFilter {
    /// A resposta do Jev à pergunta `payload`, com a chamada gravada como
    /// `command` na spec `spec` de `root`. O pedido acima do teto do serviço
    /// ([`super::validate_wire_size`]) não sai e é gravado como grande demais;
    /// a falha do serviço é gravada com o motivo. `None` em toda falha: quem
    /// chama segue pela escolha nativa. A resposta é esperada pelo prazo de
    /// toda chamada, o de quem espera no meio de um gancho.
    pub(crate) fn choose_recorded(&self, root: &Path, spec: &str, command: &str, payload: &str) -> Option<Value> {
        self.choose_recorded_within(root, spec, command, payload, self.timeouts.response)
    }

    /// [`Self::choose_recorded`] com a resposta esperada até `wait`: a
    /// pergunta que ninguém espera no meio de um gancho, mas que é grande o
    /// bastante para o Jev levar mais que o prazo de toda chamada.
    pub(crate) fn choose_recorded_within(&self, root: &Path, spec: &str, command: &str, payload: &str, wait: Duration) -> Option<Value> {
        self.choose_all_recorded_within(root, spec, command, &[payload.to_string()], wait)?.into_iter().next()
    }

    /// [`Self::choose_recorded_within`] para os pedidos `payloads`, que saem
    /// ao mesmo tempo, com uma chamada por pedido, todas numa gravação só na
    /// spec ([`record_jev_calls`]). As respostas voltam na ordem dos pedidos;
    /// um pedido acima do teto do serviço segura todos, e a falha de um é a
    /// falha de todos.
    pub(crate) fn choose_all_recorded_within(&self, root: &Path, spec: &str, command: &str, payloads: &[String], wait: Duration) -> Option<Vec<Value>> {
        let called = Instant::now();
        let answer = self.choose_orchestration(payloads, wait);
        match &answer {
            Ok(docs) => {
                let usages: Vec<FilterUsage> = docs.iter().map(|doc| usage_of(doc, called)).collect();
                record_jev_calls(root, spec, command, called, &usages.iter().map(Ok).collect::<Vec<_>>());
            }
            Err(error) => record_jev_calls(root, spec, command, called, &vec![Err(error); payloads.len()]),
        }
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

    use std::collections::BTreeSet;

    use super::*;
    use crate::shared::jev::{JevKey, TASK_TRIAGE_CALL, TRIAGE_CALL};
    use crate::shared::task_triage::{Candidate, Item, classify_with, triage_backlog_with};

    /// Um serviço em 127.0.0.1 que responde `status` a todo pedido e conta
    /// quantos chegaram.
    fn answering(status: u16) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        serving(Duration::ZERO, status, String::new())
    }

    /// Um serviço em 127.0.0.1 que responde `status` com `body` a todo pedido,
    /// `delay` depois de lê-lo, um pedido por vez, e conta quantos chegaram.
    fn serving(delay: Duration, status: u16, body: String) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
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
                let mut request = vec![0; length];
                let _ = std::io::Read::read_exact(&mut reader, &mut request);
                std::thread::sleep(delay);
                let mut stream = &stream;
                let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
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

        assert_eq!(filter.choose_recorded(root, "x", TASK_TRIAGE_CALL, r#"{"model":"jev"}"#), None);
        let recorded = calls(root, TASK_TRIAGE_CALL);
        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0]["filter"], json!("jev:no_credit"), "{recorded:?}");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);

        let oversized = format!(r#"{{"state":"{}","questions":{{}}}}"#, "a".repeat(300_000));
        assert_eq!(filter.choose_recorded(root, "x", TRIAGE_CALL, &oversized), None);
        let recorded = calls(root, TRIAGE_CALL);
        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0]["filter"], json!("jev:too_large"), "{recorded:?}");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1, "the oversized question never leaves");
    }

    /// O serviço responde depois do prazo de toda chamada e antes do prazo da
    /// reorganização da lista aberta (os dois encurtados): a reorganização
    /// recebe a resposta e junta as duas tarefas; a classificação de uma
    /// tarefa a gravar, com o prazo de toda chamada, fica sem resposta e grava
    /// a falha por prazo.
    #[test]
    fn backlog_triage_waits_longer_than_a_hook_call() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::shared::test_fixture::seeded_repo(root, &[("mustard.json", "{}")]);
        assert_eq!(crate::commands::spec_events::write::record_open(root, "x", "feature/x", "dev"), Ok(true));
        let value = json!({ "type": "score", "score": 2.0, "probabilities": { "0": 0.05, "1": 0.1, "2": 0.8, "3": 0.05 } });
        let answer = json!({ "answers": { "value": value, "same_t1": { "type": "noul", "noul": 0.93 } } });
        let (url, count) = serving(Duration::from_millis(600), 200, answer.to_string());
        let mut filter = JevFilter::at(JevKey("sk-test".to_string()), &url);
        filter.timeouts.response = Duration::from_millis(150);
        let task = |id: u64, title: &'static str| Candidate { id, title, text: "", files: BTreeSet::from(["src/resume.ts".to_string()]), joinable: true };
        let open = [task(1, "Retomada lista as ondas no ar"), task(2, "Retomada diz se o agente de cada onda vive")];

        let languages = mustard_core::domain::normalize::Languages::new(["pt-BR"]);
        let backlog = triage_backlog_with(&open, &[], &languages, &|payloads| {
            filter.choose_all_recorded_within(root, "x", TASK_TRIAGE_CALL, payloads, Duration::from_secs(5))
        });
        assert_eq!(backlog.map(|triage| triage.groups), Ok(vec![vec![1, 2]]));

        let item = Item { title: "Retomada avisa a onda sem agente".into(), text: String::new(), files: BTreeSet::from(["src/resume.ts".to_string()]) };
        let destinations = classify_with(&open, &[item], &[], &|payload| filter.choose_recorded(root, "x", TASK_TRIAGE_CALL, payload));
        assert_eq!(destinations, vec![None]);

        let recorded = calls(root, TASK_TRIAGE_CALL);
        let filters: Vec<&Value> = recorded.iter().map(|call| &call["filter"]).collect();
        assert_eq!(filters, [&json!("jev"), &json!("jev"), &json!("jev:timeout")], "one call per backlog request: {recorded:?}");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }
}
