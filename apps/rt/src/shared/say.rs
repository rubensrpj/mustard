//! `say` — um texto do catálogo com as vagas preenchidas.
//!
//! Mora à parte para que a trava de escrita, o caminho do código e a resposta
//! da busca por palavra usem o mesmo preenchimento sem se importarem uns aos
//! outros.

use mustard_core::platform::i18n::{translate, Locale};

/// Um texto do catálogo com as vagas preenchidas.
pub(crate) fn say(key: &str, lang: Locale, slots: &[(&str, &str)]) -> String {
    slots
        .iter()
        .fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slot_of_the_catalog_text_is_filled() {
        let text = say("scan.map.pointer", Locale::PtBr, &[]);
        assert!(!text.is_empty());
        let filled = say("map.search.not_found", Locale::PtBr, &[("{words}", "\"frete\""), ("{next}", "grep frete")]);
        assert!(filled.contains("\"frete\"") && filled.contains("grep frete"), "{filled}");
        assert!(!filled.contains("{words}") && !filled.contains("{next}"), "{filled}");
    }
}
