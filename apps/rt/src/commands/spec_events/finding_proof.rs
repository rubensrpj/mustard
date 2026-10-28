//! A prova que cada achado de uma volta nomeia no detalhe. O achado vira, na
//! rodada ou no fechamento, um pedido com o detalhe como relato e a tarefa
//! que cobre esse pedido; é a prova nomeada no relato que dá à onda da tarefa
//! o que rodar. Sem ela, a tarefa nasceria sem regra com prova.

use mustard_core::domain::spec_events::Refusal;
use mustard_core::domain::wave_prompt::proofs_named_in;
use serde_json::{Map, Value};

/// Confere que cada achado em `leftovers` da volta `draft` — a entrega da
/// onda ou o veredito final — nomeia no detalhe a prova que a onda da tarefa
/// dele vai rodar, pela mesma seleção do pronto-quando ([`proofs_named_in`]),
/// com o comando de teste do projeto (`test`). O achado incompleto ou com um
/// campo do tipo errado ([`well_formed`]) fica para a recusa da forma dele,
/// na conferência da volta.
///
/// # Errors
///
/// [`Refusal::FindingProofMissing`], com o número do primeiro achado sem
/// prova, a contar de um.
pub(super) fn each_finding_names_a_proof(draft: &Map<String, Value>, test: Option<&str>) -> Result<(), Refusal> {
    let leftovers = draft.get("leftovers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for (at, leftover) in leftovers.iter().enumerate() {
        let Some(detail) = well_formed(leftover) else { continue };
        if proofs_named_in(&[detail], test).is_empty() {
            return Err(Refusal::FindingProofMissing { finding: at + 1 });
        }
    }
    Ok(())
}

/// O detalhe do achado `leftover` que traz cada campo da forma dele: o
/// título, o detalhe e a regra em texto não vazio, os lugares numa lista de
/// textos não vazia, os arquivos, quando vêm, numa lista de textos, e a marca
/// de limpeza, quando vem, em sim ou não. `None` no achado fora dessa forma.
fn well_formed(leftover: &Value) -> Option<&str> {
    let text = |name: &str| leftover.get(name).and_then(Value::as_str).filter(|said| !said.trim().is_empty());
    let texts = |name: &str| leftover.get(name).and_then(Value::as_array).filter(|listed| listed.iter().all(Value::is_string));
    let files = leftover.get("files").is_none_or(|_| texts("files").is_some());
    let cleanup = leftover.get("cleanup").is_none_or(Value::is_boolean);
    let places = texts("places").is_some_and(|listed| listed.iter().any(|place| place.as_str().is_some_and(|place| !place.trim().is_empty())));
    (text("title").is_some() && text("rule").is_some() && places && files && cleanup).then(|| text("detail")).flatten()
}
