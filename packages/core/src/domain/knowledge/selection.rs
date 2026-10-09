//! Second-stage responsibility ranking inside already-discovered files.
//! Scores locate written evidence; neither scores nor model choices prove behavior.
use super::{Card, evidence, retrieval::Terms};
use crate::domain::normalize::{Languages, Normalizer};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Ranked {
    pub card: usize,
    pub score: f64,
    pub own_matches: usize,
}

pub fn within_files(cards: &[Card], query: &str, languages: &Languages, weights: &[f64]) -> BTreeMap<String, Vec<Ranked>> {
    let terms = Terms::of(query, languages);
    let mut normalizer = Normalizer::new(languages);
    let mut forms = |text: &str| -> BTreeSet<String> { normalizer.forms(text).into_iter().flatten().collect() };
    let names: Vec<_> = cards.iter().map(|c| forms(&c.name)).collect();
    let signatures: Vec<_> = cards.iter().map(|c| forms(&c.signature)).collect();
    let docs: Vec<_> = cards.iter().map(|c| forms(&format!("{} {}", c.documentation, super::annotation_text(c)))).collect();
    let comments: Vec<_> = cards.iter().map(|c| forms(&c.body_comment)).collect();
    let written: Vec<_> = cards.iter().map(|c| forms(&evidence::secondary_text(c))).collect();
    let mut groups: BTreeMap<String, Vec<Ranked>> = BTreeMap::new();
    for (i, card) in cards.iter().enumerate() {
        let mut body = written[i].clone();
        let mut own_comments = comments[i].clone();
        // A containing type must not win just because its flattened identifiers
        // include every child method. This subtraction is a ranking heuristic;
        // all original evidence remains available in the stored card.
        if !terms.definition() && !terms.callable(&card.kind) {
            for (j, _) in cards.iter().enumerate().filter(|(_, c)| {
                c.source.file == card.source.file && c.source.line > card.source.line && c.source.end_line <= card.source.end_line && terms.callable(&c.kind)
            }) {
                for word in &written[j] {
                    body.remove(word);
                }
                for word in &comments[j] {
                    own_comments.remove(word);
                }
            }
        }
        let mut own_matches = 0;
        let score = terms
            .asked
            .iter()
            .enumerate()
            .map(|(at, slot)| {
                let hits = |set: &BTreeSet<String>| slot.iter().any(|s| set.contains(s));
                let factor = if hits(&names[i]) {
                    4.0
                } else if hits(&signatures[i]) {
                    3.0
                } else if hits(&docs[i]) || hits(&own_comments) {
                    2.5
                } else if hits(&body) {
                    1.0
                } else {
                    0.0
                };
                if factor > 0.0 {
                    own_matches += 1;
                }
                factor * weights.get(at).copied().unwrap_or(1.0)
            })
            .sum::<f64>()
            * terms.responsibility_weight(&card.kind);
        if score > 0.0 {
            groups.entry(card.source.file.clone()).or_default().push(Ranked { card: i, score, own_matches });
        }
    }
    for group in groups.values_mut() {
        group.sort_by(|a, b| {
            b.own_matches
                .cmp(&a.own_matches)
                .then_with(|| b.score.total_cmp(&a.score))
                .then_with(|| {
                    cards[a.card]
                        .source
                        .end_line
                        .saturating_sub(cards[a.card].source.line)
                        .cmp(&cards[b.card].source.end_line.saturating_sub(cards[b.card].source.line))
                })
                .then_with(|| cards[a.card].id.cmp(&cards[b.card].id))
        });
    }
    groups
}

#[derive(Debug, Clone)]
pub struct Ambiguity {
    /// Stable decision scope, which may span files. It is not a source path.
    pub key: String,
    pub candidates: Vec<Card>,
    pub excerpts: BTreeMap<String, Excerpt>,
}

impl Ambiguity {
    pub fn new(key: String, candidates: Vec<Card>) -> Self {
        Self { key, candidates, excerpts: BTreeMap::new() }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Excerpt {
    pub source: super::Source,
    pub text: String,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Selected,
    NoMatch,
    InsufficientEvidence,
    BelowAcceptance,
    InvalidAnswer,
}

/// Ranking is only a reading order. A lexical lead never establishes the
/// requested responsibility. Exact identities need no external judgement;
/// all other supported alternatives share one decision across discovered files.
#[derive(Debug, Default)]
pub struct Plan {
    pub order: Vec<usize>,
    pub recommendations: Vec<String>,
    pub groups: Vec<Ambiguity>,
    pub basis: &'static str,
}

pub fn responsibility(cards: &[Card], query: &str, languages: &Languages) -> Plan {
    if query.trim().is_empty() {
        return Plan { basis: "intent-missing", ..Plan::default() };
    }
    let exact: Vec<_> = cards.iter().enumerate().filter(|(_, c)| c.name == query.trim() || c.id == query.trim()).collect();
    if let [(i, card)] = exact.as_slice() {
        return Plan {
            order: vec![*i], recommendations: vec![card.id.clone()], basis: "exact-symbol-identity", ..Plan::default()
        };
    }
    let mut ranked: Vec<_> = within_files(cards, query, languages, &[]).into_values().flatten().collect();
    ranked.sort_by(|a, b| b.own_matches.cmp(&a.own_matches).then_with(|| b.score.total_cmp(&a.score))
        .then_with(|| cards[a.card].id.cmp(&cards[b.card].id)));
    let order: Vec<_> = ranked.iter().map(|r| r.card).collect();
    let groups = if order.len() > 1 {
        vec![Ambiguity::new("responsibility".into(), order.iter().map(|&i| cards[i].clone()).collect())]
    } else { vec![] };
    Plan {
        basis: if groups.is_empty() { "insufficient-comparative-evidence" } else { "structural-reading-order; responsibility-unresolved" },
        order, groups, recommendations: vec![],
    }
}

/// Only candidates close to the native winner, with multiple written query
/// clues, can affect the next choice. No request for a missing candidate.
pub fn ambiguity(file: &str, group: &[Ranked], cards: &[Card]) -> Option<Ambiguity> {
    let first = group.first()?;
    if first.own_matches < 2 {
        return None;
    }
    let margin = (first.score * 0.12).max(0.5);
    let candidates: Vec<_> = group
        .iter()
        .take_while(|r| r.own_matches == first.own_matches && first.score - r.score <= margin)
        .filter(|r| r.own_matches >= 2)
        .map(|r| cards[r.card].clone())
        .collect();
    (candidates.len() > 1).then(|| Ambiguity::new(file.into(), candidates))
}

#[derive(Debug, Default)]
pub struct Decisions {
    /// Only a supplied candidate id can replace its own group's winner.
    pub choices: BTreeMap<String, String>,
    pub outcomes: BTreeMap<String, Outcome>,
    pub usage: Value,
}

pub trait SymbolSelector {
    fn select(&self, query: &str, groups: &[Ambiguity]) -> Decisions;
}

pub fn native_usage() -> Value {
    json!({"status":"native","remote_model_calls":0})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cards() -> Vec<Card> {
        let mut raw = json!({"modules":[{"path":"src/worker.rs","declarations":[
            {"name":"BeaconProvider","kind":"class","line":1,"end_line":20,"body_names":"quartz beacon archive","body_comment":"quartz beacon"},
            {"name":"process","kind":"method","line":3,"end_line":8,"body_names":"quartz beacon","body_comment":"quartz beacon"},
            {"name":"archive","kind":"method","line":10,"end_line":14,"body_names":"archive"}]}]});
        super::super::enrich(&mut raw);
        serde_json::from_value(raw["modules"][0]["analysis"]["knowledge"]["cards"].clone()).unwrap()
    }
    #[test]
    fn a_type_does_not_inherit_responsibility_from_its_methods() {
        let cards = cards();
        let languages = Languages::new(["en-US"]);
        let groups = within_files(&cards, "quartz beacon", &languages, &[]);
        assert_eq!(cards[groups["src/worker.rs"][0].card].name, "process");
        assert!(ambiguity("src/worker.rs", &groups["src/worker.rs"], &cards).is_none());
    }
    #[test]
    fn ambiguity_requires_multiple_own_clues_and_close_scores() {
        let mut cards = cards();
        cards[2].identifiers = "quartz beacon".into();
        cards[2].body_comment = "quartz beacon".into();
        let groups = within_files(&cards, "quartz beacon", &Languages::new(["en-US"]), &[]);
        assert_eq!(ambiguity("src/worker.rs", &groups["src/worker.rs"], &cards).unwrap().candidates.len(), 2);
        let groups = within_files(&cards, "quartz", &Languages::new(["en-US"]), &[]);
        assert!(ambiguity("src/worker.rs", &groups["src/worker.rs"], &cards).is_none());
    }

    #[test]
    fn lexical_leads_are_reading_order_not_responsibility_certainty() {
        let mut cards = cards();
        cards[2].identifiers = "beacon".into();
        let plan = responsibility(&cards, "quartz beacon", &Languages::new(["en-US"]));
        assert!(plan.recommendations.is_empty());
        assert_eq!(plan.groups.len(), 1);
        assert!(plan.groups[0].candidates.iter().any(|c| c.name == "archive"));
    }

    #[test]
    fn responsibility_compares_discovered_files_and_exact_names_need_no_choice() {
        let mut cards = cards();
        cards[2].source.file = "different/language.ext".into();
        cards[2].identifiers = "quartz beacon".into();
        let languages = Languages::new(["en-US"]);
        let plan = responsibility(&cards, "quartz beacon", &languages);
        assert_eq!(plan.groups.len(), 1);
        assert!(plan.groups[0].candidates.iter().any(|c|c.source.file == "different/language.ext"));
        let exact = responsibility(&cards, "archive", &languages);
        assert_eq!(exact.recommendations, [cards[2].id.clone()]);
        assert!(exact.groups.is_empty());
        cards[1].name = "archive".into();
        let homonyms = responsibility(&cards, "archive", &languages);
        assert!(homonyms.recommendations.is_empty());
        assert_eq!(homonyms.groups.len(), 1);
    }

    #[test]
    fn missing_and_single_owners_do_not_invent_a_semantic_recommendation() {
        let cards = cards();
        let languages = Languages::new(["en-US"]);
        for query in ["", "unknownIdentifier", "persist records"] {
            let plan = responsibility(&cards[..1], query, &languages);
            assert!(plan.groups.is_empty());
            assert!(plan.recommendations.is_empty());
        }
    }
}
