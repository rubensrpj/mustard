//! O registro de cada chamada da rodada ao Jev. A montagem das ondas, a
//! escolha dos itens do pedido, a do procedimento de cada onda, a triagem da
//! volta sem mudança e o classificador das atividades gravam, cada uma, um evento `call` no arquivo de operações da spec, do jeito que
//! a busca grava o dela: na resposta, o tempo, os tokens, o custo e o modelo;
//! na falha, o motivo no nome do filtro (`jev:<motivo>`) e o tempo até ela. A
//! rodada lê esses eventos pelo nome de cada chamada para dizer que o Jev
//! falhou.

use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::{FilterError, FilterUsage};
use serde_json::{Map, Value, json};

use crate::commands::spec_events::conversation::record_measured_calls;

/// A sessão que cada chamada ao Jev leva: a do processo. Quem pergunta ao Jev
/// são os passos do fluxo, que não recebem a sessão da entrada; a do processo
/// é a mesma que a entrada `run` leu.
fn process_session() -> Option<String> {
    crate::shared::spec_state::session_from_env()
}

/// O nome que a montagem das ondas grava.
pub(crate) const ASSEMBLY_CALL: &str = "wave assembly";

/// O nome que a escolha dos itens do pedido grava.
pub(crate) const ITEMS_CALL: &str = "wave items";

/// O nome que a triagem da volta sem mudança grava.
pub(crate) const TRIAGE_CALL: &str = "return triage";

/// O nome que o classificador das atividades da rodada grava: a sobra da
/// volta e a mudança de plano aceita.
pub(crate) const TASK_TRIAGE_CALL: &str = "task triage";

/// O nome que a escolha das entregas ligadas ao objetivo da spec nova grava.
pub(crate) const SUMMARY_CALL: &str = "project summary";

/// O nome que a escolha das regras do projeto, na entrega da spec, grava.
pub(crate) const RULES_CALL: &str = "project rules";

/// O nome que a procura na spec grava quando a palavra fica em dúvida.
pub(crate) const FIND_CALL: &str = "spec find";

/// O nome que a reserva do Jev grava: a decisão que o Jev não deu e foi ao
/// agente de análise da sessão ([`super::reserve`]).
pub(crate) const RESERVE_CALL: &str = "jev reserve";

/// Grava, na spec `spec` de `start`, a chamada de reserva da decisão que a
/// chamada `command` ao Jev não deu: o filtro `reserve`, sem motivo de falha,
/// e o nome da chamada em `reserve_for`, para o painel contar.
pub(crate) fn record_reserve_call(start: &Path, spec: &str, command: &str) {
    let mut measured = Map::new();
    measured.insert("filter".to_string(), json!("reserve"));
    measured.insert("reserve_for".to_string(), json!(command));
    let _ = record_measured_calls(start, spec, process_session().as_deref(), RESERVE_CALL, Instant::now(), vec![measured]);
}

/// Grava o evento `call` da chamada `command` ao Jev, feita em `called`, na
/// spec `spec` de `start`. `counts` leva, quando quem chama conta, quantos
/// candidatos foram ao Jev e quantos voltaram (`candidates`, `returned`);
/// `answer` é o uso da resposta ou a falha.
pub(crate) fn record_jev_call(
    start: &Path,
    spec: &str,
    command: &str,
    called: Instant,
    counts: Option<(usize, usize)>,
    answer: Result<&FilterUsage, &FilterError>,
) {
    let measured = jev_measure(counts, called, answer);
    let _ = record_measured_calls(start, spec, process_session().as_deref(), command, called, vec![measured]);
}

/// [`record_jev_call`] para as chamadas `command` que saíram juntas em
/// `called`, uma por resposta ou falha de `answers`, na ordem, no arquivo de
/// operações da spec ([`record_measured_calls`]).
pub(crate) fn record_jev_calls(start: &Path, spec: &str, command: &str, called: Instant, answers: &[Result<&FilterUsage, &FilterError>]) {
    let measured = answers.iter().map(|answer| jev_measure(None, called, *answer)).collect();
    let _ = record_measured_calls(start, spec, process_session().as_deref(), command, called, measured);
}

/// A medida de uma chamada ao Jev feita em `called`: os candidatos que foram
/// e os que voltaram, quando quem chama conta (`counts`), e o uso da resposta
/// ou a falha.
fn jev_measure(counts: Option<(usize, usize)>, called: Instant, answer: Result<&FilterUsage, &FilterError>) -> Map<String, Value> {
    let mut measured = Map::new();
    if let Some((asked, returned)) = counts {
        measured.insert("candidates".to_string(), json!(asked));
        measured.insert("returned".to_string(), json!(returned));
    }
    measure_filter_call(&mut measured, "jev", called, answer);
    measured
}

/// Põe em `measured` a medida da chamada ao filtro `name`, feita em `called`:
/// na resposta, o nome, o tempo, os tokens e o custo (só da resposta
/// inteira), os pedidos e o modelo, quando ele veio; na falha, o nome com o
/// motivo (`<nome>:<motivo>`) e o tempo até ela. O registro da chamada ao Jev
/// e a porta da busca medem por aqui.
pub(crate) fn measure_filter_call(measured: &mut Map<String, Value>, name: &str, called: Instant, answer: Result<&FilterUsage, &FilterError>) {
    match answer {
        Ok(usage) => {
            measured.insert("filter".to_string(), json!(name));
            measured.insert("filter_ms".to_string(), json!(usage.millis));
            if !usage.incomplete {
                measured.insert("tokens".to_string(), json!(usage.input_tokens));
                measured.insert("cost_micro_usd".to_string(), json!(usage.cost_micro_usd));
            }
            measured.insert("requests".to_string(), json!(usage.requests));
            if !usage.model.is_empty() {
                measured.insert("model".to_string(), json!(usage.model));
            }
        }
        Err(error) => {
            measured.insert("filter".to_string(), json!(format!("{name}:{}", error.reason())));
            measured.insert("filter_ms".to_string(), json!(u64::try_from(called.elapsed().as_millis()).unwrap_or(u64::MAX)));
        }
    }
}
