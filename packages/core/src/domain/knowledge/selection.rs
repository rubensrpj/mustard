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

/// A broad lexical OR is not an intentional list of symbol identities merely
/// because one word happens to be a function name. Every written name must
/// resolve to a current native owner before it can focus body expansion.
pub fn nominal_anchors(cards:&[Card],pattern:&str,seeds:&[String])->BTreeSet<String> {
    let names=crate::domain::code_search::pattern_names(pattern);
    let owners:Vec<_>=cards.iter().filter(|card|seeds.contains(&card.id)).collect();
    let prefixes:BTreeSet<_>=owners.iter().filter(|card|names.iter().any(|name|name.eq_ignore_ascii_case(&card.name)))
        .flat_map(|card|crate::domain::code_search::pattern_names(card.signature.split_once(&card.name).map_or("",|(prefix,_)|prefix)))
        .map(str::to_lowercase).collect();
    // Accept declaration modifiers from the actual signatures, rather than a
    // hard-coded list of language keywords (e.g. a typed declaration pattern).
    if names.is_empty() || !names.iter().all(|name|prefixes.contains(&name.to_lowercase()) || owners.iter().any(|card|name.eq_ignore_ascii_case(&card.name))) {return BTreeSet::new();}
    owners.into_iter().filter(|card|names.iter().any(|name|name.eq_ignore_ascii_case(&card.name))).map(|card|card.id.clone()).collect()
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

/// Rank the question before applying a soft cost for repeated files. A strong
/// second declaration can precede a weak first member of another file. Only
/// explicit nominal anchors get identity priority; native hits are not meaning.
pub fn task_order(cards:&[Card],query:&str,languages:&Languages,weights:&[f64],anchors:&[String])->Vec<usize> {
    let mut ranked:Vec<_>=within_files(cards,query,languages,weights).into_values().flatten().collect();
    let mut normalizer=Normalizer::new(languages);
    let lengths:Vec<_>=cards.iter().map(|card|normalizer.forms(&evidence::own_text(card)).len() as f64).collect();
    let average=lengths.iter().sum::<f64>()/(lengths.len().max(1) as f64);
    // BM25's document-length component prevents a large implementation from
    // winning merely because it contains more of the question's vocabulary.
    // Names, signatures and source witnesses still determine the numerator.
    for rank in &mut ranked {rank.score/=0.25+0.75*lengths[rank.card]/average.max(1.0);}
    let identifiers:BTreeSet<_>=query.split(|c:char|!c.is_alphanumeric()).filter(|word|word.len()>=2 && word.chars().any(char::is_uppercase) && word.chars().all(|c|c.is_uppercase() || c.is_ascii_digit())).map(str::to_lowercase).collect();
    let path_clues=|i:usize|cards[i].source.file.split(|c:char|!c.is_alphanumeric()).filter(|part|identifiers.contains(&part.to_lowercase())).count();
    let mut files=BTreeMap::<&str,usize>::new();let mut order=Vec::new();
    while !ranked.is_empty() {
        let score=|r:&Ranked|r.score/(1.0+*files.get(cards[r.card].source.file.as_str()).unwrap_or(&0) as f64).sqrt();
        let (at,_)=ranked.iter().enumerate().max_by(|(_,a),(_,b)|
            anchors.contains(&cards[a.card].id).cmp(&anchors.contains(&cards[b.card].id))
            .then_with(||path_clues(a.card).cmp(&path_clues(b.card)))
            .then_with(||score(a).total_cmp(&score(b)))
            .then_with(||a.own_matches.cmp(&b.own_matches))
            .then_with(||cards[b.card].id.cmp(&cards[a.card].id))).expect("nonempty ranking");
        let rank=ranked.remove(at);*files.entry(&cards[rank.card].source.file).or_default()+=1;order.push(rank.card);
    }
    order
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
    fn broad_search_words_do_not_turn_a_coincidental_name_into_an_exclusive_anchor() {
        let mut cards=cards();let seeds=cards.iter().map(|c|c.id.clone()).collect::<Vec<_>>();
        assert_eq!(nominal_anchors(&cards,"process",&seeds),BTreeSet::from([cards[1].id.clone()]));
        assert_eq!(nominal_anchors(&cards,"process|archive",&seeds),BTreeSet::from([cards[1].id.clone(),cards[2].id.clone()]));
        assert!(nominal_anchors(&cards,"process|request|payload|rollback",&seeds).is_empty());
        assert!(nominal_anchors(&cards,"process",&[]).is_empty());
        cards[1].signature="pub async fn process()".into();
        assert_eq!(nominal_anchors(&cards,"pub async process",&seeds),BTreeSet::from([cards[1].id.clone()]));
    }

    #[test]
    fn a_strong_second_function_precedes_a_weak_representative_of_another_file() {
        let mut cards=cards();
        for (i,card) in cards.iter_mut().enumerate() {
            card.kind="function".into();card.name=format!("f{i}");card.signature=String::new();card.identifiers=String::new();card.body_comment=String::new();
        }
        cards[0].documentation="ledger rollback".into();cards[1].documentation="ledger revision".into();cards[2].documentation="ledger".into();cards[2].source.file="other.rs".into();
        let order=task_order(&cards,"ledger rollback revision",&Languages::new(["en-US"]),&[],&[]);
        assert_eq!(order.last(),Some(&2));
    }

    #[test]
    fn broad_implementations_do_not_win_by_accumulating_common_question_words() {
        let mut cards=cards();
        for card in &mut cards {card.kind="function".into();card.signature=String::new();card.body_comment=String::new();card.documentation=String::new();}
        cards[0].name="wide".into();cards[0].identifiers=format!("ledger revision checkpoint {}",(0..400).map(|n|format!("unrelated_{n}")).collect::<Vec<_>>().join(" "));
        cards[1].name="restore_checkpoint".into();cards[1].identifiers="ledger revision".into();
        cards[2].name="other".into();cards[2].identifiers="ledger".into();
        let order=task_order(&cards,"restore revision ledger checkpoint",&Languages::new(["en-US"]),&[],&[]);
        assert_eq!(order[0],1);
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
