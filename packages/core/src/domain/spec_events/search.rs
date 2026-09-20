//! A busca: o campo `search` de cada linha, calculado só pelo binário, as
//! raízes de um termo e os eventos que um termo acha.

use std::collections::{BTreeMap, BTreeSet};

use rust_stemmers::{Algorithm, Stemmer};
use serde_json::{Map, Value};

use crate::domain::text;
use crate::platform::i18n::{translate, Locale};

use super::read::{parse_object, wave_of};
use super::{render_line, type_spec, SpecEvent};

/// As raízes das palavras de um texto: minúsculas, cada palavra reduzida à
/// raiz pelo redutor de português e, depois, sem acento. "apagar",
/// "apagando" e "apagou" dão a mesma raiz. A palavra funcional de português
/// ou de inglês pula o radical: fica só minúscula e sem acento, ela mesma,
/// porque o redutor é sempre o de português, e a raiz que ele dá para uma
/// palavra funcional inglesa pode coincidir com a de uma palavra de conteúdo
/// parecida em português — a de "some" não pode ficar igual à de "somar".
/// Sem repetição, na ordem em que aparecem.
pub(super) fn roots<'a>(pieces: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let stemmer = Stemmer::create(Algorithm::Portuguese);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for piece in pieces {
        let lower = piece.to_lowercase();
        for word in text::words(&lower) {
            let is_function_word =
                text::FUNCTION_WORDS_PT.contains(&word) || text::FUNCTION_WORDS_EN.contains(&word);
            let stemmed = if is_function_word { word.into() } else { stemmer.stem(word) };
            let root = text::fold_accents(&stemmed);
            if seen.insert(root.clone()) {
                out.push(root);
            }
        }
    }
    out
}

/// O campo `search`: as raízes de `text` e de `keys`, separadas por espaço.
/// Calculado só pelo binário e nunca mostrado.
#[must_use]
pub fn search_field(text: Option<&str>, keys: &[&str]) -> String {
    roots(text.into_iter().chain(keys.iter().copied())).join(" ")
}

/// As raízes de um termo de busca, para comparar com o `search` de cada
/// evento.
#[must_use]
pub fn search_terms(query: &str) -> Vec<String> {
    roots([query])
}

/// Os eventos que um termo acha, do mais forte para o menos forte.
///
/// O termo que é o código de um item devolve exatamente esse item: é assim que
/// a conversa e a página citam um item, e um código nunca é uma busca por
/// assunto. Qualquer outro termo passa pela busca por nota que o projeto já
/// usa nas lições e no recorte dos itens por onda, sobre o campo de busca de
/// cada evento: quem casa mais forte vem primeiro, e não é preciso ter todas
/// as palavras do termo. O termo vazio devolve tudo, na ordem do arquivo.
#[must_use]
pub fn found_by<'a>(
    events: Vec<&'a SpecEvent>,
    term: &str,
    codes: &BTreeMap<u64, String>,
) -> Vec<&'a SpecEvent> {
    let term = term.trim();
    if term.is_empty() {
        return events;
    }
    let by_code: BTreeSet<u64> = codes
        .iter()
        .filter(|(_, code)| code.eq_ignore_ascii_case(term))
        .map(|(id, _)| *id)
        .collect();
    if !by_code.is_empty() {
        return events.into_iter().filter(|e| by_code.contains(&e.id)).collect();
    }
    let docs = events.iter().map(|e| (e.id, e.str_field("search").unwrap_or_default()));
    let ranked = crate::domain::search::SearchIndex::build(docs)
        .top(&crate::domain::search::query_terms(term), events.len());
    let by_id: BTreeMap<u64, &SpecEvent> = events.iter().map(|e| (e.id, *e)).collect();
    ranked.into_iter().filter_map(|hit| by_id.get(&hit.id).copied()).collect()
}

/// Os caminhos dos arquivos que a linha cita: cada item de `files`, que vem
/// como objeto com o campo do caminho ou já como o caminho em texto.
fn cited_paths(event: &Map<String, Value>) -> Vec<&str> {
    event
        .get("files")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().or_else(|| item.get("path").and_then(Value::as_str)))
                .collect()
        })
        .unwrap_or_default()
}

/// O `search` que uma linha deve ter: as raízes do texto e das palavras-chave,
/// mais o rótulo do item, o nome do tipo em palavras nos dois idiomas, a onda
/// a que a linha pertence e os caminhos dos arquivos que ela cita. Com isso,
/// procurar pelo nome de um arquivo acha as tarefas que mexem nele, e procurar
/// por "onda 13" acha o que é dela. `None` para a linha que não tem nada
/// disso, que fica sem o campo.
pub(super) fn search_of(event: &Map<String, Value>) -> Option<String> {
    let text = event.get("text").and_then(Value::as_str);
    let mut extra: Vec<String> = Vec::new();
    if let Some(keys) = event.get("keys").and_then(Value::as_array) {
        extra.extend(keys.iter().filter_map(Value::as_str).map(str::to_string));
    }
    // A linha sem texto e sem palavras-chave — a expurgada, entre outras —
    // fica sem o campo; o resto só enriquece quem já tem o que procurar.
    if text.is_none() && extra.is_empty() {
        return None;
    }
    if let Some(label) = event.get("label").and_then(Value::as_str) {
        extra.push(label.to_string());
    }
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or_default();
    if type_spec(event_type).is_some() {
        let key = format!("page.type.{event_type}");
        extra.push(translate(&key, Locale::PtBr).to_string());
        extra.push(translate(&key, Locale::EnUs).to_string());
    }
    if let Some(n) = wave_of(event_type, |f| event.get(f).and_then(Value::as_u64)) {
        extra.push(format!(
            "{} {n} {} {n}",
            translate("page.type.wave", Locale::PtBr),
            translate("page.type.wave", Locale::EnUs)
        ));
    }
    extra.extend(cited_paths(event).into_iter().map(str::to_string));
    let keys: Vec<&str> = extra.iter().map(String::as_str).collect();
    Some(search_field(text, &keys))
}

/// O arquivo com o `search` de cada linha recalculado pelo redutor de hoje,
/// e quantas linhas mudaram. Só é reescrita a linha que tem texto ou chaves e
/// cujo `search` faltava ou era outro; as outras, inclusive as que não se
/// entendem, ficam byte a byte como estavam. É o que o índice das specs roda
/// quando o redutor muda.
#[must_use]
pub fn refresh_search_lines(content: &str) -> (String, usize) {
    let mut changed = 0usize;
    let body = content
        .split('\n')
        .map(|raw| {
            let line = raw.trim_end_matches('\r');
            let Some(mut map) = parse_object(line) else { return raw.to_string() };
            let Some(search) = search_of(&map) else { return raw.to_string() };
            if map.get("search").and_then(Value::as_str) == Some(search.as_str()) {
                return raw.to_string();
            }
            map.insert("search".into(), Value::String(search));
            changed += 1;
            render_line(&map)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (body, changed)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::spec_events::tests::obj;
    use crate::domain::spec_events::{normalize, parse_log, stamp};

    #[test]
    fn delete_and_its_inflections_share_one_search_root() {
        let a = search_terms("apagar");
        assert_eq!(a, search_terms("apagando"));
        assert_eq!(a, search_terms("apagou"));
        let field = search_field(Some("Conciliação da ação"), &["Pagamento"]);
        assert!(!field.contains('ç') && !field.contains('ã'), "{field}");
        assert!(field.split(' ').any(|w| w == search_terms("pagamentos")[0]), "{field}");
    }

    /// Além do texto e das palavras-chave, o campo de busca leva o rótulo, o
    /// nome do tipo em palavras nos dois idiomas, a onda a que o item pertence
    /// e os caminhos dos arquivos que ele cita: procurar pelo nome de um
    /// arquivo acha a tarefa que mexe nele, e procurar por "onda 13" acha o
    /// que é dela.
    #[test]
    fn the_search_of_an_item_carries_its_label_type_wave_and_files() {
        let task = stamp(
            normalize(
                obj(json!({
                    "text": "A rodada grava o que injetou.",
                    "keys": ["envio"],
                    "label": "Onda 13, tarefa 7",
                    "wave": 13,
                    "files": [{"path": "apps/rt/src/commands/flow/round.rs", "new": true}],
                    "origin": 1
                })),
                "task",
            ),
            7,
            None,
            "t",
        );
        let event = SpecEvent { id: 7, event_type: "task".into(), line: 1, fields: task };
        for term in ["round.rs", "commands/flow", "onda 13", "wave 13", "tarefa", "task", "injetou"] {
            assert!(event.matches(&search_terms(term), None), "{term}: {:?}", event.str_field("search"));
        }
        assert!(!event.matches(&search_terms("onda 12"), None), "another wave does not match");
    }

    /// Recalcular o `search` reescreve só a linha em que ele faltava ou era
    /// outro; a linha certa, a que não se entende e a que não tem texto ficam
    /// byte a byte.
    #[test]
    fn refreshing_the_search_rewrites_only_the_stale_lines() {
        let right = render_line(&stamp(
            normalize(obj(json!({"text": "Apagar a pasta.", "keys": ["pasta"], "origin": 1})), "note"),
            1,
            None,
            "t",
        ));
        let stale = r#"{"v":1,"id":2,"at":"t","type":"note","author":"assistant","keys":["k"],"text":"Trava nova.","search":"velho"}"#;
        let older = r#"{"id":3,"type":"rule","text":"Sem busca gravada."}"#;
        let bare = r#"{"v":1,"id":4,"at":"t","type":"message","author":"user","purged":9}"#;
        let content = format!("{right}\n{stale}\ngarbage\r\n{older}\n{bare}\n");

        let (fixed, changed) = refresh_search_lines(&content);
        assert_eq!(changed, 2, "{fixed}");
        let lines: Vec<&str> = fixed.split('\n').collect();
        assert_eq!(lines[0], right);
        assert_eq!(lines[2], "garbage\r", "a line that does not parse stays as it was");
        assert_eq!(lines[4], bare);
        assert_eq!(lines[5], "", "the file still ends with a newline");
        let log = parse_log(&fixed);
        assert_eq!(
            log.get(2).unwrap().str_field("search"),
            Some(search_field(Some("Trava nova."), &["k", "anotação", "note"]).as_str())
        );
        assert_eq!(
            log.get(3).unwrap().str_field("search"),
            Some(search_field(Some("Sem busca gravada."), &["regra", "rule"]).as_str())
        );
        assert_eq!(refresh_search_lines(&fixed), (fixed.clone(), 0), "a second pass changes nothing");
    }
}
