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
        // O modelo em que a onda roda, dito no cabeçalho do próprio pedido —
        // do mesmo jeito que ele já diz a cópia e a pasta de compilação —,
        // porque o molde do agente sozinho não bastou: em 20/09 a onda saiu
        // em Opus por herdar o modelo da sessão.
        ("prompt.model.wave", Locale::PtBr) => "Modelo desta onda: Opus.",
        ("prompt.model.wave", Locale::EnUs) => "This wave's model: Opus.",
        ("prompt.part.items", Locale::PtBr) => "Itens da onda",
        ("prompt.part.items", Locale::EnUs) => "Wave items",
        ("prompt.part.tasks", Locale::PtBr) => "Tarefas, na ordem em que se faz",
        ("prompt.part.tasks", Locale::EnUs) => "Tasks, in the order they are done",
        ("prompt.task.read_before", Locale::PtBr) => "leia antes",
        ("prompt.task.read_before", Locale::EnUs) => "read before",
        // O mapa do projeto conhece os arquivos de teste de um arquivo que a
        // tarefa cita: a linha do arquivo ganha, logo abaixo, quem o testa,
        // para o agente não sair procurando um por um no código.
        ("prompt.task.tested_by", Locale::PtBr) => "quem testa `{file}`: {tests}",
        ("prompt.task.tested_by", Locale::EnUs) => "who tests `{file}`: {tests}",
        ("prompt.fixed", Locale::PtBr) => {
            "**O que é isto.** A lista dos itens desta onda, em ordem de execução, montada pelo \
             binário a partir da spec. Nenhum texto vem copiado: cada parte traz só os códigos dos \
             itens, em sequência, numa linha por bloco da spec.\n\n\
             **O que devolver.** A entrega desta onda, pela ferramenta: `mustard-rt run write \
             delivered`, com o que houver a contar do trabalho dentro do campo de texto dela. Fora \
             dela, nada de texto solto."
        }
        ("prompt.fixed", Locale::EnUs) => {
            "**What this is.** The list of this wave's items, in execution order, assembled by the \
             binary from the spec. No text is copied in: each part carries only the items' codes, in \
             sequence, one line per spec block.\n\n\
             **What to return.** This wave's delivery, through the tool: `mustard-rt run write \
             delivered`, with whatever there is to tell about the work inside its text field. \
             Outside it, no loose text."
        }
        // O agente de teste dedicado, que o fechamento pede a toda obra —
        // mesmo a de uma onda só —, no lugar da revisão de cada onda.
        ("prompt.final.title", Locale::PtBr) => "{spec} — agente de teste dedicado",
        ("prompt.final.title", Locale::EnUs) => "{spec} — dedicated test agent",
        ("prompt.final.fixed", Locale::PtBr) => {
            "**O que é isto.** O pedido do agente de teste dedicado desta spec, com a obra inteira, \
             montado pelo binário a partir dela. Nenhum texto vem copiado: cada parte traz só os \
             códigos dos itens, em sequência, numa linha por bloco da spec.\n\n\
             **O que olhar.** As entregas, os critérios, as mudanças da branch, as emendas gravadas \
             entre as ondas e o que cada onda deixou aberto — como as ondas se encaixam, código \
             repetido entre ondas, decisão de uma que contradiz a de outra, verificação que uma apagou da \
             outra. Aponte só; não conserte.\n\n\
             **O que devolver.** O veredito com `\"final\":true`, gravado por `mustard-rt run write \
             verdict`. O pedido traz os requisitos \
             acordados inteiros da spec, dono ou não de onda: responda por cada item em `agreed`, com o código \
             em `item` e `met` dizendo se está atendido; quando não estiver, `text` diz o que falta \
             e `files` os arquivos, e viram uma tarefa nova. Faltar algum requisito acordado na lista \
             é veredito malformado: nada é gravado."
        }
        ("prompt.final.fixed", Locale::EnUs) => {
            "**What this is.** The dedicated test agent's request for this spec, with the whole \
             work, assembled by the binary from it. No text is copied in: each part carries only \
             the items' codes, in sequence, one line per spec block.\n\n\
             **What to look at.** The deliveries, the criteria, the branch changes, the amendments \
             recorded between waves and what each wave left open — how the waves fit together, code \
             repeated across them, a decision of one that contradicts another's, a verification one erased \
             from another. Point it out only; do not fix it.\n\n\
             **What to return.** The verdict with `\"final\":true`, recorded through `mustard-rt run \
             write verdict`. The request carries \
             the spec's whole agreed requirements, owned by a wave or not: answer for each item in `agreed`, \
             with the code in `item` and `met` saying whether it is satisfied; when it is not, \
             `text` says what is missing and `files` the files, and they become a new task. Missing \
             any agreed requirement from the list is a malformed verdict: nothing gets recorded."
        }
        // Como ler, logo depois da parte fixa. `{root}` é `--root <caminho> `
        // quando o agente trabalha numa cópia, e nada quando não trabalha.
        // O pedido de uma onda manda ler tudo o que ele lista de uma vez, com
        // o número da onda em `{n}`, e deixa a leitura por código para o item
        // que um texto cita e não veio; o da revisão final lê item por item.
        ("prompt.read.wave", Locale::PtBr) => {
            "**Como ler.** Antes de começar, leia o pedido inteiro com `mustard-rt run read dispatch-{n} \
             {root}--spec {spec}`. O item que um texto cita e não veio, leia com `mustard-rt run read \
             <bloco> {root}--spec {spec} --term <código>`."
        }
        ("prompt.read.wave", Locale::EnUs) => {
            "**How to read.** Before you start, read the whole request with `mustard-rt run read \
             dispatch-{n} {root}--spec {spec}`. For an item a text cites that did not come, read it with \
             `mustard-rt run read <block> {root}--spec {spec} --term <item-code>`."
        }
        ("prompt.read", Locale::PtBr) => {
            "**Como ler.** Leia cada código na ordem com `mustard-rt run read <bloco> {root}--spec {spec} \
             --term <código>`, trocando `<bloco>` pelo bloco que abre a linha do código."
        }
        ("prompt.read", Locale::EnUs) => {
            "**How to read.** Read each code in order with `mustard-rt run read <block> {root}--spec {spec} \
             --term <item-code>`, replacing `<block>` with the block that opens the code's line."
        }
        ("prompt.part.waves", Locale::PtBr) => "As ondas e as tarefas delas",
        ("prompt.part.waves", Locale::EnUs) => "The waves and their tasks",
        ("prompt.part.each_delivered", Locale::PtBr) => "O que cada onda entregou",
        ("prompt.part.each_delivered", Locale::EnUs) => "What each wave delivered",
        ("prompt.part.branch_changes", Locale::PtBr) => "Mudanças já na branch",
        ("prompt.part.branch_changes", Locale::EnUs) => "Changes already on the branch",
        ("prompt.part.agreed", Locale::PtBr) => "Requisitos acordados",
        ("prompt.part.agreed", Locale::EnUs) => "Agreed requirements",
        ("prompt.part.criteria", Locale::PtBr) => "Critérios",
        ("prompt.part.criteria", Locale::EnUs) => "Criteria",
        ("prompt.part.delivered", Locale::PtBr) => "O que as ondas anteriores entregaram",
        ("prompt.part.delivered", Locale::EnUs) => "What the earlier waves delivered",
        ("prompt.skill.stale", Locale::PtBr) => "a revisar",
        ("prompt.skill.stale", Locale::EnUs) => "to review",
        ("prompt.skill.read", Locale::PtBr) => "Leia o arquivo da skill antes de começar a tarefa que a nomeia.",
        ("prompt.skill.read", Locale::EnUs) => "Read the skill file before starting the task that names it.",

        // O conserto: a onda que volta por reprovação.
        ("prompt.part.fix", Locale::PtBr) => "Conserto",
        ("prompt.part.fix", Locale::EnUs) => "Fix",
        ("prompt.fix.wave", Locale::PtBr) => {
            "Esta onda voltou por reprovação. As linhas abaixo são o veredito que reprovou, a entrega \
             anterior desta onda e os requisitos acordados gravados depois do último envio. Conserte só o \
             que o veredito aponta, à luz desses itens: não refaça a onda."
        }
        ("prompt.fix.wave", Locale::EnUs) => {
            "This wave came back rejected. The lines below are the verdict that rejected it, this \
             wave's previous delivery and the agreed requirements recorded after the last send. Fix only what \
             the verdict points out, in light of those items: do not redo the wave."
        }
        ("prompt.fix.final", Locale::PtBr) => {
            "Esta é a volta do conserto. A linha abaixo é o veredito que reprovou; as ondas, as \
             emendas e as entregas que o resto do pedido traz já estão restritas a quem foi \
             reprovado. Confira só o conserto — o que o veredito apontou —, não a obra inteira de \
             novo."
        }
        ("prompt.fix.final", Locale::EnUs) => {
            "This is the fix round. The line below is the verdict that rejected it; the waves, the \
             amendments and the deliveries the rest of the request carries are already restricted to \
             whoever was rejected. Check only the fix — what the verdict pointed out —, not the \
             whole work again."
        }
        // As regras da execução: o que o orquestrador acrescentava à mão.
        ("prompt.part.execution", Locale::PtBr) => "Regras da execução",
        ("prompt.part.execution", Locale::EnUs) => "Execution rules",
        ("prompt.execution.build", Locale::PtBr) => "Compile com `{command}`.",
        ("prompt.execution.build", Locale::EnUs) => "Build with `{command}`.",
        ("prompt.execution.test", Locale::PtBr) => "Teste com `{command}`.",
        ("prompt.execution.test", Locale::EnUs) => "Test with `{command}`.",
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
        // A leitura obrigatória de uma tarefa, quando ela aponta uma
        // declaração (`caminho#declaração`), função, estrutura ou constante:
        // o pedido manda ler só aquela declaração, não o arquivo inteiro, e
        // nunca a chama de função quando ela não é.
        ("prompt.task_read.function", Locale::PtBr) => "leia só `{function}` em `{path}`",
        ("prompt.task_read.function", Locale::EnUs) => "read only `{function}` in `{path}`",
        // A mesma leitura obrigatória, quando o mapa do projeto conhece a
        // declaração e a linha em que ela termina: o pedido já manda ler só
        // as linhas atuais dela, sem número que envelhece no plano.
        ("prompt.task_read.function_lines", Locale::PtBr) => {
            "leia só as linhas {lines} de `{function}` em `{path}`"
        }
        ("prompt.task_read.function_lines", Locale::EnUs) => {
            "read only lines {lines} of `{function}` in `{path}`"
        }
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
        // O preparo que o projeto declara traz à cópia as dependências que o
        // git não leva. Ele pode mexer num arquivo comitado, como o lockfile,
        // e essa mudança não é trabalho da onda: volta ao commit, a não ser
        // que a tarefa declare o arquivo.
        ("prompt.execution.prepare", Locale::PtBr) => {
            "Antes de compilar, rode `{command}` dentro da cópia: é o preparo que o projeto declara. O \
             arquivo versionado que ele mudar, como o lockfile, volta ao commit com `git checkout -- \
             <arquivo>`, salvo o que a tarefa declara."
        }
        ("prompt.execution.prepare", Locale::EnUs) => {
            "Before building, run `{command}` inside the copy: it is the preparation the project \
             declares. A versioned file it changes, such as the lockfile, goes back to the commit with \
             `git checkout -- <file>`, unless the task declares it."
        }
        // O agente nunca comita: quem junta a cópia ao repositório principal
        // e faz o commit é a rodada. Precisa dizer isso com todas as letras,
        // porque em 22/09/2026 dois agentes comitaram dentro da cópia e
        // devolveram o código do commit no campo do título, e a rodada
        // recusou dizendo que não havia nada para comitar.
        ("prompt.execution.no_commit", Locale::PtBr) => {
            "O trabalho fica mudado só na cópia, sem `git add` e sem `git commit`: quem junta as \
             cópias no repositório principal e comita é a rodada."
        }
        ("prompt.execution.no_commit", Locale::EnUs) => {
            "The work stays changed only in the copy, without `git add` and without `git commit`: \
             the round is the one that merges the copies into the main repository and commits."
        }
        ("prompt.execution.commit_field", Locale::PtBr) => {
            "Na entrega gravada, o campo `commit` é o título da mensagem, em palavras e curto — \
             nunca o código do commit."
        }
        ("prompt.execution.commit_field", Locale::EnUs) => {
            "In the recorded delivery, the `commit` field is the message's title, in words and short \
             — never the commit's code."
        }
        // A entrega, ao lado da regra de não comitar: ela vai para a spec pela
        // ferramenta, e quem despacha manda gravar de novo quando ela faltar,
        // em vez de montá-la a partir de uma prosa que não fica registrada em
        // lugar nenhum.
        ("prompt.execution.report_lines", Locale::PtBr) => {
            "A entrega vai para a spec por `mustard-rt run write delivered`: a rodada não lê a \
             última mensagem, e o relato do trabalho mora no campo de texto da entrega."
        }
        ("prompt.execution.report_lines", Locale::EnUs) => {
            "The delivery goes into the spec through `mustard-rt run write delivered`: the round \
             does not read the last message, and the account of the work lives in the delivery's \
             text field."
        }
        // A pasta de compilação da cópia. A frase cita o Cargo, então só vai
        // ao pedido quando o mapa do projeto tem uma parte `cargo`; a pasta é
        // escolhida para toda onda, porque é a vaga das ondas que rodam juntas.
        ("prompt.execution.build_dir", Locale::PtBr) => {
            "Compile e teste só na pasta de compilação `{dir}` (no Cargo, `CARGO_TARGET_DIR={dir}`), \
             em primeiro plano: ela é fixa e passa de uma cópia para a seguinte."
        }
        ("prompt.execution.build_dir", Locale::EnUs) => {
            "Build and test only in the build folder `{dir}` (with Cargo, `CARGO_TARGET_DIR={dir}`), in \
             the foreground: it is fixed and passes from one copy to the next."
        }
        ("prompt.review.copy", Locale::PtBr) => {
            "Revise na cópia separada `{copy}`, nunca no repositório principal `{root}`: o fechamento \
             já a criou no commit `{commit}`; rode tudo dentro dela."
        }
        ("prompt.review.copy", Locale::EnUs) => {
            "Review in the separate copy `{copy}`, never in the main repository `{root}`: the close \
             already created it at commit `{commit}`; run everything inside it."
        }
        // Os arquivos locais que o git ignora não vêm com a cópia: o
        // fechamento os copia para ela, e o revisor copia de novo, pelo
        // conteúdo, o que faltar — nunca por link, que deixaria a cópia
        // escrever no repositório principal.
        ("prompt.review.local_files", Locale::PtBr) => {
            "Os arquivos locais que o git ignora e a cópia precisa são {files}: o que faltar nela, \
             copie do repositório principal `{root}` pelo conteúdo, nunca por link ou atalho."
        }
        ("prompt.review.local_files", Locale::EnUs) => {
            "The local files git ignores and the copy needs are {files}: whatever is missing from it, \
             copy from the main repository `{root}` by content, never by link or shortcut."
        }
        ("prompt.review.jobs", Locale::PtBr) => {
            "Compile e teste com menos processos em paralelo que o normal: as ondas compilam ao mesmo \
             tempo que você."
        }
        ("prompt.review.jobs", Locale::EnUs) => {
            "Build and test with fewer parallel jobs than usual: the waves are compiling at the same \
             time as you."
        }
        ("prompt.review.cleanup", Locale::PtBr) => {
            "Desfaça cada corte antes de gravar o veredito: o fechamento seguinte recusa começar sobre \
             `{copy}` com mudança, e apaga a cópia sozinho quando a obra fecha."
        }
        ("prompt.review.cleanup", Locale::EnUs) => {
            "Undo every cut before recording your verdict: the next close refuses to start on `{copy}` \
             with changes, and deletes the copy itself when the work closes."
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
            include_str!("prompt.rs"),
            super::PREFIXES,
            43,
            0xe926_46d6_ca06_dd4d,
        );
    }
}
