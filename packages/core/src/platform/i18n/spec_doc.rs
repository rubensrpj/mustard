//! O antigo resumo da spec em HTML: o tipo e o cabeçalho, as fases, as seções,
//! os sete passos do processo, as colunas das tabelas, o próximo passo e o
//! rodapé.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["doc"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Resumo da spec em HTML (`apps/rt/src/commands/spec/spec_doc.rs`) — o
        // documento que o usuário lê ANTES de aprovar. Texto de usuário, então
        // segue o `language.text` e mora aqui, não embutido no comando. `{wave}` e
        // `{term}` são preenchidos pelo chamador.
        ("doc.kind.approval", Locale::PtBr) => "spec para aprovar",
        ("doc.kind.approval", Locale::EnUs) => "spec awaiting approval",
        ("doc.kind.summary", Locale::PtBr) => "resumo da spec",
        ("doc.kind.summary", Locale::EnUs) => "spec summary",
        ("doc.meta.spec", _) => "spec",
        ("doc.meta.branch", _) => "branch",
        ("doc.meta.base", Locale::PtBr) => "sai de",
        ("doc.meta.base", Locale::EnUs) => "cut from",
        ("doc.stage.analyze", Locale::PtBr) => "em análise",
        ("doc.stage.analyze", Locale::EnUs) => "under analysis",
        ("doc.stage.awaiting", Locale::PtBr) => "aguardando aprovação",
        ("doc.stage.awaiting", Locale::EnUs) => "awaiting approval",
        ("doc.stage.approved", Locale::PtBr) => "aprovada, execução por começar",
        ("doc.stage.approved", Locale::EnUs) => "approved, execution not started",
        ("doc.stage.execute", Locale::PtBr) => "em execução",
        ("doc.stage.execute", Locale::EnUs) => "executing",
        ("doc.stage.review", Locale::PtBr) => "em revisão",
        ("doc.stage.review", Locale::EnUs) => "under review",
        ("doc.stage.verify", Locale::PtBr) => "em verificação",
        ("doc.stage.verify", Locale::EnUs) => "under verification",
        ("doc.stage.close", Locale::PtBr) => "fechando",
        ("doc.stage.close", Locale::EnUs) => "closing",
        ("doc.stage.completed", Locale::PtBr) => "fechada",
        ("doc.stage.completed", Locale::EnUs) => "closed",
        ("doc.summary.lead", Locale::PtBr) => "Resumo.",
        ("doc.summary.lead", Locale::EnUs) => "Summary.",
        ("doc.section.where", Locale::PtBr) => "Onde estamos",
        ("doc.section.where", Locale::EnUs) => "Where we are",
        ("doc.section.clarified", Locale::PtBr) => "O que foi esclarecido",
        ("doc.section.clarified", Locale::EnUs) => "What was clarified",
        ("doc.section.decisions", Locale::PtBr) => "Decisões",
        ("doc.section.decisions", Locale::EnUs) => "Decisions",
        ("doc.section.risks", Locale::PtBr) => "Riscos",
        ("doc.section.risks", Locale::EnUs) => "Risks",
        ("doc.section.flow", Locale::PtBr) => "Antes e depois",
        ("doc.section.flow", Locale::EnUs) => "Before and after",
        ("doc.section.spec", Locale::PtBr) => "A spec",
        ("doc.section.spec", Locale::EnUs) => "The spec",
        ("doc.section.criteria", Locale::PtBr) => "Critérios de aceite",
        ("doc.section.criteria", Locale::EnUs) => "Acceptance criteria",
        ("doc.section.waves", Locale::PtBr) => "Ondas e skills",
        ("doc.section.waves", Locale::EnUs) => "Waves and skills",
        ("doc.section.evidence", Locale::PtBr) => "Evidências",
        ("doc.section.evidence", Locale::EnUs) => "Evidence",
        ("doc.section.pending", Locale::PtBr) => "Pendências abertas",
        ("doc.section.pending", Locale::EnUs) => "Open pending items",
        ("doc.section.next", Locale::PtBr) => "Próximo passo",
        ("doc.section.next", Locale::EnUs) => "Next step",
        // Os sete passos do processo, na ordem em que acontecem.
        ("doc.step.analyze.name", Locale::PtBr) => "Análise",
        ("doc.step.analyze.name", Locale::EnUs) => "Analysis",
        ("doc.step.analyze.desc", Locale::PtBr) => "ler o código e a conversa.",
        ("doc.step.analyze.desc", Locale::EnUs) => "read the code and the conversation.",
        ("doc.step.plan.name", Locale::PtBr) => "Plano",
        ("doc.step.plan.name", Locale::EnUs) => "Plan",
        ("doc.step.plan.desc", Locale::PtBr) => {
            "spec e ondas escritas, e cada critério rodado para provar que hoje ele falha."
        }
        ("doc.step.plan.desc", Locale::EnUs) => {
            "spec and waves written, and each criterion run to prove it fails today."
        }
        ("doc.step.approval.name", Locale::PtBr) => "Aprovação",
        ("doc.step.approval.name", Locale::EnUs) => "Approval",
        ("doc.step.approval.desc", Locale::PtBr) => "você aprova com `/mustard:spec`.",
        ("doc.step.approval.desc", Locale::EnUs) => "you approve with `/mustard:spec`.",
        ("doc.step.execute.name", Locale::PtBr) => "Execução",
        ("doc.step.execute.name", Locale::EnUs) => "Execution",
        ("doc.step.execute.desc", Locale::PtBr) => {
            "um agente por onda escreve o código, uma onda depois da outra."
        }
        ("doc.step.execute.desc", Locale::EnUs) => {
            "one agent per wave writes the code, one wave after another."
        }
        ("doc.step.review.name", Locale::PtBr) => "Revisão",
        ("doc.step.review.name", Locale::EnUs) => "Review",
        ("doc.step.review.desc", Locale::PtBr) => "um agente revisor tenta achar falhas.",
        ("doc.step.review.desc", Locale::EnUs) => "a reviewer agent tries to find flaws.",
        ("doc.step.verify.name", Locale::PtBr) => "Verificação",
        ("doc.step.verify.name", Locale::EnUs) => "Verification",
        ("doc.step.verify.desc", Locale::PtBr) => "os critérios rodam de novo e agora precisam passar.",
        ("doc.step.verify.desc", Locale::EnUs) => "the criteria run again and must now pass.",
        ("doc.step.close.name", Locale::PtBr) => "Fechamento",
        ("doc.step.close.name", Locale::EnUs) => "Closing",
        ("doc.step.close.desc", Locale::PtBr) => "pull request e merge na base.",
        ("doc.step.close.desc", Locale::EnUs) => "pull request and merge into the base.",
        ("doc.step.here", Locale::PtBr) => "Estamos aqui.",
        ("doc.step.here", Locale::EnUs) => "We are here.",
        ("doc.col.question", Locale::PtBr) => "Pergunta",
        ("doc.col.question", Locale::EnUs) => "Question",
        ("doc.col.answer", Locale::PtBr) => "Resposta",
        ("doc.col.answer", Locale::EnUs) => "Answer",
        ("doc.clarified.definition", Locale::PtBr) => "O que quer dizer {term} aqui?",
        ("doc.clarified.definition", Locale::EnUs) => "What does {term} mean here?",
        ("doc.clarified.notes", Locale::PtBr) => "Nota:",
        ("doc.clarified.notes", Locale::EnUs) => "Note:",
        ("doc.col.severity", Locale::PtBr) => "Gravidade",
        ("doc.col.severity", Locale::EnUs) => "Severity",
        ("doc.col.risk", Locale::PtBr) => "Risco",
        ("doc.col.risk", Locale::EnUs) => "Risk",
        ("doc.col.mitigation", Locale::PtBr) => "O que atenua",
        ("doc.col.mitigation", Locale::EnUs) => "What mitigates it",
        ("doc.severity.alta", Locale::PtBr) => "Alta",
        ("doc.severity.alta", Locale::EnUs) => "High",
        ("doc.severity.media", Locale::PtBr) => "Média",
        ("doc.severity.media", Locale::EnUs) => "Medium",
        ("doc.severity.baixa", Locale::PtBr) => "Baixa",
        ("doc.severity.baixa", Locale::EnUs) => "Low",
        ("doc.criteria.lead", Locale::PtBr) => {
            "Cada critério é um comando. Antes de o código existir ele precisa falhar, e é \
             essa falha que prova que ele mede algo novo; depois da entrega, precisa passar."
        }
        ("doc.criteria.lead", Locale::EnUs) => {
            "Each criterion is a command. Before the code exists it must fail, and that \
             failure is what proves it measures something new; after delivery it must pass."
        }
        ("doc.col.id", _) => "Id",
        ("doc.col.criterion", Locale::PtBr) => "Quando… então…",
        ("doc.col.criterion", Locale::EnUs) => "When… then…",
        ("doc.col.wave", Locale::PtBr) => "Onda",
        ("doc.col.wave", Locale::EnUs) => "Wave",
        ("doc.waves.lead", Locale::PtBr) => {
            "As skills são os moldes que o agente de cada onda carrega antes de escrever os \
             arquivos que elas governam. A lista sai do cruzamento dos arquivos da onda com \
             as pastas de cada molde."
        }
        ("doc.waves.lead", Locale::EnUs) => {
            "Skills are the molds each wave's agent loads before writing the files they \
             govern. The list comes from crossing the wave's files with each mold's folders."
        }
        ("doc.wave.done", Locale::PtBr) => "concluída",
        ("doc.wave.done", Locale::EnUs) => "done",
        ("doc.wave.skills", _) => "Skills",
        ("doc.wave.covers", Locale::PtBr) => "cobre",
        ("doc.wave.covers", Locale::EnUs) => "covers",
        ("doc.wave.criteria", Locale::PtBr) => "Critérios",
        ("doc.wave.criteria", Locale::EnUs) => "Criteria",
        ("doc.wave.obligations", Locale::PtBr) => "Obrigação externa",
        ("doc.wave.obligations", Locale::EnUs) => "External obligation",
        ("doc.col.seen", Locale::PtBr) => "O que foi visto no código",
        ("doc.col.seen", Locale::EnUs) => "What was seen in the code",
        ("doc.col.where", Locale::PtBr) => "Onde",
        ("doc.col.where", Locale::EnUs) => "Where",
        ("doc.col.pending", Locale::PtBr) => "Pendência",
        ("doc.col.pending", Locale::EnUs) => "Pending item",
        ("doc.next.analyze", Locale::PtBr) => "A análise segue; a spec é escrita no passo seguinte.",
        ("doc.next.analyze", Locale::EnUs) => "Analysis continues; the spec is written in the next step.",
        ("doc.next.approve", Locale::PtBr) => {
            "Para aprovar, digite `/mustard:spec` neste branch. A onda 1 começa."
        }
        ("doc.next.approve", Locale::EnUs) => {
            "To approve, type `/mustard:spec` on this branch. Wave 1 starts."
        }
        ("doc.next.adjust", Locale::PtBr) => "Para ajustar, diga o que mudar.",
        ("doc.next.adjust", Locale::EnUs) => "To adjust, say what to change.",
        ("doc.next.approved", Locale::PtBr) => "A spec está aprovada. `/mustard:spec` começa a onda 1.",
        ("doc.next.approved", Locale::EnUs) => "The spec is approved. `/mustard:spec` starts wave 1.",
        ("doc.next.execute", Locale::PtBr) => "A execução segue na onda {wave}, com `/mustard:spec`.",
        ("doc.next.execute", Locale::EnUs) => "Execution continues with wave {wave}, via `/mustard:spec`.",
        ("doc.next.execute_done", Locale::PtBr) => "Todas as ondas terminaram; a revisão vem a seguir.",
        ("doc.next.execute_done", Locale::EnUs) => "Every wave is done; review comes next.",
        ("doc.next.review", Locale::PtBr) => {
            "A revisão e a verificação estão rodando; o resultado aparece nesta página."
        }
        ("doc.next.review", Locale::EnUs) => {
            "Review and verification are running; the result shows up on this page."
        }
        ("doc.next.close", Locale::PtBr) => "Falta fechar: pull request e merge na base.",
        ("doc.next.close", Locale::EnUs) => "What is left is closing: pull request and merge into the base.",
        ("doc.next.completed", Locale::PtBr) => "A unidade está fechada. Não há mais nada a fazer aqui.",
        ("doc.next.completed", Locale::EnUs) => "The unit is closed. Nothing else to do here.",
        ("doc.footer", Locale::PtBr) => {
            "Montado pelo Mustard a partir da spec, das ondas, do material da conversa, da \
             prova dos critérios e da lista de pendências."
        }
        ("doc.footer", Locale::EnUs) => {
            "Built by Mustard from the spec, the waves, the conversation material, the \
             criteria proof and the pending list."
        }
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
            include_str!("spec_doc.rs"),
            super::PREFIXES,
            73,
            0x3ebd_f7b5_3df6_598e,
        );
    }

    /// O resumo da spec em HTML tira todo o texto do catálogo, nos dois idiomas,
    /// e o próximo passo da execução carrega o número da onda.
    #[test]
    fn i18n_translates_spec_doc_keys() {
        for key in [
            "doc.section.where",
            "doc.step.plan.name",
            "doc.next.approve",
            "doc.footer",
        ] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_ne!(translate(key, lang), "<missing-key>", "{key} missing for {lang}");
            }
            assert_ne!(
                translate(key, Locale::PtBr),
                translate(key, Locale::EnUs),
                "{key} must differ per locale (proof it is catalogue-driven)"
            );
        }
        for lang in [Locale::PtBr, Locale::EnUs] {
            assert!(translate("doc.next.execute", lang).contains("{wave}"));
            assert!(translate("doc.clarified.definition", lang).contains("{term}"));
        }
    }
}
