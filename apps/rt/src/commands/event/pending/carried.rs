//! As pendências que uma obra levou até o merge: o que o merge fecha.
//!
//! Duas ligações valem. A nota "virou a spec X", que a abertura por
//! `open --pending` grava no item, e a citação do item (`P-12`) no contexto
//! que o levantamento aprovou. As pendências que NASCERAM na spec (o pedido
//! adiado) ficam de fora: são elas que a entrega pergunta, e adiado não é
//! entregue.

use super::{born_in, ledger_root, load, Path, Status};
use crate::shared::spec_state::DiskSpecState;
use mustard_core::domain::spec_events::{Block, BlockQuery, SpecLog};
use mustard_core::domain::spec_state::SpecState;

/// Os números `P-N` que o texto cita, na ordem em que aparecem. O número só
/// vale inteiro: `P-12` não é `P-1`, e `XP-3` ou `P-3a` não citam nada.
fn cited_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for (at, _) in text.match_indices("P-") {
        if at > 0 && bytes[at - 1].is_ascii_alphanumeric() {
            continue;
        }
        let digits: String = text[at + 2..].chars().take_while(char::is_ascii_digit).collect();
        let after = text[at + 2 + digits.len()..].chars().next();
        if digits.is_empty() || after.is_some_and(char::is_alphanumeric) {
            continue;
        }
        let id = format!("P-{}", digits.trim_start_matches('0'));
        if id != "P-" && !found.contains(&id) {
            found.push(id);
        }
    }
    found
}

/// Os números que o contexto do levantamento cita.
fn cited_by_context(log: &SpecLog) -> Vec<String> {
    log.block(BlockQuery::Block(Block::Specification))
        .into_iter()
        .filter(|event| event.event_type == "context")
        .flat_map(|event| {
            let title = event.str_field("title").unwrap_or_default();
            let text = event.str_field("text").unwrap_or_default();
            let mut ids = cited_in(title);
            ids.extend(cited_in(text));
            ids
        })
        .collect()
}

/// As pendências abertas que a spec `spec` levou, na ordem da lista: as de
/// nota "virou a spec" e as que o contexto do levantamento cita, menos as que
/// nasceram na própria spec.
#[must_use]
pub(crate) fn carried_by(root: &Path, spec: &str) -> Vec<String> {
    let project = ledger_root(root);
    let Some(items) = mustard_core::ClaudePaths::for_project(&project)
        .ok()
        .and_then(|paths| load(&paths.pending_ledger_path()).ok())
        .map(|ledger| ledger.items)
    else {
        return Vec::new();
    };
    let log = DiskSpecState::new(root).log(spec);
    let cited = log.as_ref().map(cited_by_context).unwrap_or_default();
    let born = log.as_ref().map(born_in).unwrap_or_default();
    items
        .into_iter()
        .filter(|item| item.status == Status::Open && !born.contains(&item.id))
        .filter(|item| item.became.as_deref() == Some(spec) || cited.contains(&item.id))
        .map(|item| item.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::cited_in;

    /// O número citado vale inteiro e só quando não está colado a outra
    /// palavra.
    #[test]
    fn a_citation_counts_only_as_a_whole_number() {
        assert_eq!(cited_in("as P-148 a P-150 e a P-7."), ["P-148", "P-150", "P-7"]);
        assert_eq!(cited_in("(P-12)"), ["P-12"]);
        assert_eq!(cited_in("P-12 e P-12 de novo"), ["P-12"], "a repetida entra uma vez");
        assert!(cited_in("XP-3 e P-3a e P- e P-").is_empty());
        assert!(cited_in("sem número").is_empty());
    }
}
