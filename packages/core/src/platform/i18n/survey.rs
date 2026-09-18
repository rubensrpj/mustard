//! O levantamento: as recusas e os passos do grill e o rótulo de cada lacuna
//! por tipo de trabalho.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["grill", "survey"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        ("grill.goal_missing", Locale::PtBr) => {
            "A spec {spec} ainda não tem o objetivo. Pergunte ao usuário \"Qual o objetivo, numa \
             frase?\" e rode o grill depois da resposta. Nada foi gravado."
        }
        ("grill.goal_missing", Locale::EnUs) => {
            "Spec {spec} has no goal yet. Ask the user \"What is the goal, in one sentence?\" and \
             run grill after the answer. Nothing was written."
        }
        ("grill.not_in_survey", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}, e o levantamento só roda na fase de levantamento. \
             Nada foi gravado."
        }
        ("grill.not_in_survey", Locale::EnUs) => {
            "Spec {spec} is in the {phase} phase, and the survey only runs in the survey phase. \
             Nothing was written."
        }
        ("grill.kinds_missing", Locale::PtBr) => {
            "Diga o tipo de trabalho em --kinds: feature, fix ou refactor, mais de um no pedido \
             misto. Nada foi gravado."
        }
        ("grill.kinds_missing", Locale::EnUs) => {
            "Give the work type in --kinds: feature, fix or refactor, more than one for a mixed \
             request. Nothing was written."
        }
        ("grill.kinds_narrowed", Locale::PtBr) => {
            "O levantamento da spec {spec} já tem as lacunas de {recorded}. Um tipo a menos não tira \
             pontos: feche os que não valem com \"não se aplica\" e o motivo. Nada foi gravado."
        }
        ("grill.kinds_narrowed", Locale::EnUs) => {
            "The survey of spec {spec} already has the gaps of {recorded}. Dropping a type does not \
             remove points: close the ones that do not apply with \"not applicable\" and the reason. \
             Nothing was written."
        }
        ("grill.work_type_by_grill", Locale::PtBr) => {
            "O tipo de trabalho é gravado pelo `mustard-rt run grill`, que monta a lista de pontos \
             junto. Nada foi gravado."
        }
        ("grill.work_type_by_grill", Locale::EnUs) => {
            "The work type is written by `mustard-rt run grill`, which builds the point list with \
             it. Nothing was written."
        }
        ("survey.present_point", Locale::PtBr) => {
            "Apresente o ponto {code}, e só ele, na ordem de explicar do estilo de resposta. Grave \
             cada resposta na hora e feche o ponto com `closes`: {id}."
        }
        ("survey.present_point", Locale::EnUs) => {
            "Present point {code}, and only it, in the order of explaining from the response style. \
             Record each answer right away and close the point with `closes`: {id}."
        }
        ("survey.present_all", Locale::PtBr) => {
            "Pedido pequeno: preencha todas as lacunas de `points` a partir do pedido e do código, \
             mostre tudo de uma vez e peça um sim só. Com o sim, grave as respostas e feche cada \
             ponto."
        }
        ("survey.present_all", Locale::EnUs) => {
            "Small request: fill every gap in `points` from the request and the code, show it all \
             at once and ask for a single yes. With the yes, record the answers and close each point."
        }
        ("survey.touched", Locale::PtBr) => {
            "A spec voltou ao levantamento por: {reason}. {count} itens já gravados são tocados por \
             esse motivo — mostre cada um ao usuário e pergunte se ele fica, muda ou sai. O que \
             mudar vira versão nova do item, apontando a antiga, que continua visível na conversa. \
             O que o motivo não toca fica como está, e não se pergunta de novo."
        }
        ("survey.touched", Locale::EnUs) => {
            "The spec went back to the survey because: {reason}. {count} recorded items are touched \
             by that reason — show each one to the user and ask whether it stays, changes or goes. \
             What changes becomes a new version of the item, pointing at the old one, which stays \
             visible in the conversation. What the reason does not touch stays as it is, and is \
             never asked again."
        }
        ("survey.record_points", Locale::PtBr) => {
            "Grave cada ponto de `points` que ainda não tem `id` com `mustard-rt run write point \
             --spec {spec}`, na ordem: copie os campos como vieram, com `status` open, e ponha em \
             `facts` o que você conferiu no código ou na conversa, cada fato com a fonte (arquivo e \
             linha, comando ou número da mensagem); os fatos que já vêm no ponto ficam. A gravação \
             do último ponto já devolve o primeiro, para mostrar ao usuário."
        }
        ("survey.record_points", Locale::EnUs) => {
            "Record each point in `points` that has no `id` yet with `mustard-rt run write point \
             --spec {spec}`, in order: copy its fields as they came, with `status` open, and put in \
             `facts` what you checked in the code or in the conversation, each fact with its source \
             (file and line, command or message number); the facts the point already brings stay. \
             Recording the last point already returns the first one, to show the user."
        }
        ("survey.done", Locale::PtBr) => {
            "O levantamento não tem ponto aberto. Mostre ao usuário as mensagens de `unrouted`, que \
             nenhum registro aponta, e pergunte o que fazer com cada uma; depois grave a \
             especificação, as ondas e as tarefas."
        }
        ("survey.done", Locale::EnUs) => {
            "The survey has no open point. Show the user the messages in `unrouted`, which no record \
             points to, and ask what to do with each one; then record the specification, the waves \
             and the tasks."
        }
        ("survey.review_step", Locale::PtBr) => {
            "O bloco {block} fechou. Releia só as decisões dele e ache o que pode ter ficado de fora \
             ou se contradiz. Apresente os achados na ordem de explicar do estilo de resposta e faça \
             a pergunta de `question`, com os achados como opções e \"{continue}\" por último."
        }
        ("survey.review_step", Locale::EnUs) => {
            "Block {block} is closed. Reread only its decisions and find what may have been left out \
             or contradicts itself. Present the findings in the order of explaining from the response \
             style and ask the question in `question`, with the findings as options and \
             \"{continue}\" last."
        }
        ("survey.review_question", Locale::PtBr) => "Quer ver mais algum ponto ou aprofundar algum?",
        ("survey.review_question", Locale::EnUs) => "Would you like to see another point or go deeper into one?",
        ("survey.continue_option", Locale::PtBr) => "Seguir",
        ("survey.continue_option", Locale::EnUs) => "Continue",
        ("survey.outside_review_step", Locale::PtBr) => {
            "Depois, antes do fim, rode o revisor de fora: despache ao agente `mustard-review` a \
             conferência do levantamento inteiro da spec {spec}, que aponta o que ficou de fora ou se \
             contradiz. Grave cada achado dele como ponto aberto, com `block` e `from` \
             outside_review, e apresente-o como os outros; sem achado, siga."
        }
        ("survey.outside_review_step", Locale::EnUs) => {
            "Then, before the end, run the outside reviewer: dispatch to the `mustard-review` agent \
             the check of the whole survey of spec {spec}, which points out what was left out or \
             contradicts itself. Record each of its findings as an open point, with `block` and \
             `from` outside_review, and present it like the others; with no finding, go on."
        }
        ("survey.fact_declared", Locale::PtBr) => "`{name}` é declarado em {path}, linha {line}.",
        ("survey.fact_declared", Locale::EnUs) => "`{name}` is declared in {path}, line {line}.",
        ("survey.fact_importers", Locale::PtBr) => "{path} é importado por: {importers}.",
        ("survey.fact_importers", Locale::EnUs) => "{path} is imported by: {importers}.",
        ("survey.gap.who_uses", Locale::PtBr) => "Quem usa e para quê",
        ("survey.gap.who_uses", Locale::EnUs) => "Who uses it and what for",
        ("survey.gap.rules", Locale::PtBr) => "Cada regra, com um exemplo com números",
        ("survey.gap.rules", Locale::EnUs) => "Each rule, with an example with numbers",
        ("survey.gap.limits", Locale::PtBr) => "Os limites, com os valores",
        ("survey.gap.limits", Locale::EnUs) => "The limits, with their values",
        ("survey.gap.contracts", Locale::PtBr) => "Os contratos de entrada e saída, com um exemplo real",
        ("survey.gap.contracts", Locale::EnUs) => "Input and output contracts, with a real example",
        ("survey.gap.errors", Locale::PtBr) => "Cada erro, com a mensagem exata",
        ("survey.gap.errors", Locale::EnUs) => "Each error, with its exact message",
        ("survey.gap.edge_cases", Locale::PtBr) => "Os casos de borda",
        ("survey.gap.edge_cases", Locale::EnUs) => "The edge cases",
        ("survey.gap.out_of_scope", Locale::PtBr) => "O que fica fora",
        ("survey.gap.out_of_scope", Locale::EnUs) => "What stays out",
        ("survey.gap.done_proof", Locale::PtBr) => "Como provar que ficou pronto",
        ("survey.gap.done_proof", Locale::EnUs) => "How to prove it is done",
        ("survey.gap.external_deps", Locale::PtBr) => "As dependências externas",
        ("survey.gap.external_deps", Locale::EnUs) => "The external dependencies",
        ("survey.gap.symptom", Locale::PtBr) => "O sintoma",
        ("survey.gap.symptom", Locale::EnUs) => "The symptom",
        ("survey.gap.reproduction", Locale::PtBr) => "Como reproduzir",
        ("survey.gap.reproduction", Locale::EnUs) => "How to reproduce it",
        ("survey.gap.expected_vs_actual", Locale::PtBr) => "O esperado contra o obtido",
        ("survey.gap.expected_vs_actual", Locale::EnUs) => "Expected versus actual",
        ("survey.gap.cause", Locale::PtBr) => "A causa",
        ("survey.gap.cause", Locale::EnUs) => "The cause",
        ("survey.gap.measured_reason", Locale::PtBr) => "O motivo, com um número medido",
        ("survey.gap.measured_reason", Locale::EnUs) => "The reason, with a measured number",
        ("survey.gap.must_not_change", Locale::PtBr) => "O que não pode mudar, e o teste que prova",
        ("survey.gap.must_not_change", Locale::EnUs) => "What must not change, and the test that proves it",
        ("survey.gap.removed_and_users", Locale::PtBr) => "O que sai e quem usa",
        ("survey.gap.removed_and_users", Locale::EnUs) => "What goes away and who uses it",
        ("survey.gap.moves", Locale::PtBr) => "O que muda de lugar",
        ("survey.gap.moves", Locale::EnUs) => "What moves",
        ("survey.gap.dependents", Locale::PtBr) => "Quem depende, dentro e fora do projeto",
        ("survey.gap.dependents", Locale::EnUs) => "Who depends on it, inside and outside the project",
        ("survey.gap.green_order", Locale::PtBr) => "A ordem, com os testes verdes a cada passo",
        ("survey.gap.green_order", Locale::EnUs) => "The order, with green tests at every step",
        ("survey.gap.before_after", Locale::PtBr) => "Os números antes e depois: linhas, tempo e tokens",
        ("survey.gap.before_after", Locale::EnUs) => "The numbers before and after: lines, time and tokens",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::translate;

    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("survey.rs"),
            super::PREFIXES,
            36,
            0x77e9_fc03_69c4_09ea,
        );
    }

    /// As recusas e os passos do levantamento e o rótulo de cada lacuna saem
    /// do catálogo nos dois idiomas, cada um com as vagas que o chamador
    /// preenche.
    #[test]
    fn i18n_translates_grill_and_survey_keys() {
        let mut keys: Vec<(String, &[&str])> = vec![
            ("grill.goal_missing".into(), &["{spec}"][..]),
            ("grill.not_in_survey".into(), &["{spec}", "{phase}"][..]),
            ("grill.kinds_missing".into(), &[][..]),
            ("grill.kinds_narrowed".into(), &["{spec}", "{recorded}"][..]),
            ("grill.work_type_by_grill".into(), &[][..]),
            ("reopen.reason_missing".into(), &[][..]),
            ("reopen.settled".into(), &["{spec}", "{phase}"][..]),
            ("reopen.next".into(), &["{spec}"][..]),
            ("reopen.already".into(), &["{spec}"][..]),
            ("survey.present_point".into(), &["{code}", "{id}"][..]),
            ("survey.present_all".into(), &[][..]),
            ("survey.record_points".into(), &["{spec}"][..]),
            ("survey.touched".into(), &["{count}", "{reason}"][..]),
            ("survey.done".into(), &[][..]),
            ("survey.review_step".into(), &["{block}", "{continue}"][..]),
            ("survey.review_question".into(), &[][..]),
            ("survey.continue_option".into(), &[][..]),
            ("survey.outside_review_step".into(), &["{spec}"][..]),
            ("survey.fact_declared".into(), &["{name}", "{path}", "{line}"][..]),
            ("survey.fact_importers".into(), &["{path}", "{importers}"][..]),
        ];
        for gap in crate::domain::survey::GapKey::ALL {
            keys.push((gap.label_key().to_string(), &[][..]));
        }
        for (key, slots) in keys {
            let (pt, en) = (translate(&key, Locale::PtBr), translate(&key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }
}
