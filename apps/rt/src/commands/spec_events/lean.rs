//! A forma enxuta da leitura de um bloco: o conteúdo de cada item, sem as
//! marcas que o binário carimba para si.
//!
//! Cada linha sai sem a versão do formato (`v`) e sem a sessão. O número
//! (`id`) sai quando a linha tem código, que é o jeito de citar o item. Fora
//! da conversa, saem a hora e o autor; na conversa, o autor fica, e a hora
//! fica até o minuto, porque diz quem falou e quando. As ligações entre itens
//! (`origin`, `result`, `reused_results`, `replaces`, `closes`, `reply_to`,
//! `covered_by`) saem pelo código do item apontado; o número sem código fica
//! como está.
//!
//! Cada fato sai sem as marcas de conferência: as impressões da fonte, a
//! classificação, a verificação e a situação da fonte. O mesmo fato (a mesma
//! fonte e o mesmo texto) sai uma vez na resposta: onde ele volta, sai
//! `{"same_as": "<código>", "fact": <posição>}`, que aponta o item em que ele
//! saiu inteiro e a posição dele ali, contada a partir de 1.
//!
//! Numa spec de campo, o mesmo ponto trazia os mesmos cinco fatos de outro,
//! cada um com duas impressões de 64 letras, e a leitura do bloco passava de
//! 40 mil letras. A leitura do item pelo código (`item-<código>`) continua com
//! o evento inteiro.

use std::collections::BTreeMap;

use mustard_core::domain::spec_events::{Block, shown_line, type_spec};
use serde_json::{Map, Value, json};

/// As marcas que nenhuma linha enxuta mostra.
const STAMPS: &[&str] = &["v", "session"];

/// As ligações entre itens, que a linha enxuta mostra pelo código.
const LINKS: &[&str] = &["origin", "result", "reused_results", "replaces", "closes", "reply_to", "covered_by"];

/// As marcas de conferência de um fato: as impressões da fonte e da medida,
/// a classificação, a verificação e a situação da fonte.
const FACT_MARKS: &[&str] =
    &["source_version", "current_source_version", "classification", "verification", "source_status", "input_sha256", "output_sha256"];

/// Quantas letras da hora ficam na conversa: `2026-10-22T14:38`.
const MINUTE: usize = 16;

/// A leitura enxuta de uma resposta: lembra cada fato já mostrado, para que
/// a repetição dele aponte para onde ele saiu.
pub(super) struct Lean<'a> {
    codes: &'a BTreeMap<u64, String>,
    facts: BTreeMap<(String, String), (String, usize)>,
}

impl<'a> Lean<'a> {
    /// A leitura enxuta com os códigos da spec, para as ligações.
    pub(super) fn new(codes: &'a BTreeMap<u64, String>) -> Self {
        Self { codes, facts: BTreeMap::new() }
    }

    /// A linha enxuta de `line`, na ordem da resposta. A linha que não é um
    /// objeto JSON sai como veio.
    pub(super) fn line(&mut self, line: &str) -> String {
        let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(line) else {
            return line.to_string();
        };
        self.fields(&mut fields);
        shown_line(&fields)
    }

    fn fields(&mut self, fields: &mut Map<String, Value>) {
        let own = fields.get("code").and_then(Value::as_str).map(str::to_string).or_else(|| fields.get("id").map(Value::to_string));
        for stamp in STAMPS {
            fields.remove(*stamp);
        }
        if fields.contains_key("code") {
            fields.remove("id");
        }
        let conversation = fields.get("type").and_then(Value::as_str).and_then(type_spec).is_some_and(|spec| spec.block == Block::Conversation);
        if conversation {
            if let Some(Value::String(at)) = fields.get_mut("at") {
                *at = to_the_minute(at);
            }
        } else {
            fields.remove("at");
            fields.remove("author");
        }
        for link in LINKS {
            if let Some(value) = fields.get_mut(*link) {
                *value = self.by_code(value);
            }
        }
        if let Some(Value::Array(facts)) = fields.get_mut("facts") {
            self.facts(facts, own.as_deref().unwrap_or_default());
        }
    }

    /// Os fatos de um item cujo código é `own`, sem as marcas de conferência;
    /// o fato que a resposta já mostrou vira o apontador para ele.
    fn facts(&mut self, facts: &mut [Value], own: &str) {
        for (at, fact) in facts.iter_mut().enumerate() {
            let Value::Object(marks) = fact else { continue };
            for mark in FACT_MARKS {
                marks.remove(*mark);
            }
            let key = (marks.get("source").map(Value::to_string).unwrap_or_default(), marks.get("text").map(Value::to_string).unwrap_or_default());
            match self.facts.get(&key) {
                Some((first, position)) => *fact = json!({"same_as": first, "fact": position}),
                None => {
                    self.facts.insert(key, (own.to_string(), at + 1));
                }
            }
        }
    }

    /// O número apontado, ou a lista deles, pelo código do item; o número
    /// sem código e o que já é texto ficam como estão.
    fn by_code(&self, value: &Value) -> Value {
        match value {
            Value::Array(list) => Value::Array(list.iter().map(|one| self.by_code(one)).collect()),
            one => one.as_u64().and_then(|id| self.codes.get(&id)).map_or_else(|| one.clone(), |code| json!(code)),
        }
    }
}

/// A hora até o minuto, quando ela tem a forma `AAAA-MM-DDTHH:MM…`; a que
/// não tem fica como está.
fn to_the_minute(at: &str) -> String {
    let bytes = at.as_bytes();
    let shaped = bytes.len() >= MINUTE && bytes[10] == b'T' && bytes[13] == b':' && at.is_char_boundary(MINUTE);
    if shaped { at[..MINUTE].to_string() } else { at.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_without_the_date_and_hour_shape_stays_as_it_is() {
        for at in ["ontem", "", "2026-10-22 14:38:48", "2026-10-22T14:3ç:48"] {
            assert_eq!(to_the_minute(at), at);
        }
    }
}
