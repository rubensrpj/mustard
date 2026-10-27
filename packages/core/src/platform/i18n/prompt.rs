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
        ("subagent.wave_not_running", Locale::PtBr) => {
            "[Mustard] O despacho foi barrado: a onda {wave} da spec {spec} não está em andamento. Só \
             sai a onda que a rodada soltou."
        }
        ("subagent.wave_not_running", Locale::EnUs) => {
            "[Mustard] The dispatch was blocked: wave {wave} of the spec {spec} is not in progress. \
             Only a wave the round sent out goes."
        }
        // A onda em andamento que já tem agente: um segundo agente na mesma
        // cópia desfaria o trabalho do primeiro.
        ("subagent.wave_has_agent", Locale::PtBr) => {
            "[Mustard] O despacho foi barrado: a onda {wave} da spec {spec} já tem um agente trabalhando. \
             Espere a volta dele; a rodada avisa se ele ficar calado."
        }
        ("subagent.wave_has_agent", Locale::EnUs) => {
            "[Mustard] The dispatch was blocked: wave {wave} of the spec {spec} already has an agent working. \
             Wait for it to return; the round warns if it goes silent."
        }
        // O texto do condutor trocado pelo que o Mustard monta: o recado diz
        // o que saiu.
        ("subagent.dispatch_replaced", Locale::PtBr) => {
            "[Mustard] O despacho da onda {wave} saiu só com o título e o comando de leitura. Leia o pedido completo \
             gravado na spec; instruções adicionais são preservadas nele antes do envio."
        }
        ("subagent.dispatch_replaced", Locale::EnUs) => {
            "[Mustard] The dispatch of wave {wave} went out with only the title and the read command. \
             Read the complete request recorded in the spec; additional instructions are \
             preserved there before sending."
        }
        ("subagent.fix_replaced", Locale::PtBr) => {
            "[Mustard] A mensagem ao agente da onda {wave} inclui o conserto da rodada e suas instruções \
             adicionais, preservadas no pedido. Elas não substituem as regras do projeto."
        }
        ("subagent.fix_replaced", Locale::EnUs) => {
            "[Mustard] The message to wave {wave} includes the round repair and your additional instructions, \
             preserved in the request. They do not override project rules."
        }
        // A onda recusada que espera um agente novo: a frase que abre o
        // trecho de conserto ao agente novo e o recado ao condutor. Elas valem
        // para a volta que a rodada recusou, com o Claude Code que mandou a
        // onda já fechado, e para a que o condutor reprovou, com esse Claude
        // Code aberto ou fechado. Por isso nenhuma diz quem recusou, nem que o
        // agente anterior sumiu: só que ele terminou. O motivo vem no trecho
        // de conserto.
        ("subagent.new_agent_fix", Locale::PtBr) => {
            "[Mustard] Um agente anterior já fez a onda {wave}, e o código dele está na cópia. A entrega \
             dele foi recusada: conserte só o que vem abaixo, teste e entregue de novo."
        }
        ("subagent.new_agent_fix", Locale::EnUs) => {
            "[Mustard] An earlier agent already did wave {wave}, and its code is in the copy. Its delivery \
             was refused: fix only what follows, test, and deliver again."
        }
        ("subagent.new_agent", Locale::PtBr) => {
            "[Mustard] A entrega da onda {wave} foi recusada, e o agente que a fez já terminou. O despacho \
             saiu para um agente novo, com o título, o comando de leitura e o trecho de conserto."
        }
        ("subagent.new_agent", Locale::EnUs) => {
            "[Mustard] Wave {wave}'s delivery was refused, and the agent that did it has already finished. \
             The dispatch went to a new agent, with the title, the read command and the fix section."
        }
        // A mensagem ao agente que a reprovação de quem conduz tirou da onda:
        // ela é barrada, e o motivo vai só ao agente novo. Com a onda à espera
        // dele, a frase diz o título que o despacha; com ele já despachado,
        // que ele recebeu o motivo.
        ("subagent.rejected_agent", Locale::PtBr) => {
            "[Mustard] A mensagem foi barrada: você reprovou a volta da onda {wave}, e o agente que a \
             fez saiu da onda. Uma mensagem a ele poria dois agentes na mesma cópia. Despache um agente \
             novo com o título `{title}`: o Mustard junta o pedido e o seu motivo."
        }
        ("subagent.rejected_agent", Locale::EnUs) => {
            "[Mustard] The message was blocked: you rejected wave {wave}'s return, and the agent that \
             did it left the wave. A message to it would put two agents in the same copy. Dispatch a new \
             agent with the title `{title}`: Mustard adds its request and your reason."
        }
        ("subagent.rejected_agent_working", Locale::PtBr) => {
            "[Mustard] A mensagem foi barrada: você reprovou a volta da onda {wave}, e o agente que a \
             fez saiu da onda. Uma mensagem a ele poria dois agentes na mesma cópia. O agente novo da \
             onda já recebeu o seu motivo no despacho."
        }
        ("subagent.rejected_agent_working", Locale::EnUs) => {
            "[Mustard] The message was blocked: you rejected wave {wave}'s return, and the agent that \
             did it left the wave. A message to it would put two agents in the same copy. The wave's new \
             agent already got your reason in its dispatch."
        }
        // A recusa a toda chamada do próprio agente que a reprovação tirou
        // da onda, ler e gravar na spec inclusive.
        ("subagent.rejected_agent_locked", Locale::PtBr) => {
            "[Mustard] A chamada foi barrada: quem conduz a obra reprovou a volta da onda {wave}, e a \
             onda saiu de você. O conserto é de um agente novo. Só passa a devolução do relatório a \
             quem conduz. Nenhuma outra chamada sua passa, nem ler ou gravar na spec: uma entrega sua \
             desfaria a reprovação. Pare aqui e termine a resposta."
        }
        ("subagent.rejected_agent_locked", Locale::EnUs) => {
            "[Mustard] The call was blocked: whoever leads the work rejected wave {wave}'s return, and \
             the wave left you. A new agent does the fix. Only handing the report back to whoever \
             leads passes. No other call of yours does, not even reading or writing the spec: a \
             delivery from you would undo the rejection. Stop here and end your reply."
        }
        // O lançamento do agente de onda ou do revisor que pede modelo ou
        // esforço diferente do que o `mustard.json` declara para ele: aviso,
        // e o lançamento segue.
        ("subagent.launch_settings", Locale::PtBr) => {
            "[Mustard] O agente {agent} saiu com modelo {asked_model} e esforço {asked_effort}. O \
             Mustard escolheu modelo {model} e esforço {effort} para ele. O modelo da onda é o do envio \
             dela. O do revisor vem do `mustard.json`, salvo o do revisor final: esse fica o que a \
             chamada pediu. O esforço é sempre xhigh. Nos próximos lançamentos, não passe `effort`. \
             Em `model`, passe só o da entrada de `dispatch`."
        }
        ("subagent.launch_settings", Locale::EnUs) => {
            "[Mustard] The agent {agent} went out with model {asked_model} and effort {asked_effort}. \
             Mustard chose model {model} and effort {effort} for it. A wave's model is the one its send \
             recorded. The reviewer's comes from `mustard.json`, except the final reviewer's: that one \
             stays what the call asked. The effort is always xhigh. In the next launches, do not pass \
             `effort`. In `model`, pass only the one in the `dispatch` entry."
        }
        // O lançamento que pedia modelo ou esforço diferente do escolhido sai
        // com o escolhido: o gancho troca os dois na chamada.
        ("subagent.launch_settings_forced", Locale::PtBr) => {
            "[Mustard] O agente {agent} foi pedido com modelo {asked_model} e esforço {asked_effort}. \
             Saiu com modelo {model} e esforço {effort}. O modelo da onda é o do envio dela. O do \
             revisor vem do `mustard.json`, salvo o do revisor final: esse fica o que a chamada \
             pediu. O esforço é sempre xhigh. Nos próximos lançamentos, não passe `effort`. Em \
             `model`, passe só o da entrada de `dispatch`."
        }
        ("subagent.launch_settings_forced", Locale::EnUs) => {
            "[Mustard] The agent {agent} was asked with model {asked_model} and effort {asked_effort}. \
             It went out with model {model} and effort {effort}. A wave's model is the one its send \
             recorded. The reviewer's comes from `mustard.json`, except the final reviewer's: that one \
             stays what the call asked. The effort is always xhigh. In the next launches, do not pass \
             `effort`. In `model`, pass only the one in the `dispatch` entry."
        }

        // O agente que escreve código nunca sai no modelo pequeno da sessão.
        ("subagent.no_small_model_for_code", Locale::PtBr) => {
            "[Mustard] O agente {agent} escreve código, e código não vai no modelo {asked_model}. \
             Saiu com modelo {model}. O modelo pequeno da sessão lê e decide; quem escreve é um \
             modelo maior."
        }
        ("subagent.no_small_model_for_code", Locale::EnUs) => {
            "[Mustard] The agent {agent} writes code, and code does not go on model {asked_model}. \
             It went out with model {model}. The session's small model reads and decides; writing \
             goes to a larger one."
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
        // A linha logo abaixo do título do pedido da onda que trabalha numa
        // cópia: o agente que via o `--root` do repositório principal entrava
        // nele com `cd`, o gancho recusava, e a primeira leitura se perdia.
        ("prompt.copy.first", Locale::PtBr) => {
            "Rode cada comando de dentro da cópia `{copy}`, sem `cd` para fora dela: o `--root` dos \
             comandos só diz onde a spec mora."
        }
        ("prompt.copy.first", Locale::EnUs) => {
            "Run every command from inside the copy `{copy}`, without a `cd` out of it: the commands' \
             `--root` only says where the spec lives."
        }
        // A linha do texto do despacho entre o título e o comando que lê o
        // pedido, pela mesma razão: o pedido, com a pasta da cópia, só chega
        // depois desse comando.
        ("prompt.dispatch.as_is", Locale::PtBr) => {
            "Rode o comando abaixo como está, sem `cd`: o `--root` só diz onde a spec mora. Depois, rode \
             cada comando de dentro da cópia que o pedido diz."
        }
        ("prompt.dispatch.as_is", Locale::EnUs) => {
            "Run the command below as it is, without a `cd`: `--root` only says where the spec lives. \
             After that, run every command from inside the copy the request names."
        }
        // Os dois idiomas do projeto, lidos do `mustard.json`, no topo de todo
        // pedido a um agente: o dos textos que a pessoa lê e o dos nomes no
        // código. Sem ela, o agente só adivinhava o idioma dos nomes.
        ("prompt.languages", Locale::PtBr) => {
            "Idiomas deste projeto. O texto sai em {text}: comentários, entregas e commits. O código \
             sai em {code}: nomes de variáveis, funções, testes, arquivos, comandos e tabelas do \
             banco. Isso vale mesmo quando o arquivo já traz nomes em outro idioma. Regra do projeto que \
             trate de idioma vence esta linha."
        }
        ("prompt.languages", Locale::EnUs) => {
            "This project's languages. Text is written in {text}: comments, deliveries and commits. \
             Code is written in {code}: names of variables, functions, tests, files, commands and \
             database tables. This holds even when the file already has names in another \
             language. A project rule that deals with language overrides this line."
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
        // obedecer"; o segundo traz as tarefas da onda só pelo código e pelo
        // título, na ordem, com a primeira marcada; a entrega fecha a lista.
        // `{part}` é o título da seção a que o passo aponta.
        ("prompt.step.read", Locale::PtBr) => "Leia o texto inteiro de cada item de \"{part}\".",
        ("prompt.step.read", Locale::EnUs) => "Read the whole text of each item under \"{part}\".",
        ("prompt.step.tasks", Locale::PtBr) => {
            "Faça as tarefas uma por vez, nesta ordem. A primeira vem inteira logo abaixo. Cada uma das \
             outras chega na resposta do passo que termina a anterior."
        }
        ("prompt.step.tasks", Locale::EnUs) => {
            "Do the tasks one at a time, in this order. The first comes whole right below. Each of the \
             others arrives in the answer to the step that finishes the one before it."
        }
        ("prompt.step.first", Locale::PtBr) => "{item} (a primeira, logo abaixo)",
        ("prompt.step.first", Locale::EnUs) => "{item} (the first, right below)",
        ("prompt.step.attends", Locale::PtBr) => "Atende: {item}",
        ("prompt.step.attends", Locale::EnUs) => "Addresses: {item}",
        ("prompt.step.user_message", Locale::PtBr) => "mensagem do usuário",
        ("prompt.step.user_message", Locale::EnUs) => "user message",
        // A mensagem do usuário que responde a uma pergunta com opções: a
        // pergunta do assistente, acima da resposta dele.
        ("prompt.message.question", Locale::PtBr) => "Pergunta: {question}",
        ("prompt.message.question", Locale::EnUs) => "Question: {question}",
        ("prompt.message.answer", Locale::PtBr) => "Resposta: {answer}",
        ("prompt.message.answer", Locale::EnUs) => "Answer: {answer}",
        // O bloco de uma tarefa: o título, com a posição dela na onda, e cada
        // skill dela inteira, pelo nome e pelo caminho. A skill que o teto do
        // bloco cortou fica numa linha, com o caminho para lê-la.
        ("prompt.task.heading", Locale::PtBr) => "Tarefa {n} de {total}: {item}",
        ("prompt.task.heading", Locale::EnUs) => "Task {n} of {total}: {item}",
        ("prompt.task.skill", Locale::PtBr) => "Skill {name}, de `{path}`",
        ("prompt.task.skill", Locale::EnUs) => "Skill {name}, from `{path}`",
        // A tarefa que cria a skill de uma falha repetida, antes do
        // skill-creator inteiro (`io::wave_prompt`).
        ("prompt.task.skill_creator", Locale::PtBr) => {
            "Esta tarefa cria ou atualiza uma skill. Siga o processo do skill-creator abaixo, sem pedir aprovação a \
             ninguém. Os scripts e os agentes dele ficam em `{folder}`. Use o formato oficial, com nome \
             curto. A descrição fica em terceira pessoa e diz o que a skill faz e quando usar. O \
             corpo tem menos de 500 linhas, com o mais importante no topo. No cabeçalho, `paths:` \
             traz os caminhos onde ela vale. Escreva só o que o modelo não sabe: a armadilha, a convenção não óbvia, a ferramenta \
             certa. Nada de visão geral do código. Teste com e sem a skill nas tarefas que a \
             motivaram, pela avaliação do próprio skill-creator, numa pasta fora do projeto. Só \
             com ganho ela vai para `{skills}/<nome>/SKILL.md`. A versão nova de uma skill que já existe \
             vai para o caminho reservado que a tarefa dá, dentro da sua cópia. No fim, grave o resultado: \
             `mustard-rt run write skill --root {root} --spec {spec} --json \
             '{\"name\":\"<nome>\",\"action\":\"created\",\"text\":\"<o ganho medido, numa \
             frase>\",\"files\":[\"<arquivo citado>\"]}'`, com `\"action\":\"updated\"` quando \
             atualiza uma skill que já existe. Sem ganho, grave `\"action\":\"discarded\"` com o motivo em \
             `text`."
        }
        ("prompt.task.skill_creator", Locale::EnUs) => {
            "This task creates or updates a skill. Follow the skill-creator process below without asking anyone \
             for approval. Its scripts and agents live in `{folder}`. Use the official format, with a \
             short name. The description is in the third person and says what the skill does and \
             when to use it. The body stays under 500 lines, with the most important part on top. \
             In the header, `paths:` lists the paths where it applies. Write only what the model does not know: the trap, the non-obvious convention, \
             the right tool. No overview of the code. Test with and without the skill on the tasks \
             that motivated it, with the skill-creator's own evaluation, in a folder outside the \
             project. Only with a gain does it go to `{skills}/<name>/SKILL.md`. The new version of a skill \
             that exists goes to the reserved path the task gives, inside your copy. At the end, record \
             the result: `mustard-rt run write skill --root {root} --spec {spec} --json \
             '{\"name\":\"<name>\",\"action\":\"created\",\"text\":\"<the measured gain, in one \
             sentence>\",\"files\":[\"<cited file>\"]}'`, with `\"action\":\"updated\"` when it \
             updates a skill that exists. Without a gain, record `\"action\":\"discarded\"` with the \
             reason in `text`."
        }
        ("prompt.task.skill_cut", Locale::PtBr) => {
            "A skill {name} ficou fora deste bloco, que passou do teto de tamanho. Se a tarefa pedir, \
             leia `{path}`."
        }
        ("prompt.task.skill_cut", Locale::EnUs) => {
            "The skill {name} was left out of this block, which went past the size cap. If the task \
             needs it, read `{path}`."
        }
        ("prompt.step.file", Locale::PtBr) => "Arquivo: {files}",
        ("prompt.step.file", Locale::EnUs) => "File: {files}",
        ("prompt.step.files", Locale::PtBr) => "Arquivos: {files}",
        ("prompt.step.files", Locale::EnUs) => "Files: {files}",
        ("prompt.step.read_before", Locale::PtBr) => "Leia antes: {hints}",
        ("prompt.step.read_before", Locale::EnUs) => "Read before: {hints}",
        ("prompt.step.deliver", Locale::PtBr) => "Grave a entrega, como diz \"{part}\".",
        ("prompt.step.deliver", Locale::EnUs) => "Record the delivery, as \"{part}\" says.",
        // Sem lição para os arquivos da onda, a seção diz isso em vez de
        // calar.
        ("prompt.obey.no_lessons", Locale::PtBr) => "Lições: nenhuma vale para os arquivos desta onda.",
        ("prompt.obey.no_lessons", Locale::EnUs) => "Lessons: none applies to this wave's files.",
        ("prompt.return.loose", Locale::PtBr) => "Fora da entrega, nenhum texto solto.",
        ("prompt.return.loose", Locale::EnUs) => "Outside the delivery, no loose text.",
        // Os dois comandos de gravação da onda, prontos, com a spec, o número
        // da onda e, na cópia, `--root <caminho> ` em `{root}`: sem eles, os
        // agentes pediam a ajuda inteira do `run write` dezenas de vezes por
        // obra.
        ("prompt.return.step", Locale::PtBr) => {
            "Ao provar cada critério e ao terminar cada tarefa, grave o passo: `mustard-rt run write step \
             {root}--spec {spec} --json '{\"wave\":{wave},\"item\":\"<código da tarefa ou do critério>\",\
             \"text\":\"<o resumo>\"}'`. A resposta do passo que termina uma tarefa traz a próxima \
             tarefa inteira, em `next_task`, ou a ordem de entregar. Não tente ler uma tarefa que ainda \
             não chegou."
        }
        ("prompt.return.step", Locale::EnUs) => {
            "On proving each criterion and on finishing each task, record the step: `mustard-rt run write \
             step {root}--spec {spec} --json '{\"wave\":{wave},\"item\":\"<task or criterion code>\",\
             \"text\":\"<the summary>\"}'`. The answer to the step that finishes a task brings the next \
             whole task, in `next_task`, or the order to deliver. Do not try to read a task that has not \
             arrived yet."
        }
        ("prompt.return.delivered", Locale::PtBr) => {
            "Grave a entrega assim, com os outros campos das suas instruções quando valem: `mustard-rt run \
             write delivered {root}--spec {spec} --json '{\"wave\":{wave},\"text\":\"<a entrega>\",\
             \"files\":[\"<arquivo>\"],\"commit\":\"<o título do commit>\"}'`"
        }
        ("prompt.return.delivered", Locale::EnUs) => {
            "Record the delivery like this, adding the other fields your instructions describe when they \
             apply: `mustard-rt run write delivered {root}--spec {spec} --json '{\"wave\":{wave},\
             \"text\":\"<the delivery>\",\"files\":[\"<file>\"],\"commit\":\"<the commit title>\"}'`"
        }
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
        ("prompt.pattern.example_named", Locale::PtBr) => "exemplo: `{name}` em `{path}`",
        ("prompt.pattern.example_named", Locale::EnUs) => "example: `{name}` in `{path}`",
        // Sem regra de importação, o bloco abre só com o que o código e o git
        // mostram: o arquivo grande e a receita.
        ("prompt.pattern.head_plain", Locale::PtBr) => "O que o projeto mostra sobre esta tarefa, tirado do código e da história do git.",
        ("prompt.pattern.head_plain", Locale::EnUs) => "What the project shows about this task, taken from the code and the git history.",
        // O arquivo da tarefa entre os maiores do projeto: só informa, e pede
        // o código novo num arquivo novo.
        ("prompt.pattern.large", Locale::PtBr) => "Entre os {percent}% maiores arquivos do projeto: {files}. Ponha o código novo num arquivo novo.",
        ("prompt.pattern.large", Locale::EnUs) => "Among the {percent}% largest files in the project: {files}. Put the new code in a new file.",
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
             verdict`. O pedido traz só o que nenhuma prova verde cobre. É a tarefa sem passo de \
             término, o requisito acordado sem onda e o critério sem execução que passou. \
             Responda por cada requisito acordado do pedido em `agreed`, com o código em `item` e \
             `met` dizendo se está atendido. Quando não estiver, `text` diz o que falta e `files` os \
             arquivos, e eles viram uma tarefa nova. Faltar algum requisito acordado do pedido é \
             veredito malformado: nada é gravado."
        }
        ("prompt.final.fixed", Locale::EnUs) => {
            "**What this is.** The dedicated test agent's request for this spec, assembled by the \
             binary from it. No text is copied in: each part carries only the type, the code and \
             the title of each item, on one line per item.\n\n\
             **What to return.** The verdict with `\"final\":true`, recorded through `mustard-rt run \
             write verdict`. The request carries only what no green proof covers. That is the task \
             without a finishing step, the agreed requirement without a wave and the criterion \
             without a passing run. Answer for each agreed requirement of the request in `agreed`, with the \
             code in `item` and `met` saying whether it is satisfied. When it is not, `text` says \
             what is missing and `files` the files, and they become a new task. Missing any agreed \
             requirement of the request is a malformed verdict: nothing gets recorded."
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
        // A revisão que volta depois de uma reprovação confere só a cadeia de
        // cada conserto: a regra que ele faz valer, cada lugar dela e a prova,
        // que a linha da tarefa traz. Não caça achado no resto da obra. Para o
        // que não mudou, vale a conclusão anterior; o veredito continua
        // respondendo por todo o combinado. É o único lugar que diz o recorte:
        // quando o veredito reprovou uma onda, a parte das ondas traz só as
        // tarefas dela.
        ("prompt.final.look_again", Locale::PtBr) => {
            "**O que olhar.** Esta revisão volta depois de um veredito que reprovou. A parte do que \
             mudou lista esse veredito, os commits e os itens gravados ou regravados depois dele. Ela \
             lista também os itens que ele deu como não atendidos e as tarefas com passo gravado \
             depois dele. Se ele reprovou uma onda, a parte das ondas traz só as tarefas dessa onda; \
             senão, traz as de todas as ondas. Confira só a cadeia de cada conserto: a regra que ele \
             faz valer, cada lugar dela e a prova que a confere. A cadeia vem sob a linha da tarefa \
             que a grava. Não procure achado no resto da obra. Para os outros requisitos acordados, \
             repita a conclusão do veredito anterior. Aponte só; não conserte."
        }
        ("prompt.final.look_again", Locale::EnUs) => {
            "**What to look at.** This review comes back after a verdict that rejected the work. The \
             part on what changed lists that verdict, the commits and the items recorded or \
             re-recorded after it. It also lists the items that verdict marked as not met and the \
             tasks with a step recorded after it. If it rejected one wave, the waves part carries \
             only that wave's tasks; otherwise, it carries every wave's. Check only the chain of each \
             fix: the rule it enforces, each place of that rule and the proof that checks it. The \
             chain comes under the line of the task that records it. Do not look for findings in the \
             rest of the work. For the other agreed requirements, repeat the previous verdict's \
             conclusion. Point it out only; do not fix it."
        }
        // A revisão que volta depois de uma aprovação, para o ajuste feito
        // com a obra reaberta, confere só a cadeia desse ajuste: o que a
        // aprovação conferiu e não mudou fica fora do pedido e do veredito.
        ("prompt.final.look_since_approval", Locale::PtBr) => {
            "**O que olhar.** Esta revisão volta depois de um veredito que aprovou a obra, para um \
             ajuste feito depois dele. A parte do que mudou lista esse veredito e os commits e os \
             itens gravados ou regravados depois do pedido que ele respondeu. Ela lista também as \
             tarefas com passo gravado depois desse pedido. Confira só a cadeia do ajuste: a regra \
             que ele faz valer, cada lugar dela e a prova que a confere. A cadeia vem sob a linha da \
             tarefa que a grava. Não procure achado no resto da obra. O que ele aprovou e não mudou \
             fica fora do pedido e do veredito. Aponte só; não conserte."
        }
        ("prompt.final.look_since_approval", Locale::EnUs) => {
            "**What to look at.** This review comes back after a verdict that approved the work, for \
             an adjustment made after it. The part on what changed lists that verdict and the commits \
             and items recorded or re-recorded after the request it answered. It also lists the tasks \
             with a step recorded after that request. Check only the chain of the adjustment: the rule \
             it enforces, each place of that rule and the proof that checks it. The chain comes under \
             the line of the task that records it. Do not look for findings in the rest of the work. \
             What it approved and did not change stays out of the request and of the verdict. Point it \
             out only; do not fix it."
        }
        ("prompt.part.since_verdict", Locale::PtBr) => "O que mudou desde o veredito anterior",
        ("prompt.part.since_verdict", Locale::EnUs) => "What changed since the previous verdict",
        // A cadeia de uma tarefa no pedido da revisão final, sob a linha
        // dela: a regra que ela faz valer, os lugares e as provas. Cada parte
        // sai só quando a tarefa a tem.
        ("prompt.final.chain.rule", Locale::PtBr) => "Regra: {rule}.",
        ("prompt.final.chain.rule", Locale::EnUs) => "Rule: {rule}.",
        ("prompt.final.chain.places", Locale::PtBr) => "Lugares: {places}.",
        ("prompt.final.chain.places", Locale::EnUs) => "Places: {places}.",
        ("prompt.final.chain.proof", Locale::PtBr) => "Prova: {proofs}.",
        ("prompt.final.chain.proof", Locale::EnUs) => "Proof: {proofs}.",
        // Como ler os itens do pedido da onda. `{root}` é `--root <caminho> `
        // quando o agente trabalha numa cópia, e nada quando não trabalha. O
        // pedido traz só o tipo, o código e o título de cada item; o texto
        // inteiro de todos, lições inclusive, se lê numa chamada só, que deixa
        // um recibo só, e um item sozinho pelo código. A entrega sem a leitura
        // completa é recusada.
        ("prompt.read.wave", Locale::PtBr) => {
            "A tarefa atual vem inteira neste pedido, com o texto de tudo o que ela atende. Os outros \
             itens trazem só o tipo, o código e o título. Leia o texto inteiro deles numa chamada só:\n\
             - os itens e lições já liberados deste pedido: `mustard-rt run read \
             dispatch-{wave} {root}--spec {spec}`\n\
             - um item só, para reler: `mustard-rt run read item-<código> {root}--spec {spec}`\n\
             A entrega é recusada se algum item liberado não foi lido. A recusa diz qual."
        }
        ("prompt.read.wave", Locale::EnUs) => {
            "The current task comes whole in this request, with the text of all it addresses. The other \
             items carry only the type, the code and the title. Read their whole text in one call:\n\
             - the items and lessons of this request released so far: `mustard-rt run read \
             dispatch-{wave} {root}--spec {spec}`\n\
             - a single item, to reread it: `mustard-rt run read item-<item-code> {root}--spec {spec}`\n\
             The delivery is refused if any released item was not read. The refusal says which."
        }
        // Como ler os itens do pedido do revisor final: a mesma leitura em
        // lote, pelo pedido aberto da revisão, e o aviso de que o veredito sem
        // a leitura completa é recusado, com a recusa dizendo qual item faltou.
        ("prompt.read.final", Locale::PtBr) => {
            "Este pedido traz só o tipo, o código e o título de cada item. Leia o texto inteiro de \
             todos numa chamada só, antes de dar o veredito:\n\
             - todos os itens deste pedido: `mustard-rt run read dispatch-review \
             {root}--spec {spec}`\n\
             - um item só, para reler: `mustard-rt run read item-<código> {root}--spec {spec}`\n\
             O veredito é recusado se algum item deste pedido não foi lido. A recusa diz qual."
        }
        ("prompt.read.final", Locale::EnUs) => {
            "This request carries only the type, the code and the title of each item. Read the whole \
             text of all of them in one call, before you give the verdict:\n\
             - every item of this request: `mustard-rt run read dispatch-review \
             {root}--spec {spec}`\n\
             - a single item, to reread it: `mustard-rt run read item-<item-code> {root}--spec {spec}`\n\
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
        ("prompt.part.validation", Locale::PtBr) => "Recibos da validação final registrada",
        ("prompt.part.validation", Locale::EnUs) => "Recorded final-validation receipts",
        ("prompt.part.criteria", Locale::PtBr) => "Critérios",
        ("prompt.part.criteria", Locale::EnUs) => "Criteria",
        ("prompt.skill.stale", Locale::PtBr) => "a revisar",
        ("prompt.skill.stale", Locale::EnUs) => "to review",

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
        ("prompt.task_read.function_lines", Locale::PtBr) => "leia só as linhas {lines} de `{function}` em `{path}`",
        ("prompt.task_read.function_lines", Locale::EnUs) => "read only lines {lines} of `{function}` in `{path}`",
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
        ("prompt.execution.prepared_native", Locale::PtBr) => {
            "O binário preparou as dependências desta cópia antes do despacho. Não repita o preparo sem mudança nos insumos. Não crie atalho para node_modules de outra cópia."
        }
        ("prompt.execution.prepared_native", Locale::EnUs) => {
            "The binary prepared this copy's dependencies before dispatch. Do not repeat preparation unless its inputs changed. Do not symlink another checkout's node_modules."
        }
        ("prompt.execution.prepared_changed", Locale::PtBr) => "Arquivos alterados desde o último uso da cópia: {files}.",
        ("prompt.execution.prepared_changed", Locale::EnUs) => "Files changed since the previous use of this copy: {files}.",
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
        ("prompt.project_rules.source", Locale::PtBr) => "O texto do `CLAUDE.md` da raiz do projeto. Siga-o nesta execução; onde ele trata de idioma, vence a linha de idiomas do pedido.",
        ("prompt.project_rules.source", Locale::EnUs) => "The text of the `CLAUDE.md` at the project root. Follow it during this work; where it deals with language, it overrides the request's languages line.",
        ("prompt.project_rules.sources", Locale::PtBr) => {
            "Os arquivos de regras da raiz e das pastas onde a obra mexeu, cada um sob o caminho dele. Siga todos \
             nesta execução; onde tratam de idioma, vencem a linha de idiomas do pedido."
        }
        ("prompt.project_rules.sources", Locale::EnUs) => {
            "The rules files of the root and of the folders the work touched, each under its path. Follow them all \
             during this work; where they deal with language, they override the request's languages line."
        }
        // A cópia do desenho que o usuário aprovou, no fim do despacho ao
        // revisor quando a pasta da spec tem uma.
        ("prompt.part.approved_design", Locale::PtBr) => "Desenho aprovado",
        ("prompt.part.approved_design", Locale::EnUs) => "Approved design",
        ("prompt.approved_design", Locale::PtBr) => {
            "A cópia do desenho que o usuário aprovou para esta spec está em `{path}`. Compare cada tela com ela."
        }
        ("prompt.approved_design", Locale::EnUs) => {
            "The copy of the design the user approved for this spec is at `{path}`. Compare each screen with it."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas, como o arquivo de impressões dela em `stamps/` guarda. Quem muda
    /// um texto de propósito regrava o arquivo pelo comando que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged("prompt", include_str!("prompt.rs"), super::PREFIXES);
    }

    /// O bloco do padrão sob a tarefa passa na conferência de escrita nos
    /// dois idiomas, com cada lacuna trocada por uma palavra.
    #[test]
    fn the_pattern_block_texts_read_clearly() {
        use crate::platform::i18n::{Locale, translate};
        for lang in [Locale::PtBr, Locale::EnUs] {
            for key in [
                "prompt.pattern.head",
                "prompt.pattern.rule",
                "prompt.pattern.info",
                "prompt.pattern.example",
                "prompt.pattern.example_named",
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
        use crate::platform::i18n::{Locale, translate};
        for lang in [Locale::PtBr, Locale::EnUs] {
            for key in [
                "prompt.read.wave",
                "prompt.read.final",
                "prompt.fix.wave",
                "prompt.step.read",
                "prompt.step.tasks",
                "prompt.step.first",
                "prompt.step.attends",
                "prompt.step.user_message",
                "prompt.message.question",
                "prompt.message.answer",
                "prompt.task.heading",
                "prompt.task.skill",
                "prompt.task.skill_cut",
                "prompt.step.file",
                "prompt.step.files",
                "prompt.step.read_before",
                "prompt.step.deliver",
                "prompt.obey.no_lessons",
                "prompt.return.loose",
                "prompt.task.tested_by",
            ] {
                let text = translate(key, lang)
                    .replace("{part}", "part")
                    .replace("{item}", "item")
                    .replace("{question}", "Which orders?")
                    .replace("{answer}", "The paid ones")
                    .replace("{files}", "`src/order.service.ts`")
                    .replace("{hints}", "`create`")
                    .replace("{command}", "make check")
                    .replace("{file}", "src/order.service.ts")
                    .replace("{tests}", "`order_test`")
                    .replace("{root}", "")
                    .replace("{wave}", "2")
                    .replace("{spec}", "orders")
                    .replace("{n}", "1")
                    .replace("{total}", "3")
                    .replace("{name}", "orders")
                    .replace("{path}", "/repo/.claude/skills/orders/SKILL.md");
                assert!(!text.contains('{'), "{key} {lang:?}: {text}");
                let report = crate::domain::clarity::measure(&text, &[], Some(lang));
                assert!(report.passed, "{key} {lang:?}: {report:?}");
            }
        }
    }
}
