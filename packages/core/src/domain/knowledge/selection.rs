//! Second-stage responsibility ranking inside already-discovered files.
//! Scores locate written evidence; neither scores nor model choices prove behavior.
use super::{Card, evidence, retrieval::Terms};
use crate::domain::normalize::{Languages, Normalizer};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub mod policy;

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

/// One responsibility-bearing declaration per file precedes supplementary
/// symbols. This is a reading plan, not a semantic recommendation.
pub fn task_order(cards:&[Card],query:&str,languages:&Languages,weights:&[f64],anchors:&[String])->Vec<usize> {
    let groups=within_files(cards,query,languages,weights);
    let mut ranked:Vec<_>=groups.values().flatten().cloned().collect();
    // Explicit acronym identifiers may be shorter than lexical stems. An
    // exact path component is a useful navigation clue, never a domain rule.
    let identifiers:BTreeSet<_>=query.split(|c:char|!c.is_alphanumeric()).filter(|word|word.len()>=2 && word.chars().any(char::is_uppercase) && word.chars().all(|c|c.is_uppercase() || c.is_ascii_digit())).map(str::to_lowercase).collect();
    let path_clues=|i:usize|cards[i].source.file.split(|c:char|!c.is_alphanumeric()).filter(|part|identifiers.contains(&part.to_lowercase())).count();
    ranked.sort_by(|a,b|anchors.contains(&cards[b.card].id).cmp(&anchors.contains(&cards[a.card].id))
        .then_with(||path_clues(b.card).cmp(&path_clues(a.card))).then_with(||b.score.total_cmp(&a.score)).then_with(||b.own_matches.cmp(&a.own_matches)).then_with(||cards[a.card].id.cmp(&cards[b.card].id)));
    let mut files=BTreeSet::new();
    let mut leaders=Vec::new();let mut additional=Vec::new();
    for rank in ranked {
        if files.insert(&cards[rank.card].source.file) {leaders.push(rank.card);} else {additional.push(rank.card);}
    }
    leaders.extend(additional);leaders
}

/// Bodies add new written clues; other candidates remain expandable ranges.
/// Containing types cannot spend context repeating their callable children.
pub fn task_bodies(cards:&[Card],slots:&[BTreeSet<usize>],primary:&BTreeSet<String>,recommended:&[String],anchors:&BTreeSet<String>,query:&str,languages:&Languages)->BTreeSet<usize> {
    let terms=Terms::of(query,languages);
    let mut covered=BTreeSet::new();let mut bodies=BTreeSet::new();
    let container=|card:&Card|!terms.definition() && !terms.callable(&card.kind) && cards.iter().any(|child|child.source.file==card.source.file && child.source.line>card.source.line && child.source.end_line<=card.source.end_line && terms.callable(&child.kind));
    let focused:BTreeSet<_>=cards.iter().filter(|card|anchors.contains(&card.id) && !container(card)).map(|card|card.id.as_str()).collect();
    let mut order:Vec<_>=(0..cards.len()).collect();
    order.sort_by_key(|&i|(!focused.contains(cards[i].id.as_str()),!recommended.contains(&cards[i].id),i));
    for i in order {
        let card=&cards[i];
        let chosen=recommended.contains(&card.id);
        // An exact name in the native pattern anchors a local investigation.
        // Neighbouring declarations that only share words stay expandable;
        // they cannot spend context on an unrelated implementation.
        let discover=focused.is_empty();
        if (chosen && !container(card)) || focused.contains(card.id.as_str()) || (primary.contains(&card.id) && !container(card) && discover && slots[i].iter().any(|slot|!covered.contains(slot))) {
            bodies.insert(i);covered.extend(slots[i].iter().copied());
        }
    }
    bodies
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

    #[test]
    fn task_reading_keeps_native_anchors_and_explicit_short_path_identifiers() {
        let mut cards=cards();cards[0].source.file="src/xy/archive.ext".into();cards[1].source.file="src/qz/archive.ext".into();
        let anchors=vec![cards[0].id.clone(),cards[1].id.clone()];
        let order=task_order(&cards,"QZ quartz beacon",&Languages::new(["en-US"]),&[],&anchors);
        assert_eq!(order[0],1);
        assert!(responsibility(&cards,"QZ quartz beacon",&Languages::new(["en-US"])).recommendations.is_empty());
    }

    #[test]
    fn initial_task_bodies_do_not_repeat_containers_but_preserve_a_requested_choice() {
        let cards = cards();
        let primary = cards.iter().map(|card| card.id.clone()).collect();
        let slots = vec![BTreeSet::from([0, 1]), BTreeSet::from([0, 1]), BTreeSet::from([1])];
        let languages = Languages::new(["en-US"]);
        assert_eq!(task_bodies(&cards, &slots, &primary, &[], &BTreeSet::new(), "quartz beacon", &languages), BTreeSet::from([1]));
        assert_eq!(task_bodies(&cards, &slots, &primary, &[cards[2].id.clone()], &BTreeSet::new(), "quartz beacon", &languages), BTreeSet::from([1, 2]));
        assert_eq!(task_bodies(&cards, &slots, &primary, &[cards[0].id.clone()], &BTreeSet::from([cards[0].id.clone()]), "quartz beacon", &languages), BTreeSet::from([1]));
    }

    #[test]
    fn native_names_focus_bodies_without_suppressing_an_explicit_choice_or_navigation() {
        let cards=cards();
        let primary=cards.iter().map(|card|card.id.clone()).collect();
        let anchors=BTreeSet::from([cards[1].id.clone()]);
        let slots=vec![BTreeSet::from([0,1]),BTreeSet::from([0]),BTreeSet::from([1])];
        let languages=Languages::new(["en-US"]);
        assert_eq!(task_bodies(&cards,&slots,&primary,&[],&anchors,"quartz archive",&languages),BTreeSet::from([1]));
        assert_eq!(task_bodies(&cards,&slots,&primary,&[cards[2].id.clone()],&anchors,"quartz archive",&languages),BTreeSet::from([1,2]));
        let mut linked=cards.clone();
        linked[1].outgoing.push(json!({"target":cards[2].id}));
        assert_eq!(task_bodies(&linked,&slots,&primary,&[],&anchors,"quartz archive",&languages),BTreeSet::from([1]));
    }
}
