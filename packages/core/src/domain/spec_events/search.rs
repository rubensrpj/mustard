//! A busca: o campo `search` de cada linha, calculado só pelo binário, e os
//! eventos que um termo acha.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::{Map, Value};

use crate::domain::normalize::{plain_words, Languages, Normalizer};
use crate::domain::search::SearchIndex;
use crate::platform::i18n::{translate, Locale};

use super::read::{parse_object, wave_of};
use super::{render_line, type_spec, SpecEvent};

/// As palavras de `pieces`, como [`plain_words`] as dá, sem repetição entre
/// um pedaço e outro, na ordem em que aparecem.
fn words<'a>(pieces: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for piece in pieces {
        for word in plain_words(piece) {
            if seen.insert(word.clone()) {
                out.push(word);
            }
        }
    }
    out
}

/// O campo `search`: as palavras de `text` e de `keys`, separadas por espaço,
/// antes de qualquer língua ([`plain_words`]). As formas de cada palavra saem
/// na hora da busca, nas línguas do projeto; por isso o campo gravado não
/// muda quando o projeto muda de língua. O campo gravado por uma versão
/// anterior, com as raízes do português, continua valendo: cada raiz dele é
/// lida como uma palavra. Calculado só pelo binário e nunca mostrado.
#[must_use]
pub fn search_field(text: Option<&str>, keys: &[&str]) -> String {
    words(text.into_iter().chain(keys.iter().copied())).join(" ")
}

/// Os eventos que um termo acha, do mais forte para o menos forte, nas
/// línguas `languages`.
///
/// O termo que é o código de um item devolve exatamente esse item: é assim que
/// a conversa e a página citam um item, e um código nunca é uma busca por
/// assunto. O termo que é o nome do comando de uma chamada, como `map
/// search`, devolve as chamadas dele, na ordem do arquivo: a chamada não tem
/// texto, e a busca por nota nunca a acharia. Qualquer outro termo passa pela
/// busca por nota que o projeto já
/// usa nas lições e no recorte dos itens por onda, sobre o campo de busca de
/// cada evento: quem casa mais forte vem primeiro, e não é preciso ter todas
/// as palavras do termo. O termo vazio devolve tudo, na ordem do arquivo.
#[must_use]
pub fn found_by<'a>(
    events: Vec<&'a SpecEvent>,
    term: &str,
    codes: &BTreeMap<u64, String>,
    languages: &Languages,
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
    if events.iter().any(|e| calls_command(e, term)) {
        return events.into_iter().filter(|e| calls_command(e, term)).collect();
    }
    let mut normalizer = Normalizer::new(languages);
    let docs: Vec<(u64, Vec<Vec<String>>)> =
        events.iter().map(|e| (e.id, normalizer.forms(e.str_field("search").unwrap_or_default()))).collect();
    let ranked = SearchIndex::build(docs).top(&normalizer.query(term), events.len());
    let by_id: BTreeMap<u64, &SpecEvent> = events.iter().map(|e| (e.id, *e)).collect();
    ranked.into_iter().filter_map(|hit| by_id.get(&hit.id).copied()).collect()
}

/// Se `event` é uma chamada do comando `command`. O nome casa sem olhar
/// maiúsculas nem os espaços de sobra: `Map  search` é o comando `map search`.
#[must_use]
pub fn calls_command(event: &SpecEvent, command: &str) -> bool {
    let words = |name: &str| name.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
    event.event_type == "call" && event.str_field("command").is_some_and(|name| words(name) == words(command))
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

/// O `search` que uma linha deve ter: as palavras do texto, do título, da
/// parte do agente e das palavras-chave, mais o rótulo do item, o nome do
/// tipo em palavras nos dois idiomas, a onda a que a linha pertence e os
/// caminhos dos arquivos que ela cita. Com isso, procurar pelo nome de um
/// arquivo acha as tarefas que mexem nele, procurar pelo título acha o item,
/// e procurar por "onda 13" acha o que é dela. `None` para a linha que não
/// tem nada disso, que fica sem o campo.
pub(super) fn search_of(event: &Map<String, Value>) -> Option<String> {
    let text = event.get("text").and_then(Value::as_str);
    // O título e a parte do agente entram por último: a linha antiga, que não
    // os tem, fica com o mesmo campo de antes.
    let parts: Vec<&str> = ["title", "agent"].iter().filter_map(|f| event.get(*f).and_then(Value::as_str)).collect();
    let mut extra: Vec<String> = Vec::new();
    if let Some(keys) = event.get("keys").and_then(Value::as_array) {
        extra.extend(keys.iter().filter_map(Value::as_str).map(str::to_string));
    }
    // A linha sem texto, sem título, sem parte do agente e sem palavras-chave
    // — a expurgada, entre outras — fica sem o campo; o resto só enriquece
    // quem já tem o que procurar.
    if text.is_none() && extra.is_empty() && parts.is_empty() {
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
    extra.extend(parts.into_iter().map(str::to_string));
    let keys: Vec<&str> = extra.iter().map(String::as_str).collect();
    Some(search_field(text, &keys))
}

/// O arquivo com o `search` posto nas linhas que não o têm, e quantas linhas
/// o ganharam. O `search` já gravado nunca é reescrito, nem quando foi
/// calculado por outra regra: o arquivo de eventos só cresce, e a busca lê o
/// campo antigo como está. Só ganha o campo a linha que tem texto ou chaves e
/// ainda não o tem; as outras, inclusive as que não se entendem, ficam byte a
/// byte como estavam. É o que o índice das specs roda, e a instalação roda
/// uma vez em cada projeto.
#[must_use]
pub fn refresh_search_lines(content: &str) -> (String, usize) {
    let mut changed = 0usize;
    let body = content
        .split('\n')
        .map(|raw| {
            let line = raw.trim_end_matches('\r');
            let Some(mut map) = parse_object(line) else { return raw.to_string() };
            if map.contains_key("search") {
                return raw.to_string();
            }
            let Some(search) = search_of(&map) else { return raw.to_string() };
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

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    /// `true` quando o termo acha o evento, nas línguas `languages`.
    fn finds_in(event: &SpecEvent, term: &str, languages: &Languages) -> bool {
        !found_by(vec![event], term, &BTreeMap::new(), languages).is_empty()
    }

    fn finds(event: &SpecEvent, term: &str) -> bool {
        finds_in(event, term, &languages())
    }

    fn note(text: &str, keys: &[&str]) -> SpecEvent {
        let fields = stamp(normalize(obj(json!({"text": text, "keys": keys, "origin": 1})), "note"), 5, None, "t");
        SpecEvent { id: 5, event_type: "note".into(), line: 1, fields }
    }

    /// A palavra e as flexões dela acham o mesmo item, com e sem acento.
    #[test]
    fn a_word_and_its_inflections_find_the_same_item() {
        let event = note("Apagar a pasta. Conciliação da ação.", &["Pagamento"]);
        for term in ["apagar", "apagando", "apagou", "conciliacao", "conciliação", "pagamentos"] {
            assert!(finds(&event, term), "{term}: {:?}", event.str_field("search"));
        }
        assert!(!finds(&event, "imposto"), "a word the item lacks does not find it");
    }

    /// A linha gravada por uma versão anterior, com as raízes do português
    /// no campo de busca, continua sendo achada: o campo antigo não é
    /// reescrito, e cada raiz dele é lida como uma palavra.
    #[test]
    fn an_item_written_before_keeps_being_found_by_its_old_search_field() {
        let mut fields = obj(json!({"v": 1, "id": 9, "type": "note", "text": "Apagar a pasta velha."}));
        fields.insert("search".into(), json!("apag a past velh"));
        let event = SpecEvent { id: 9, event_type: "note".into(), line: 1, fields };
        for term in ["apagando", "pastas", "velhas"] {
            assert!(finds(&event, term), "{term}");
        }
        assert!(!finds(&event, "imposto"));
    }

    /// Além do texto e das palavras-chave, o campo de busca leva o rótulo, o
    /// nome do tipo em palavras nos dois idiomas, a onda a que o item pertence
    /// e os caminhos dos arquivos que ele cita: procurar pelo nome de um
    /// arquivo acha a tarefa que mexe nele, e procurar pelo número da onda
    /// acha o que é dela.
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
        for term in ["round.rs", "commands/flow", "onda 13", "wave 13", "13", "tarefa", "task", "injetou"] {
            assert!(finds(&event, term), "{term}: {:?}", event.str_field("search"));
        }
        assert!(!finds(&event, "12"), "another wave does not match");
    }

    /// O título e a parte do agente entram no campo de busca: a palavra que só
    /// o título tem, ou só a parte do agente, acha o item. Os dois vêm por
    /// último, então o campo da linha antiga, sem eles, é o começo do campo da
    /// linha nova, e a linha antiga não muda.
    #[test]
    fn the_search_finds_an_item_by_its_title_and_by_its_agent_part() {
        let old = obj(json!({"text": "A fatura soma centavos.", "keys": ["soma"], "origin": 1}));
        let mut new = old.clone();
        new.insert("title".into(), json!("Arredondar a fatura"));
        new.insert("agent".into(), json!("- conferir billing na linha 40"));
        let fields = stamp(normalize(new.clone(), "decision"), 3, None, "t");
        let event = SpecEvent { id: 3, event_type: "decision".into(), line: 1, fields };
        for term in ["arredondar", "billing"] {
            assert!(finds(&event, term), "{term}: {:?}", event.str_field("search"));
        }
        assert!(!finds(&event, "imposto"), "a word the item lacks does not match");

        let before = search_of(&old).expect("the old line has a search field");
        let after = search_of(&new).expect("the new line has a search field");
        assert!(after.starts_with(&format!("{before} ")), "{before} / {after}");
    }

    /// Com o texto do projeto em espanhol, a busca na spec corta as palavras
    /// como espanhol: "correr" acha o item que diz "corriendo", o que o corte
    /// do português e do inglês não acha.
    #[test]
    fn a_spanish_text_language_cuts_the_spec_search_as_spanish() {
        use crate::domain::config::{LanguageConfig, ProjectConfig};
        let config = ProjectConfig {
            language: LanguageConfig { text: Some("es-ES".into()), code: Some("en-US".into()) },
            ..ProjectConfig::default()
        };
        let spanish = Languages::of(&config);
        let event = note("El proceso sigue corriendo en segundo plano.", &[]);
        assert!(finds_in(&event, "correr", &spanish), "{:?}", event.str_field("search"));
        assert!(!finds(&event, "correr"), "the Portuguese and English cut does not join the two");
    }

    /// Pôr o `search` preenche só a linha em que ele faltava; a linha com o
    /// campo, mesmo o calculado por outra regra, a que não se entende e a que
    /// não tem texto ficam byte a byte.
    #[test]
    fn refreshing_the_search_fills_only_the_lines_without_it() {
        let right = render_line(&stamp(
            normalize(obj(json!({"text": "Apagar a pasta.", "keys": ["pasta"], "origin": 1})), "note"),
            1,
            None,
            "t",
        ));
        let old = r#"{"v":1,"id":2,"at":"t","type":"note","author":"assistant","keys":["k"],"text":"Trava nova.","search":"velho"}"#;
        let missing = r#"{"id":3,"type":"rule","text":"Sem busca gravada."}"#;
        let bare = r#"{"v":1,"id":4,"at":"t","type":"message","author":"user","purged":9}"#;
        let content = format!("{right}\n{old}\ngarbage\r\n{missing}\n{bare}\n");

        let (fixed, changed) = refresh_search_lines(&content);
        assert_eq!(changed, 1, "{fixed}");
        let lines: Vec<&str> = fixed.split('\n').collect();
        assert_eq!(lines[0], right);
        assert_eq!(lines[1], old, "a search written by another rule is never rewritten");
        assert_eq!(lines[2], "garbage\r", "a line that does not parse stays as it was");
        assert_eq!(lines[4], bare);
        assert_eq!(lines[5], "", "the file still ends with a newline");
        let log = parse_log(&fixed);
        assert_eq!(
            log.get(3).unwrap().str_field("search"),
            Some(search_field(Some("Sem busca gravada."), &["regra", "rule"]).as_str())
        );
        assert_eq!(refresh_search_lines(&fixed), (fixed.clone(), 0), "a second pass changes nothing");
    }
}
