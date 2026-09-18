//! A linha do arquivo: o evento carimbado pelo binário e escrito com o
//! envelope na ordem fixa, com os mesmos bytes para a mesma entrada.

use serde_json::{Map, Value};

use super::search::search_of;
use super::FORMAT_VERSION;

/// O envelope, na ordem em que abre cada linha do arquivo. O expurgo guarda o
/// envelope, então o código do item continua no arquivo depois dele.
const LEAD_FIELDS: &[&str] = &["v", "id", "code", "at", "type", "author"];

/// O evento pronto para o arquivo: a versão do formato, o número, o código do
/// item (veja [`code_after`](super::code_after)), a hora e o campo de busca, calculado de `text`
/// e `keys`.
#[must_use]
pub fn stamp(mut event: Map<String, Value>, id: u64, code: Option<&str>, at: &str) -> Map<String, Value> {
    event.insert("v".into(), Value::from(FORMAT_VERSION));
    event.insert("id".into(), Value::from(id));
    if let Some(code) = code {
        event.insert("code".into(), Value::String(code.to_string()));
    }
    event.insert("at".into(), Value::String(at.to_string()));
    if let Some(search) = search_of(&event) {
        event.insert("search".into(), Value::String(search));
    }
    event
}

/// A linha do arquivo: o envelope na ordem fixa, os outros campos em ordem
/// alfabética e o `search` por último. A ordem não depende de como o JSON foi
/// lido, então a mesma entrada dá sempre os mesmos bytes.
#[must_use]
pub fn render_line(event: &Map<String, Value>) -> String {
    render(event, true)
}

/// A linha como a leitura mostra: igual à do arquivo, sem o `search`, que
/// nunca é mostrado.
#[must_use]
pub fn shown_line(event: &Map<String, Value>) -> String {
    render(event, false)
}

fn render(event: &Map<String, Value>, with_search: bool) -> String {
    let mut keys: Vec<&str> = LEAD_FIELDS.iter().copied().filter(|k| event.contains_key(*k)).collect();
    let mut rest: Vec<&str> = event
        .keys()
        .map(String::as_str)
        .filter(|k| !LEAD_FIELDS.contains(k) && *k != "search")
        .collect();
    rest.sort_unstable();
    keys.extend(rest);
    if with_search && event.contains_key("search") {
        keys.push("search");
    }
    let mut out = String::from("{");
    for (i, key) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_str(&mut out, key);
        out.push(':');
        write_value(&mut out, &event[*key]);
    }
    out.push('}');
    out
}

fn write_str(out: &mut String, s: &str) {
    out.push_str(&Value::String(s.to_string()).to_string());
}

/// JSON compacto com as chaves de cada objeto em ordem alfabética.
fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(out, key);
                out.push(':');
                write_value(out, &map[key.as_str()]);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::spec_events::normalize;
    use crate::domain::spec_events::tests::obj;

    #[test]
    fn a_line_starts_with_the_envelope_and_ends_with_search() {
        let event = stamp(normalize(obj(json!({"text": "x", "keys": ["k"], "origin": 1})), "note"), 7, None, "t");
        let line = render_line(&event);
        assert!(line.starts_with(r#"{"v":1,"id":7,"at":"t","type":"note","author":"assistant","keys":"#), "{line}");
        assert!(line.ends_with(r#""search":"x k anot not"}"#), "{line}");
        assert!(!shown_line(&event).contains("search"));
    }
}
