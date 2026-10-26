//! Os alvos de uma remoção: a conta única que a leitura usa para tirar os
//! eventos dela e que as regras de gravação usam para julgar o que ela tira.

use std::collections::BTreeSet;

use super::{SpecEvent, SpecLog, TimeFilter};

impl SpecLog {
    /// Os números que o `remove` `removal` aponta, em ordem e sem repetição:
    /// os de `targets` e, com o filtro, cada evento do tipo e do intervalo
    /// gravado antes dele ([`Self::filter_matches`]). São os que a leitura
    /// tira, então a regra que guarda um evento da remoção lê os mesmos.
    #[must_use]
    pub fn removal_targets(&self, removal: &SpecEvent) -> BTreeSet<u64> {
        let mut targets: BTreeSet<u64> = removal.ints("targets").into_iter().collect();
        if let Some(filter) = removal.fields.get("filter").and_then(TimeFilter::from_value) {
            targets.extend(self.filter_matches(&filter, removal.id));
        }
        targets
    }
}
