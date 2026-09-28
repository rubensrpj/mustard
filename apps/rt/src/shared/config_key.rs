//! `config_key` — a chave do Jev no arquivo de configuração do Mustard, que
//! nenhuma leitura mostra.
//!
//! A chave mora no `mustard.json` do projeto. Quem lê o arquivo, pela
//! ferramenta de leitura ou pelo terminal, recebe o texto dele com o valor de
//! cada chave trocado por `***` ([`masked`]), e nunca o valor.

use std::path::Path;

use mustard_core::platform::i18n::{translate, Locale};

/// O nome do arquivo de configuração do Mustard.
pub(crate) const CONFIG_FILE: &str = "mustard.json";

/// O que fica no lugar do valor de cada chave.
const HIDDEN: &str = "***";

/// `true` quando o caminho termina no arquivo de configuração do Mustard.
pub(crate) fn is_config_file(path: &str) -> bool {
    Path::new(&path.replace('\\', "/")).file_name().and_then(|name| name.to_str()) == Some(CONFIG_FILE)
}

/// `true` quando o nome de um campo guarda uma chave: `key`, ou terminado em
/// `Key`, `_key` ou `-key`.
fn is_key_name(name: &str) -> bool {
    name == "key" || name.ends_with("Key") || name.ends_with("_key") || name.ends_with("-key")
}

/// Um pedaço do texto, para achar o campo de chave: um texto entre aspas,
/// com as posições do que fica dentro delas, os dois-pontos, ou outra coisa.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    Text(usize, usize),
    Colon,
    Other,
}

/// `text` com o valor de cada campo de chave trocado por `***`, quando há
/// algum com valor; `None` quando não há. Lê só os textos entre aspas e os
/// dois-pontos, e por isso acha a chave também num arquivo que não se lê como
/// JSON.
pub(crate) fn masked(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut hidden: Vec<(usize, usize)> = Vec::new();
    let mut last: [Token; 2] = [Token::Other, Token::Other];
    let mut at = 0;
    while at < bytes.len() {
        let token = match bytes[at] {
            b'"' => {
                let start = at + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(bytes.len());
                at = end + 1;
                Token::Text(start, end)
            }
            b':' => {
                at += 1;
                Token::Colon
            }
            byte if byte.is_ascii_whitespace() => {
                at += 1;
                continue;
            }
            _ => {
                at += 1;
                Token::Other
            }
        };
        if let (Token::Text(name_start, name_end), Token::Colon, Token::Text(start, end)) = (last[0], last[1], token)
            && end > start
            && is_key_name(&text[name_start..name_end])
        {
            hidden.push((start, end));
        }
        last = [last[1], token];
    }
    if hidden.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for (start, end) in hidden {
        out.push_str(&text[from..start]);
        out.push_str(HIDDEN);
        from = end;
    }
    out.push_str(&text[from..]);
    Some(out)
}

/// A recusa da leitura do arquivo de configuração em `path` (como quem lê o
/// nomeou), quando ele guarda uma chave: o texto dele com a chave escondida.
/// `None` quando o arquivo não se lê ou não guarda chave.
pub(crate) fn refusal(path: &str, on_disk: &Path, lang: Locale) -> Option<String> {
    let shown = masked(&std::fs::read_to_string(on_disk).ok()?)?;
    Some(translate("config_key.hidden", lang).replace("{file}", path).replace("{text}", &shown))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma chave inventada para os testes; nenhuma chave de verdade entra
    /// aqui.
    const FAKE: &str = "chave-falsa-0123456789";

    /// O valor de cada campo de chave sai trocado, em qualquer altura do
    /// arquivo e em qualquer dos nomes de chave; o resto do texto fica como
    /// estava.
    #[test]
    fn every_key_value_is_hidden_and_the_rest_stays() {
        let text = format!(
            "{{\n  \"language\": {{\"text\": \"pt-BR\"}},\n  \"jev\": {{ \"key\" : \"{FAKE}\" }},\n  \"apiKey\": \"{FAKE}\",\n  \"x_key\": \"a\\\"b\"\n}}\n"
        );
        let shown = masked(&text).expect("the file holds a key");
        assert!(!shown.contains(FAKE), "{shown}");
        assert!(!shown.contains("a\\\"b"), "the escaped quote stays inside the value: {shown}");
        assert_eq!(shown.matches(HIDDEN).count(), 3, "{shown}");
        assert!(shown.contains("\"language\": {\"text\": \"pt-BR\"}"), "{shown}");
        assert!(shown.contains("\"jev\": { \"key\" : \"***\" }"), "{shown}");
    }

    /// Sem campo de chave, ou com a chave vazia, nada se esconde; um nome que
    /// só contém a palavra no meio não é chave. O arquivo que não fecha como
    /// JSON ainda tem a chave escondida.
    #[test]
    fn only_a_key_field_with_a_value_counts() {
        assert_eq!(masked(r#"{"git": {"flow": {"monkey": "dev"}}, "keys": "x"}"#), None);
        assert_eq!(masked(r#"{"jev": {"key": ""}}"#), None);
        assert_eq!(masked(r#"{"key": 12}"#), None, "only a text value is a key");
        let broken = format!("{{\"jev\": {{\"key\": \"{FAKE}\"");
        assert!(!masked(&broken).expect("the key is found").contains(FAKE));
        let open = format!("{{\"key\": \"{FAKE}");
        assert!(!masked(&open).expect("the key is found").contains(FAKE), "a value left open hides to the end");
    }

    /// O arquivo de configuração é o `mustard.json`, em qualquer pasta.
    #[test]
    fn the_config_file_is_named_mustard_json() {
        assert!(is_config_file("mustard.json"));
        assert!(is_config_file("/p/proj/mustard.json"));
        assert!(is_config_file("C:\\p\\mustard.json"));
        assert!(!is_config_file("mustard.json.bak"));
        assert!(!is_config_file("docs/mustard.md"));
    }
}
