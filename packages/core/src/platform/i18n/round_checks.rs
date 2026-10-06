//! A recusa da rodada quando a compilação, o lint, a suíte que o projeto
//! declara ou a verificação de um critério cai antes do commit: o próximo
//! passo de quem conduz, as ondas a quem o conserto volta e o trecho que
//! chega ao agente de cada uma.
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
        // A verificação de um critério que cai antes do commit: o próximo
        // passo de quem conduz, depois da recusa da verificação, e uma linha
        // por onda que cobre o critério.
        ("round_checks.criterion_next", Locale::PtBr) => {
            "Mande cada onda abaixo de volta ao agente dela, na mesma cópia, com uma mensagem curta. \
             A rodada gravou o trecho de conserto de cada uma, com a verificação que não passou. O \
             Mustard troca a mensagem por ele. O agente conserta o código, o teste ou a verificação. \
             A verificação nova vai na entrega dele. Ele grava a entrega de novo, e você roda a \
             rodada outra vez. Não reprove a volta com a linha `<REJECTED>`: ela já ficou fora do \
             commit, e a reprovação a daria a um agente novo."
        }
        ("round_checks.criterion_next", Locale::EnUs) => {
            "Send each wave below back to its agent, in the same copy, with a short message. The \
             round recorded each one's fix section, with the verification that did not pass. \
             Mustard swaps the message for it. The agent fixes the code, the test or the \
             verification. The new verification goes in its delivery. It records the delivery \
             again, and you run the round once more. Do not reject the return with the \
             `<REJECTED>` line: it is already out of the commit, and the rejection would hand it to \
             a new agent."
        }
        ("round_checks.criterion_wave", Locale::PtBr) => "Onda {wave}: ela cobre o critério {code}.",
        ("round_checks.criterion_wave", Locale::EnUs) => "Wave {wave}: it covers criterion {code}.",
        // O trecho de conserto que chega ao agente da onda que cobre o
        // critério, um por motivo da recusa: a verificação que caiu, a que
        // saiu verde sem rodar teste e a que cita um teste que não existe.
        ("round_checks.criterion_failed_fix", Locale::PtBr) => {
            "A rodada juntou a entrega da onda {wave} no repositório principal e rodou a verificação \
             do critério {code}, que ela cobre. A verificação `{command}` não executou ou não \
             passou. Nada foi comitado. Ache o que vem desta onda, conserte na cópia, rode a \
             verificação e grave a entrega de novo. Se a verificação é que ficou velha, grave a nova \
             na entrega, em `proofs`. Se nada vem dela, diga isso no texto da entrega. O fim da \
             saída:\n{output}"
        }
        ("round_checks.criterion_failed_fix", Locale::EnUs) => {
            "The round joined wave {wave}'s delivery into the main repository and ran the \
             verification of criterion {code}, which it covers. The verification `{command}` did not \
             run or did not pass. Nothing was committed. Find what comes from this wave, fix it in \
             the copy, run the verification, and record the delivery again. If the verification \
             itself is out of date, record the new one in the delivery, under `proofs`. If nothing \
             comes from it, say so in the delivery text. The end of the output:\n{output}"
        }
        ("round_checks.criterion_no_test_fix", Locale::PtBr) => {
            "A rodada juntou a entrega da onda {wave} no repositório principal e rodou a verificação \
             do critério {code}, que ela cobre. A verificação `{command}` saiu verde sem rodar teste \
             nenhum: ela diz que rodou {count} testes. Nada foi comitado. Grave na entrega, em \
             `proofs`, a verificação que roda o teste do critério. Rode-a na cópia e grave a entrega \
             de novo."
        }
        ("round_checks.criterion_no_test_fix", Locale::EnUs) => {
            "The round joined wave {wave}'s delivery into the main repository and ran the \
             verification of criterion {code}, which it covers. The verification `{command}` came out \
             green without running any test: it says it ran {count} tests. Nothing was committed. \
             Record in the delivery, under `proofs`, the verification that runs the criterion's test. \
             Run it in the copy and record the delivery again."
        }
        ("round_checks.criterion_missing_test_fix", Locale::PtBr) => {
            "A rodada juntou a entrega da onda {wave} no repositório principal e rodou a verificação \
             do critério {code}, que ela cobre. A verificação cita o teste {name}, que não aparece \
             em nenhum arquivo do projeto. Nada foi comitado. Escreva esse teste na cópia, ou grave \
             na entrega, em `proofs`, a verificação com o nome certo. Rode a verificação e grave a \
             entrega de novo."
        }
        ("round_checks.criterion_missing_test_fix", Locale::EnUs) => {
            "The round joined wave {wave}'s delivery into the main repository and ran the \
             verification of criterion {code}, which it covers. The verification names the test \
             {name}, which appears in no file of the project. Nothing was committed. Write that test \
             in the copy, or record in the delivery, under `proofs`, the verification with the right \
             name. Run the verification and record the delivery again."
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
            9,
            0x0fb1_c884_4b4d_e828,
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
            ("round_checks.criterion_next", &[][..]),
            ("round_checks.criterion_wave", &["{wave}", "{code}"][..]),
            ("round_checks.criterion_failed_fix", &["{wave}", "{code}", "{command}", "{output}"][..]),
            ("round_checks.criterion_no_test_fix", &["{wave}", "{code}", "{command}", "{count}"][..]),
            ("round_checks.criterion_missing_test_fix", &["{wave}", "{code}", "{name}"][..]),
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
