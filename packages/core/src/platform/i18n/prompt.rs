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
             um teste que prova a regra dele e nasce vermelho: corte a ligação no caminho que o \
             usuário usa — o comando ou o evento do gancho, e não só a função auxiliar —, veja o \
             teste cair e desfaça o corte.\n\n\
             **Quando parar.** Se faltar alguma coisa, ou se uma tarefa parecer pedir o que a spec \
             não diz, pare e relate: não decida sozinho e não invente peça nenhuma.\n\n\
             **O que devolver.** O que mudou, arquivo por arquivo; o teste que prova cada critério e \
             como a prova do vermelho foi feita — o que foi cortado e o que o teste disse ao cair; e o \
             que ficou aberto."
        }
        ("prompt.fixed", Locale::EnUs) => {
            "**What this is.** The list of this wave's items, in execution order, assembled by the \
             binary from the spec. No text is copied in: each line carries the item's number, its \
             type and the command that reads it.\n\n\
             **Reading the item by its number is part of the work.** Run the line's command when \
             you get to that item, one at a time, and read any item its text cites the same way. \
             Never look for the content in another project file.\n\n\
             **What to do.** This wave's tasks, and only those. Every criterion listed below gets a \
             test that proves its rule and is born red: cut the link on the path the user takes — the \
             command or the hook event, not only the helper function —, watch the test fail and undo \
             the cut.\n\n\
             **When to stop.** If something is missing, or a task seems to ask for what the spec \
             does not say, stop and report: do not decide alone and do not invent anything.\n\n\
             **What to return.** What changed, file by file; the test that proves each criterion and \
             how the red proof was made — what was cut and what the test said when it failed; and \
             what is left open."
        }
        ("prompt.review.title", Locale::PtBr) => "{spec} — revisão da onda {n}",
        ("prompt.review.title", Locale::EnUs) => "{spec} — review of wave {n}",
        ("prompt.review.fixed", Locale::PtBr) => {
            "**O que é isto.** O pedido da revisão desta onda, montado pelo binário a partir da \
             spec. Nenhum texto vem copiado: cada linha traz o número do item, o tipo dele e o \
             comando que o lê.\n\n\
             **O que fazer.** Confira o que a onda entregou contra cada critério listado abaixo, \
             lendo cada item pelo número. Rode a prova gravada de cada critério e leia as provas do \
             vermelho que a entrega relata; gaste os seus cortes onde a onda não cortou, sem repetir \
             os dela. Olhe primeiro os defeitos já vistos nestes arquivos: o erro que já aconteceu \
             ali é o que tem mais chance de voltar.\n\n\
             **O que devolver.** O veredito — aprovado ou reprovado —, o que cada critério provou, \
             e a lição que valha para as próximas ondas."
        }
        ("prompt.review.fixed", Locale::EnUs) => {
            "**What this is.** This wave's review request, assembled by the binary from the spec. \
             No text is copied in: each line carries the item's number, its type and the command \
             that reads it.\n\n\
             **What to do.** Check what the wave delivered against every criterion listed below, \
             reading each item by its number. Run each criterion's recorded proof and read the red \
             proofs the delivery reports; spend your cuts where the wave did not cut, without \
             repeating its own. Look first at the defects already seen in these files: the mistake \
             that happened there is the one most likely to come back.\n\n\
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

        // O conserto: a onda que volta por reprovação e a revisão dela.
        ("prompt.part.fix", Locale::PtBr) => "Conserto",
        ("prompt.part.fix", Locale::EnUs) => "Fix",
        ("prompt.fix.wave", Locale::PtBr) => {
            "Esta onda voltou por reprovação. As linhas abaixo são o veredito que reprovou, a entrega \
             anterior desta onda e os itens combinados gravados depois do último envio. Conserte só o \
             que o veredito aponta, à luz desses itens: não refaça a onda."
        }
        ("prompt.fix.wave", Locale::EnUs) => {
            "This wave came back rejected. The lines below are the verdict that rejected it, this \
             wave's previous delivery and the agreed items recorded after the last send. Fix only what \
             the verdict points out, in light of those items: do not redo the wave."
        }
        ("prompt.fix.review", Locale::PtBr) => {
            "Esta revisão é de um conserto. As linhas abaixo são o veredito que reprovou, a entrega \
             anterior e os itens combinados gravados depois do último envio. Olhe só o conserto — o \
             que o veredito apontou —, não a onda inteira de novo."
        }
        ("prompt.fix.review", Locale::EnUs) => {
            "This review is of a fix. The lines below are the verdict that rejected the wave, its \
             previous delivery and the agreed items recorded after the last send. Look only at the \
             fix — what the verdict pointed out —, not the whole wave again."
        }
        ("prompt.part.own_delivered", Locale::PtBr) => "O que esta onda entregou",
        ("prompt.part.own_delivered", Locale::EnUs) => "What this wave delivered",

        // As regras da execução: o que o orquestrador acrescentava à mão.
        ("prompt.part.execution", Locale::PtBr) => "Regras da execução",
        ("prompt.part.execution", Locale::EnUs) => "Execution rules",
        ("prompt.execution.build", Locale::PtBr) => "Compile com `{command}`.",
        ("prompt.execution.build", Locale::EnUs) => "Build with `{command}`.",
        ("prompt.execution.test", Locale::PtBr) => "Teste com `{command}`.",
        ("prompt.execution.test", Locale::EnUs) => "Test with `{command}`.",
        ("prompt.execution.no_commit", Locale::PtBr) => "Não comite e não use `git add`: o commit é da rodada.",
        ("prompt.execution.no_commit", Locale::EnUs) => {
            "Do not commit and do not use `git add`: the commit belongs to the round."
        }
        ("prompt.execution.running", Locale::PtBr) => {
            "Ondas em andamento, cada uma na sua cópia: o arquivo que você dividir com elas é juntado \
             na volta, e o trecho que conflitar para a rodada até ser resolvido."
        }
        ("prompt.execution.running", Locale::EnUs) => {
            "Waves in flight, each in its own copy: a file you share with them is merged on the way \
             back, and a conflicting hunk stops the round until it is resolved."
        }
        ("prompt.execution.wave", Locale::PtBr) => "Onda {n}",
        ("prompt.execution.wave", Locale::EnUs) => "Wave {n}",
        ("prompt.execution.copy", Locale::PtBr) => {
            "Trabalhe só na cópia separada `{copy}`, que a rodada criou no commit atual, e rode cada \
             comando de dentro dela; nunca crie outra. Nada se edita no repositório principal \
             `{root}`, e a cópia fica onde está: na volta, a rodada junta os arquivos entregues, novos \
             e apagados inclusive, e depois do commit a apaga."
        }
        ("prompt.execution.copy", Locale::EnUs) => {
            "Work only in the separate copy `{copy}`, which the round created at the current commit, \
             and run every command from inside it; never create another. Nothing is edited in the \
             main repository `{root}`, and the copy stays where it is: on the way back, the round \
             merges the delivered files, new and deleted ones included, and deletes it after the commit."
        }
        ("prompt.execution.build_dir", Locale::PtBr) => {
            "Compile e teste só na pasta de compilação `{dir}` (no Cargo, `CARGO_TARGET_DIR={dir}`), \
             em primeiro plano: ela é fixa e passa de uma cópia para a seguinte."
        }
        ("prompt.execution.build_dir", Locale::EnUs) => {
            "Build and test only in the build folder `{dir}` (with Cargo, `CARGO_TARGET_DIR={dir}`), in \
             the foreground: it is fixed and passes from one copy to the next."
        }
        ("prompt.execution.root", Locale::PtBr) => {
            "A spec mora no repositório principal: toda leitura dela leva `--root {root}`, como em \
             `mustard-rt run read waves --root {root} --spec …`."
        }
        ("prompt.execution.root", Locale::EnUs) => {
            "The spec lives in the main repository: every read of it takes `--root {root}`, as in \
             `mustard-rt run read waves --root {root} --spec …`."
        }
        ("prompt.review.copy", Locale::PtBr) => {
            "Revise na cópia separada `{copy}`, nunca no repositório principal `{root}`: crie-a no \
             commit da onda com `git worktree add --detach {copy} {commit}` e rode tudo dentro dela."
        }
        ("prompt.review.copy", Locale::EnUs) => {
            "Review in the separate copy `{copy}`, never in the main repository `{root}`: create it at \
             the wave's commit with `git worktree add --detach {copy} {commit}` and run everything \
             inside it."
        }
        ("prompt.review.jobs", Locale::PtBr) => {
            "Compile e teste com menos processos em paralelo que o normal: as ondas compilam ao mesmo \
             tempo que você."
        }
        ("prompt.review.jobs", Locale::EnUs) => {
            "Build and test with fewer parallel jobs than usual: the waves are compiling at the same \
             time as you."
        }
        ("prompt.review.cleanup", Locale::PtBr) => "No fim, apague a cópia com `git worktree remove --force {copy}`.",
        ("prompt.review.cleanup", Locale::EnUs) => "At the end, delete the copy with `git worktree remove --force {copy}`.",
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
            34,
            0x6ea9_a7c8_8043_3820,
        );
    }
}
