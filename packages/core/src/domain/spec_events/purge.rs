//! O expurgo: o trecho que nunca podia ter sido gravado vira "…" na própria
//! linha, em todas as versões do item e nos dois lados do par de um ponto do
//! levantamento, e o resto do arquivo fica byte a byte como estava.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::domain::spec_state::original_of;
use crate::domain::survey;

use super::read::parse_object;
use super::search::search_of;
use super::{render_line, type_spec, Kind, Refusal, SpecEvent, SpecLog, PURGED_MARK};

/// Os trechos que um expurgo troca em cada alvo: o trecho que o pedido indica
/// (`asked`) ou, sem ele, os que `find` acha em algum campo de texto do alvo.
/// Um item apontado pelo código traz todas as versões dele, e basta o trecho
/// aparecer numa delas; o item em que ele não aparece em versão nenhuma é
/// recusado com o código dele.
///
/// # Errors
///
/// [`Refusal::PurgeExcerptNotFound`] com o código do primeiro item sem trecho.
pub fn purge_excerpts(
    log: &SpecLog,
    targets: &[u64],
    asked: Option<&str>,
    find: &dyn Fn(&str) -> Vec<String>,
) -> Result<BTreeMap<u64, Vec<String>>, Refusal> {
    let codes = log.codes();
    let item_of = |id: &u64| codes.get(id).cloned().unwrap_or_else(|| id.to_string());
    let mut out = BTreeMap::new();
    for id in targets {
        let texts = log.get(*id).map(|event| texts_of(&event.fields)).unwrap_or_default();
        let mut found: Vec<String> = Vec::new();
        let candidates = match asked.map(str::trim).filter(|a| !a.is_empty()) {
            Some(asked) => vec![asked.to_string()],
            None => texts.iter().flat_map(|text| find(text)).collect(),
        };
        for excerpt in candidates {
            if !excerpt.is_empty() && texts.iter().any(|t| t.contains(&excerpt)) && !found.contains(&excerpt) {
                found.push(excerpt);
            }
        }
        if found.is_empty() {
            continue;
        }
        // O trecho mais longo primeiro: um trecho dentro de outro não deixa
        // sobra.
        found.sort_by_key(|excerpt| std::cmp::Reverse(excerpt.len()));
        out.insert(*id, found);
    }
    let touched: BTreeSet<String> = out.keys().map(item_of).collect();
    if let Some(item) = targets.iter().map(item_of).find(|item| !touched.contains(item)) {
        return Err(Refusal::PurgeExcerptNotFound { code: item });
    }
    reach_point_pairs(log, &mut out);
    Ok(out)
}

/// O fechamento de um ponto copia a lacuna do original, e por isso o trecho
/// achado num ponto do levantamento sai de todas as linhas do mesmo par — o
/// original e o fechamento, em qualquer versão, inclusive as que já saíram
/// da leitura — em que ele aparece. A lacuna segue igual nos dois lados.
fn reach_point_pairs(log: &SpecLog, out: &mut BTreeMap<u64, Vec<String>>) {
    let pair_of = |event: &SpecEvent| {
        (event.event_type == "point")
            .then(|| survey::closed_first(log, event).unwrap_or_else(|| original_of(log, event)))
    };
    let mut by_pair: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for (id, excerpts) in out.iter() {
        if let Some(pair) = log.get(*id).and_then(pair_of) {
            by_pair.entry(pair).or_default().extend(excerpts.iter().cloned());
        }
    }
    for event in &log.events {
        let Some(excerpts) = pair_of(event).and_then(|pair| by_pair.get(&pair)) else { continue };
        let texts = texts_of(&event.fields);
        let mut reached: Vec<String> = out.get(&event.id).cloned().unwrap_or_default();
        for excerpt in excerpts {
            if !reached.contains(excerpt) && texts.iter().any(|t| t.contains(excerpt.as_str())) {
                reached.push(excerpt.clone());
            }
        }
        if !reached.is_empty() {
            reached.sort_by_key(|excerpt| std::cmp::Reverse(excerpt.len()));
            out.insert(event.id, reached);
        }
    }
}

/// O campo `key` de um item do tipo `event_type` guarda texto de quem gravou:
/// o texto, as palavras-chave, o rótulo e os campos do tipo que são texto,
/// lista ou objeto — a lacuna de um ponto também. A situação, o bloco, a fase
/// e os outros campos de palavra fixa, os números e as horas não são texto, e
/// o expurgo não mexe neles.
fn holds_text(event_type: &str, key: &str) -> bool {
    if matches!(key, "text" | "keys" | "label") {
        return true;
    }
    type_spec(event_type)
        .and_then(|spec| spec.fields.iter().find(|field| field.name == key))
        .is_some_and(|field| {
            matches!(field.kind, Kind::Text | Kind::Texts | Kind::Object | Kind::Objects | Kind::List | Kind::TextOrObject)
        })
}

/// Os campos de texto de um item ([`holds_text`]), em qualquer profundidade.
fn texts_of(fields: &Map<String, Value>) -> Vec<&str> {
    fn walk<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
        match value {
            Value::String(text) => out.push(text),
            Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            Value::Object(map) => map.values().for_each(|item| walk(item, out)),
            _ => {}
        }
    }
    let event_type = fields.get("type").and_then(Value::as_str).unwrap_or_default();
    let mut out = Vec::new();
    for (key, value) in fields {
        if holds_text(event_type, key) {
            walk(value, &mut out);
        }
    }
    out
}

/// Troca cada trecho de `excerpts` por [`PURGED_MARK`] em todo campo de texto
/// de `value`.
fn redact(value: &mut Value, excerpts: &[String]) {
    match value {
        Value::String(text) => {
            for excerpt in excerpts {
                if text.contains(excerpt.as_str()) {
                    *text = text.replace(excerpt.as_str(), PURGED_MARK);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| redact(item, excerpts)),
        Value::Object(map) => map.values_mut().for_each(|item| redact(item, excerpts)),
        _ => {}
    }
}

/// O arquivo depois do expurgo: em cada linha de `redactions`, os trechos dela
/// viram [`PURGED_MARK`] em todos os campos de texto, e o `search` é
/// recalculado do texto que ficou; as outras linhas, inclusive as que não se
/// entendem, ficam byte a byte como estavam.
#[must_use]
pub fn purge_lines(content: &str, redactions: &BTreeMap<u64, Vec<String>>) -> String {
    content
        .split('\n')
        .map(|raw| {
            let line = raw.trim_end_matches('\r');
            let Some(mut map) = parse_object(line) else {
                return raw.to_string();
            };
            let Some(excerpts) = map.get("id").and_then(Value::as_u64).and_then(|id| redactions.get(&id)) else {
                return raw.to_string();
            };
            let event_type = map.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
            for (key, value) in &mut map {
                if holds_text(&event_type, key) {
                    redact(value, excerpts);
                }
            }
            map.remove("search");
            if let Some(search) = search_of(&map) {
                map.insert("search".into(), Value::String(search));
            }
            render_line(&map)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
