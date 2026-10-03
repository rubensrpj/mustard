//! Os textos curtos da onda: o rótulo ("Onda 3", "W3") e o motivo gravado
//! quando a onda desenhada à mão que nunca saiu é tirada da spec.
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
        // O motivo gravado na remoção da onda desenhada à mão que nunca saiu,
        // quando a spec antiga passa para o backlog; a página o mostra na lista
        // do que saiu.
        ("wave.hand_drawn_removed", Locale::PtBr) => {
            "Desenhada à mão antes do backlog e nunca saiu; as tarefas dela voltaram para o backlog."
        }
        ("wave.hand_drawn_removed", Locale::EnUs) => {
            "Drawn by hand before the backlog and never sent; its tasks went back to the backlog."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("spec_text.rs"),
            super::PREFIXES,
            2,
            0x6e7c_a2d3_f78b_b4dd,
        );
    }
}
