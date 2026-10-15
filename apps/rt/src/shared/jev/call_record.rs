//! O registro de cada chamada da rodada ao Jev. A montagem das ondas, a
//! escolha dos itens do pedido, a do procedimento de cada onda e a triagem da
//! volta sem mudança gravam, cada uma, um evento `call` na spec, do jeito que
//! a busca grava o dela: na resposta, o tempo, os tokens, o custo e o modelo;
//! na falha, o motivo no nome do filtro (`jev:<motivo>`) e o tempo até ela. A
//! rodada lê esses eventos pelo nome de cada chamada para dizer que o Jev
//! falhou.

use std::path::Path;
use std::time::Instant;

use mustard_core::domain::map_filter::{FilterError, FilterUsage};
use serde_json::{Map, json};

use crate::commands::spec_events::conversation::record_measured_call;

/// O nome que a montagem das ondas grava.
pub(crate) const ASSEMBLY_CALL: &str = "wave assembly";

/// O nome que a escolha dos itens do pedido grava.
pub(crate) const ITEMS_CALL: &str = "wave items";

/// O nome que a escolha do procedimento de cada onda grava.
pub(crate) const EXPERTISE_CALL: &str = "wave expertise";

/// O nome que a triagem da volta sem mudança grava.
pub(crate) const TRIAGE_CALL: &str = "return triage";

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
    let mut measured = Map::new();
    if let Some((asked, returned)) = counts {
        measured.insert("candidates".to_string(), json!(asked));
        measured.insert("returned".to_string(), json!(returned));
    }
    match answer {
        Ok(usage) => {
            measured.insert("filter".to_string(), json!("jev"));
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
            measured.insert("filter".to_string(), json!(format!("jev:{}", error.reason())));
            measured.insert("filter_ms".to_string(), json!(u64::try_from(called.elapsed().as_millis()).unwrap_or(u64::MAX)));
        }
    }
    let report = json!({ "ok": true, "spec": spec });
    let _ = record_measured_call(start, command, Some(spec), None, called, &report, measured);
}
