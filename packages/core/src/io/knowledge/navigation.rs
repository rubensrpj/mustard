//! Navigation starts at exact persisted identities. Homonyms and uncertain
//! targets remain evidence candidates, never extra graph destinations.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use super::current;
use crate::domain::knowledge::{Card, Source};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    #[default]
    Outgoing,
    Callers,
    Both,
}

pub(super) struct Walk {
    pub order: Vec<usize>,
    pub paths: Vec<Value>,
    pub omitted: usize,
    pub stale: usize,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn walk(
    tree: &Path,
    cards: &[Card],
    seeds: &[usize],
    max: usize,
    depth: usize,
    direction: Direction,
    hashes: &mut BTreeMap<String, Option<(String, u64)>>,
) -> Walk {
    let by_id: BTreeMap<_, _> = cards
        .iter()
        .enumerate()
        .map(|(i, card)| (card.id.as_str(), i))
        .collect();
    let mut links = vec![BTreeMap::new(); cards.len()];
    for (from, card) in cards.iter().enumerate() {
        for edge in &card.outgoing {
            if edge["resolution"] != "unique-static-target" {
                continue;
            }
            let Some(&to) = edge["target"].as_str().and_then(|id| by_id.get(id)) else {
                continue;
            };
            // A link with a different target receipt cannot borrow the fresh
            // identity of a card, even if its identifier happens to match.
            if serde_json::from_value::<Source>(edge["source"].clone())
                .ok()
                .as_ref()
                != Some(&cards[to].source)
            {
                continue;
            }
            if direction != Direction::Callers {
                links[from].insert(to, "outgoing");
            }
            if direction != Direction::Outgoing {
                links[to].insert(from, "caller");
            }
        }
    }
    let mut selected: BTreeSet<_> = seeds.iter().copied().collect();
    let mut frontier: Vec<_> = seeds.to_vec();
    let mut result = Walk {
        order: Vec::new(),
        paths: Vec::new(),
        omitted: 0,
        stale: 0,
    };
    let mut omitted = BTreeSet::new();
    let mut stale = BTreeSet::new();
    // Inspect the last frontier too, so the response discloses that depth
    // stopped navigation rather than implying there are no more consumers.
    for distance in 1..=depth.min(4) + 1 {
        let mut next = BTreeMap::new();
        for &from in &frontier {
            if !current(tree, &cards[from].source, hashes) {
                continue;
            }
            for (&to, &via) in &links[from] {
                if selected.contains(&to) || next.contains_key(&to) {
                    continue;
                }
                if !current(tree, &cards[to].source, hashes) {
                    stale.insert(to);
                } else if distance > depth.min(4) || selected.len() + next.len() >= max {
                    omitted.insert(to);
                } else {
                    next.insert(to, (from, via));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next.keys().copied().collect();
        for (&to, &(from, via)) in &next {
            selected.insert(to);
            result.order.push(to);
            result.paths.push(
                json!({"from":cards[from].id,"to":cards[to].id,"via":via,"distance":distance,
                "resolution":"unique-static-target"}),
            );
        }
    }
    result.omitted = omitted.difference(&selected).count();
    result.stale = stale.len();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::sha256::Sha256;
    #[test]
    fn cycles_homonyms_ambiguity_and_corrupt_receipts_do_not_expand_impact() {
        let dir = tempfile::tempdir().unwrap();
        let mut cards: Vec<Card> = ['a', 'b', 'c', 'd']
            .into_iter()
            .map(|name| {
                let file = format!("{name}.rs");
                let bytes = b"fn run() {}\n";
                std::fs::write(dir.path().join(&file), bytes).unwrap();
                let mut hash = Sha256::new();
                hash.update(bytes);
                serde_json::from_value(
                    json!({"id":format!("{file}:1:run"),"name":"run","kind":"function",
                "source":{"file":file,"line":1,"end_line":1,"sha256":hash.hex_digest()}}),
                )
                .unwrap()
            })
            .collect();
        let edge = |to: &Card| json!({"target":to.id,"source":to.source,"call_line":1,"resolution":"unique-static-target"});
        cards[0].outgoing = vec![edge(&cards[1])];
        cards[1].outgoing = vec![edge(&cards[0])];
        let mut uncertain = edge(&cards[1]);
        uncertain["resolution"] = json!("ambiguous");
        cards[2].outgoing = vec![uncertain];
        cards[3].outgoing = vec![edge(&cards[0])];
        let mut corrupt = edge(&cards[2]);
        corrupt["source"]["sha256"] = json!("0".repeat(64));
        cards[0].outgoing.push(corrupt);
        for direction in [Direction::Callers, Direction::Both] {
            let result = walk(
                dir.path(),
                &cards,
                &[1],
                8,
                4,
                direction,
                &mut BTreeMap::new(),
            );
            assert_eq!(result.order, vec![0, 3]);
            assert_eq!(result.paths.len(), 2);
            assert_eq!(result.paths[1]["distance"], 2);
        }
        let limited = walk(
            dir.path(),
            &cards,
            &[1],
            2,
            4,
            Direction::Callers,
            &mut BTreeMap::new(),
        );
        assert_eq!(limited.order, vec![0]);
        assert_eq!(limited.omitted, 1);
        let outgoing = walk(
            dir.path(),
            &cards,
            &[1],
            8,
            4,
            Direction::Outgoing,
            &mut BTreeMap::new(),
        );
        assert_eq!(outgoing.order, vec![0]);
        std::fs::write(dir.path().join("a.rs"), "fn changed() {}\n").unwrap();
        let stale = walk(
            dir.path(),
            &cards,
            &[1],
            8,
            4,
            Direction::Callers,
            &mut BTreeMap::new(),
        );
        assert!(stale.order.is_empty());
        assert_eq!(stale.stale, 1);
    }
}
