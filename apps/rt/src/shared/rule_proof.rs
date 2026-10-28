//! Os itens que dão à onda a prova que ela roda, pela mesma seleção do
//! pronto-quando dela ([`item_proofs`]): o critério com a prova gravada e o
//! pedido cujo relato nomeia a prova. A tarefa que o programa grava sem que o
//! modelo a escreva — a do item combinado não cumprido e a da porta de
//! conserto — cobre também esses itens, para toda tarefa cobrir uma regra com
//! a prova que a onda roda de verdade.

use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::domain::wave_prompt::item_proofs;

/// Os critérios vigentes da obra lida em `log`, pelo número de cada um.
pub(crate) fn work_criteria(log: &SpecLog) -> Vec<u64> {
    log.block(BlockQuery::Block(Block::Criteria))
        .into_iter()
        .filter(|event| event.event_type == "criterion")
        .map(|event| event.id)
        .collect()
}

/// Os itens de `items`, lidos em `log` pela versão vigente, que dão à onda
/// uma prova que ela roda, com o comando de teste que o projeto declara
/// (`test`): cada um pelo número da versão vigente, na ordem de `items` e
/// sem repetir. O item que saiu da spec não entra.
pub(crate) fn with_proof(log: &SpecLog, items: impl IntoIterator<Item = u64>, test: Option<&str>) -> Vec<u64> {
    let mut proven: Vec<u64> = Vec::new();
    for id in items {
        let Some(item) = log.current(id) else { continue };
        if !item_proofs(item, test).is_empty() && !proven.contains(&item.id) {
            proven.push(item.id);
        }
    }
    proven
}

