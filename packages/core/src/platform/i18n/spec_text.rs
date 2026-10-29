//! Os textos curtos da onda: o rótulo ("Onda 3", "W3").
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["wave"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Rótulo curto da onda ("W3" ou "Onda 3"). Quem chama junta o número:
        // `format!("{} {n}", translate(...))`.
        ("wave.label", Locale::PtBr) => "Onda",
        ("wave.label", Locale::EnUs) => "W",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    /// Cada chave desta parte começa por um dos começos que ela declara e tem
    /// texto nos dois idiomas.
    #[test]
    fn the_part_answers_each_of_its_keys() {
        crate::platform::i18n::tests::assert_part_answers(include_str!("spec_text.rs"), super::PREFIXES);
    }
}
