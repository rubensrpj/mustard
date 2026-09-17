//! O pedido da onda: os títulos e as instruções fixas que o agente recebe, e
//! por que o despacho de um agente foi barrado.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["prompt", "subagent"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Generic continue / confirm prompt.
        ("prompt.continue", Locale::PtBr) => "Continuar?",
        ("prompt.continue", Locale::EnUs) => "Continue?",

        // O pedido da onda no despacho de um agente: por que o despacho foi
        // barrado.
        ("subagent.ticket_unreadable", Locale::PtBr) => {
            "[Mustard] O despacho foi barrado: a linha {ticket} traz \"{found}\", e ela leva a spec \
             e o número da onda, como `{ticket} minha-spec 2`."
        }
        ("subagent.ticket_unreadable", Locale::EnUs) => {
            "[Mustard] The dispatch was blocked: the {ticket} line carries \"{found}\", and it takes \
             the spec and the wave number, as in `{ticket} my-spec 2`."
        }
        ("subagent.not_approved", Locale::PtBr) => {
            "[Mustard] O despacho foi barrado: a spec {spec} está na fase {phase}, e só uma spec \
             aprovada tem pedido de onda."
        }
        ("subagent.not_approved", Locale::EnUs) => {
            "[Mustard] The dispatch was blocked: the spec {spec} is in the {phase} phase, and only an \
             approved spec has a wave request."
        }
        ("subagent.no_wave", Locale::PtBr) => {
            "[Mustard] O despacho foi barrado: o plano da spec {spec} não tem a onda {wave}."
        }
        ("subagent.no_wave", Locale::EnUs) => {
            "[Mustard] The dispatch was blocked: the plan of the spec {spec} has no wave {wave}."
        }

        // O pedido de uma onda: o texto que o agente dela recebe.
        ("prompt.title", Locale::PtBr) => "{spec} — onda {n}",
        ("prompt.title", Locale::EnUs) => "{spec} — wave {n}",
        ("prompt.fixed", Locale::PtBr) => {
            "**O que é isto.** A lista dos itens desta onda, em ordem de execução, montada pelo \
             binário a partir da spec. Nenhum texto vem copiado: cada linha traz o número do item, \
             o tipo dele e o comando que o lê.\n\n\
             **Ler o item pelo número é parte do trabalho.** Rode o comando da linha na hora de \
             trabalhar naquele item, um de cada vez, e leia do mesmo jeito qualquer item que o \
             texto dele citar. Nunca procure o conteúdo em outro arquivo do projeto.\n\n\
             **O que fazer.** As tarefas desta onda, e só elas. Cada critério listado abaixo ganha \
             um teste que prova a regra dele.\n\n\
             **Quando parar.** Se faltar alguma coisa, ou se uma tarefa parecer pedir o que a spec \
             não diz, pare e relate: não decida sozinho e não invente peça nenhuma.\n\n\
             **O que devolver.** O que mudou, arquivo por arquivo; o teste que prova cada critério; \
             e o que ficou aberto."
        }
        ("prompt.fixed", Locale::EnUs) => {
            "**What this is.** The list of this wave's items, in execution order, assembled by the \
             binary from the spec. No text is copied in: each line carries the item's number, its \
             type and the command that reads it.\n\n\
             **Reading the item by its number is part of the work.** Run the line's command when \
             you get to that item, one at a time, and read any item its text cites the same way. \
             Never look for the content in another project file.\n\n\
             **What to do.** This wave's tasks, and only those. Every criterion listed below gets a \
             test that proves its rule.\n\n\
             **When to stop.** If something is missing, or a task seems to ask for what the spec \
             does not say, stop and report: do not decide alone and do not invent anything.\n\n\
             **What to return.** What changed, file by file; the test that proves each criterion; \
             and what is left open."
        }
        ("prompt.review.title", Locale::PtBr) => "{spec} — revisão da onda {n}",
        ("prompt.review.title", Locale::EnUs) => "{spec} — review of wave {n}",
        ("prompt.review.fixed", Locale::PtBr) => {
            "**O que é isto.** O pedido da revisão desta onda, montado pelo binário a partir da \
             spec. Nenhum texto vem copiado: cada linha traz o número do item, o tipo dele e o \
             comando que o lê.\n\n\
             **O que fazer.** Confira o que a onda entregou contra cada critério listado abaixo, \
             lendo cada item pelo número. Olhe primeiro os defeitos já vistos nestes arquivos: o \
             erro que já aconteceu ali é o que tem mais chance de voltar.\n\n\
             **O que devolver.** O veredito — aprovado ou reprovado —, o que cada critério provou, \
             e a lição que valha para as próximas ondas."
        }
        ("prompt.review.fixed", Locale::EnUs) => {
            "**What this is.** This wave's review request, assembled by the binary from the spec. \
             No text is copied in: each line carries the item's number, its type and the command \
             that reads it.\n\n\
             **What to do.** Check what the wave delivered against every criterion listed below, \
             reading each item by its number. Look first at the defects already seen in these \
             files: the mistake that happened there is the one most likely to come back.\n\n\
             **What to return.** The verdict — approved or rejected —, what each criterion proved, \
             and any lesson worth keeping for the next waves."
        }
        ("prompt.part.defects", Locale::PtBr) => "Defeitos já vistos nestes arquivos",
        ("prompt.part.defects", Locale::EnUs) => "Defects already seen in these files",
        ("prompt.part.specification", Locale::PtBr) => "Especificação",
        ("prompt.part.specification", Locale::EnUs) => "Specification",
        ("prompt.part.agreed", Locale::PtBr) => "Combinado",
        ("prompt.part.agreed", Locale::EnUs) => "Agreed",
        ("prompt.part.wave", Locale::PtBr) => "A onda e as tarefas dela",
        ("prompt.part.wave", Locale::EnUs) => "The wave and its tasks",
        ("prompt.part.criteria", Locale::PtBr) => "Critérios",
        ("prompt.part.criteria", Locale::EnUs) => "Criteria",
        ("prompt.part.lessons", Locale::PtBr) => "Lições",
        ("prompt.part.lessons", Locale::EnUs) => "Lessons",
        ("prompt.part.skills", Locale::PtBr) => "Skills das tarefas",
        ("prompt.part.skills", Locale::EnUs) => "Task skills",
        ("prompt.part.delivered", Locale::PtBr) => "O que as ondas anteriores entregaram",
        ("prompt.part.delivered", Locale::EnUs) => "What the earlier waves delivered",
        ("prompt.skill.stale", Locale::PtBr) => "a revisar",
        ("prompt.skill.stale", Locale::EnUs) => "to review",
        ("prompt.skill.read", Locale::PtBr) => "Leia o arquivo da skill antes de começar a tarefa que a nomeia.",
        ("prompt.skill.read", Locale::EnUs) => "Read the skill file before starting the task that names it.",
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
            include_str!("prompt.rs"),
            super::PREFIXES,
            18,
            0xf8fa_636a_46db_172c,
        );
    }
}
