//! Deterministic groups of the returned evidence, not inferred business
//! capabilities. Edges retain their direction and exact source receipts.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn groups(cards: &[Value]) -> Vec<Value> {
    let by_id: BTreeMap<_, _> = cards.iter().enumerate().filter_map(|(i, c)| Some((c["id"].as_str()?, i))).collect();
    let mut adjacent = vec![BTreeSet::new(); cards.len()];
    let mut edges = BTreeMap::new();
    let mut incoming = BTreeSet::new();
    for (from, card) in cards.iter().enumerate() {
        for edge in card["outgoing"].as_array().into_iter().flatten() {
            if edge["resolution"] != "unique-static-target" {
                continue;
            }
            let Some(&to) = edge["target"].as_str().and_then(|id| by_id.get(id)) else { continue };
            // Summary projections omit the receipt; a detailed report keeps
            // it. Callers must supply detail when requesting a grouping.
            if edge["source"] != cards[to]["source"] {
                continue;
            }
            adjacent[from].insert(to);
            adjacent[to].insert(from);
            incoming.insert(to);
            edges.insert((from, to), json!({"from":card["id"],"to":cards[to]["id"],"line":edge["call_line"],"resolution":"unique-static-target"}));
        }
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for first in 0..cards.len() {
        if !seen.insert(first) {
            continue;
        }
        let mut component = BTreeSet::from([first]);
        let mut frontier = vec![first];
        while let Some(at) = frontier.pop() {
            for &to in &adjacent[at] {
                if seen.insert(to) {
                    component.insert(to);
                    frontier.push(to);
                }
            }
        }
        let declarations: Vec<_> = component.iter().map(|&i| cards[i]["id"].clone()).collect();
        let files: BTreeSet<_> = component.iter().filter_map(|&i| cards[i]["source"]["file"].as_str()).collect();
        let mut entries: Vec<_>=component.iter().filter(|&&i|!incoming.contains(&i) || cards[i]["routes"].as_array().is_some_and(|r|!r.is_empty()))
            .map(|&i|json!({"symbol":cards[i]["id"],"name":cards[i]["name"],"routes":cards[i]["routes"],"reason":if incoming.contains(&i){"declared-route"}else{"no-incoming-edge-in-selected-subgraph"}})).collect();
        if entries.is_empty() {
            entries.push(json!({"symbol":cards[first]["id"],"name":cards[first]["name"],"reason":"cycle-representative; no root inferred"}));
        }
        let declared_intent: Vec<_> = component
            .iter()
            .flat_map(|&i| {
                cards[i]["annotations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|a| a["tag"] == "intent")
                    .map(move |a| json!({"symbol":cards[i]["id"],"text":a["text"],"status":"author-assertion"}))
            })
            .collect();
        let links: Vec<_> = edges.iter().filter(|((from, to), _)| component.contains(from) && component.contains(to)).map(|(_, edge)| edge.clone()).collect();
        out.push(json!({"id":format!("group:{}",cards[first]["id"].as_str().unwrap_or_default()),"label":cards[first]["name"],
            "entry_candidates":entries,"symbols":declarations,"files":files,"static_edges":links,"declared_intent":declared_intent,
            "scope":"selected-current-evidence","runtime_order":"unknown","business_meaning":"not inferred"}));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directed_receipted_edges_group_cycles_but_ambiguity_and_mismatched_receipts_do_not_merge() {
        let mut cards: Vec<Value> = (0..4)
            .map(|i| json!({"id":format!("s{i}"),"name":format!("f{i}"),"source":{"file":format!("{i}"),"sha256":format!("hash{i}")},"outgoing":[]}))
            .collect();
        cards[0]["outgoing"] = json!([{"target":"s1","source":cards[1]["source"],"resolution":"unique-static-target","call_line":3}]);
        cards[1]["outgoing"] = json!([{"target":"s0","source":cards[0]["source"],"resolution":"unique-static-target","call_line":8}]);
        cards[2]["outgoing"] = json!([{"target":"s0","source":cards[0]["source"],"resolution":"ambiguous"}]);
        cards[3]["outgoing"] = json!([{"target":"s0","source":{"sha256":"wrong"},"resolution":"unique-static-target"}]);
        let groups = groups(&cards);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0]["static_edges"].as_array().unwrap().len(), 2);
        assert_eq!(groups[0]["entry_candidates"][0]["reason"], "cycle-representative; no root inferred");
    }
}
