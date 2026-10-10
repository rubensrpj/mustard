//! Reading views of source evidence, fused by rank rather than incompatible scores.
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
    /// Balance lexical consensus and an independent code index as two families.
    /// Correlated lexical views must not outvote code retrieval just by count.
    #[must_use]
    pub fn with_code(mut self, ids:&[String], cards:&[Card], anchors:&[String])->Self {
        let by_id:BTreeMap<_,_>=cards.iter().enumerate().map(|(i,c)|(c.id.as_str(),i)).collect();
        let code:Vec<_>=ids.iter().filter_map(|id|by_id.get(id.as_str()).copied()).collect();
        if code.is_empty(){return self;}
        let scores=reciprocal_ranks(&[self.cards.clone(),code.clone()]);
        self.cards=scores.keys().copied().collect();
        self.cards.sort_by(|&a,&b|scores[&b].total_cmp(&scores[&a]).then(cards[a].id.cmp(&cards[b].id)));
        self.channels.push(("local-code-meaning".into(),code));
        self.preserve_leaders(anchors,cards);self
    }
    /// Preserve leaders of each retrieval view before the fused consensus.
    /// Correlated channels must not erase a strong independent discovery.
    pub fn preserve_leaders(&mut self, anchors: &[String], cards: &[Card]) {
        let mut seen=BTreeSet::new();
        let mut order=Vec::new();
        for &i in &self.cards {
            if anchors.contains(&cards[i].id) && seen.insert(i) {order.push(i);}
        }
        for rank in 0..3 {
            for (_,channel) in &self.channels {
                if let Some(&i)=channel.get(rank) && seen.insert(i) {order.push(i);}
            }
        }
        for &i in &self.cards {if seen.insert(i) {order.push(i);}}
        self.cards=order;
    }
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

/// Equal votes from complementary, potentially correlated views. A repeated candidate inside
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
    field_order_with_context(cards,query,role_query,languages,weights,&BTreeMap::new(),&BTreeMap::new())
}

fn field_order_with_context(cards: &[Card], query: &str, role_query: &str, languages: &Languages, weights: &[f64], context:&BTreeMap<usize,f64>, positions:&BTreeMap<usize,usize>) -> (Vec<usize>, Vec<Value>) {
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
    scored.sort_by(|(a, x), (b, y)| y.total_cmp(x)
        .then_with(||context.get(b).copied().unwrap_or(0.0).total_cmp(&context.get(a).copied().unwrap_or(0.0)))
        .then_with(||positions.get(a).copied().unwrap_or(usize::MAX).cmp(&positions.get(b).copied().unwrap_or(usize::MAX)))
        .then_with(|| cards[*a].id.cmp(&cards[*b].id)));
    (scored.into_iter().map(|(i, _)| i).collect(), fields)
}

/// Keep the original lexical ranking as one channel; current native owners and
/// field-aware evidence contribute independently. No project vocabulary here.
pub fn rank(cards: &[Card], pattern: &str, intent: &str, languages: &Languages,
    weights: &[f64], anchors: &[String], seeds: &[String]) -> Order {
    let question = if intent.trim().is_empty() {pattern} else {intent};
    let (fields_order, fields) = field_order(cards, question, question, languages, weights);
    // A native occurrence and its question are a conjunction. Sorting native
    // owners by spelling alone makes unrelated homonyms tie by path, even
    // when current evidence distinguishes their requested responsibility.
    let positions:BTreeMap<_,_>=fields_order.iter().enumerate().map(|(rank,&i)|(i,rank)).collect();
    let mut literal=Normalizer::new(&Languages::new([]));
    let native_words=literal.written_forms(pattern);
    let context_words:BTreeSet<_>=literal.written_forms(question).difference(&native_words).cloned().collect();
    let terms=Terms::of(question,languages);
    let context:BTreeMap<_,_>=cards.iter().enumerate().map(|(i,card)| {
        let written=literal.written_forms(&format!("{} {}",card.name,card.source.file));
        let score=terms.asked.iter().enumerate().filter(|(_,slot)|slot.iter().any(|word|context_words.contains(word) && written.contains(word)))
            .map(|(at,_)|weights.get(at).copied().unwrap_or(1.0)).sum();(i,score)
    }).collect();
    // Context resolves ties only: a body reference cannot displace a named
    // declaration just because its unrelated body repeats more question words.
    let (mut native, _)=field_order_with_context(cards,pattern,question,languages,&[],&context,&positions);
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
    if !native.is_empty() {channels.push(("native-with-question".into(), native));}
    let scores = reciprocal_ranks(&channels.iter().map(|(_, order)| order.clone()).collect::<Vec<_>>());
    let mut order: Vec<_> = scores.keys().copied().collect();
    order.sort_by(|&a, &b| anchors.contains(&cards[b].id).cmp(&anchors.contains(&cards[a].id))
        .then_with(|| scores[&b].total_cmp(&scores[&a]))
        .then_with(|| cards[a].id.cmp(&cards[b].id)));
    let mut out=Order {cards:order, channels, fields};
    out.preserve_leaders(anchors,cards);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_single_channel_leader_survives_consensus_and_keeps_exact_anchors() {
        let cards:Vec<Card>=(0..40).map(|i|serde_json::from_value(json!({"id":i.to_string(),"name":i.to_string(),"kind":"function",
            "source":{"file":"a.rs","line":i+1,"end_line":i+1,"sha256":""}})).unwrap()).collect();
        let mut order=Order {cards:(0..40).collect(),channels:vec![("intent".into(),vec![35,34,33]),("native".into(),vec![0,1,2])],fields:vec![]};
        order.preserve_leaders(&["10".into()],&cards);
        assert_eq!(&order.cards[..5],&[10,35,0,34,1]);
        assert_eq!(order.cards.len(),40);
        assert_eq!(order.cards.iter().collect::<BTreeSet<_>>().len(),40);
    }
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
    #[test]
    fn native_homonyms_are_ordered_by_the_question_before_cutting_the_pool() {
        let cards:Vec<Card>=[("a","authenticate request"),("z","persist quartz checkpoint")].into_iter()
            .map(|(id,doc)|serde_json::from_value(json!({"id":id,"name":"handleEvent","kind":"method","documentation":doc,
                "source":{"file":format!("{id}.ts"),"line":1,"end_line":4,"sha256":""}})).unwrap()).collect();
        let order=rank(&cards,"event","persist quartz checkpoint",&Languages::new(["en-US"]),&[],&[],&["a".into(),"z".into()]);
        assert_eq!(order.channels.iter().find(|(name,_)|name=="native-with-question").unwrap().1,vec![1,0]);
        assert_eq!(order.cards[0],1);
    }
}
