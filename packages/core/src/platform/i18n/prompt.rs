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
        ("subagent.wave_not_running", Locale::PtBr) => {
            "[Mustard] O despacho foi barrado: a onda {wave} da spec {spec} não está em andamento. Só \
             sai a onda que a rodada soltou."
        }
        ("subagent.wave_not_running", Locale::EnUs) => {
            "[Mustard] The dispatch was blocked: wave {wave} of the spec {spec} is not in progress. \
             Only a wave the round sent out goes."
        }
        // O texto do condutor trocado pelo que o Mustard monta: o recado diz
        // o que saiu.
        ("subagent.dispatch_replaced", Locale::PtBr) => {
            "[Mustard] O despacho da onda {wave} saiu só com o título e o comando de leitura. O texto a \
             mais foi tirado. O que a onda precisa saber se grava na spec, e o pedido dela já traz."
        }
        ("subagent.dispatch_replaced", Locale::EnUs) => {
            "[Mustard] The dispatch of wave {wave} went out with only the title and the read command. \
             The extra text was taken out. What the wave needs to know is recorded in the spec, and its \
             request already carries it."
        }
        ("subagent.fix_replaced", Locale::PtBr) => {
            "[Mustard] A mensagem ao agente da onda {wave} saiu só com o que a rodada devolveu para ela. \
             O texto a mais foi tirado. O que a onda precisa saber se grava na spec."
        }
        ("subagent.fix_replaced", Locale::EnUs) => {
            "[Mustard] The message to the agent of wave {wave} went out with only what the round sent \
             back for it. The extra text was taken out. What the wave needs to know is recorded in the spec."
        }
        // A onda recusada cujo agente se foi com o Claude Code que a mandou:
        // a frase que abre o trecho de conserto ao agente novo e o recado ao
        // condutor.
        ("subagent.new_agent_fix", Locale::PtBr) => {
            "[Mustard] Um agente anterior já fez a onda {wave}, e o código dele está na cópia. A rodada \
             recusou a entrega: conserte só o que vem abaixo, teste e entregue de novo."
        }
        ("subagent.new_agent_fix", Locale::EnUs) => {
            "[Mustard] An earlier agent already did wave {wave}, and its code is in the copy. The round \
             refused the delivery: fix only what follows, test, and deliver again."
        }
        ("subagent.new_agent", Locale::PtBr) => {
            "[Mustard] O agente da onda {wave} não existe mais. O despacho saiu para um agente novo, com o \
             título, o comando de leitura e o trecho de conserto."
        }
        ("subagent.new_agent", Locale::EnUs) => {
            "[Mustard] The agent of wave {wave} no longer exists. The dispatch went to a new agent, with \
             the title, the read command and the fix section."
        }

        // O pedido de uma onda: o texto que o agente dela recebe.
        ("prompt.title", Locale::PtBr) => "{spec} — onda {n}",
        ("prompt.title", Locale::EnUs) => "{spec} — wave {n}",
        // O modelo e o esforço em que a onda roda — o que o `mustard.json`
        // declara em `agents.model` e `agents.effort` —, ditos no cabeçalho do
        // próprio pedido, do mesmo jeito que ele já diz a cópia, porque o
        // molde do agente sozinho não bastou: em 20/09 a onda saiu em Opus por
        // herdar o modelo da sessão.
        ("prompt.model.wave", Locale::PtBr) => "Modelo desta onda: {model}. Esforço: {effort}.",
        ("prompt.model.wave", Locale::EnUs) => "This wave's model: {model}. Effort: {effort}.",
        // Os dois idiomas do projeto, lidos do `mustard.json`, no topo de todo
        // pedido a um agente: o dos textos que a pessoa lê e o dos nomes no
        // código. Sem ela, o agente só adivinhava o idioma dos nomes.
        ("prompt.languages", Locale::PtBr) => {
            "Idiomas deste projeto. O texto sai em {text}: comentários, entregas e commits. O código \
             sai em {code}: nomes de variáveis, funções, testes, arquivos, comandos e tabelas do \
             banco. Isso vale mesmo quando o arquivo já traz nomes em outro idioma."
        }
        ("prompt.languages", Locale::EnUs) => {
            "This project's languages. Text is written in {text}: comments, deliveries and commits. \
             Code is written in {code}: names of variables, functions, tests, files, commands and \
             database tables. This holds even when the file already has names in another \
             language."
        }
        // As seções do pedido da onda, sempre nesta ordem: o que ela entrega,
        // como ler cada item, o que fazer, o que obedecer, o que devolver e
        // como trabalhar.
        ("prompt.part.delivers", Locale::PtBr) => "O que esta onda entrega",
        ("prompt.part.delivers", Locale::EnUs) => "What this wave delivers",
        ("prompt.part.read", Locale::PtBr) => "Como ler cada item",
        ("prompt.part.read", Locale::EnUs) => "How to read each item",
        ("prompt.part.do", Locale::PtBr) => "O que fazer",
        ("prompt.part.do", Locale::EnUs) => "What to do",
        ("prompt.part.obey", Locale::PtBr) => "O que obedecer",
        ("prompt.part.obey", Locale::EnUs) => "What to obey",
        ("prompt.part.return", Locale::PtBr) => "O que devolver",
        ("prompt.part.return", Locale::EnUs) => "What to return",
        ("prompt.part.work", Locale::PtBr) => "Como trabalhar",
        ("prompt.part.work", Locale::EnUs) => "How to work",
        // O tipo de cada item, por extenso, no começo da linha dele. O tipo
        // que o pedido não conhece sai como "Item".
        ("prompt.kind.task", Locale::PtBr) => "Tarefa",
        ("prompt.kind.task", Locale::EnUs) => "Task",
        ("prompt.kind.rule", Locale::PtBr) => "Regra",
        ("prompt.kind.rule", Locale::EnUs) => "Rule",
        ("prompt.kind.limit", Locale::PtBr) => "Limite",
        ("prompt.kind.limit", Locale::EnUs) => "Limit",
        ("prompt.kind.contract", Locale::PtBr) => "Contrato",
        ("prompt.kind.contract", Locale::EnUs) => "Contract",
        ("prompt.kind.error", Locale::PtBr) => "Erro",
        ("prompt.kind.error", Locale::EnUs) => "Error",
        ("prompt.kind.edge_case", Locale::PtBr) => "Caso de borda",
        ("prompt.kind.edge_case", Locale::EnUs) => "Edge case",
        ("prompt.kind.out_of_scope", Locale::PtBr) => "Fora do escopo",
        ("prompt.kind.out_of_scope", Locale::EnUs) => "Out of scope",
        ("prompt.kind.decision", Locale::PtBr) => "Decisão",
        ("prompt.kind.decision", Locale::EnUs) => "Decision",
        ("prompt.kind.context", Locale::PtBr) => "Contexto",
        ("prompt.kind.context", Locale::EnUs) => "Context",
        ("prompt.kind.concern", Locale::PtBr) => "Preocupação",
        ("prompt.kind.concern", Locale::EnUs) => "Concern",
        ("prompt.kind.criterion", Locale::PtBr) => "Critério",
        ("prompt.kind.criterion", Locale::EnUs) => "Criterion",
        ("prompt.kind.message", Locale::PtBr) => "Mensagem",
        ("prompt.kind.message", Locale::EnUs) => "Message",
        ("prompt.kind.verdict", Locale::PtBr) => "Veredito",
        ("prompt.kind.verdict", Locale::EnUs) => "Verdict",
        ("prompt.kind.delivered", Locale::PtBr) => "Entrega",
        ("prompt.kind.delivered", Locale::EnUs) => "Delivery",
        ("prompt.kind.commit", Locale::PtBr) => "Commit",
        ("prompt.kind.commit", Locale::EnUs) => "Commit",
        ("prompt.kind.wave", Locale::PtBr) => "Onda",
        ("prompt.kind.wave", Locale::EnUs) => "Wave",
        ("prompt.kind.lesson", Locale::PtBr) => "Lição",
        ("prompt.kind.lesson", Locale::EnUs) => "Lesson",
        ("prompt.kind.item", Locale::PtBr) => "Item",
        ("prompt.kind.item", Locale::EnUs) => "Item",
        // Os passos de "O que fazer". O primeiro manda ler os itens de "O que
        // obedecer"; cada tarefa é um passo, com o que ela atende, os
        // arquivos e o que ler antes embaixo; a suíte e a entrega fecham a
        // lista. `{part}` é o título da seção a que o passo aponta.
        ("prompt.step.read", Locale::PtBr) => "Leia o texto inteiro de cada item de \"{part}\".",
        ("prompt.step.read", Locale::EnUs) => "Read the whole text of each item under \"{part}\".",
        ("prompt.step.task", Locale::PtBr) => "Faça a tarefa {item}",
        ("prompt.step.task", Locale::EnUs) => "Do the task {item}",
        ("prompt.step.attends", Locale::PtBr) => "Atende: {item}",
        ("prompt.step.attends", Locale::EnUs) => "Addresses: {item}",
        ("prompt.step.user_message", Locale::PtBr) => "mensagem do usuário",
        ("prompt.step.user_message", Locale::EnUs) => "user message",
        ("prompt.step.read_task_message", Locale::PtBr) => "Leia a tarefa e a mensagem inteiras antes de mexer.",
        ("prompt.step.read_task_message", Locale::EnUs) => "Read the whole task and the message before you start.",
        ("prompt.step.read_task", Locale::PtBr) => "Leia a tarefa inteira antes de mexer.",
        ("prompt.step.read_task", Locale::EnUs) => "Read the whole task before you start.",
        ("prompt.step.read_task_attends", Locale::PtBr) => "Leia a tarefa e o que ela atende, inteiros, antes de mexer.",
        ("prompt.step.read_task_attends", Locale::EnUs) => "Read the whole task and what it addresses before you start.",
        ("prompt.step.file", Locale::PtBr) => "Arquivo: {files}",
        ("prompt.step.file", Locale::EnUs) => "File: {files}",
        ("prompt.step.files", Locale::PtBr) => "Arquivos: {files}",
        ("prompt.step.files", Locale::EnUs) => "Files: {files}",
        ("prompt.step.read_before", Locale::PtBr) => "Leia antes: {hints}",
        ("prompt.step.read_before", Locale::EnUs) => "Read before: {hints}",
        ("prompt.step.suite", Locale::PtBr) => "Rode a suíte do projeto com `{command}`.",
        ("prompt.step.suite", Locale::EnUs) => "Run the project's suite with `{command}`.",
        ("prompt.step.deliver", Locale::PtBr) => "Grave a entrega, como diz \"{part}\".",
        ("prompt.step.deliver", Locale::EnUs) => "Record the delivery, as \"{part}\" says.",
        // Sem lição para os arquivos da onda, a seção diz isso em vez de
        // calar.
        ("prompt.obey.no_lessons", Locale::PtBr) => "Lições: nenhuma vale para os arquivos desta onda.",
        ("prompt.obey.no_lessons", Locale::EnUs) => "Lessons: none applies to this wave's files.",
        ("prompt.return.loose", Locale::PtBr) => "Fora da entrega, nenhum texto solto.",
        ("prompt.return.loose", Locale::EnUs) => "Outside the delivery, no loose text.",
        // O mapa do projeto conhece os arquivos de teste de um arquivo que a
        // tarefa cita: o passo da tarefa ganha, logo abaixo, quem o testa,
        // para o agente não sair procurando um por um no código.
        ("prompt.task.tested_by", Locale::PtBr) => "Quem testa `{file}`: {tests}",
        ("prompt.task.tested_by", Locale::EnUs) => "Who tests `{file}`: {tests}",
        // O padrão do projeto sob a tarefa que toca um papel com regra: as
        // regras que o código já segue e exemplos que as seguem, sem código.
        ("prompt.pattern.head", Locale::PtBr) => {
            "O padrão do projeto, tirado das importações do código. A importação contra uma regra é \
             recusada na volta da onda; contra um costume, só gera aviso."
        }
        ("prompt.pattern.head", Locale::EnUs) => {
            "The project pattern, taken from the imports in the code. An import against a rule is \
             refused when the wave returns; against a habit, it only raises a warning."
        }
        ("prompt.pattern.rule", Locale::PtBr) => "regra: {from} importa {to} em {along} de {total} importações",
        ("prompt.pattern.rule", Locale::EnUs) => "rule: {from} imports {to} in {along} of {total} imports",
        ("prompt.pattern.info", Locale::PtBr) => "costume: {from} importa {to} em {along} de {total} importações",
        ("prompt.pattern.info", Locale::EnUs) => "habit: {from} imports {to} in {along} of {total} imports",
        ("prompt.pattern.example", Locale::PtBr) => "exemplo: `{name}` em `{path}`, linhas {start} a {end}",
        ("prompt.pattern.example", Locale::EnUs) => "example: `{name}` in `{path}`, lines {start} to {end}",
        // Sem regra de importação, o bloco abre só com o que o código e o git
        // mostram: o arquivo grande e a receita.
        ("prompt.pattern.head_plain", Locale::PtBr) => "O que o projeto mostra sobre esta tarefa, tirado do código e da história do git.",
        ("prompt.pattern.head_plain", Locale::EnUs) => "What the project shows about this task, taken from the code and the git history.",
        // O arquivo da tarefa entre os maiores do projeto: só informa, e pede
        // o código novo num arquivo novo.
        ("prompt.pattern.large", Locale::PtBr) => {
            "Entre os {percent}% maiores arquivos do projeto: {files}. Ponha o código novo num arquivo novo."
        }
        ("prompt.pattern.large", Locale::EnUs) => {
            "Among the {percent}% largest files in the project: {files}. Put the new code in a new file."
        }
        // A receita do git: o que os commits do mesmo trabalho fizeram junto,
        // com a fração de cada coisa.
        ("prompt.pattern.recipe.created", Locale::PtBr) => "Receita do git, de {commits} commits que criaram um arquivo `{kind}`:",
        ("prompt.pattern.recipe.created", Locale::EnUs) => "Git recipe, from {commits} commits that created a `{kind}` file:",
        ("prompt.pattern.recipe.changed", Locale::PtBr) => "Receita do git, de {commits} commits que mudaram `{path}`:",
        ("prompt.pattern.recipe.changed", Locale::EnUs) => "Git recipe, from {commits} commits that changed `{path}`:",
        ("prompt.pattern.recipe.file", Locale::PtBr) => "mudou `{path}` em {count} de {commits}",
        ("prompt.pattern.recipe.file", Locale::EnUs) => "changed `{path}` in {count} of {commits}",
        ("prompt.pattern.recipe.tests", Locale::PtBr) => "criou um teste em {count} de {commits}",
        ("prompt.pattern.recipe.tests", Locale::EnUs) => "created a test in {count} of {commits}",
        // O agente de teste dedicado, que o fechamento pede a toda obra —
        // mesmo a de uma onda só —, no lugar da revisão de cada onda. A
        // parte fixa diz o que o pedido é e o que devolver; o que olhar vem
        // logo depois, e muda na revisão que volta depois de uma reprovação.
        ("prompt.final.title", Locale::PtBr) => "{spec} — agente de teste dedicado",
        ("prompt.final.title", Locale::EnUs) => "{spec} — dedicated test agent",
        ("prompt.final.fixed", Locale::PtBr) => {
            "**O que é isto.** O pedido do agente de teste dedicado desta spec, montado pelo binário a \
             partir dela. Nenhum texto vem copiado: cada parte traz só o tipo, o código e o título \
             de cada item, numa linha por item.\n\n\
             **O que devolver.** O veredito com `\"final\":true`, gravado por `mustard-rt run write \
             verdict`. O pedido traz os requisitos acordados inteiros da spec, dono ou não de onda. \
             Responda por cada item em `agreed`, com o código em `item` e `met` dizendo se está \
             atendido. Quando não estiver, `text` diz o que falta e `files` os arquivos, e eles viram \
             uma tarefa nova. Faltar algum requisito acordado na lista é veredito malformado: nada é \
             gravado."
        }
        ("prompt.final.fixed", Locale::EnUs) => {
            "**What this is.** The dedicated test agent's request for this spec, assembled by the \
             binary from it. No text is copied in: each part carries only the type, the code and \
             the title of each item, on one line per item.\n\n\
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
        // Como ler cada item do pedido da onda. `{root}` é `--root <caminho> `
        // quando o agente trabalha numa cópia, e nada quando não trabalha. O
        // pedido traz só o tipo, o código e o título de cada item; o texto
        // inteiro se lê pelo código, e a lição pelo número dela no banco. A
        // entrega sem a leitura completa é recusada.
        ("prompt.read.wave", Locale::PtBr) => {
            "Este pedido traz só o tipo, o código e o título de cada item. Leia o texto inteiro de \
             cada um antes de usá-lo:\n\
             - tarefa, regra, decisão, mensagem ou outro item: `mustard-rt run read item-<código> \
             {root}--spec {spec}`\n\
             - lição: `mustard-rt run read lessons --term <número> {root}--spec {spec}`\n\
             A entrega é recusada se algum item deste pedido não foi lido. A recusa diz qual."
        }
        ("prompt.read.wave", Locale::EnUs) => {
            "This request carries only the type, the code and the title of each item. Read the whole \
             text of each one before using it:\n\
             - task, rule, decision, message or any other item: `mustard-rt run read item-<item-code> \
             {root}--spec {spec}`\n\
             - lesson: `mustard-rt run read lessons --term <number> {root}--spec {spec}`\n\
             The delivery is refused if any item of this request was not read. The refusal says which."
        }
        // Como ler cada item do pedido do revisor final: os mesmos dois comandos
        // do pedido da onda e o aviso de que o veredito sem a leitura completa
        // é recusado, com a recusa dizendo qual item faltou.
        ("prompt.read.final", Locale::PtBr) => {
            "Este pedido traz só o tipo, o código e o título de cada item. Leia o texto inteiro de \
             cada um antes de dar o veredito:\n\
             - onda, tarefa, requisito acordado, entrega, critério, commit ou outro item: `mustard-rt \
             run read item-<código> {root}--spec {spec}`\n\
             - lição: `mustard-rt run read lessons --term <número> {root}--spec {spec}`\n\
             O veredito é recusado se algum item deste pedido não foi lido. A recusa diz qual."
        }
        ("prompt.read.final", Locale::EnUs) => {
            "This request carries only the type, the code and the title of each item. Read the whole \
             text of each one before you give the verdict:\n\
             - wave, task, agreed requirement, delivery, criterion, commit or any other item: \
             `mustard-rt run read item-<item-code> {root}--spec {spec}`\n\
             - lesson: `mustard-rt run read lessons --term <number> {root}--spec {spec}`\n\
             The verdict is refused if any item of this request was not read. The refusal says which."
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
        ("prompt.skill.stale", Locale::PtBr) => "a revisar",
        ("prompt.skill.stale", Locale::EnUs) => "to review",
        ("prompt.skill.read", Locale::PtBr) => "Leia o arquivo da skill antes de começar a tarefa que a nomeia.",
        ("prompt.skill.read", Locale::EnUs) => "Read the skill file before starting the task that names it.",

        // O conserto: a onda que volta por reprovação.
        ("prompt.fix.wave", Locale::PtBr) => {
            "Esta onda voltou por reprovação. As linhas abaixo são o veredito que reprovou, a entrega \
             anterior desta onda e os requisitos acordados gravados depois do último envio. Leia o texto \
             inteiro de cada uma antes de consertar. Conserte só o que o veredito aponta, à luz desses \
             itens: não refaça a onda."
        }
        ("prompt.fix.wave", Locale::EnUs) => {
            "This wave came back rejected. The lines below are the verdict that rejected it, this \
             wave's previous delivery and the agreed requirements recorded after the last send. Read the \
             whole text of each one before fixing. Fix only what the verdict points out, in light of \
             those items: do not redo the wave."
        }
        // As regras da execução: o que o orquestrador acrescentava à mão.
        ("prompt.part.execution", Locale::PtBr) => "Regras da execução",
        ("prompt.part.execution", Locale::EnUs) => "Execution rules",
        ("prompt.execution.build", Locale::PtBr) => "Compile com `{command}`.",
        ("prompt.execution.build", Locale::EnUs) => "Build with `{command}`.",
        ("prompt.execution.median", Locale::PtBr) => {
            "A mediana das entregas deste projeto é de {median} linhas postas; passar dela pede \
             justificativa na entrega (Fronteira da tarefa)."
        }
        ("prompt.execution.median", Locale::EnUs) => {
            "The median delivery of this project puts {median} lines; going past it needs a reason \
             in the delivery (Task boundary)."
        }
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
             já a criou no commit `{commit}`; rode tudo dentro dela e nunca crie outra."
        }
        ("prompt.review.copy", Locale::EnUs) => {
            "Review in the separate copy `{copy}`, never in the main repository `{root}`: the close \
             already created it at commit `{commit}`; run everything inside it and never create another."
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
        // As regras do projeto, no fim de todo pedido ao revisor.
        ("prompt.part.project_rules", Locale::PtBr) => "Regras do projeto",
        ("prompt.part.project_rules", Locale::EnUs) => "Project rules",
        ("prompt.project_rules.source", Locale::PtBr) => {
            "O texto do `CLAUDE.md` da raiz do projeto. Siga-o nesta revisão."
        }
        ("prompt.project_rules.source", Locale::EnUs) => {
            "The text of the `CLAUDE.md` at the project root. Follow it in this review."
        }
        ("prompt.project_rules.sources", Locale::PtBr) => {
            "Os arquivos de regras da raiz e das pastas onde a obra mexeu, cada um sob o caminho dele. Siga todos \
             nesta revisão."
        }
        ("prompt.project_rules.sources", Locale::EnUs) => {
            "The rules files of the root and of the folders the work touched, each under its path. Follow them all \
             in this review."
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
            100,
            0x53f7_a4e6_8d29_4e8d,
        );
    }

    /// O bloco do padrão sob a tarefa passa na conferência de escrita nos
    /// dois idiomas, com cada lacuna trocada por uma palavra.
    #[test]
    fn the_pattern_block_texts_read_clearly() {
        use crate::platform::i18n::{translate, Locale};
        for lang in [Locale::PtBr, Locale::EnUs] {
            for key in [
                "prompt.pattern.head",
                "prompt.pattern.rule",
                "prompt.pattern.info",
                "prompt.pattern.example",
                "prompt.pattern.head_plain",
                "prompt.pattern.large",
                "prompt.pattern.recipe.created",
                "prompt.pattern.recipe.changed",
                "prompt.pattern.recipe.file",
                "prompt.pattern.recipe.tests",
            ] {
                let text = translate(key, lang)
                    .replace("{from}", "controller")
                    .replace("{to}", "service")
                    .replace("{along}", "24")
                    .replace("{total}", "25")
                    .replace("{name}", "create")
                    .replace("{path}", "src/order.controller.ts")
                    .replace("{start}", "12")
                    .replace("{end}", "30")
                    .replace("{percent}", "5")
                    .replace("{files}", "`src/order.service.ts`")
                    .replace("{commits}", "10")
                    .replace("{count}", "9")
                    .replace("{kind}", "src/orders/*.ts");
                assert!(!text.contains('{'), "{key} {lang:?}: {text}");
                let report = crate::domain::clarity::measure(&text, &[], Some(lang));
                assert!(report.passed, "{key} {lang:?}: {report:?}");
            }
        }
    }

    /// Os textos novos do pedido da onda — a seção de como ler, os passos de
    /// "O que fazer", a frase das lições e a ordem de não deixar texto solto —
    /// passam na conferência de escrita nos dois idiomas, com cada lacuna
    /// trocada por uma palavra.
    #[test]
    fn the_request_step_texts_read_clearly() {
        use crate::platform::i18n::{translate, Locale};
        for lang in [Locale::PtBr, Locale::EnUs] {
            for key in [
                "prompt.read.wave",
                "prompt.read.final",
                "prompt.fix.wave",
                "prompt.step.read",
                "prompt.step.task",
                "prompt.step.attends",
                "prompt.step.read_task",
                "prompt.step.read_task_attends",
                "prompt.step.read_task_message",
                "prompt.step.user_message",
                "prompt.step.file",
                "prompt.step.files",
                "prompt.step.read_before",
                "prompt.step.suite",
                "prompt.step.deliver",
                "prompt.obey.no_lessons",
                "prompt.return.loose",
                "prompt.task.tested_by",
            ] {
                let text = translate(key, lang)
                    .replace("{part}", "part")
                    .replace("{item}", "item")
                    .replace("{files}", "`src/order.service.ts`")
                    .replace("{hints}", "`create`")
                    .replace("{command}", "make check")
                    .replace("{file}", "src/order.service.ts")
                    .replace("{tests}", "`order_test`")
                    .replace("{root}", "")
                    .replace("{spec}", "orders");
                assert!(!text.contains('{'), "{key} {lang:?}: {text}");
                let report = crate::domain::clarity::measure(&text, &[], Some(lang));
                assert!(report.passed, "{key} {lang:?}: {report:?}");
            }
        }
    }
}
