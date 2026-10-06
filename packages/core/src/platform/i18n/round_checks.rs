//! A recusa da rodada quando a compilação, o lint ou a suíte que o projeto
//! declara cai antes do commit: o próximo passo de quem conduz, as ondas a
//! quem o conserto volta e o trecho que chega ao agente de cada uma.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["round_checks"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // O próximo passo, depois do que caiu e do fim da saída: a volta já
        // ficou fora do commit, e o conserto vai ao agente que fez a onda.
        ("round_checks.next", Locale::PtBr) => {
            "Mande cada onda abaixo de volta ao agente dela, na mesma cópia, com uma mensagem curta. \
             A rodada gravou o trecho de conserto de cada uma, com o comando e o fim da saída. O \
             Mustard troca a mensagem por ele. O agente conserta, grava a entrega de novo, e você \
             roda a rodada outra vez. Não reprove a volta com a linha `<REJECTED>`: ela já ficou \
             fora do commit, e a reprovação a daria a um agente novo."
        }
        ("round_checks.next", Locale::EnUs) => {
            "Send each wave below back to its agent, in the same copy, with a short message. The \
             round recorded each one's fix section, with the command and the end of the output. \
             Mustard swaps the message for it. The agent fixes it, records the delivery again, and \
             you run the round once more. Do not reject the return with the `<REJECTED>` line: it is \
             already out of the commit, and the rejection would hand it to a new agent."
        }
        // Uma linha por onda a quem o conserto volta: a que a saída cita, ou,
        // sem nenhuma citada, cada uma que entrou na rodada.
        ("round_checks.cited", Locale::PtBr) => "Onda {wave}: a saída cita arquivo que ela mudou.",
        ("round_checks.cited", Locale::EnUs) => "Wave {wave}: the output cites a file it changed.",
        ("round_checks.joined", Locale::PtBr) => {
            "Onda {wave}: a saída não cita arquivo de onda nenhuma, e ela entrou na rodada."
        }
        ("round_checks.joined", Locale::EnUs) => "Wave {wave}: the output cites no wave's file, and it was in the round.",
        // O trecho de conserto que chega ao agente da onda, no lugar da
        // mensagem de quem conduz.
        ("round_checks.fix", Locale::PtBr) => {
            "A rodada juntou a entrega da onda {wave} no repositório principal e rodou `{command}`, \
             que caiu. Nada foi comitado. Ache na saída o que vem desta onda, conserte na cópia, rode \
             os testes do que mudou e grave a entrega de novo. Se nada vem dela, diga isso no texto \
             da entrega. O fim da saída:\n{output}"
        }
        ("round_checks.fix", Locale::EnUs) => {
            "The round joined wave {wave}'s delivery into the main repository and ran `{command}`, \
             which failed. Nothing was committed. Find in the output what comes from this wave, fix \
             it in the copy, run the tests of what changed, and record the delivery again. If \
             nothing comes from it, say so in the delivery text. The end of the output:\n{output}"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::platform::i18n::{translate, Locale};

    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("round_checks.rs"),
            super::PREFIXES,
            4,
            0xb4d6_3e53_3b60_4505,
        );
    }

    /// Cada texto sai nos dois idiomas com as vagas que a rodada preenche.
    #[test]
    fn each_text_carries_the_slots_the_round_fills() {
        for (key, slots) in [
            ("round_checks.next", &[][..]),
            ("round_checks.cited", &["{wave}"][..]),
            ("round_checks.joined", &["{wave}"][..]),
            ("round_checks.fix", &["{wave}", "{command}", "{output}"][..]),
        ] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                let text = translate(key, lang);
                assert_ne!(text, "<missing-key>", "{key} in {lang}");
                for slot in slots {
                    assert!(text.contains(slot), "{key} in {lang} lacks {slot}");
                }
            }
        }
    }
}
