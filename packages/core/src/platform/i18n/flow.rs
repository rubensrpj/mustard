//! Os passos do fluxo de uma spec: abrir, planejar, rodar as ondas, fechar,
//! abrir o pull request, retomar, reabrir e descartar; o pedido que muda as
//! ondas, a mensagem do commit e as recusas dos comandos que saíram do fluxo.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &[
    "open", "plan", "round", "close", "resume", "reopen", "discard", "request", "message", "pr", "approve_spec",
    "retired", "banner", "stuck", "conversation_size",
];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // CLOSE-phase success banner.
        ("banner.close.success", Locale::PtBr) => "Pipeline fechado com sucesso.",
        ("banner.close.success", Locale::EnUs) => "Pipeline closed successfully.",
        // O passo do plano: o que a conferência acha e o próximo passo.
        ("plan.not_ready", Locale::PtBr) => {
            "O plano tem {count} coisas a corrigir antes da pergunta de aprovação. Nada foi gravado."
        }
        ("plan.not_ready", Locale::EnUs) => {
            "The plan has {count} things to fix before the approval question. Nothing was written."
        }
        ("plan.next", Locale::PtBr) => {
            "Depois, faça a pergunta de aprovação na ordem de explicar do estilo de resposta, com o \
             texto exato \"{question}\" e as opções \"{option}\" e \"Ajustar\": com outro texto, a \
             aprovação não vale."
        }
        ("plan.next", Locale::EnUs) => {
            "Then ask the approval question in the order of explaining from the response style, with \
             the exact text \"{question}\" and the options \"{option}\" and \"Adjust\": with another \
             text, the approval does not count."
        }
        // Quem executa a obra, pela soma das notas de todas as tarefas do
        // plano e pelo número de ondas que ele já tem: até 3 pontos numa
        // onda só, o orquestrador faz sem ondas; de 4 a 13 numa onda só, um
        // agente faz; acima de 13, ou já com mais de uma onda no plano,
        // ondas de até 13 pontos cada — sem dizer que o total passou de 13,
        // porque isso pode ser falso quando a razão é a onda já dividida.
        ("plan.execution.solo", Locale::PtBr) => {
            "A obra soma {points} pontos: até 3, sem ondas, o orquestrador faz."
        }
        ("plan.execution.solo", Locale::EnUs) => {
            "The work totals {points} points: up to 3, no waves, the orchestrator does it."
        }
        ("plan.execution.one_wave", Locale::PtBr) => {
            "A obra soma {points} pontos: de 4 a 13, um agente faz, numa onda só."
        }
        ("plan.execution.one_wave", Locale::EnUs) => {
            "The work totals {points} points: from 4 to 13, one agent does it, in a single wave."
        }
        ("plan.execution.many_waves", Locale::PtBr) => {
            "A obra soma {points} pontos: vai em ondas de até 13 pontos cada, uma por agente."
        }
        ("plan.execution.many_waves", Locale::EnUs) => {
            "The work totals {points} points: it goes in waves of up to 13 points each, one agent per \
             wave."
        }
        ("plan.execution.ends_with_test_agent", Locale::PtBr) => {
            "Em todo tamanho, a obra termina com o agente de teste dedicado."
        }
        ("plan.execution.ends_with_test_agent", Locale::EnUs) => {
            "In every size, the work ends with the dedicated test agent."
        }
        ("plan.wave_loop", Locale::PtBr) => {
            "As ondas {waves} dependem umas das outras em círculo, e nenhuma pode começar. Corte uma \
             das dependências."
        }
        ("plan.wave_loop", Locale::EnUs) => {
            "Waves {waves} depend on each other in a circle, and none of them can start. Cut one of \
             the dependencies."
        }
        ("plan.depends_on_missing", Locale::PtBr) => {
            "A onda {wave} depende da onda {on}, que o plano não tem. Corrija o número ou grave a onda."
        }
        ("plan.depends_on_missing", Locale::EnUs) => {
            "Wave {wave} depends on wave {on}, which the plan does not have. Fix the number or record \
             the wave."
        }
        ("plan.task_without_wave", Locale::PtBr) => {
            "A tarefa {task} é da onda {wave}, que o plano não tem. Corrija o número ou grave a onda."
        }
        ("plan.task_without_wave", Locale::EnUs) => {
            "Task {task} belongs to wave {wave}, which the plan does not have. Fix the number or \
             record the wave."
        }
        ("plan.shared_file", Locale::PtBr) => {
            "As ondas {waves} saem na mesma rodada e mexem em {files}. Encadeie uma na outra ou divida \
             o arquivo entre elas ({chain})."
        }
        ("plan.shared_file", Locale::EnUs) => {
            "Waves {waves} go out in the same round and both touch {files}. Chain one after the other \
             or split the file between them ({chain})."
        }
        ("plan.wave_should_split", Locale::PtBr) => {
            "A onda {wave} tem partes que não dividem arquivo entre si ({parts}): ela sai dividida, \
             uma onda por parte, e as partes rodam em paralelo."
        }
        ("plan.wave_should_split", Locale::EnUs) => {
            "Wave {wave} has parts that share no file with each other ({parts}): it goes out split, \
             one wave per part, and the parts run in parallel."
        }
        ("plan.spec_should_split", Locale::PtBr) => {
            "A spec tem partes que não dividem arquivo entre si ({parts}): ela pode ser dividida, \
             uma spec por parte."
        }
        ("plan.spec_should_split", Locale::EnUs) => {
            "The spec has parts that share no file with each other ({parts}): it can be split, one \
             spec per part."
        }
        ("plan.file_outside_git", Locale::PtBr) => {
            "A tarefa {task} cita {path}, que o git não guarda: um agente noutra sessão ou noutra \
             máquina não o vê."
        }
        ("plan.file_outside_git", Locale::EnUs) => {
            "Task {task} cites {path}, which git does not track: an agent in another session or on \
             another machine cannot see it."
        }
        ("plan.item_without_task", Locale::PtBr) => {
            "Nenhuma tarefa diz que cobre o item {code}. Se ele não vira código, diga por quê."
        }
        ("plan.item_without_task", Locale::EnUs) => {
            "No task says it covers item {code}. If it does not become code, say why."
        }
        // O dono de cada item combinado: o item novo depois da aprovação nasce
        // com dono.
        ("plan.owner_missing", Locale::PtBr) => {
            "O item novo do tipo {type} não tem dono, e a spec já foi aprovada: todo item combinado tem \
             dono. Diga em `waves` as ondas cujas tarefas o cobrem ou vão cobrir, como `\"waves\":[3]`, \
             ou, quando ele vale para todas as ondas, grave-o com \
             `\"applies_to\":{\"files\":[\"**\"]}`. Nada foi gravado."
        }
        ("plan.owner_missing", Locale::EnUs) => {
            "The new {type} item has no owner, and the spec is already approved: every agreed item has \
             an owner. Name in `waves` the waves whose tasks cover it or will cover it, as in \
             `\"waves\":[3]`, or, when it holds for every wave, record it with \
             `\"applies_to\":{\"files\":[\"**\"]}`. Nothing was written."
        }
        ("plan.contract_without_criterion", Locale::PtBr) => {
            "Nenhum critério cita o contrato {code}: nada prova que ele foi cumprido."
        }
        ("plan.contract_without_criterion", Locale::EnUs) => {
            "No criterion cites contract {code}: nothing proves it was met."
        }
        ("plan.task_without_file", Locale::PtBr) => {
            "A tarefa {task} mexe em código e não diz em que arquivo. O mapa sugere: {files}. \
             A tarefa que não mexe em arquivo nenhum diz isso no texto dela."
        }
        ("plan.task_without_file", Locale::EnUs) => {
            "Task {task} changes code and does not say which file. The map suggests: {files}. \
             A task that changes no file says so in its own text."
        }
        ("plan.task_wrong_wave", Locale::PtBr) => {
            "A tarefa {task} está na onda {wave} e o texto dela casa com o dessa onda menos do que \
             casa, em média, com o das outras; casa melhor com a onda {best}. Mova a tarefa ou \
             reescreva o texto da onda."
        }
        ("plan.task_wrong_wave", Locale::EnUs) => {
            "Task {task} sits in wave {wave} and its text matches that wave's text less than it \
             matches the other waves' on average; it matches wave {best} better. Move the task or \
             rewrite the wave's text."
        }
        ("plan.task_matches_no_wave", Locale::PtBr) => {
            "A tarefa {task} está na onda {wave} e o texto dela não casa com o de onda nenhuma do \
             plano. Reescreva o texto da tarefa ou o da onda."
        }
        ("plan.task_matches_no_wave", Locale::EnUs) => {
            "Task {task} sits in wave {wave} and its text matches no wave of the plan. Rewrite the \
             task's text or the wave's."
        }
        ("plan.task_could_name_a_skill", Locale::PtBr) => {
            "A tarefa {task} não nomeia skill, e a skill {skill} serve para ela. Nomeie-a na tarefa."
        }
        ("plan.task_could_name_a_skill", Locale::EnUs) => {
            "Task {task} names no skill, and skill {skill} fits it. Name it in the task."
        }
        ("plan.skill_to_be_born", Locale::PtBr) => {
            "O trabalho da tarefa {task} se repete no projeto e nenhuma skill serve para ela. \
             Inclua no plano a tarefa que cria a skill dela."
        }
        ("plan.skill_to_be_born", Locale::EnUs) => {
            "The work of task {task} repeats across the project and no skill fits it. \
             Add to the plan the task that creates its skill."
        }
        ("plan.no_suggestion", Locale::PtBr) => "nada — o mapa não achou arquivo para esta tarefa",
        ("plan.no_suggestion", Locale::EnUs) => "nothing — the map found no file for this task",
        // A nota de trabalho de cada tarefa: a escala e o exemplo de cada nota
        // moram só aqui, e a recusa da tarefa sem nota os mostra.
        ("plan.points_scale", Locale::PtBr) => {
            "A nota vai em `\"points\"`, na escala do Scrum, comparando a tarefa com o exemplo de \
             cada nota. 1: trocar um texto, uma lista ou um número, como o texto de um botão ou o \
             valor de um limite. 2: mudar uma regra num lugar só, com teste, como validar um campo \
             novo de um formulário. 3: mudar uma regra que passa por vários arquivos, como um campo \
             novo que vai da tela até o banco de dados. 5: mexer no caminho que grava ou junta os \
             dados, como mudar o jeito de salvar e conferir um pedido. 8: mudar uma parte inteira do \
             sistema, como trocar o jeito de entrar com usuário e senha. 13: tarefa grande e incerta, \
             que vale quebrar antes de gravar."
        }
        ("plan.points_scale", Locale::EnUs) => {
            "The points go in `\"points\"`, on the Scrum scale, comparing the task with the example of \
             each value. 1: change a text, a list or a number, like the text of a button or the value \
             of a limit. 2: change a rule in one place only, with a test, like validating a new field \
             in a form. 3: change a rule that runs through several files, like a new field that goes \
             from the screen to the database. 5: touch the path that writes or merges the data, like \
             changing the way an order is saved and checked. 8: change a whole part of the system, \
             like changing the way of logging in with a username and password. 13: a large and \
             uncertain task, worth breaking up before recording."
        }
        ("plan.task_without_points", Locale::PtBr) => {
            "A tarefa sem nota segura a aprovação: {tasks}. Grave uma versão nova de cada uma, com a \
             nota dela. {scale}"
        }
        ("plan.task_without_points", Locale::EnUs) => {
            "A task without points holds the approval: {tasks}. Record a new version of each one, \
             with its points. {scale}"
        }
        ("plan.wave_points_over_cap", Locale::PtBr) => {
            "A onda {wave} soma {points} pontos, acima do teto de {cap}: é trabalho demais para uma \
             onda só. O aviso não segura a aprovação, e quem aprova decide se a onda segue assim."
        }
        ("plan.wave_points_over_cap", Locale::EnUs) => {
            "Wave {wave} adds up to {points} points, over the cap of {cap}: too much work for a single \
             wave. The warning does not hold the approval, and whoever approves decides whether the \
             wave goes on as it is."
        }
        ("plan.command_not_declared", Locale::PtBr) => {
            "O projeto não declara `{field}` no mustard.json: preencha esse campo com o comando de \
             verdade. Até lá, o pedido de cada onda sai sem essa linha."
        }
        ("plan.command_not_declared", Locale::EnUs) => {
            "The project does not declare `{field}` in mustard.json: fill in that field with the real \
             command. Until then, each wave's request goes out without that line."
        }
        // O descarte de uma spec (`commands/flow/discard.rs`).
        ("discard.preview", Locale::PtBr) => {
            "Descartar a spec {spec} fecha o pull request dela, apaga a branch {branch} \
             (no servidor: {remote}) e {what}. \
             Isso não tem volta. Mostre ao usuário e, com o sim dele, repita com o código {token}."
        }
        ("discard.preview", Locale::EnUs) => {
            "Discarding spec {spec} closes its pull request, deletes branch {branch} \
             (on the server: {remote}) and {what}. \
             This cannot be undone. Show it to the user and, once they say yes, run again with code {token}."
        }
        ("discard.archive", Locale::PtBr) => {
            "guarda a pasta da spec, que continua no índice e na página do projeto, marcada como descartada"
        }
        ("discard.archive", Locale::EnUs) => {
            "archives the spec folder, which stays in the index and on the project page, marked as discarded"
        }
        ("discard.delete", Locale::PtBr) => "apaga a pasta da spec e tira a linha dela do índice",
        ("discard.delete", Locale::EnUs) => "deletes the spec folder and drops its line from the index",
        ("discard.yes", Locale::PtBr) => "sim",
        ("discard.yes", Locale::EnUs) => "yes",
        ("discard.no", Locale::PtBr) => "não",
        ("discard.no", Locale::EnUs) => "no",
        ("discard.reason", Locale::PtBr) => "descartada pelo usuário",
        ("discard.reason", Locale::EnUs) => "discarded by the user",
        ("discard.confirm_mismatch", Locale::PtBr) => {
            "O código não é o deste descarte: peça a primeira chamada de novo. Nada saiu."
        }
        ("discard.confirm_mismatch", Locale::EnUs) => {
            "That code is not this discard's: ask for the first call again. Nothing was removed."
        }
        ("discard.incomplete", Locale::PtBr) => {
            "O descarte não terminou: a pasta da spec não saiu do lugar, ou a linha dela no índice não mudou."
        }
        ("discard.incomplete", Locale::EnUs) => {
            "The discard did not finish: the spec folder did not move, or its index line did not change."
        }
        ("discard.done", Locale::PtBr) => "A spec está descartada.",
        ("discard.done", Locale::EnUs) => "The spec is discarded.",

        // A retomada de uma spec (`commands/flow/resume.rs`).
        ("resume.line", Locale::PtBr) => {
            "Retomada: spec {spec}, fase {phase}; último passo: {last}; próximo: {next}."
        }
        ("resume.line", Locale::EnUs) => {
            "Resume: spec {spec}, phase {phase}; last step: {last}; next: {next}."
        }
        ("resume.none", Locale::PtBr) => "nenhum",
        ("resume.none", Locale::EnUs) => "none",
        ("resume.wave", Locale::PtBr) => "onda {n}",
        ("resume.wave", Locale::EnUs) => "wave {n}",
        ("resume.next.survey", Locale::PtBr) => {
            "A spec está no levantamento: rode o levantamento e grave a resposta de cada ponto."
        }
        ("resume.next.survey", Locale::EnUs) => {
            "The spec is in the survey: run the survey and record the answer to each point."
        }
        ("resume.next.plan", Locale::PtBr) => {
            "O plano está gravado: confira o plano, publique a página da spec e faça a pergunta de \
             aprovação na ordem de explicar do estilo de resposta, com o texto exato \"{question}\" e \
             as opções \"{option}\" e \"Ajustar\": com outro texto, a aprovação não vale."
        }
        ("resume.next.plan", Locale::EnUs) => {
            "The plan is recorded: check the plan, publish the spec page and ask the approval \
             question in the order of explaining from the response style, with the exact text \
             \"{question}\" and the options \"{option}\" and \"Adjust\": with another text, the \
             approval does not count."
        }
        ("resume.next.running", Locale::PtBr) => {
            "A spec está aprovada: rode a próxima rodada de ondas."
        }
        ("resume.next.running", Locale::EnUs) => "The spec is approved: run the next round of waves.",
        ("resume.next.closed", Locale::PtBr) => "A spec está fechada: abra o pull request.",
        ("resume.next.closed", Locale::EnUs) => "The spec is closed: open the pull request.",
        ("resume.next.pr_open", Locale::PtBr) => {
            "O pull request está aberto: espere a revisão e faça o merge quando o usuário pedir."
        }
        ("resume.next.pr_open", Locale::EnUs) => {
            "The pull request is open: wait for the review and merge when the user asks."
        }
        ("resume.next.delivered", Locale::PtBr) => "A spec foi entregue: não há passo seguinte.",
        ("resume.next.delivered", Locale::EnUs) => "The spec was delivered: there is no next step.",
        ("resume.next.discarded", Locale::PtBr) => "A spec foi descartada: não há passo seguinte.",
        ("resume.next.discarded", Locale::EnUs) => "The spec was discarded: there is no next step.",

        // O fechamento de uma spec (`commands/flow/close.rs`).
        ("close.not_running", Locale::PtBr) => {
            "A spec está na fase {phase} e não está em execução: só fecha o que estava correndo."
        }
        ("close.not_running", Locale::EnUs) => {
            "The spec is in the {phase} phase and is not running: only work in flight closes."
        }
        ("close.wave_without_commit", Locale::PtBr) => {
            "A onda {wave} não tem commit: refaça a onda {wave} antes de fechar."
        }
        ("close.wave_without_commit", Locale::EnUs) => {
            "Wave {wave} has no commit: redo wave {wave} before closing."
        }
        ("close.wave_rejected", Locale::PtBr) => {
            "A última revisão da onda {wave} reprovou: refaça a onda {wave} antes de fechar."
        }
        ("close.wave_rejected", Locale::EnUs) => {
            "The last review of wave {wave} rejected it: redo wave {wave} before closing."
        }
        ("close.request_not_delivered", Locale::PtBr) => {
            "O pedido {code} chegou depois da última entrega e nenhuma onda o entregou: \
             leve-o para uma onda antes de fechar."
        }
        ("close.request_not_delivered", Locale::EnUs) => {
            "Request {code} arrived after the last delivery and no wave delivered it: \
             take it into a wave before closing."
        }
        ("close.criterion_failed", Locale::PtBr) => {
            "A prova do critério {code} não passou: {output}"
        }
        ("close.criterion_failed", Locale::EnUs) => "The proof of criterion {code} did not pass: {output}",
        ("close.criterion_ran_no_test", Locale::PtBr) => {
            "A prova do critério {code} saiu verde sem rodar teste nenhum: `{command}` diz que rodou \
             {count} testes. Grave a versão nova do critério com a prova certa e feche de novo."
        }
        ("close.criterion_ran_no_test", Locale::EnUs) => {
            "The proof of criterion {code} came out green without running any test: `{command}` says \
             it ran {count} tests. Record the criterion's new version with the right proof and close again."
        }
        ("close.lint_failed", Locale::PtBr) => {
            "O lint do projeto (`{command}`) não passou, e a spec não fechou: {output}"
        }
        ("close.lint_failed", Locale::EnUs) => {
            "The project lint (`{command}`) did not pass, and the spec did not close: {output}"
        }
        ("close.final_review", Locale::PtBr) => {
            "A máquina passou: antes do pull request, despache ao agente de teste dedicado o pedido \
             em `review.prompt` e feche de novo com a linha do fim dele, como veio: \
             `mustard-rt run close --spec {spec} --report '<VERDICT>…</VERDICT>'`."
        }
        ("close.final_review", Locale::EnUs) => {
            "The machine passed: before the pull request, dispatch the request in `review.prompt` to \
             the dedicated test agent, and close again with its closing line, as it came: \
             `mustard-rt run close --spec {spec} --report '<VERDICT>…</VERDICT>'`."
        }
        ("close.next", Locale::PtBr) => "Depois, abra o pull request: `{command}`.",
        ("close.next", Locale::EnUs) => "Then open the pull request: `{command}`.",
        // A pergunta de destino de uma pendência ligada à spec `{spec}`: a
        // mesma linha que a gravação de uma pendência devolve na hora, e que
        // o fechamento repete para cada uma que a spec ainda deixa aberta.
        ("close.pending_destination", Locale::PtBr) => {
            "A pendência {id} ({title}) está ligada à spec {spec}: ela entra nesta obra, fica para \
             depois com o motivo, ou sai?"
        }
        ("close.pending_destination", Locale::EnUs) => {
            "The pending item {id} ({title}) is linked to the spec {spec}: does it enter this work, \
             stay for later with a reason, or go?"
        }
        // O item combinado sem dono que nenhum envio de onda levou: o
        // fechamento avisa em vez de deixá-lo fora do código para sempre.
        ("close.unowned_item", Locale::PtBr) => {
            "O item {code} ({title}) não tem dono e nenhuma onda o levou: fica fora do código."
        }
        ("close.unowned_item", Locale::EnUs) => {
            "Item {code} ({title}) has no owner and no wave carried it: it stays out of the code."
        }

        // A rodada de ondas (`commands/flow/round.rs`).
        ("round.bad_report", Locale::PtBr) => {
            "O relatório da rodada não se entende: {detail}. Nada foi gravado."
        }
        ("round.bad_report", Locale::EnUs) => {
            "The round report cannot be read: {detail}. Nothing was recorded."
        }
        ("round.line_field", Locale::PtBr) => {
            "Uma linha `<{line}>` do relatório não traz o campo `{field}`: peça ao agente a linha \
             inteira, como o texto dele ensina. Nada foi gravado."
        }
        ("round.line_field", Locale::EnUs) => {
            "A `<{line}>` line of the report lacks the `{field}` field: ask the agent for the whole \
             line, as its text teaches. Nothing was recorded."
        }
        ("round.merge_conflict", Locale::PtBr) => {
            "A entrega da onda {wave} conflita com o repositório principal nestes trechos: {conflicts}. \
             Nada dela foi gravado. Resolva na cópia {copy}: leve-a ao commit atual com \
             `git -C {copy} checkout --merge --detach {head}`, acerte os trechos marcados e rode a \
             rodada de novo só com a linha `DELIVERED` da onda {wave}."
        }
        ("round.merge_conflict", Locale::EnUs) => {
            "Wave {wave}'s delivery conflicts with the main repository in these hunks: {conflicts}. \
             Nothing of it was recorded. Resolve it in the copy {copy}: bring it to the current commit \
             with `git -C {copy} checkout --merge --detach {head}`, fix the marked hunks and run the \
             round again with only wave {wave}'s `DELIVERED` line."
        }
        ("round.copy_failed", Locale::PtBr) => {
            "A cópia da onda {wave} não pôde ser criada: {detail}. A onda não saiu nesta rodada; \
             corrija e rode a rodada de novo."
        }
        ("round.copy_failed", Locale::EnUs) => {
            "Wave {wave}'s copy could not be created: {detail}. The wave did not go out this round; \
             fix it and run the round again."
        }
        ("round.copy_kept", Locale::PtBr) => {
            "A cópia da onda {wave}, {copy}, ficou no disco: {files} mudou nela e não estava na \
             entrega. Leve o que servir ao repositório principal e apague a cópia com \
             `git worktree remove --force {copy}`."
        }
        ("round.copy_kept", Locale::EnUs) => {
            "Wave {wave}'s copy, {copy}, stayed on disk: {files} changed in it and was not in the \
             delivery. Bring what is useful to the main repository and delete the copy with \
             `git worktree remove --force {copy}`."
        }
        // O que um agente deixou preso, encerrado no início da sessão, em
        // cada rodada e no fechamento (`apps/rt/src/commands/flow/stuck.rs`).
        ("stuck.ended", Locale::PtBr) => "Processo(s) preso(s) encerrado(s): {list}.",
        ("stuck.ended", Locale::EnUs) => "Stuck process(es) ended: {list}.",
        ("stuck.reason.waiting_loop", Locale::PtBr) => "laço de espera",
        ("stuck.reason.waiting_loop", Locale::EnUs) => "waiting loop",
        ("stuck.reason.deleted_copy", Locale::PtBr) => "cópia de onda apagada",
        ("stuck.reason.deleted_copy", Locale::EnUs) => "deleted wave copy",
        // O tamanho da conversa: a ordem de pausa ao agente de onda e o
        // aviso de compactar ao orquestrador.
        ("conversation_size.pause", Locale::PtBr) => {
            "A conversa passou de 200 mil tokens. Grave o passo da onda {wave} na spec e pare com a \
             linha `<PAUSED>{\"wave\":{wave}}</PAUSED>`."
        }
        ("conversation_size.pause", Locale::EnUs) => {
            "The conversation passed 200 thousand tokens. Save the step of wave {wave} on the spec and \
             stop with the line `<PAUSED>{\"wave\":{wave}}</PAUSED>`."
        }
        ("conversation_size.compact", Locale::PtBr) => {
            "Esta conversa passou de mais um degrau de 200 mil tokens. Rode `/compact` e, depois, \
             {command}. O que fica: spec {spec}, fase {phase}. {next}"
        }
        ("conversation_size.compact", Locale::EnUs) => {
            "This conversation passed another 200-thousand-token step. Run `/compact` and, after, \
             {command}. What stays: spec {spec}, phase {phase}. {next}"
        }
        ("conversation_size.compact_running", Locale::PtBr) => {
            "Esta conversa passou de mais um degrau de 200 mil tokens. As ondas {waves} estão em \
             andamento; a volta delas chega pela rodada. Rode `/compact` quando puder."
        }
        ("conversation_size.compact_running", Locale::EnUs) => {
            "This conversation passed another 200-thousand-token step. Waves {waves} are in flight; \
             their return comes through the round. Run `/compact` when you can."
        }
        ("round.file_unknown", Locale::PtBr) => {
            "A onda {wave} entregou {file}, que não está no disco nem no git: o commit não teria o que \
             levar. Peça ao agente o caminho certo. Nada foi gravado."
        }
        ("round.file_unknown", Locale::EnUs) => {
            "Wave {wave} delivered {file}, which is neither on disk nor in git: the commit would have \
             nothing to take. Ask the agent for the right path. Nothing was recorded."
        }
        ("round.proof_ran_no_test", Locale::PtBr) => {
            "A prova nova do critério {code} saiu verde sem rodar teste nenhum: o nome do teste não \
             casa. Peça a prova certa antes de fechar."
        }
        ("round.proof_ran_no_test", Locale::EnUs) => {
            "The new proof of criterion {code} came out green without running any test: the test name \
             does not match. Ask for the right proof before closing."
        }
        ("round.commit.scope.one", Locale::PtBr) => "onda-{waves}",
        ("round.commit.scope.one", Locale::EnUs) => "wave-{waves}",
        ("round.commit.scope.many", Locale::PtBr) => "ondas-{waves}",
        ("round.commit.scope.many", Locale::EnUs) => "waves-{waves}",
        ("round.commit.line", Locale::PtBr) => "- onda {wave}: {summary}",
        ("round.commit.line", Locale::EnUs) => "- wave {wave}: {summary}",
        ("round.commit.fixes", Locale::PtBr) => "(conserta: onda {waves})",
        ("round.commit.fixes", Locale::EnUs) => "(fixes: wave {waves})",
        ("round.not_approved", Locale::PtBr) => {
            "A spec está na fase {phase} e ainda não foi aprovada: nenhuma onda sai antes do sim do usuário."
        }
        ("round.not_approved", Locale::EnUs) => {
            "The spec is in the {phase} phase and is not approved yet: no wave goes out before the user says yes."
        }
        ("round.delivered_too_long", Locale::PtBr) => {
            "O que a onda {wave} entregou tem {chars} caracteres e o teto é {max}. Encurte o relato."
        }
        ("round.delivered_too_long", Locale::EnUs) => {
            "What wave {wave} delivered has {chars} characters and the cap is {max}. Shorten the report."
        }
        ("round.commit_too_long", Locale::PtBr) => {
            "O {part} da mensagem do commit tem {chars} caracteres e o teto é {max}."
        }
        ("round.commit_too_long", Locale::EnUs) => {
            "The commit message {part} has {chars} characters and the cap is {max}."
        }
        ("round.commit_forbidden", Locale::PtBr) => {
            "A mensagem do commit traz {found}, que ela nunca leva. Tire e peça a rodada de novo."
        }
        ("round.commit_forbidden", Locale::EnUs) => {
            "The commit message carries {found}, which it never carries. Take it out and run the round again."
        }
        ("round.formatter_missing", Locale::PtBr) => {
            "O projeto usa {name} e ele não foi achado: os arquivos da rodada ficaram sem formatar."
        }
        ("round.formatter_missing", Locale::EnUs) => {
            "The project uses {name} and it was not found: the round's files were left unformatted."
        }
        ("round.replan", Locale::PtBr) => {
            "A onda {wave} diz que o plano dela não funciona. Mudança proposta: {change}. \
             Mostre a mudança ao usuário e faça a pergunta com opções \"{question}\", com \
             \"{yes}\" e \"{no}\". O sim é o clique em \"{yes}\": depois dele, repita a \
             rodada com o mesmo relatório."
        }
        ("round.replan", Locale::EnUs) => {
            "Wave {wave} says its plan does not work. Proposed change: {change}. \
             Show the change to the user and ask the question with options \"{question}\", \
             with \"{yes}\" and \"{no}\". The yes is the click on \"{yes}\": after it, run \
             the round again with the same report."
        }
        ("round.git_refused", Locale::PtBr) => {
            "O git recusou o commit da rodada: {detail}\nNada foi gravado. Corrija o que o git \
             apontou, ou peça ao agente a linha corrigida, e rode a rodada de novo."
        }
        ("round.git_refused", Locale::EnUs) => {
            "Git refused the round's commit: {detail}\nNothing was recorded. Fix what git pointed \
             out, or ask the agent for the corrected line, and run the round again."
        }
        ("round.next", Locale::PtBr) => {
            "Despache os pedidos desta rodada: cada onda ao agente `mustard-wave` e cada revisão ao \
             agente `mustard-review`."
        }
        ("round.next", Locale::EnUs) => {
            "Dispatch this round's requests: each wave to the `mustard-wave` agent and each review \
             to the `mustard-review` agent."
        }
        // A obra de até 3 pontos: sem cópia separada e sem agente, é o
        // orquestrador — a própria conversa que chamou a rodada — quem faz a
        // onda, na própria janela, no checkout principal.
        ("round.next.solo", Locale::PtBr) => {
            "Faça a onda desta rodada você mesmo, nesta janela, no checkout principal e sem cópia \
             separada: leia o pedido abaixo e implemente."
        }
        ("round.next.solo", Locale::EnUs) => {
            "Do this round's wave yourself, in this window, on the main checkout and without a \
             separate copy: read the request below and implement it."
        }
        // O pedido de publicar e copiar a página fica por extenso só no
        // arquivo, e a resposta leva a linha curta.
        ("round.next.copy_file", Locale::PtBr) => "Leia `{path}` e siga as instruções de lá.",
        ("round.next.copy_file", Locale::EnUs) => "Read `{path}` and follow the instructions there.",
        ("round.report", Locale::PtBr) => {
            "Quando voltarem, rode a rodada de novo com a linha do fim de cada agente, como ela veio, \
             uma por linha, todas no mesmo `--report '…'`: a do agente de onda é \
             `<DELIVERED>{\"wave\":1,\"text\":\"…\",\"files\":[\"…\"],\"commit\":\"…\"}</DELIVERED>`, e a do revisor, \
             `<VERDICT>{\"wave\":1,\"result\":\"approved\",\"text\":\"…\",\"criteria\":[…]}</VERDICT>`. A rodada lê só \
             essas linhas e monta o commit do `commit` de cada entrega."
        }
        ("round.report", Locale::EnUs) => {
            "When they come back, run the round again with each agent's closing line, as it came, one \
             per line, all in the same `--report '…'`: the wave agent's is \
             `<DELIVERED>{\"wave\":1,\"text\":\"…\",\"files\":[\"…\"],\"commit\":\"…\"}</DELIVERED>`, and the reviewer's, \
             `<VERDICT>{\"wave\":1,\"result\":\"approved\",\"text\":\"…\",\"criteria\":[…]}</VERDICT>`. The round reads \
             only those lines and builds the commit from each delivery's `commit`."
        }
        ("round.waiting", Locale::PtBr) => {
            "Nada novo a despachar nem a revisar: as ondas {waves} estão em andamento, e o pedido \
             delas já saiu."
        }
        ("round.waiting", Locale::EnUs) => {
            "Nothing new to dispatch or review: waves {waves} are in flight, and their requests \
             already went out."
        }
        ("round.close", Locale::PtBr) => {
            "Todas as ondas estão entregues e aprovadas: feche a spec com `{command}`."
        }
        ("round.close", Locale::EnUs) => {
            "Every wave is delivered and approved: close the spec with `{command}`."
        }
        ("round.missing", Locale::PtBr) => {
            "Nada a despachar nem a revisar, e a onda {wave} ainda não está entregue e aprovada: \
             mostre ao usuário o que a segura antes de fechar."
        }
        ("round.missing", Locale::EnUs) => {
            "Nothing to dispatch or review, and wave {wave} is not delivered and approved yet: \
             show the user what holds it before closing."
        }
        ("round.fix_limit", Locale::PtBr) => {
            "A onda {wave} foi reprovada {count} vezes seguidas, e o limite é de {max} rodadas de \
             conserto: a rodada não a manda de novo, nem as ondas que dependem dela, e o resto \
             segue. Mostre ao usuário os vereditos {verdicts} e faça a pergunta desta onda em \
             `stopped`. As saídas são duas: revisar o plano dela, e a versão nova zera a conta; ou \
             tirá-la do plano, com a onda e as tarefas dela, e os vereditos dela deixam de contar."
        }
        ("round.fix_limit", Locale::EnUs) => {
            "Wave {wave} was rejected {count} times in a row, and the limit is {max} fix rounds: \
             the round does not send it again, nor the waves that depend on it, and the rest goes \
             on. Show the user the verdicts {verdicts} and ask this wave's question in `stopped`. \
             There are two ways out: revise its plan, and the new version resets the count; or take \
             it out of the plan, with its wave and tasks, and its verdicts stop counting."
        }
        ("round.fix_limit.question", Locale::PtBr) => {
            "A onda {wave} foi reprovada de novo depois de {max} rodadas de conserto. Revisar o plano dela ou tirá-la do plano?"
        }
        ("round.fix_limit.question", Locale::EnUs) => {
            "Wave {wave} was rejected again after {max} fix rounds. Revise its plan or take it out of the plan?"
        }
        // A escolha do pedido antes do envio, que o orquestrador faz: o
        // próximo passo, com a linha que devolve a escolha, e os avisos.
        ("round.analysis", Locale::PtBr) => {
            "Antes de soltar as ondas {waves}, escolha os itens do pedido de cada uma. O pedido é a \
             lista dos itens da spec que o agente da onda lê; os itens que as tarefas da onda fazem \
             vão sempre. Em `analysis`, cada onda traz os candidatos, cada um com o título: as \
             regras do projeto todo (`project`) e as lições (`lessons`) vão, a menos que você tire; \
             os itens sem dono (`unowned`) ficam fora, a menos que você ponha. Tire o que não ajuda \
             a onda, como uma regra da entrega numa onda que só cria uma tabela, e ponha o item sem \
             dono que ajuda. A conferência das tarefas no código — achar se o que cada uma pede já \
             está feito ou ainda falta — vai a um agente separado, que devolve só a tarefa ajustada \
             para ser gravada: você não lê arquivo inteiro nem saída longa para isso. Rode a rodada \
             de novo com uma linha por onda no `--report '…'`, só com o que muda e o motivo de cada \
             um numa frase: <ANALYSIS>{\"wave\":<n>,\"removed\":[{\"item\":\"<código>\",\"why\":\"<o \
             motivo>\"},{\"lesson\":<número>,\"why\":\"<o motivo>\"}],\"added\":[{\"item\":\"<código>\
             \",\"why\":\"<o motivo>\"}]}</ANALYSIS>. Sem mudança, as duas listas vão vazias. Sem \
             essa linha, a onda não sai."
        }
        ("round.analysis", Locale::EnUs) => {
            "Before sending out waves {waves}, choose the items of each one's request. The request \
             is the list of spec items the wave agent reads; the items the wave's tasks do always \
             go. In `analysis`, each wave brings its candidates, each with its title: the \
             whole-project rules (`project`) and the lessons (`lessons`) go unless you take them \
             out; the items without an owner (`unowned`) stay out unless you put them in. Take out \
             what does not help the wave, such as a rule about the delivery in a wave that only \
             creates a table, and put in the item without an owner that helps. Checking the tasks \
             against the code — finding whether what each one asks is already done or still missing \
             — goes to a separate agent, which returns only the adjusted task to record: you do not \
             read a whole file nor long output for this. Run the round again with one line per wave \
             in the `--report '…'`, with only what changes and each one's reason in one sentence: \
             <ANALYSIS>{\"wave\":<n>,\"removed\":[{\"item\":\"<item code>\",\"why\":\"<the \
             reason>\"},{\"lesson\":<number>,\"why\":\"<the reason>\"}],\"added\":[{\"item\":\"<item \
             code>\",\"why\":\"<the reason>\"}]}</ANALYSIS>. With no change, both lists go empty. \
             Without that line, the wave does not go out."
        }
        ("round.analysis_ignored", Locale::PtBr) => {
            "Na escolha da onda {wave}, o item {item} ficou como estava: ele não está entre os \
             candidatos dela, a lição só sai e não entra, ou ele veio sem motivo."
        }
        ("round.analysis_ignored", Locale::EnUs) => {
            "In the choice for wave {wave}, item {item} stayed as it was: it is not among the \
             wave's candidates, a lesson can only go out and not in, or it came without a reason."
        }
        ("round.analysis_unreadable", Locale::PtBr) => {
            "Uma linha `<ANALYSIS>` não se leu e ficou de fora ({detail}): a onda dela pede a \
             escolha de novo."
        }
        ("round.analysis_unreadable", Locale::EnUs) => {
            "An `<ANALYSIS>` line could not be read and was left out ({detail}): its wave asks for \
             the choice again."
        }
        // A retomada: a onda pausada ou órfã sai de novo com o pedido
        // anterior, mais os passos gravados e o aviso de ver o que mudou na
        // cópia; e a onda viva sem sinal por 40 minutos vira aviso.
        ("round.resume.steps", Locale::PtBr) => "Passos já gravados desta onda:",
        ("round.resume.steps", Locale::EnUs) => "Steps already recorded for this wave:",
        ("round.resume.notice", Locale::PtBr) => "Comece vendo o que mudou na cópia.",
        ("round.resume.notice", Locale::EnUs) => "Start by seeing what changed in the copy.",
        ("round.resume.silent", Locale::PtBr) => {
            "A onda {wave} está sem sinal de vida há mais de 40 minutos. Pare o agente dela e mande \
             `<PAUSED>{\"wave\":{wave}}</PAUSED>` na próxima rodada, para reenviá-la."
        }
        ("round.resume.silent", Locale::EnUs) => {
            "Wave {wave} has had no sign of life for more than 40 minutes. Stop its agent and send \
             `<PAUSED>{\"wave\":{wave}}</PAUSED>` on the next round, to resend it."
        }
        ("plan.finding.label", Locale::PtBr) => "achado do plano",
        ("plan.finding.label", Locale::EnUs) => "plan finding",
        ("approve_spec.open_points", Locale::PtBr) => "Pontos do levantamento ainda abertos ({count}): {points}",
        ("approve_spec.open_points", Locale::EnUs) => "Survey points still open ({count}): {points}",
        ("open.choose_kind", Locale::PtBr) => {
            "Falta o tipo. Pergunte ao usuário o tipo da branch, como feature ou fix. Nada foi criado."
        }
        ("open.choose_kind", Locale::EnUs) => {
            "The kind is missing. Ask the user for the branch kind, such as feature or fix. Nothing \
             was created."
        }
        ("open.choose_name", Locale::PtBr) => {
            "Falta o nome. Pergunte ao usuário o nome da spec; ele vira a branch {kind}/<nome>. Nada \
             foi criado."
        }
        ("open.choose_name", Locale::EnUs) => {
            "The name is missing. Ask the user for the spec's name; it becomes the branch \
             {kind}/<name>. Nothing was created."
        }
        ("open.choose_base", Locale::PtBr) => {
            "Falta a base. Pergunte ao usuário de qual branch a spec sai; as candidatas estão em \
             `candidates`. Nada foi criado."
        }
        ("open.choose_base", Locale::EnUs) => {
            "The base is missing. Ask the user which branch the spec starts from; the candidates are \
             in `candidates`. Nothing was created."
        }
        ("open.confirm_name", Locale::PtBr) => {
            "O git não aceita \"{asked}\" como está. Mostre ao usuário o nome ajustado, \
             \"{adjusted}\", e, com o sim dele, chame o open de novo com esse nome. Nada foi criado."
        }
        ("open.confirm_name", Locale::EnUs) => {
            "Git does not accept \"{asked}\" as it is. Show the user the adjusted name, \
             \"{adjusted}\", and, with their yes, call open again with that name. Nothing was \
             created."
        }
        ("open.ask_goal", Locale::PtBr) => {
            "Qual o objetivo, numa frase? Pode ser a sua ou a que eu sugerir, se você aprovar. Junto \
             dele, mande o card, os critérios de aceite e os documentos antigos, se tiver."
        }
        ("open.ask_goal", Locale::EnUs) => {
            "What is the goal, in one sentence? It can be yours, or the one I suggest, if you approve \
             it. Along with it, send the card, the acceptance criteria and the old documents, if you \
             have them."
        }
        ("open.next_goal", Locale::PtBr) => {
            "A spec {spec} nasceu na branch {branch}. Faça ao usuário a pergunta de `question` e \
             espere a resposta. O objetivo da spec é uma frase que diz o que ele pediu, a dele ou a \
             que você sugeriu e ele aprovou; grave-o como o primeiro `context`, com `origin` na \
             mensagem dele. O card, os critérios de aceite e os documentos antigos que vierem junto \
             vão logo depois, como `context`, com o mesmo `origin`."
        }
        ("open.next_goal", Locale::EnUs) => {
            "Spec {spec} was born on branch {branch}. Ask the user the question in `question` and \
             wait for the answer. The spec's goal is one sentence saying what they asked for, theirs \
             or the one you suggested and they approved; record it as the first `context`, with \
             `origin` on their message. The card, the acceptance criteria and the old documents that \
             come along go right after it, as `context`, with the same `origin`."
        }
        ("open.no_flow", Locale::PtBr) => {
            "O mustard.json não declara as bases (git.flow): as candidatas são as branches do \
             repositório, e nenhuma fica protegida."
        }
        ("open.no_flow", Locale::EnUs) => {
            "mustard.json declares no bases (git.flow): the candidates are the repository's \
             branches, and none is protected."
        }
        ("open.map_warning", Locale::PtBr) => {
            "O mapa do projeto não foi atualizado: {detail}. A spec foi aberta assim mesmo; rode \
             `mustard-rt run scan` depois."
        }
        ("open.map_warning", Locale::EnUs) => {
            "The project map was not refreshed: {detail}. The spec was opened anyway; run \
             `mustard-rt run scan` later."
        }
        ("open.kind_invalid", Locale::PtBr) => {
            "\"{kind}\" não serve como tipo de branch: use só letras minúsculas, números, - ou _, \
             como feature ou fix. Nada foi criado."
        }
        ("open.kind_invalid", Locale::EnUs) => {
            "\"{kind}\" cannot be a branch kind: use only lowercase letters, digits, - or _, such as \
             feature or fix. Nothing was created."
        }
        ("open.name_empty", Locale::PtBr) => {
            "O nome \"{asked}\" fica vazio depois do ajuste que o git pede. Peça ao usuário um nome \
             com letras ou números. Nada foi criado."
        }
        ("open.name_empty", Locale::EnUs) => {
            "The name \"{asked}\" is empty after the adjustment git requires. Ask the user for a name \
             with letters or digits. Nothing was created."
        }
        ("open.base_not_found", Locale::PtBr) => {
            "A branch {base} não existe neste repositório. Escolha uma destas: {candidates}. Nada foi \
             criado."
        }
        ("open.base_not_found", Locale::EnUs) => {
            "Branch {base} does not exist in this repository. Pick one of these: {candidates}. \
             Nothing was created."
        }
        ("open.branch_taken", Locale::PtBr) => {
            "A branch {branch} já existe. Escolha outro nome, ou retome a spec dela. Nada foi criado."
        }
        ("open.branch_taken", Locale::EnUs) => {
            "Branch {branch} already exists. Pick another name, or resume its spec. Nothing was \
             created."
        }
        ("open.spec_taken", Locale::PtBr) => {
            "Já existe uma spec chamada {spec}. Escolha outro nome, ou retome essa spec. Nada foi \
             criado."
        }
        ("open.spec_taken", Locale::EnUs) => {
            "A spec named {spec} already exists. Pick another name, or resume that spec. Nothing was \
             created."
        }
        ("open.tree_busy", Locale::PtBr) => {
            "O checkout tem mudanças que não são desta spec: {paths}. Faça o commit delas, ou \
             guarde-as, antes de abrir a spec. Nada foi criado."
        }
        ("open.tree_busy", Locale::EnUs) => {
            "The checkout holds changes that are not this spec's: {paths}. Commit them, or set them \
             aside, before opening the spec. Nothing was created."
        }
        ("open.git_failed", Locale::PtBr) => {
            "O git recusou criar a branch {branch}: {detail}. Nada foi gravado na spec."
        }
        ("open.git_failed", Locale::EnUs) => {
            "Git refused to create branch {branch}: {detail}. Nothing was written to the spec."
        }
        ("open.unborn_branch", Locale::PtBr) => {
            "O checkout está numa branch sem nenhum commit, e o open não teria para onde voltar se \
             a gravação da spec falhasse. Faça o primeiro commit nesta branch, ou saia para uma \
             branch com commit, como a {base}, e rode o open de novo. Nada foi criado."
        }
        ("open.unborn_branch", Locale::EnUs) => {
            "The checkout is on a branch without any commit, so open would have nowhere to go back \
             to if writing the spec failed. Make the first commit on this branch, or switch to a \
             branch with a commit, such as {base}, and run open again. Nothing was created."
        }
        ("open.pending_unknown", Locale::PtBr) => {
            "A pendência {pending} não existe na lista. Confira o número com `mustard-rt run pending`. \
             Nada foi criado."
        }
        ("open.pending_unknown", Locale::EnUs) => {
            "Pending item {pending} is not on the list. Check the number with `mustard-rt run pending`. \
             Nothing was created."
        }
        ("open.pending_closed", Locale::PtBr) => {
            "A pendência {pending} já está fechada ou descartada, e só uma pendência aberta vira spec. \
             Reabra-a com `mustard-rt run pending --reopen {pending}`, ou abra a spec sem ela. Nada \
             foi criado."
        }
        ("open.pending_closed", Locale::EnUs) => {
            "Pending item {pending} is already closed or dropped, and only an open item becomes a spec. \
             Reopen it with `mustard-rt run pending --reopen {pending}`, or open the spec without it. \
             Nothing was created."
        }
        ("open.pending_note_failed", Locale::PtBr) => {
            "A spec {spec} foi aberta, mas a nota \"virou a spec {spec}\" não entrou na pendência \
             {pending}, e o merge não vai fechá-la sozinho. Chame o open de novo, com os mesmos \
             dados, para gravar a nota."
        }
        ("open.pending_note_failed", Locale::EnUs) => {
            "Spec {spec} was opened, but the note \"became spec {spec}\" did not reach pending item \
             {pending}, so the merge will not close it by itself. Call open again with the same \
             arguments to record the note."
        }
        // O aviso, na abertura, das pendências do projeto (sem dono de obra):
        // quantas são e onde a lista inteira está. `{count}` e `{list}` vêm
        // de quem chama; é aviso, nunca recusa.
        ("open.pending_project.one", Locale::PtBr) => {
            "1 pendência do projeto esperando. {list} Quer resolver alguma nesta obra?"
        }
        ("open.pending_project.one", Locale::EnUs) => {
            "1 project pending item waiting. {list} Do you want to work on any of them in this unit?"
        }
        ("open.pending_project.many", Locale::PtBr) => {
            "{count} pendências do projeto esperando. {list} Quer resolver alguma nesta obra?"
        }
        ("open.pending_project.many", Locale::EnUs) => {
            "{count} project pending items waiting. {list} Do you want to work on any of them in this unit?"
        }
        // O trecho de onde a lista inteira está: com o endereço da página do
        // projeto, quando ele já foi gravado, ou o comando que a mostra,
        // senão. `{url}` vem de quem chama.
        ("open.pending_project.list_page", Locale::PtBr) => {
            "A lista inteira está na página do projeto: {url}."
        }
        ("open.pending_project.list_page", Locale::EnUs) => {
            "The whole list is on the project page: {url}."
        }
        ("open.pending_project.list_command", Locale::PtBr) => {
            "A lista inteira sai com `mustard-rt run pending`."
        }
        ("open.pending_project.list_command", Locale::EnUs) => {
            "The whole list comes from `mustard-rt run pending`."
        }
        ("retired.wait_round", Locale::PtBr) => {
            "O `{command}` não grava mais o veredito na spec: o veredito de cada onda é gravado \
             pelo `mustard-rt run round`. Nada foi gravado."
        }
        ("retired.wait_round", Locale::EnUs) => {
            "`{command}` no longer records the verdict in the spec: each wave's verdict is \
             recorded by `mustard-rt run round`. Nothing was written."
        }
        ("pr.qa_pending", Locale::PtBr) => {
            "Nem todo critério de `{spec}` tem uma execução aprovada: {passed} de {criteria} \
             passaram. A ordem do fluxo roda os critérios ANTES da integração, e integrar agora \
             integra trabalho que ninguém conferiu."
        }
        ("pr.qa_pending", Locale::EnUs) => {
            "Not every criterion of `{spec}` has a passing run: {passed} of {criteria} passed. The \
             flow runs the criteria BEFORE integration, and integrating now integrates work \
             nobody checked."
        }
        // Os pull requests de uma spec que mexe em submódulo: o principal fica
        // como rascunho até os dos submódulos entrarem.
        ("pr.submodules.waiting", Locale::PtBr) => {
            "O pull request #{pr} do principal segue como rascunho: falta entrar o pull request do \
             submódulo {paths}."
        }
        ("pr.submodules.waiting", Locale::EnUs) => {
            "The main pull request #{pr} stays a draft: the pull request of the submodule {paths} \
             has not been merged yet."
        }
        ("pr.submodules.ready", Locale::PtBr) => {
            "O pull request do submódulo {paths} entrou: o ponteiro foi atualizado e enviado, e o \
             pull request #{pr} do principal ficou pronto."
        }
        ("pr.submodules.ready", Locale::EnUs) => {
            "The pull request of the submodule {paths} was merged: the pointer was updated and \
             pushed, and the main pull request #{pr} is ready."
        }
        ("pr.submodules.stuck", Locale::PtBr) => {
            "O pull request #{pr} do principal segue como rascunho: {reason}. A próxima conferência \
             tenta de novo."
        }
        ("pr.submodules.stuck", Locale::EnUs) => {
            "The main pull request #{pr} stays a draft: {reason}. The next check tries again."
        }
        ("pr.pointer_commit", Locale::PtBr) => "chore(submódulo): atualiza o ponteiro",
        ("pr.pointer_commit", Locale::EnUs) => "chore(submodule): update the pointer",
        ("message.too_long", Locale::PtBr) => {
            "A parte `{part}` da mensagem tem {chars} caracteres e o limite é {max}. Escreva outro: o \
             corte automático mentiria sobre o que a mensagem diz. Nada foi enviado."
        }
        ("message.too_long", Locale::EnUs) => {
            "The message part `{part}` has {chars} characters and the limit is {max}. Write another one: \
             truncating would misstate what the message says. Nothing was sent."
        }
        ("message.forbidden", Locale::PtBr) => {
            "A mensagem traz `{found}`, que nunca vai num commit nem num pull request. O trecho: \
             \"{excerpt}\". Tire e mande de novo; nada foi enviado."
        }
        ("message.forbidden", Locale::EnUs) => {
            "The message carries `{found}`, which never goes into a commit or a pull request. The \
             excerpt: \"{excerpt}\". Remove it and send again; nothing was sent."
        }
        ("message.no_title", Locale::PtBr) => {
            "Esta spec não tem objetivo escrito, e é dele que sai o título do pull request. \
             Escreva o contexto da spec primeiro. Nada foi enviado."
        }
        ("message.no_title", Locale::EnUs) => {
            "This spec has no goal written down, and the pull request title comes from it. Write \
             the spec's context first. Nothing was sent."
        }
        ("reopen.reason_missing", Locale::PtBr) => {
            "A volta ao levantamento precisa do motivo: passe `--reason` com uma frase dizendo por \
             que a spec volta. O motivo fica gravado no evento da volta. Nada foi gravado."
        }
        ("reopen.reason_missing", Locale::EnUs) => {
            "Going back to the survey needs a reason: pass `--reason` with one sentence saying why \
             the spec goes back. The reason is written into the event. Nothing was written."
        }
        ("reopen.settled", Locale::PtBr) => {
            "A spec {spec} está na fase {phase} e não volta ao levantamento: o que ela decidiu já \
             saiu. Abra uma spec nova com `mustard-rt run open`. Nada foi gravado."
        }
        ("reopen.settled", Locale::EnUs) => {
            "The spec {spec} is in the phase {phase} and does not go back to the survey: what it \
             decided is already out. Open a new spec with `mustard-rt run open`. Nothing was \
             written."
        }
        ("reopen.next", Locale::PtBr) => {
            "A spec {spec} voltou ao levantamento, e o motivo ficou gravado. Rode `mustard-rt run \
             grill --spec {spec} --kinds <tipos>` para montar os pontos: os novos convivem com o \
             que já foi decidido, e nada do que estava gravado foi apagado."
        }
        ("reopen.next", Locale::EnUs) => {
            "The spec {spec} is back in the survey, and the reason is on the record. Run \
             `mustard-rt run grill --spec {spec} --kinds <types>` to build the points: the new ones \
             live alongside what was already decided, and nothing written was erased."
        }
        ("reopen.already", Locale::PtBr) => {
            "A spec {spec} já está em levantamento, e nada foi gravado. Rode `mustard-rt run grill \
             --spec {spec} --kinds <tipos>` para montar os pontos."
        }
        ("reopen.already", Locale::EnUs) => {
            "The spec {spec} is already under survey, and nothing was written. Run `mustard-rt run \
             grill --spec {spec} --kinds <types>` to build the points."
        }
        ("request.new_waves", Locale::PtBr) => {
            "Pedido gravado. Grave as ondas novas no fim do plano; a spec e a branch continuam as \
             mesmas, e não há nova aprovação."
        }
        ("request.new_waves", Locale::EnUs) => {
            "Request recorded. Record the new waves at the end of the plan; the spec and the branch \
             stay the same, and there is no new approval."
        }
        ("request.adjust_waves", Locale::PtBr) => {
            "Pedido gravado. Grave as versões novas das ondas que mudam, com `replaces`; a spec e a \
             branch continuam as mesmas, e não há nova aprovação."
        }
        ("request.adjust_waves", Locale::EnUs) => {
            "Request recorded. Record the new versions of the waves that change, with `replaces`; \
             the spec and the branch stay the same, and there is no new approval."
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
            include_str!("flow.rs"),
            super::PREFIXES,
            136,
            0x9d88_70e3_58cd_ff48,
        );
    }

    /// Os passos, as recusas e os avisos da abertura de uma spec saem do
    /// catálogo nos dois idiomas, cada um com as vagas que o chamador preenche.
    #[test]
    fn i18n_translates_open_keys() {
        for (key, slots) in [
            ("open.choose_kind", &[][..]),
            ("open.choose_name", &["{kind}"][..]),
            ("open.choose_base", &[][..]),
            ("open.confirm_name", &["{asked}", "{adjusted}"][..]),
            ("open.ask_goal", &[][..]),
            ("open.next_goal", &["{spec}", "{branch}"][..]),
            ("open.no_flow", &[][..]),
            ("open.map_warning", &["{detail}"][..]),
            ("open.kind_invalid", &["{kind}"][..]),
            ("open.name_empty", &["{asked}"][..]),
            ("open.base_not_found", &["{base}", "{candidates}"][..]),
            ("open.branch_taken", &["{branch}"][..]),
            ("open.spec_taken", &["{spec}"][..]),
            ("open.tree_busy", &["{paths}"][..]),
            ("open.git_failed", &["{branch}", "{detail}"][..]),
            ("open.unborn_branch", &["{base}"][..]),
            ("open.pending_unknown", &["{pending}"][..]),
            ("open.pending_closed", &["{pending}"][..]),
            ("open.pending_note_failed", &["{spec}", "{pending}"][..]),
            ("open.pending_project.one", &["{list}"][..]),
            ("open.pending_project.many", &["{count}", "{list}"][..]),
            ("open.pending_project.list_page", &["{url}"][..]),
            ("open.pending_project.list_command", &[][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }

    /// As recusas das portas antigas que ainda respondem saem do catálogo nos
    /// dois idiomas, cada uma com as vagas que o chamador preenche, e todas
    /// dizem por qual comando esperar.
    #[test]
    fn i18n_translates_retired_keys() {
        for (key, slots, points_to) in [
            ("retired.wait_round", &["{command}"][..], "mustard-rt run round"),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
            assert!(pt.contains(points_to) && en.contains(points_to), "{key} does not name {points_to}");
        }
    }

    /// A escala de notas do plano compara a tarefa com um exemplo de
    /// qualquer projeto, nunca com algo que só existe no próprio Mustard,
    /// como o scan, uma lição parecida ou o pedido de uma onda.
    #[test]
    fn the_points_scale_uses_examples_of_any_project() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let scale = translate("plan.points_scale", lang).to_lowercase();
            for word in ["scan", "lições parecidas", "similar lessons", "pedido da onda", "wave request", "onda só"] {
                assert!(!scale.contains(&word.to_lowercase()), "{lang:?}: a escala ainda cita {word:?}: {scale}");
            }
        }
    }

    /// Os títulos e as instruções fixas do pedido de uma onda, e o que a
    /// conferência do plano acha, saem do catálogo nos dois idiomas, com as
    /// vagas que o montador preenche.
    #[test]
    fn i18n_translates_wave_prompt_keys() {
        for (key, slots) in [
            ("plan.not_ready", &["{count}"][..]),
            ("plan.next", &["{question}", "{option}"][..]),
            ("plan.execution.solo", &["{points}"][..]),
            ("plan.execution.one_wave", &["{points}"][..]),
            ("plan.execution.many_waves", &["{points}"][..]),
            ("plan.execution.ends_with_test_agent", &[][..]),
            ("page.copy.publish", &["{page}", "{template}", "{capabilities}", "{spec}", "{key}", "{milestone}"][..]),
            ("page.copy.batches", &["{page}", "{url}", "{files}"][..]),
            ("page.copy.record", &["{spec}", "{record}"][..]),
            ("page.copy.old_page", &["{page}"][..]),
            ("page.copy.agent", &["{page}", "{order}"][..]),
            ("page.migration.unrated", &["{tasks}", "{scale}", "{cap}"][..]),
            ("page.migration.over_cap", &["{wave}", "{points}", "{cap}"][..]),
            ("page.copy.new_address", &[][..]),
            ("page.copy.no_links", &[][..]),
            ("page.copy.failed", &[][..]),
            ("page.purge_pending", &["{codes}", "{spec}"][..]),
            ("plan.wave_loop", &["{waves}"][..]),
            ("plan.depends_on_missing", &["{wave}", "{on}"][..]),
            ("plan.task_without_wave", &["{task}", "{wave}"][..]),
            ("plan.shared_file", &["{waves}", "{files}", "{chain}"][..]),
            ("plan.wave_should_split", &["{wave}", "{parts}"][..]),
            ("plan.spec_should_split", &["{parts}"][..]),
            ("plan.file_outside_git", &["{task}", "{path}"][..]),
            ("plan.item_without_task", &["{code}"][..]),
            ("plan.owner_missing", &["{type}"][..]),
            ("plan.contract_without_criterion", &["{code}"][..]),
            ("plan.task_without_file", &["{task}", "{files}"][..]),
            ("plan.task_wrong_wave", &["{task}", "{wave}", "{best}"][..]),
            ("plan.task_matches_no_wave", &["{task}", "{wave}"][..]),
            ("plan.task_could_name_a_skill", &["{task}", "{skill}"][..]),
            ("plan.skill_to_be_born", &["{task}"][..]),
            ("plan.no_suggestion", &[][..]),
            ("plan.points_scale", &[][..]),
            ("plan.task_without_points", &["{tasks}", "{scale}"][..]),
            ("plan.wave_points_over_cap", &["{wave}", "{points}", "{cap}"][..]),
            ("plan.finding.label", &[][..]),
            ("discard.preview", &["{spec}", "{branch}", "{remote}", "{what}", "{token}"][..]),
            ("discard.archive", &[][..]),
            ("discard.delete", &[][..]),
            ("discard.yes", &[][..]),
            ("discard.no", &[][..]),
            ("discard.reason", &[][..]),
            ("discard.confirm_mismatch", &[][..]),
            ("discard.incomplete", &[][..]),
            ("discard.done", &[][..]),
            ("resume.line", &["{spec}", "{phase}", "{last}", "{next}"][..]),
            ("resume.none", &[][..]),
            ("resume.wave", &["{n}"][..]),
            ("resume.next.survey", &[][..]),
            ("resume.next.plan", &["{question}", "{option}"][..]),
            ("resume.next.running", &[][..]),
            ("resume.next.closed", &[][..]),
            ("resume.next.pr_open", &[][..]),
            ("resume.next.delivered", &[][..]),
            ("resume.next.discarded", &[][..]),
            ("close.not_running", &["{phase}"][..]),
            ("close.wave_without_commit", &["{wave}"][..]),
            ("close.wave_rejected", &["{wave}"][..]),
            ("close.request_not_delivered", &["{code}"][..]),
            ("close.criterion_failed", &["{code}", "{output}"][..]),
            ("close.criterion_ran_no_test", &["{code}", "{command}", "{count}"][..]),
            ("close.lint_failed", &["{command}", "{output}"][..]),
            ("close.final_review", &["{spec}"][..]),
            ("close.next", &["{command}"][..]),
            ("close.pending_destination", &["{id}", "{title}", "{spec}"][..]),
            ("close.unowned_item", &["{code}", "{title}"][..]),
            ("round.bad_report", &["{detail}"][..]),
            ("round.line_field", &["{line}", "{field}"][..]),
            ("round.merge_conflict", &["{wave}", "{conflicts}", "{copy}", "{head}"][..]),
            ("round.copy_failed", &["{wave}", "{detail}"][..]),
            ("pr.submodules.waiting", &["{pr}", "{paths}"][..]),
            ("pr.submodules.ready", &["{pr}", "{paths}"][..]),
            ("pr.submodules.stuck", &["{pr}", "{reason}"][..]),
            ("pr.pointer_commit", &[][..]),
            ("round.copy_kept", &["{wave}", "{copy}", "{files}"][..]),
            ("stuck.ended", &["{list}"][..]),
            ("stuck.reason.waiting_loop", &[][..]),
            ("stuck.reason.deleted_copy", &[][..]),
            ("conversation_size.pause", &["{wave}"][..]),
            ("conversation_size.compact", &["{spec}", "{phase}", "{command}", "{next}"][..]),
            ("conversation_size.compact_running", &["{waves}"][..]),
            ("round.file_unknown", &["{file}", "{wave}"][..]),
            ("round.proof_ran_no_test", &["{code}"][..]),
            ("round.commit.scope.one", &["{waves}"][..]),
            ("round.commit.scope.many", &["{waves}"][..]),
            ("round.commit.line", &["{wave}", "{summary}"][..]),
            ("round.commit.fixes", &["{waves}"][..]),
            ("round.not_approved", &["{phase}"][..]),
            ("round.delivered_too_long", &["{wave}", "{chars}", "{max}"][..]),
            ("round.commit_too_long", &["{part}", "{chars}", "{max}"][..]),
            ("round.commit_forbidden", &["{found}"][..]),
            ("round.formatter_missing", &["{name}"][..]),
            ("round.replan", &["{wave}", "{change}", "{question}", "{yes}", "{no}"][..]),
            ("round.git_refused", &["{detail}"][..]),
            ("round.next", &[][..]),
            ("round.next.copy_file", &["{path}"][..]),
            ("round.next.solo", &[][..]),
            ("round.report", &[][..]),
            ("round.waiting", &["{waves}"][..]),
            ("round.close", &["{command}"][..]),
            ("round.missing", &["{wave}"][..]),
            ("round.fix_limit", &["{wave}", "{count}", "{max}", "{verdicts}"][..]),
            ("round.fix_limit.question", &["{wave}", "{max}"][..]),
            ("round.analysis", &["{waves}"][..]),
            ("round.analysis_ignored", &["{wave}", "{item}"][..]),
            ("round.analysis_unreadable", &["{detail}"][..]),
            ("round.resume.steps", &[][..]),
            ("round.resume.notice", &[][..]),
            ("round.resume.silent", &["{wave}"][..]),
            ("page.findings.heading", &[][..]),
            ("prompt.title", &["{spec}", "{n}"][..]),
            ("prompt.fixed", &[][..]),
            ("prompt.final.title", &["{spec}"][..]),
            ("prompt.final.fixed", &[][..]),
            ("prompt.read", &["{root}", "{spec}"][..]),
            ("prompt.part.waves", &[][..]),
            ("prompt.part.each_delivered", &[][..]),
            ("prompt.part.specification", &[][..]),
            ("prompt.part.agreed", &[][..]),
            ("prompt.part.wave", &[][..]),
            ("prompt.part.criteria", &[][..]),
            ("prompt.part.lessons", &[][..]),
            ("prompt.part.skills", &[][..]),
            ("prompt.part.delivered", &[][..]),
            ("prompt.skill.stale", &[][..]),
            ("prompt.skill.read", &[][..]),
            ("prompt.execution.copy", &["{copy}", "{root}"][..]),
            ("prompt.execution.build_dir", &["{dir}"][..]),
            ("prompt.review.copy", &["{copy}", "{root}", "{commit}"][..]),
            ("prompt.review.cleanup", &["{copy}"][..]),
            ("page.wave.prompt", &["{n}"][..]),
            ("page.wave.prompt.summary", &["{lines}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }

    /// A dica que pede a escolha dos itens do pedido de uma onda manda a
    /// conferência das tarefas no código para um agente separado, que devolve
    /// só a tarefa ajustada para ser gravada, e diz que o orquestrador não lê
    /// arquivo inteiro nem saída longa para isso — nos dois idiomas.
    #[test]
    fn the_analysis_hint_sends_the_reading_to_an_agent() {
        for (lang, agent_word, whole_file, long_output) in [
            (Locale::PtBr, "agente separado", "arquivo inteiro", "saída longa"),
            (Locale::EnUs, "separate agent", "whole file", "long output"),
        ] {
            let hint = translate("round.analysis", lang).replace("{waves}", "1");
            assert!(hint.contains(agent_word), "{lang:?}: sem o agente separado: {hint}");
            assert!(hint.contains("tarefa ajustada") || hint.contains("adjusted task"), "{lang:?}: sem a tarefa ajustada: {hint}");
            assert!(hint.contains(whole_file), "{lang:?}: não proíbe ler arquivo inteiro: {hint}");
            assert!(hint.contains(long_output), "{lang:?}: não proíbe ler saída longa: {hint}");
        }
    }

    /// As dicas que mandam perguntar ou responder (a de apresentar um ponto
    /// do levantamento, a de revisar um bloco, a do plano e a da retomada) e o
    /// mapa do início da sessão apontam para a ordem de explicar do estilo de
    /// resposta, sem pedir a forma antiga: o fato com a fonte, o que já está
    /// decidido, o que falta decidir e uma recomendação. O estilo de cada
    /// idioma tem essa ordem, os quatro passos em sequência, e deixou de dizer
    /// que a página é publicada em cada marco. A dica do plano e a da retomada
    /// trazem o texto exato da pergunta de aprovação, entre aspas, lido da
    /// chave dela no catálogo: "Aprovar esta spec?" em português e "Approve
    /// this spec?" em inglês.
    #[test]
    fn the_question_hints_follow_the_answer_style() {
        let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin");
        let cases = [
            (
                Locale::PtBr,
                "Aprovar esta spec?",
                "na ordem de explicar do estilo de resposta",
                "## A ordem de explicar",
                [
                    "Toda pergunta, toda resposta e todo texto de item gravado na spec",
                    "1. O que é a coisa e onde ela age.",
                    "2. Para que ela serve.",
                    "3. Um exemplo que o próprio usuário viu.",
                    "4. Só então o problema e a proposta; numa pergunta, por último a pergunta de sim ou não.",
                ],
                ["com a fonte", "já está decidido", "falta decidir", "recomendação"],
                "só é publicada na aprovação",
            ),
            (
                Locale::EnUs,
                "Approve this spec?",
                "in the order of explaining from the response style",
                "## The order of explaining",
                [
                    "Every question, every answer and every item text recorded in the spec",
                    "1. What the thing is and where it acts.",
                    "2. What it is for.",
                    "3. An example the user saw for themselves.",
                    "4. Only then the problem and the proposal; in a question, the yes-or-no question comes last.",
                ],
                ["with its source", "already decided", "left to decide", "recommendation"],
                "published only at approval",
            ),
        ];
        for (lang, question, pointer, section, steps, old_shape, old_example) in cases {
            let style = std::fs::read_to_string(plugin.join(format!("output-styles/mustard-{lang}.md")))
                .unwrap_or_else(|e| panic!("the {lang} answer style is unreadable: {e}"));
            let order = style.find(section).unwrap_or_else(|| panic!("the {lang} style has no {section}"));
            let mut at = order;
            for step in steps {
                let found = style[at..].find(step).unwrap_or_else(|| panic!("{lang}: `{step}` missing or out of order"));
                at += found + step.len();
            }
            assert!(!style.contains(old_example), "the {lang} style still says the page waits for a milestone");

            let hints = ["survey.present_point", "survey.review_step", "plan.next", "resume.next.plan"];
            let texts = hints.map(|key| (key, translate(key, lang))).into_iter().chain([(
                "session map",
                crate::platform::seeds::session_map(lang),
            )]);
            for (key, text) in texts {
                assert!(text.contains(pointer), "{key} in {lang} does not point to the answer style: {text}");
                for old in old_shape {
                    assert!(!text.contains(old), "{key} in {lang} still asks for the old shape `{old}`: {text}");
                }
            }

            assert_eq!(translate("approval.question", lang), question);
            for key in ["plan.next", "resume.next.plan"] {
                let said = translate(key, lang).replace("{question}", translate("approval.question", lang));
                assert!(said.contains(&format!("\"{question}\"")), "{key} in {lang} lacks the exact question: {said}");
            }
        }
    }
}
