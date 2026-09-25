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
        // do mesmo jeito que ele já diz a cópia —, porque o molde do agente
        // sozinho não bastou: em 20/09 a onda saiu em Opus por herdar o
        // modelo da sessão.
        ("prompt.model.wave", Locale::PtBr) => "Modelo desta onda: Opus.",
        ("prompt.model.wave", Locale::EnUs) => "This wave's model: Opus.",
        // Os dois idiomas do projeto, lidos do `mustard.json`, no topo de todo
        // pedido a um agente: o dos textos que a pessoa lê e o dos nomes no
        // código. Sem ela, o agente só adivinhava o idioma dos nomes.
        ("prompt.languages", Locale::PtBr) => {
            "Idiomas deste projeto. O texto sai em {text}: comentários, entregas e commits. O código \
             sai em {code}: nomes de variáveis, funções, arquivos, comandos e tabelas do banco."
        }
        ("prompt.languages", Locale::EnUs) => {
            "This project's languages. Text is written in {text}: comments, deliveries and commits. \
             Code is written in {code}: names of variables, functions, files, commands and database \
             tables."
        }
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
             binário a partir da spec. Cada item vem numa linha, com o código e o título, e \
             embaixo dela a parte do agente: arquivos, comandos e o que testar.\n\n\
             **O que devolver.** A entrega desta onda, pela ferramenta: `mustard-rt run write \
             delivered`, com o que houver a contar do trabalho dentro do campo de texto dela. Fora \
             dela, nada de texto solto."
        }
        ("prompt.fixed", Locale::EnUs) => {
            "**What this is.** The list of this wave's items, in execution order, assembled by the \
             binary from the spec. Each item opens with a line holding its code and title. Below \
             it comes the agent part: files, commands and what to test.\n\n\
             **What to return.** This wave's delivery, through the tool: `mustard-rt run write \
             delivered`, with whatever there is to tell about the work inside its text field. \
             Outside it, no loose text."
        }
        // O agente de teste dedicado, que o fechamento pede a toda obra —
        // mesmo a de uma onda só —, no lugar da revisão de cada onda. A
        // parte fixa diz o que o pedido é e o que devolver; o que olhar vem
        // logo depois, e muda na revisão que volta depois de uma reprovação.
        ("prompt.final.title", Locale::PtBr) => "{spec} — agente de teste dedicado",
        ("prompt.final.title", Locale::EnUs) => "{spec} — dedicated test agent",
        ("prompt.final.fixed", Locale::PtBr) => {
            "**O que é isto.** O pedido do agente de teste dedicado desta spec, montado pelo binário a \
             partir dela. Nenhum texto vem copiado: cada parte traz só os códigos dos itens, em \
             sequência, numa linha por bloco da spec.\n\n\
             **O que devolver.** O veredito com `\"final\":true`, gravado por `mustard-rt run write \
             verdict`. O pedido traz os requisitos acordados inteiros da spec, dono ou não de onda. \
             Responda por cada item em `agreed`, com o código em `item` e `met` dizendo se está \
             atendido. Quando não estiver, `text` diz o que falta e `files` os arquivos, e eles viram \
             uma tarefa nova. Faltar algum requisito acordado na lista é veredito malformado: nada é \
             gravado."
        }
        ("prompt.final.fixed", Locale::EnUs) => {
            "**What this is.** The dedicated test agent's request for this spec, assembled by the \
             binary from it. No text is copied in: each part carries only the items' codes, in \
             sequence, one line per spec block.\n\n\
             **What to return.** The verdict with `\"final\":true`, recorded through `mustard-rt run \
             write verdict`. The request carries the spec's whole agreed requirements, owned by a \
             wave or not. Answer for each item in `agreed`, with the code in `item` and `met` \
             saying whether it is satisfied. When it is not, `text` says what is missing and `files` \
             the files, and they become a new task. Missing any agreed requirement from the list is \
             a malformed verdict: nothing gets recorded."
        }
        // A primeira revisão da obra confere tudo.
        ("prompt.final.look", Locale::PtBr) => {
            "**O que olhar.** As entregas, os critérios, as mudanças da branch, as emendas gravadas \
             entre as ondas e o que cada onda deixou aberto. Olhe como as ondas se encaixam: código \
             repetido entre ondas, decisão de uma que contradiz a de outra, verificação que uma \
             apagou da outra. Aponte só; não conserte."
        }
        ("prompt.final.look", Locale::EnUs) => {
            "**What to look at.** The deliveries, the criteria, the branch changes, the amendments \
             recorded between waves and what each wave left open. Look at how the waves fit \
             together: code repeated across them, a decision of one that contradicts another's, a \
             verification one erased from another. Point it out only; do not fix it."
        }
        // A revisão que volta depois de uma reprovação confere só o que mudou
        // e o encaixe disso no resto. Para o que não mudou, vale a conclusão
        // anterior; o veredito continua respondendo por todo o combinado. É o
        // único lugar que diz o recorte: quando o veredito reprovou uma onda,
        // as ondas e as entregas do pedido trazem só ela.
        ("prompt.final.look_again", Locale::PtBr) => {
            "**O que olhar.** Esta revisão volta depois de um veredito que reprovou. A parte do que \
             mudou lista esse veredito, os commits e os itens gravados ou regravados depois dele. Ela \
             lista também os itens que ele deu como não atendidos. Se ele reprovou uma onda, as partes \
             das ondas e das entregas trazem só essa onda; senão, trazem todas as ondas. Confira essa \
             parte e como ela se encaixa no resto da obra. Para os outros requisitos acordados, repita \
             a conclusão do veredito anterior. Aponte só; não conserte."
        }
        ("prompt.final.look_again", Locale::EnUs) => {
            "**What to look at.** This review comes back after a verdict that rejected the work. The \
             part on what changed lists that verdict, the commits and the items recorded or \
             re-recorded after it. It also lists the items that verdict marked as not met. If it \
             rejected one wave, the waves and deliveries parts carry only that wave; otherwise, they \
             carry every wave. Check that part and how it fits the rest of the work. For the other \
             agreed requirements, repeat the previous verdict's conclusion. Point it out only; do not \
             fix it."
        }
        ("prompt.part.since_verdict", Locale::PtBr) => "O que mudou desde o veredito anterior",
        ("prompt.part.since_verdict", Locale::EnUs) => "What changed since the previous verdict",
        // Como ler, logo depois da parte fixa. `{root}` é `--root <caminho> `
        // quando o agente trabalha numa cópia, e nada quando não trabalha.
        // O pedido de uma onda já traz o título e a parte do agente de cada
        // item: o texto completo de tudo o que ele lista, com o número da
        // onda em `{n}`, é para o caso de dúvida, e um item só se lê pelo
        // código. O da revisão final lê item por item.
        ("prompt.read.wave", Locale::PtBr) => {
            "**Como ler.** Em caso de dúvida, leia o texto completo com `mustard-rt run read dispatch-{n} \
             {root}--spec {spec}`. Para ler um item só, use `mustard-rt run read <bloco> {root}--spec \
             {spec} --term <código>`."
        }
        ("prompt.read.wave", Locale::EnUs) => {
            "**How to read.** In case of doubt, read the whole text with `mustard-rt run read \
             dispatch-{n} {root}--spec {spec}`. To read a single item, use `mustard-rt run read <block> \
             {root}--spec {spec} --term <item-code>`."
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
        // As regras da execução: o que o orquestrador acrescentava à mão.
        ("prompt.part.execution", Locale::PtBr) => "Regras da execução",
        ("prompt.part.execution", Locale::EnUs) => "Execution rules",
        ("prompt.execution.build", Locale::PtBr) => "Compile com `{command}`.",
        ("prompt.execution.build", Locale::EnUs) => "Build with `{command}`.",
        ("prompt.execution.test", Locale::PtBr) => "Teste com `{command}`.",
        ("prompt.execution.test", Locale::EnUs) => "Test with `{command}`.",
        ("prompt.execution.running", Locale::PtBr) => {
            "Ondas em andamento, cada uma na sua cópia. O arquivo que você dividir com elas é \
             juntado na volta; o trecho que conflitar para a rodada até ser resolvido."
        }
        ("prompt.execution.running", Locale::EnUs) => {
            "Waves in flight, each in its own copy. A file you share with them is merged on the way \
             back; a conflicting hunk stops the round until it is resolved."
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
        // A cópia é a vaga fixa da onda: depois do commit ela fica, com a
        // compilação dentro, e a próxima onda que cair nela só refaz o que
        // o git mudou.
        ("prompt.execution.copy", Locale::PtBr) => {
            "Trabalhe só na cópia separada `{copy}`, que a rodada preparou no commit atual, e rode \
             cada comando de dentro dela; nunca crie outra. Nada se edita no repositório principal \
             `{root}`, e a cópia fica onde está. Na volta, a rodada junta os arquivos entregues, \
             novos e apagados inclusive. Depois do commit, a cópia fica para a próxima onda, com a \
             compilação dentro dela."
        }
        ("prompt.execution.copy", Locale::EnUs) => {
            "Work only in the separate copy `{copy}`, which the round prepared at the current commit, \
             and run every command from inside it; never create another. Nothing is edited in the \
             main repository `{root}`, and the copy stays where it is. On the way back, the round \
             merges the delivered files, new and deleted ones included. After the commit, the copy \
             stays for the next wave, with the build inside it."
        }
        // O preparo que o projeto declara traz à cópia as dependências que o
        // git não leva. Ele pode mexer num arquivo comitado, como o lockfile,
        // e essa mudança não é trabalho da onda: volta ao commit, a não ser
        // que a tarefa declare o arquivo. É a frase do revisor, que não sabe
        // o que mudou na cópia dele.
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
        // A cópia nova da onda não tem preparo nenhum.
        ("prompt.execution.prepare_new", Locale::PtBr) => {
            "Esta cópia é nova: antes de compilar, rode `{command}` dentro dela, que é o preparo que o \
             projeto declara. O arquivo versionado que ele mudar, como o lockfile, volta ao commit com \
             `git checkout -- <arquivo>`, salvo o que a tarefa declara."
        }
        ("prompt.execution.prepare_new", Locale::EnUs) => {
            "This copy is new: before building, run `{command}` inside it, which is the preparation the \
             project declares. A versioned file it changes, such as the lockfile, goes back to the \
             commit with `git checkout -- <file>`, unless the task declares it."
        }
        // A cópia reaproveitada guarda o preparo de quem a usou antes. Quem
        // julga se um arquivo da lista declara dependências é o agente: o
        // binário não sabe isso de linguagem nenhuma.
        ("prompt.execution.prepare_reused", Locale::PtBr) => {
            "Esta cópia já foi usada e guarda o preparo anterior. Desde então, mudaram {files}. Rode \
             `{command}` dentro dela só se um desses arquivos declara dependências. O arquivo \
             versionado que ele mudar volta ao commit com `git checkout -- <arquivo>`, salvo o que a \
             tarefa declara."
        }
        ("prompt.execution.prepare_reused", Locale::EnUs) => {
            "This copy was already used and keeps the earlier preparation. Since then, these changed: \
             {files}. Run `{command}` inside it only if one of these files declares dependencies. A \
             versioned file it changes goes back to the commit with `git checkout -- <file>`, unless \
             the task declares it."
        }
        ("prompt.execution.prepare_same", Locale::PtBr) => {
            "Esta cópia já foi usada e guarda o preparo anterior. Nenhum arquivo mudou desde então: \
             não rode `{command}` de novo."
        }
        ("prompt.execution.prepare_same", Locale::EnUs) => {
            "This copy was already used and keeps the earlier preparation. No file changed since then: \
             do not run `{command}` again."
        }
        // O fim da lista longa dos arquivos mudados, com o comando que a
        // mostra inteira.
        ("prompt.execution.prepare_more", Locale::PtBr) => "e mais {n}, que `{diff}` lista",
        ("prompt.execution.prepare_more", Locale::EnUs) => "and {n} more, listed by `{diff}`",
        // O agente nunca comita: quem junta a cópia ao repositório principal
        // e faz o commit é a rodada. Precisa dizer isso com todas as letras,
        // porque em 22/09/2026 dois agentes comitaram dentro da cópia e
        // devolveram o código do commit no campo do título, e a rodada
        // recusou dizendo que não havia nada para comitar.
        ("prompt.execution.no_commit", Locale::PtBr) => {
            "O trabalho fica mudado só na cópia, sem `git add` e sem `git commit`. Quem junta as \
             cópias no repositório principal e comita é a rodada."
        }
        ("prompt.execution.no_commit", Locale::EnUs) => {
            "The work stays changed only in the copy, without `git add` and without `git commit`. \
             The round merges the copies into the main repository and commits."
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
            "A entrega vai para a spec por `mustard-rt run write delivered`. A rodada não lê a \
             última mensagem, e o relato do trabalho mora no campo de texto da entrega."
        }
        ("prompt.execution.report_lines", Locale::EnUs) => {
            "The delivery goes into the spec through `mustard-rt run write delivered`. The round \
             does not read the last message, and the account of the work lives in the delivery's \
             text field."
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
            "Os arquivos locais que o git ignora e a cópia precisa são {files}. O que faltar nela, \
             copie do repositório principal `{root}` pelo conteúdo, nunca por link ou atalho."
        }
        ("prompt.review.local_files", Locale::EnUs) => {
            "The local files git ignores and the copy needs are {files}. Whatever is missing from \
             it, copy from the main repository `{root}` by content, never by link or shortcut."
        }
        // O fechamento roda a suíte e os critérios antes de pedir a revisão,
        // no mesmo commit em que a cópia do revisor nasce. O revisor não a
        // repete: roda só o que cerca os cortes que ele faz.
        ("prompt.review.suite", Locale::PtBr) => {
            "A suíte `{command}` e os critérios já passaram no fechamento, no commit `{commit}` desta \
             cópia. Não rode a suíte inteira de novo: rode só os testes em volta de cada corte, com \
             esse comando filtrado."
        }
        ("prompt.review.suite", Locale::EnUs) => {
            "The suite `{command}` and the criteria already passed at the close, at commit `{commit}` \
             of this copy. Do not run the whole suite again: run only the tests around each cut, \
             with that command filtered."
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
            49,
            0xf67b_7d1b_0bdf_aae8,
        );
    }
}
