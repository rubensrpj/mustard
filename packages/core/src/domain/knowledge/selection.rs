//! Second-stage responsibility ranking inside already-discovered files.
//! Scores locate written evidence; neither scores nor model choices prove behavior.
use super::{Card, evidence, retrieval::Terms};
use crate::domain::normalize::{Languages, Normalizer};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub mod policy;
pub mod fusion;

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

/// For an area survey, written identifier components in the original
/// search identify the area to read before generic verbs elsewhere. This
/// is reading priority only; it is never an exact identity or recommendation.
pub fn area_anchors(cards:&[Card],pattern:&str,seeds:&[String])->BTreeSet<String> {
    let names=crate::domain::code_search::pattern_names(pattern);
    // Keep literal identifier spelling here: no stemming, synonyms or joined
    // prose. CreateCharge matches CreateChargeAsync, but not DischargeAsync.
    let mut normalizer=Normalizer::new(&Languages::new([]));
    cards.iter().filter(|card|seeds.contains(&card.id) && {
        let written=normalizer.written_forms(&format!("{} {}",card.source.file,card.name));
        names.iter().any(|name|written.contains(&name.to_lowercase()))
    })
        .map(|card|card.id.clone()).collect()
}

pub fn within_files(cards: &[Card], query: &str, languages: &Languages, weights: &[f64]) -> BTreeMap<String, Vec<Ranked>> {
    let terms = Terms::of(query, languages);
    let mut normalizer = Normalizer::new(languages);
    let mut forms = |text: &str| -> BTreeSet<String> { normalizer.written_forms(text) };
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
    /// An explicitly supplied subquestion, never inferred from project terms.
    pub question: Option<String>,
}

impl Ambiguity {
    pub fn new(key: String, candidates: Vec<Card>) -> Self {
        Self { key, candidates, excerpts: BTreeMap::new(), question: None }
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
    let questions = explicit_questions(query);
    if questions.len() > 1 {
        let mut plans:Vec<_>=questions.iter().map(|question|responsibility(cards,question,languages)).collect();
        let votes=fusion::reciprocal_ranks(&plans.iter().map(|plan|plan.order.clone()).collect::<Vec<_>>());
        let mut order:Vec<_>=votes.keys().copied().collect();
        order.sort_by(|&a,&b|votes[&b].total_cmp(&votes[&a]).then_with(||cards[a].id.cmp(&cards[b].id)));
        let mut groups=Vec::new();let mut recommendations=BTreeSet::new();
        for (at,plan) in plans.iter_mut().enumerate() {
            recommendations.extend(plan.recommendations.iter().cloned());
            for mut group in std::mem::take(&mut plan.groups) {
                group.key=format!("responsibility-{}",at+1);group.question=Some(questions[at].to_string());groups.push(group);
            }
        }
        return Plan{order,recommendations:recommendations.into_iter().collect(),groups,basis:"explicit-subquestions; responsibility-unresolved"};
    }
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
        let mut group=Ambiguity::new("responsibility".into(), order.iter().map(|&i| cards[i].clone()).collect());
        group.question=Some(query.to_string());vec![group]
    } else { vec![] };
    Plan {
        basis: if groups.is_empty() { "insufficient-comparative-evidence" } else { "structural-reading-order; responsibility-unresolved" },
        order, groups, recommendations: vec![],
    }
}

/// A host may submit a short bullet list in the existing intent field. Prose,
/// conjunctions and constraints are not split into invented responsibilities.
pub fn explicit_questions(query:&str)->Vec<&str> {
    let lines:Vec<_>=query.lines().map(str::trim).filter(|line|!line.is_empty()).collect();
    let bullets:Option<Vec<_>>=lines.iter().map(|line|line.strip_prefix("- ").or_else(||line.strip_prefix("* "))
        .map(str::trim).filter(|line|!line.is_empty())).collect();
    if let Some(bullets)=bullets && (2..=8).contains(&bullets.len()) {bullets} else {vec![query]}
}

/// Rank the question before applying a soft cost for repeated files. A strong
/// second declaration can precede a weak first member of another file. Only
/// supplied reading anchors get priority; native hits are not meaning.
pub fn task_order(cards:&[Card],query:&str,languages:&Languages,weights:&[f64],anchors:&[String])->Vec<usize> {
    let mut ranked:Vec<_>=within_files(cards,query,languages,weights).into_values().flatten().collect();
    // A current declaration explicitly located by the original tool survives
    // even if its name is absent from the user's broader intent vocabulary.
    for (i,_) in cards.iter().enumerate().filter(|(_,card)|anchors.contains(&card.id)) {
        if !ranked.iter().any(|r|r.card==i){ranked.push(Ranked{card:i,score:0.0,own_matches:0});}
    }
    let mut normalizer=Normalizer::new(languages);
    let lengths:Vec<_>=cards.iter().map(|card|normalizer.forms(&evidence::own_text(card)).len() as f64).collect();
    let average=lengths.iter().sum::<f64>()/(lengths.len().max(1) as f64);
    // Length-normalized distinct written clues, not a full BM25 model.
    // Preserve the reading order measured before adding native discovery;
    // long implementations must not crowd out small, specific helpers.
    let terms=Terms::of(query,languages);
    for rank in &mut ranked {
        // A tiny field/type is not a tiny implementation. Reserve the short
        // source advantage for callables unless the question asks definitions.
        let length=if !terms.definition() && !terms.callable(&cards[rank.card].kind) {lengths[rank.card].max(average)} else {lengths[rank.card]};
        rank.score/=0.25+0.75*length/average.max(1.0);
    }
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
    fn only_explicit_bullets_become_independent_responsibility_questions() {
        let cards=cards();let languages=Languages::new(["en-US"]);
        let intent="- Which method processes the quartz beacon?\n- Which method archives the beacon?";
        let plan=responsibility(&cards,intent,&languages);
        assert_eq!(plan.groups.len(),2);
        assert_eq!(plan.groups[0].key,"responsibility-1");
        assert_eq!(plan.groups[1].question.as_deref(),Some("Which method archives the beacon?"));
        for prose in ["Create and archive the beacon", "Question: recover quartz.\nConstraint: avoid writes."] {
            assert_eq!(explicit_questions(prose),[prose]);
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
    fn survey_priority_matches_written_identifier_components_only() {
        let mut cards=cards();cards[0].name="CreateChargeAsync".into();cards[1].name="DischargeAsync".into();cards[2].name="Other".into();
        let seeds=cards.iter().map(|c|c.id.clone()).collect::<Vec<_>>();
        assert_eq!(area_anchors(&cards,"CreateCharge|charge",&seeds),BTreeSet::from([cards[0].id.clone()]));
        assert!(area_anchors(&cards,"CreateCharge",&[]).is_empty());
        let order=task_order(&cards,"unseen_question_word",&Languages::new(["en-US"]),&[],std::slice::from_ref(&cards[0].id));
        assert_eq!(order,vec![0]);assert!(responsibility(&cards,"unseen_question_word",&Languages::new(["en-US"])).recommendations.is_empty());
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
    fn a_short_generic_field_cannot_displace_specific_callable_evidence() {
        let mut cards=cards();
        for (i,card) in cards.iter_mut().enumerate(){card.source.file=format!("file{i}.rs");card.signature.clear();card.documentation.clear();card.body_comment.clear();card.identifiers.clear();}
        cards[0].kind="field".into();cards[0].name="metadata".into();cards[0].documentation="record output".into();
        cards[1].kind="function".into();cards[1].name="serialize_response".into();cards[1].identifiers=format!("record output {}",(0..40).map(|i|format!("element_{i}")).collect::<Vec<_>>().join(" "));
        cards[2].kind="function".into();cards[2].name="unrelated".into();cards[2].identifiers=(0..60).map(|i|format!("noise_{i}")).collect::<Vec<_>>().join(" ");
        assert_eq!(task_order(&cards,"record output serialize response",&Languages::new(["en-US"]),&[1.0,1.0,2.0,2.0],&[])[0],1);
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
