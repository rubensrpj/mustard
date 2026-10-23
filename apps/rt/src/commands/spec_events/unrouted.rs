//! As mensagens do usuário sem destino como o levantamento as mostra: uma
//! linha por mensagem, com o código e o começo do texto. O texto inteiro e o
//! resto do evento ficam no arquivo da spec e saem por `run read
//! item-<código>`; a lista só diz quais mensagens são, para o usuário
//! reconhecê-las.

use std::collections::BTreeMap;

use mustard_core::domain::spec_events::SpecEvent;
use mustard_core::domain::text::{CutAt, cut, one_line};

/// Quantos caracteres do texto de uma mensagem entram na linha dela, antes
/// das reticências.
const START_CHARS: usize = 100;

/// A linha de cada mensagem de `messages`, na ordem recebida.
pub(crate) fn lines<'a>(messages: impl IntoIterator<Item = &'a SpecEvent>, codes: &BTreeMap<u64, String>) -> Vec<String> {
    messages.into_iter().map(|message| line(message, codes)).collect()
}

/// O código da mensagem, dois pontos e o começo do texto dela numa linha só:
/// as quebras e os espaços repetidos viram um espaço, e o texto que não cabe
/// em [`START_CHARS`] caracteres e as reticências fica com o começo e termina
/// nelas.
fn line(message: &SpecEvent, codes: &BTreeMap<u64, String>) -> String {
    let code = codes.get(&message.id).cloned().unwrap_or_else(|| message.id.to_string());
    let text = one_line(message.str_field("text").unwrap_or_default());
    format!("{code}: {}", cut(&text, START_CHARS + 1, CutAt::Char))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn message(id: u64, text: &str) -> SpecEvent {
        let fields = json!({"type": "message", "author": "user", "text": text}).as_object().cloned().unwrap_or_default();
        SpecEvent { id, event_type: "message".to_string(), line: 1, fields }
    }

    /// A linha leva o código, junta as quebras e os espaços repetidos e corta
    /// o texto longo no começo; o texto curto sai inteiro, sem reticências.
    #[test]
    fn a_line_has_the_code_and_the_start_of_the_text_in_one_line() {
        let codes = BTreeMap::from([(4, "MSTD-MSG-0002".to_string())]);
        let short = message(4, "E o  painel?\n\nE o aviso?");
        assert_eq!(line(&short, &codes), "MSTD-MSG-0002: E o painel? E o aviso?");

        let pasted = message(4, &"abcdefghi ".repeat(40));
        let cut = line(&pasted, &codes);
        assert_eq!(cut, format!("MSTD-MSG-0002: {}abcdefghi…", "abcdefghi ".repeat(9)));

        let uncoded = message(9, "sem código");
        assert_eq!(line(&uncoded, &codes), "9: sem código");
    }

    /// O texto de 101 caracteres cabe inteiro na linha, no lugar das
    /// reticências; o de 102 fica com os 100 primeiros e termina nelas.
    #[test]
    fn a_line_keeps_a_hundred_characters_before_the_ellipsis() {
        let codes = BTreeMap::new();
        let whole = "x".repeat(101);
        assert_eq!(line(&message(1, &whole), &codes), format!("1: {whole}"));
        assert_eq!(line(&message(1, &"x".repeat(102)), &codes), format!("1: {}…", "x".repeat(100)));
    }
}
