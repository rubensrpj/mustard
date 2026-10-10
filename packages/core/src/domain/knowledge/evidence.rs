//! Search source values rather than JSON metadata. Compact matching witnesses
//! point back to the card's source; they do not establish runtime behavior.
use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{Card, retrieval};
use crate::domain::normalize::{Languages, Normalizer};

pub fn source_literal(text: &Value) -> Value {
    let value = text["full_value"].as_str().filter(|value| !value.is_empty()).or_else(|| text["value"].as_str()).unwrap_or_default();
    json!({"line":text["line"],"kind":text["kind"],"value":value})
}

pub fn secondary_text(card: &Card) -> String {
    let literals = card.literals.iter().filter_map(|value| value["value"].as_str()).collect::<Vec<_>>().join(" ");
    let routes =
        card.routes.iter().flat_map(|value| ["method", "path", "handler"].map(|key| value[key].as_str().unwrap_or_default())).collect::<Vec<_>>().join(" ");
    format!("{} {} {literals} {routes}", card.signature, card.identifiers)
}

pub fn own_text(card: &Card) -> String {
    format!("{} {} {} {} {}", card.name, card.documentation, card.body_comment, super::annotation_text(card), secondary_text(card))
}

pub fn searchable_text(card: &Card) -> String {
    format!("{} {} {}", own_text(card), card.source.file, card.file_documentation)
}

/// At most three short witnesses in the first-read response. Full identifiers
/// and literal values remain available through --detail, without a re-scan.
pub fn witnesses(card: &Card, query: &str, languages: &Languages) -> Vec<Value> {
    let slots = retrieval::Terms::of(query, languages).asked;
    if slots.is_empty() {
        return vec![];
    }
    let mut normalizer = Normalizer::new(languages);
    let mut values = Vec::new();
    let mut seen = BTreeSet::new();
    let visible = format!(
        "{} {} {} {} {}",
        card.name,
        super::short(&card.signature, 320),
        super::short(&card.documentation, 320),
        super::short(&card.file_documentation, 220),
        if card.kind == "source-file" { super::short(&card.identifiers, 320) } else { String::new() }
    )
    .to_lowercase();
    for identifier in card.identifiers.split_whitespace() {
        if visible.contains(&identifier.to_lowercase()) {
            continue;
        }
        let forms: BTreeSet<_> = normalizer.forms(identifier).into_iter().flatten().collect();
        let matched: Vec<_> = slots.iter().enumerate().filter(|(_, slot)| slot.iter().any(|form| forms.contains(form))).map(|(i, _)| i).collect();
        if !matched.is_empty() && seen.insert(identifier.to_string()) {
            values.push((matched.len(), identifier.to_string(), json!({"kind":"identifier","value":super::short(identifier,120)})));
        }
    }
    for literal in &card.literals {
        let Some(value) = literal["value"].as_str() else { continue };
        if visible.contains(&value.to_lowercase()) {
            continue;
        }
        let forms: BTreeSet<_> = normalizer.forms(value).into_iter().flatten().collect();
        let matched: Vec<_> = slots.iter().enumerate().filter(|(_, slot)| slot.iter().any(|form| forms.contains(form))).map(|(i, _)| i).collect();
        if !matched.is_empty() && seen.insert(value.to_string()) {
            let (preview, _, compacted) = super::resources::preview_bounded(
                &super::resources::Section {
                    line: literal["line"].as_u64().unwrap_or(card.source.line),
                    end_line: literal["line"].as_u64().unwrap_or(card.source.line),
                    title: String::new(),
                    text: value.into(),
                },
                &slots,
                languages,
                180,
            );
            values.push((matched.len(), value.to_string(), json!({"kind":"literal","value":preview,"line":literal["line"],"compacted":compacted})));
        }
    }
    values.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    values.into_iter().take(3).map(|(_, _, value)| value).collect()
}

pub fn compact_witnesses(card: &Card, query: &str, languages: &Languages) -> Value {
    let mut identifiers = Vec::new();
    let mut literals = Vec::new();
    for value in witnesses(card, query, languages) {
        if value["kind"] == "identifier" {
            identifiers.push(value["value"].clone());
        } else {
            let mut literal = json!({"line":value["line"],"value":value["value"]});
            if value["compacted"] == true {
                literal["compacted"] = json!(true);
            }
            literals.push(literal);
        }
    }
    if identifiers.is_empty() && literals.is_empty() {
        return Value::Null;
    }
    let mut result = json!({});
    if !identifiers.is_empty() {
        result["identifiers"] = json!(identifiers);
    }
    if !literals.is_empty() {
        result["literals"] = json!(literals);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_never_becomes_source_evidence_and_long_literal_witnesses_include_the_match() {
        let mut raw = json!({"modules":[{"path":"src/worker.rs","declarations":[{"name":"process","line":1,"end_line":1800}],
            "texts":[{"owner":"process","line":1777,"kind":"text","value":format!("{} quartzWitness", "ação ".repeat(140))}]}]});
        super::super::enrich(&mut raw);
        let card: Card = serde_json::from_value(raw["modules"][0]["analysis"]["knowledge"]["cards"][0].clone()).unwrap();
        let languages = Languages::new(["pt-BR", "en-US"]);
        assert!(!own_text(&card).contains("1777"));
        for metadata in ["1777", "kind", "value"] {
            assert!(super::super::ranked(std::slice::from_ref(&card), metadata, &languages).is_empty());
        }
        let found = witnesses(&card, "quartzWitness", &languages);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["line"], 1777);
        let text = found[0]["value"].as_str().unwrap();
        assert!(text.contains("quartzWitness"));
        assert!(text.chars().count() <= 182);
        assert_eq!(found[0]["compacted"], true);
    }
}
