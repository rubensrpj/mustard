//! O levantamento: as recusas e os passos do grill, as recusas da resposta de
//! um ponto e o rótulo de cada lacuna por tipo de trabalho.
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
        ("grill.cited_in_the_request", Locale::PtBr) => {
            "O documento do usuário cita este lugar no diagnóstico dele. A conferir."
        }
        ("grill.cited_in_the_request", Locale::EnUs) => {
            "The user's own document cites this place in its diagnosis. To be checked."
        }
        ("grill.citation_left_out", Locale::PtBr) => {
            "O pedido cita {source}, e essa citação não se confere: o arquivo não está no projeto \
             ou não tem a linha citada. Ela ficou fora dos fatos do defeito; confira com o \
             usuário o lugar certo."
        }
        ("grill.citation_left_out", Locale::EnUs) => {
            "The request cites {source}, and that citation does not check: the file is not in the \
             project or lacks the cited line. It was left out of the defect's facts; check the \
             right place with the user."
        }
        ("grill.citation_left_out.same_name", Locale::PtBr) => {
            "O pedido cita {source} só pelo nome, e {count} arquivos do projeto têm esse nome: \
             {found}. A citação ficou fora dos fatos do defeito; pergunte ao usuário de qual deles \
             ele fala."
        }
        ("grill.citation_left_out.same_name", Locale::EnUs) => {
            "The request cites {source} by its name alone, and {count} files of the project have \
             that name: {found}. The citation was left out of the defect's facts; ask the user \
             which one they mean."
        }
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
            "A spec não tem tipo de trabalho gravado, e a branch dela não começa por um tipo \
             (feature/, fix/ ou refactor/). Diga o tipo em --kinds: feature, fix ou refactor, mais de \
             um no pedido misto. Nada foi gravado."
        }
        ("grill.kinds_missing", Locale::EnUs) => {
            "The spec has no work type recorded, and its branch does not start with a type \
             (feature/, fix/ or refactor/). Give the type in --kinds: feature, fix or refactor, more \
             than one for a mixed request. Nothing was written."
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
        ("survey.answer_options", Locale::PtBr) => {
            "Diga a resposta do ponto num item novo, com `--type` e `--json`, ou em itens já \
             gravados, com `--result`. Os dois podem vir juntos. O ponto que não se aplica fecha com \
             `--not-applicable` e `--reason`. Nada foi gravado."
        }
        ("survey.answer_options", Locale::EnUs) => {
            "Give the answer to the point as a new item, with `--type` and `--json`, or as recorded \
             items, with `--result`. Both can come together. A point that does not apply closes with \
             `--not-applicable` and `--reason`. Nothing was written."
        }
        ("survey.answer_not_applicable_mixed", Locale::PtBr) => {
            "O ponto fecha com a resposta ou como não se aplica, nunca com os dois: tire o \
             `--not-applicable` ou a resposta. Nada foi gravado."
        }
        ("survey.answer_not_applicable_mixed", Locale::EnUs) => {
            "The point closes with an answer or as not applicable, never both: drop \
             `--not-applicable` or the answer. Nothing was written."
        }
        ("survey.answer_no_open_point", Locale::PtBr) => {
            "A spec {spec} não tem ponto aberto no levantamento: não há o que responder. Nada foi \
             gravado."
        }
        ("survey.answer_no_open_point", Locale::EnUs) => {
            "Spec {spec} has no open point in the survey: there is nothing to answer. Nothing was \
             written."
        }
        ("survey.present_all", Locale::PtBr) => {
            "Os pontos abertos estão em `points`. Antes do sim, preencha cada lacuna a partir do \
             pedido e do código. Mostre tudo de uma vez, na ordem de explicar do estilo de \
             resposta, e peça um sim só. Com o sim, \
             grave a resposta de cada ponto com `mustard-rt run answer --point <número> --type \
             <tipo> --json '{…}'`. Se o ponto não tem fato, o fechamento leva `--facts` (lista JSON de \
             `{\"text\":\"…\",\"source\":\"arquivo:linha\"}`), ou os fatos entram antes numa versão do ponto. \
             Cada resposta fecha o ponto. Ela devolve em `points` o código, o \
             número e a lacuna de cada ponto ainda aberto, e em `point` o próximo inteiro."
        }
        ("survey.present_all", Locale::EnUs) => {
            "The open points are in `points`. Before the yes, fill every gap from the request and \
             the code. Show it all at once, in the order of explaining from the response style, \
             and ask for a single yes. With the yes, \
             record the answer to each point with `mustard-rt run answer --point <number> --type \
             <type> --json '{…}'`. If the point has no fact, the closing takes `--facts` (JSON \
             array of `{\"text\":\"…\",\"source\":\"file:line\"}`), or the facts go first into a version of \
             the point. Each answer closes the point. It returns in `points` the code, \
             the number and the gap of each point still open, and in `point` the next one in full."
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
        ("survey.rerun_grill", Locale::PtBr) => {
            "Falta ponto para alguma lacuna do tipo de trabalho. Rode `mustard-rt run grill --spec \
             {spec}`, que grava os pontos que faltam e devolve os abertos."
        }
        ("survey.rerun_grill", Locale::EnUs) => {
            "Some gap of the work type has no point. Run `mustard-rt run grill --spec {spec}`, which \
             records the missing points and returns the open ones."
        }
        ("survey.done", Locale::PtBr) => "O levantamento não tem ponto aberto. Grave a especificação e as tarefas.",
        ("survey.done", Locale::EnUs) => "The survey has no open point. Record the specification and the tasks.",
        ("survey.done_unrouted", Locale::PtBr) => {
            "O levantamento não tem ponto aberto. Mostre ao usuário as mensagens de `unrouted`, que \
             nenhum registro aponta (cada linha traz o código e o começo da mensagem; o texto \
             inteiro sai por `mustard-rt run read item-<código>`), e pergunte o que fazer com cada \
             uma. Depois grave a especificação e as tarefas."
        }
        ("survey.done_unrouted", Locale::EnUs) => {
            "The survey has no open point. Show the user the messages in `unrouted`, which no record \
             points to (each line has the code and the start of the message; the whole text comes \
             from `mustard-rt run read item-<item-code>`), and ask what to do with each one. Then record \
             the specification and the tasks."
        }
        // O nome do arquivo é o mesmo nos dois idiomas: é ele que o gancho do
        // despacho procura na pasta da spec para apontar o desenho ao revisor.
        // Sai com os pontos que o `grill` abre, antes de qualquer sim: a cópia
        // é salva enquanto o modelo aprovado ainda está à mão, e o fim do
        // levantamento não a pede de novo.
        ("survey.save_design", Locale::PtBr) => {
            "Se o usuário aprovar um modelo visual de tela neste levantamento, salve a cópia aprovada \
             em `.claude/spec/{spec}/desenho.html`. Salve logo depois de gravar o sim, enquanto o \
             modelo ainda está à mão. Com o arquivo ali, o pedido do revisor aponta o desenho."
        }
        ("survey.save_design", Locale::EnUs) => {
            "If the user approves a visual model of a screen in this survey, save the approved copy at \
             `.claude/spec/{spec}/desenho.html`. Save it right after recording the yes, while the \
             model is still at hand. With the file there, the reviewer's request points to the design."
        }
        ("survey.fact_declared", Locale::PtBr) => "`{name}` é declarado em {path}, linha {line}.",
        ("survey.fact_declared", Locale::EnUs) => "`{name}` is declared in {path}, line {line}.",
        ("survey.fact_importers", Locale::PtBr) => "{path} é importado por: {importers}.",
        ("survey.fact_importers", Locale::EnUs) => "{path} is imported by: {importers}.",
        ("survey.fact_test_candidates", Locale::PtBr) => {
            "Testes candidatos para {path}: {tests}; testes no próprio arquivo: {inline}. A associação do scan não comprova cobertura nem execução; confira os testes e o comportamento."
        }
        ("survey.fact_test_candidates", Locale::EnUs) => {
            "Candidate tests for {path}: {tests}; inline tests: {inline}. The scan association proves neither coverage nor execution; check the tests and behavior."
        }
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
        ("survey.project_summary", Locale::PtBr) => {
            "Entregas anteriores do projeto ligadas a este objetivo. São contexto para você: \
             nenhuma vira ponto da lista."
        }
        ("survey.project_summary", Locale::EnUs) => {
            "Earlier project deliveries related to this goal. They are context for you: none \
             becomes a point of the list."
        }
        ("survey.project_summary.reserve", Locale::PtBr) => {
            "O Jev não respondeu a tempo sobre as entregas do projeto ligadas a este objetivo. Lance o agente de análise da sessão com o pedido em `reserve`. Mande o recibo por `mustard-rt run knowledge --complete-session <recibo.json>`. Depois rode `mustard-rt run grill` de novo: o recibo escolhe as entregas."
        }
        ("survey.project_summary.reserve", Locale::EnUs) => {
            "Jev did not answer in time about the project deliveries related to this goal. Launch the session's analysis agent with the request in `reserve`. Send its receipt with `mustard-rt run knowledge --complete-session <receipt.json>`. Then run `mustard-rt run grill` again: the receipt chooses the deliveries."
        }
        ("survey.project_summary.recent", Locale::PtBr) => {
            "As entregas mais recentes do projeto, sem a escolha pelo objetivo. São contexto para \
             você: nenhuma vira ponto da lista."
        }
        ("survey.project_summary.recent", Locale::EnUs) => {
            "The project's most recent deliveries, not chosen by the goal. They are context for \
             you: none becomes a point of the list."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::translate;

    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas, como o arquivo de impressões dela em `stamps/` guarda. Quem muda
    /// um texto de propósito regrava o arquivo pelo comando que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged("survey", include_str!("survey.rs"), super::PREFIXES);
    }

    /// O ponto aberto sem fato só fecha com os fatos: a dica de mostrar os
    /// pontos cita `--facts` nos dois idiomas, com a forma de cada fato, e o
    /// caminho pela versão do ponto.
    #[test]
    fn the_closing_hints_name_the_facts_a_point_without_fact_needs() {
        for key in ["survey.present_all"] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                let text = translate(key, lang);
                assert!(text.contains("`--facts`"), "{key} ({lang}): {text}");
                assert!(text.contains("\"text\"") && text.contains("\"source\""), "{key} ({lang}): {text}");
                assert!(text.contains("versão do ponto") || text.contains("version of the point"), "{key} ({lang}): {text}");
            }
        }
    }

    /// As recusas e os passos do levantamento e o rótulo de cada lacuna saem
    /// do catálogo nos dois idiomas, cada um com as vagas que o chamador
    /// preenche.
    #[test]
    fn i18n_translates_grill_and_survey_keys() {
        let mut keys: Vec<(String, &[&str])> = vec![
            ("grill.citation_left_out".into(), &["{source}"][..]),
            ("grill.citation_left_out.same_name".into(), &["{source}", "{count}", "{found}"][..]),
            ("grill.goal_missing".into(), &["{spec}"][..]),
            ("grill.not_in_survey".into(), &["{spec}", "{phase}"][..]),
            ("grill.kinds_missing".into(), &[][..]),
            ("grill.kinds_narrowed".into(), &["{spec}", "{recorded}"][..]),
            ("grill.work_type_by_grill".into(), &[][..]),
            ("reopen.reason_missing".into(), &[][..]),
            ("reopen.settled".into(), &["{spec}", "{phase}"][..]),
            ("reopen.next".into(), &["{spec}"][..]),
            ("reopen.already".into(), &["{spec}"][..]),
            ("reopen.reopened".into(), &["{spec}"][..]),
            ("reopen.fix_not_red".into(), &["{spec}", "{phase}"][..]),
            ("reopen.merge_unchecked".into(), &["{spec}", "{reason}"][..]),
            ("survey.answer_options".into(), &[][..]),
            ("survey.answer_not_applicable_mixed".into(), &[][..]),
            ("survey.answer_no_open_point".into(), &["{spec}"][..]),
            ("survey.present_all".into(), &[][..]),
            ("survey.rerun_grill".into(), &["{spec}"][..]),
            ("survey.touched".into(), &["{count}", "{reason}"][..]),
            ("survey.done".into(), &[][..]),
            ("survey.done_unrouted".into(), &[][..]),
            ("survey.save_design".into(), &["{spec}"][..]),
            ("survey.fact_declared".into(), &["{name}", "{path}", "{line}"][..]),
            ("survey.fact_importers".into(), &["{path}", "{importers}"][..]),
            ("survey.fact_test_candidates".into(), &["{path}", "{tests}", "{inline}"][..]),
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
