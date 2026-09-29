//! `map_select` — o que a busca com filtro devolve: o corte do filtro e o que
//! cada item dele puxa pelas ligações do mapa.
//!
//! O filtro classifica os candidatos e devolve o que passa do corte, na ordem
//! da chance. O tipo achado sozinho não diz onde está o trabalho, e o método
//! de interface não diz quem o faz: por isso cada item do corte puxa, logo
//! depois dele, o método do tipo ou a implementação do método de contrato.
//! Nada mais entra: o que o filtro barrou não volta pelo banco. O teto corta
//! o fim da volta.
//!
//! Função pura: as ligações chegam prontas, lidas do mapa pela porta. Sem
//! disco, sem rede, sem relógio.

use std::collections::{HashMap, HashSet};

/// Quantas peças a busca com filtro devolve, no máximo, quando o projeto
/// não diz outro número. Nas quatro réguas do laboratório (1.967 buscas,
/// com 200 candidatos), o teto de 15 perdeu 6 buscas e o de 12 perdeu 29;
/// sem teto, a volta chegou a 21 peças.
pub const MAX_RETURNED: usize = 15;

/// Os tipos que puxam o seu método.
const TYPE_KINDS: [&str; 7] = ["class", "struct", "record", "interface", "trait", "enum", "type"];

/// As funções que, num contrato, puxam a sua implementação.
const FUNCTION_KINDS: [&str; 2] = ["function", "method"];

/// O que o mapa liga a uma declaração do corte.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Linked {
    /// O tipo da declaração (`class`, `method`…).
    pub kind: String,
    /// O caminho do arquivo dela.
    pub path: String,
    /// Num tipo, os seus métodos.
    pub methods: Vec<i64>,
    /// Num método de contrato, os métodos que o cumprem, cada um com o
    /// caminho do arquivo dele: só os que podem ser candidatos da busca, sem
    /// o dublê de teste.
    pub implementations: Vec<(i64, String)>,
}

/// As ligações de cada declaração do corte, pelo id.
pub type Links = HashMap<i64, Linked>;

/// Por que uma peça voltou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Passou do corte do filtro.
    Cut,
    /// Um item do corte a puxou: o método do tipo ou a implementação.
    Pulled,
}

/// Uma peça da volta: a declaração e por que ela voltou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    pub id: i64,
    pub source: Source,
}

/// A volta da busca com filtro, em ordem: cada item de `cut` (na ordem da
/// chance), seguido do que ele puxa.
///
/// O tipo puxa o seu método de melhor posição entre os candidatos. A função
/// ou o método de contrato puxa a implementação: a de melhor posição entre os
/// candidatos; se nenhuma está neles, a de melhor posição em `whole`, a lista
/// inteira de onde os candidatos saíram; se nenhuma está nela, a de caminho
/// mais parecido com o do item (mais pastas em comum no começo) e, no
/// empate, a de menor id. O dublê de teste nunca é puxado: ele não chega nas
/// implementações de `links`, mesmo quando o caminho dele é o mais parecido.
/// O puxado entra só se ainda não voltou nem está no corte.
#[must_use]
pub fn select(cut: &[i64], candidates: &[i64], whole: &[i64], links: &Links) -> Vec<Pick> {
    let in_cut: HashSet<i64> = cut.iter().copied().collect();
    let bank_at = positions(candidates);
    let whole_at = positions(whole);
    let mut out: Vec<Pick> = Vec::new();
    let mut taken: HashSet<i64> = HashSet::new();
    for &id in cut {
        if taken.insert(id) {
            out.push(Pick { id, source: Source::Cut });
        }
        let Some(linked) = links.get(&id) else { continue };
        let pulled = if TYPE_KINDS.contains(&linked.kind.as_str()) {
            best_placed(linked.methods.iter().copied(), &bank_at)
        } else if FUNCTION_KINDS.contains(&linked.kind.as_str()) {
            implementation(linked, &bank_at, &whole_at)
        } else {
            None
        };
        if let Some(pulled) = pulled.filter(|pulled| !in_cut.contains(pulled) && taken.insert(*pulled)) {
            out.push(Pick { id: pulled, source: Source::Pulled });
        }
    }
    out
}

/// A volta com no máximo `max` peças: as primeiras, na ordem da volta.
#[must_use]
pub fn capped(picks: &[Pick], max: usize) -> Vec<Pick> {
    picks.iter().take(max).copied().collect()
}

/// A posição de cada id na lista, a primeira quando ele se repete.
fn positions(ids: &[i64]) -> HashMap<i64, usize> {
    let mut out = HashMap::new();
    for (at, &id) in ids.iter().enumerate() {
        out.entry(id).or_insert(at);
    }
    out
}

/// O id de melhor posição em `at`, entre os que estão nele.
fn best_placed(ids: impl Iterator<Item = i64>, at: &HashMap<i64, usize>) -> Option<i64> {
    ids.filter_map(|id| at.get(&id).map(|&place| (place, id))).min().map(|(_, id)| id)
}

/// A implementação que o método de contrato puxa: entre os candidatos; senão
/// na lista inteira; senão pelo caminho mais parecido e, no empate, pelo
/// menor id.
fn implementation(linked: &Linked, bank_at: &HashMap<i64, usize>, whole_at: &HashMap<i64, usize>) -> Option<i64> {
    let ids = || linked.implementations.iter().map(|(id, _)| *id);
    best_placed(ids(), bank_at).or_else(|| best_placed(ids(), whole_at)).or_else(|| {
        linked
            .implementations
            .iter()
            .map(|(id, path)| (std::cmp::Reverse(shared_folders(path, &linked.path)), *id))
            .min()
            .map(|(_, id)| id)
    })
}

/// Quantas partes do começo dos dois caminhos são iguais, separadas por `/`.
fn shared_folders(a: &str, b: &str) -> usize {
    a.split('/').zip(b.split('/')).take_while(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(picks: &[Pick]) -> Vec<i64> {
        picks.iter().map(|pick| pick.id).collect()
    }

    fn linked(kind: &str, path: &str) -> Linked {
        Linked { kind: kind.to_string(), path: path.to_string(), ..Linked::default() }
    }

    /// Os candidatos na ordem do banco: 1 a 20.
    fn bank() -> Vec<i64> {
        (1..=20).collect()
    }

    #[test]
    fn a_class_in_the_cut_pulls_its_best_placed_method_right_after_it() {
        let mut links = Links::new();
        // A classe 10 tem os métodos 15, 7 e 40; o 7 está mais acima no banco,
        // e o 40 nem é candidato.
        links.insert(10, Linked { methods: vec![15, 7, 40], ..linked("class", "src/a.rs") });
        let picks = select(&[10, 12], &bank(), &bank(), &links);
        assert_eq!(ids(&picks), vec![10, 7, 12]);
        assert_eq!(picks[1].source, Source::Pulled);
    }

    #[test]
    fn a_contract_method_pulls_its_implementation_from_the_candidates_the_whole_list_or_the_closest_path() {
        let candidates = bank();
        let whole: Vec<i64> = (1..=60).collect();
        let contract = |implementations: Vec<(i64, &str)>| {
            let mut links = Links::new();
            links.insert(
                5,
                Linked {
                    implementations: implementations.into_iter().map(|(id, path)| (id, path.to_string())).collect(),
                    ..linked("method", "src/pay/port.rs")
                },
            );
            links
        };
        // Entre os candidatos: a de melhor posição.
        let picks = select(&[5], &candidates, &whole, &contract(vec![(18, "x.rs"), (9, "y.rs"), (45, "z.rs")]));
        assert_eq!(ids(&picks), vec![5, 9]);
        // Nenhuma nos candidatos: a de melhor posição na lista inteira.
        let picks = select(&[5], &candidates, &whole, &contract(vec![(55, "x.rs"), (41, "y.rs")]));
        assert_eq!(ids(&picks), vec![5, 41]);
        // Nenhuma na lista: a de mais pastas em comum com o método.
        let picks = select(&[5], &candidates, &whole, &contract(vec![(90, "src/web/pay.rs"), (80, "src/pay/card.rs")]));
        assert_eq!(ids(&picks), vec![5, 80]);
        // O mesmo caminho parecido: a de menor id, a primeira do banco.
        let picks = select(&[5], &candidates, &whole, &contract(vec![(95, "src/pay/b.rs"), (85, "src/pay/a.rs")]));
        assert_eq!(ids(&picks), vec![5, 85]);
    }

    #[test]
    fn what_is_already_in_the_cut_does_not_come_back() {
        let mut links = Links::new();
        links.insert(4, Linked { methods: vec![6], ..linked("struct", "src/a.rs") });
        links.insert(8, Linked { implementations: vec![(6, "src/a.rs".to_string())], ..linked("function", "src/b.rs") });
        // O 6 está no corte: nem a struct 4 nem a função 8 o puxam.
        let picks = select(&[4, 6, 8, 2], &bank(), &bank(), &links);
        assert_eq!(ids(&picks), vec![4, 6, 8, 2]);
        assert!(picks.iter().all(|pick| pick.source == Source::Cut));
    }

    #[test]
    fn an_empty_cut_returns_nothing_and_the_bank_does_not_fill_it() {
        assert!(select(&[], &bank(), &bank(), &Links::new()).is_empty());
    }

    #[test]
    fn the_answer_is_only_the_cut_in_the_order_of_the_chance() {
        let picks = select(&[14, 3, 9], &bank(), &bank(), &Links::new());
        assert_eq!(ids(&picks), vec![14, 3, 9], "the bank order does not come back and no bank piece is added");
    }

    #[test]
    fn a_cap_keeps_the_first_places_in_the_order_of_the_answer() {
        let mut links = Links::new();
        links.insert(10, Linked { methods: vec![11], ..linked("class", "src/a.rs") });
        links.insert(12, Linked { methods: vec![13], ..linked("class", "src/b.rs") });
        // Corte de 4 (10, 12, 14, 16) que puxa 2 (11 e 13).
        let picks = select(&[10, 12, 14, 16], &bank(), &bank(), &links);
        assert_eq!(ids(&picks), vec![10, 11, 12, 13, 14, 16]);
        assert_eq!(ids(&capped(&picks, 3)), vec![10, 11, 12]);
        // Abaixo do teto, nada sai.
        assert_eq!(capped(&picks, 9), picks);
        assert_eq!(capped(&picks, MAX_RETURNED), picks);
    }
}
