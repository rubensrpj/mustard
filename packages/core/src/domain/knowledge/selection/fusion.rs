//! Independent reading channels, fused by rank rather than incompatible scores.
//! A position is a retrieval signal, never a semantic judgement or a source fact.
use super::{Card, evidence, task_order};
use crate::domain::knowledge::retrieval::Terms;
use crate::domain::normalize::{Languages, Normalizer};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Order {
    pub cards: Vec<usize>,
    pub channels: Vec<(String, Vec<usize>)>,
    pub fields: Vec<Value>,
}

impl Order {
    /// Reserve destinations from each channel before source admission. Output
    /// size is independent of this internal source-read reservoir.
    pub fn files(&self, cards: &[Card], per_channel: usize) -> Vec<String> {
        let lists: Vec<Vec<_>> = self.channels.iter().map(|(_, order)| {
            let mut seen = BTreeSet::new();
            order.iter().filter_map(|&i| seen.insert(cards[i].source.file.clone())
                .then_some(cards[i].source.file.clone())).take(per_channel).collect()
        }).collect();
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for at in 0..per_channel {
            for list in &lists {
                if let Some(file) = list.get(at) && seen.insert(file.clone()) { out.push(file.clone()); }
            }
        }
        out
    }

    pub fn trace(&self, i: usize) -> Value {
        let ranks: BTreeMap<_, _> = self.channels.iter().filter_map(|(name, order)| {
            order.iter().position(|&at| at == i).map(|at| (name.clone(), at + 1))
        }).collect();
        json!({"channel_ranks":ranks,"matched_fields":self.fields.get(i)})
    }
}

/// Equal votes from genuinely different channels. A repeated candidate inside
/// one channel cannot add votes. Missing channels do not invent a zero score.
pub fn reciprocal_ranks(channels: &[Vec<usize>]) -> BTreeMap<usize, f64> {
    let mut scores = BTreeMap::new();
    for channel in channels {
        let mut seen = BTreeSet::new();
        for &i in channel {
            if seen.insert(i) {
                let rank = seen.len() as f64;
                *scores.entry(i).or_default() += 1.0 / (60.0 + rank);
            }
        }
    }
    scores
}

fn field_order(cards: &[Card], query: &str, role_query: &str, languages: &Languages, weights: &[f64]) -> (Vec<usize>, Vec<Value>) {
    let terms = Terms::of(query, languages);
    let role = Terms::of(role_query, languages);
    let mut normalizer = Normalizer::new(languages);
    let mut scored = Vec::new();
    let mut fields = Vec::new();
    for (i, card) in cards.iter().enumerate() {
        let values = [card.name.clone(), card.signature.clone(),
            format!("{} {}", card.documentation, card.body_comment),
            evidence::secondary_text(card), card.source.file.clone()];
        let forms: Vec<_> = values.iter().map(|value| normalizer.written_forms(value)).collect();
        let matches: Vec<Vec<usize>> = forms.iter().map(|forms| terms.asked.iter().enumerate()
            .filter_map(|(at, slot)| slot.iter().any(|word| forms.contains(word)).then_some(at)).collect()).collect();
        let mut score = 0.0;
        for at in 0..terms.asked.len() {
            // The same clue gets its strongest field, never five votes. Paths
            // establish context; a containing type cannot borrow child bodies.
            let factor = if matches[0].contains(&at) {5.0}
                else if matches[1].contains(&at) {3.0}
                else if matches[2].contains(&at) {2.0}
                else if matches[3].contains(&at) && (terms.callable(&card.kind) || terms.definition() || card.kind == "source-file") {1.0}
                else if matches[4].contains(&at) {0.25} else {0.0};
            score += factor * weights.get(at).copied().unwrap_or(1.0);
        }
        // A native pattern describes the spelling to find; the written intent
        // describes whether the user needs behavior or a data definition.
        score *= role.responsibility_weight(&card.kind);
        if score > 0.0 {scored.push((i, score));}
        fields.push(json!({"name":matches[0],"signature":matches[1],"documentation":matches[2],"body":matches[3],"path":matches[4]}));
    }
    scored.sort_by(|(a, x), (b, y)| y.total_cmp(x).then_with(|| cards[*a].id.cmp(&cards[*b].id)));
    (scored.into_iter().map(|(i, _)| i).collect(), fields)
}

/// Keep the original lexical ranking as one channel; current native owners and
/// field-aware evidence contribute independently. No project vocabulary here.
pub fn rank(cards: &[Card], pattern: &str, intent: &str, languages: &Languages,
    weights: &[f64], anchors: &[String], seeds: &[String]) -> Order {
    let question = if intent.trim().is_empty() {pattern} else {intent};
    let (fields_order, fields) = field_order(cards, question, question, languages, weights);
    let (mut native, _) = field_order(cards, pattern, question, languages, &[]);
    native.retain(|&i| seeds.contains(&cards[i].id));
    let questions=super::explicit_questions(question);
    let mut channels=Vec::new();
    if questions.len()==1 {
        channels.push(("intent".into(),task_order(cards,question,languages,weights,anchors)));
        channels.push(("source-fields".into(),fields_order));
    } else {
        for (at,question) in questions.iter().enumerate() {
            channels.push((format!("question-{}-intent",at+1),task_order(cards,question,languages,&[],anchors)));
            channels.push((format!("question-{}-fields",at+1),field_order(cards,question,question,languages,&[]).0));
        }
    }
    if !native.is_empty() {channels.push(("native-pattern".into(), native));}
    let scores = reciprocal_ranks(&channels.iter().map(|(_, order)| order.clone()).collect::<Vec<_>>());
    let mut order: Vec<_> = scores.keys().copied().collect();
    order.sort_by(|&a, &b| anchors.contains(&cards[b].id).cmp(&anchors.contains(&cards[a].id))
        .then_with(|| scores[&b].total_cmp(&scores[&a]))
        .then_with(|| cards[a].id.cmp(&cards[b].id)));
    Order {cards:order, channels, fields}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agreement_and_deduplication_are_independent_of_channel_score_scales() {
        let scores = reciprocal_ranks(&[vec![0, 0, 1], vec![1, 2]]);
        assert!(scores[&1] > scores[&0]);
        assert!((scores[&0] - 1.0 / 61.0).abs() < f64::EPSILON);
        assert!((scores[&1] - (1.0 / 62.0 + 1.0 / 61.0)).abs() < f64::EPSILON);
    }
    #[test]
    fn file_admission_preserves_each_channel_instead_of_only_the_merged_leader() {
        let cards = ["native.rs", "intent.rs", "native.rs"].map(|file| serde_json::from_value::<Card>(json!({
            "id":file,"name":"entry","kind":"function","source":{"file":file,"line":1,"end_line":2,"sha256":""},"parse_complete":true
        })).unwrap());
        let order = Order {channels:vec![("a".into(),vec![0,2]),("b".into(),vec![1])],..Order::default()};
        assert_eq!(order.files(&cards, 1), ["native.rs", "intent.rs"]);
    }
    #[test]
    fn symbol_fields_distinguish_a_definition_from_repeated_body_words() {
        let cards:Vec<Card>=[("archiveRecord","function",""),("process","function","archiveRecord archiveRecord archiveRecord"),
            ("Envelope","class","archiveRecord")].into_iter().map(|(name,kind,body)|serde_json::from_value(json!({
                "id":name,"name":name,"kind":kind,"identifiers":body,"source":{"file":"src/archive.rs","line":1,"end_line":2,"sha256":""}
            })).unwrap()).collect();
        let (order,_)=field_order(&cards,"archive record","archive record",&Languages::new(["en-US"]),&[]);
        assert_eq!(order[0],0);
        assert!(order.iter().position(|&i|i==1)<order.iter().position(|&i|i==2));
    }
    #[test]
    fn native_spelling_uses_the_written_intent_for_declaration_roles() {
        let cards:Vec<Card>=[("a-data","field"),("z-behavior","method")].into_iter().map(|(id,kind)|serde_json::from_value(json!({
            "id":id,"name":"archive","kind":kind,"source":{"file":"src/entry.rs","line":1,"end_line":2,"sha256":""}
        })).unwrap()).collect();
        let language=Languages::new(["en-US"]);
        assert_eq!(field_order(&cards,"archive","validate archive",&language,&[]).0[0],1);
        assert_eq!(field_order(&cards,"archive","field archive",&language,&[]).0[0],0);
    }
}
