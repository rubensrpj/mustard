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
    "open",
    "plan",
    "round",
    "close",
    "resume",
    "reopen",
    "discard",
    "request",
    "message",
    "pr",
    "approve_spec",
    "retired",
    "banner",
    "stuck",
    "copies",
    "conversation_size",
    "wave_prompt",
    "triage",
];

/// Os comandos do mapa que os textos do catálogo citam para achar e ler o
/// código, ditos numa frase só: o comando e a hora de usar moram aqui, num
/// lugar único, e cada texto só põe a frase que os introduz.
macro_rules! map_commands_pt {
    () => {
        "`mustard-rt run map search \"<padrão>\"` acha onde mexer, \
         `mustard-rt run map slice --file <arquivo> --name <nome>` lê só a declaração e \
         `mustard-rt run map users --name <nome>` mostra quem a usa"
    };
}

/// A mesma frase de `map_commands_pt`, em inglês.
macro_rules! map_commands_en {
    () => {
        "`mustard-rt run map search \"<pattern>\"` finds where to change, \
         `mustard-rt run map slice --file <file> --name <name>` reads one declaration and \
         `mustard-rt run map users --name <name>` lists its users"
    };
}

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // CLOSE-phase success banner.
        ("banner.close.success", Locale::PtBr) => "Pipeline fechado com sucesso.",
        ("banner.close.success", Locale::EnUs) => "Pipeline closed successfully.",
        // O passo do plano: o que a conferência acha e o próximo passo.
        ("plan.not_ready", Locale::PtBr) => {
            "O plano tem {count} coisas a corrigir antes da pergunta de aprovação. Os achados ficam \
             anotados na spec, e a aprovação fica travada até um plano passar."
        }
        ("plan.not_ready", Locale::EnUs) => {
            "The plan has {count} things to fix before the approval question. The findings are noted \
             in the spec, and approval stays locked until a plan passes."
        }
        ("plan.next", Locale::PtBr) => {
            "Depois, faça a pergunta de aprovação na ordem de explicar do estilo de resposta, com o \
             texto exato \"{question}\". As opções são \"{option}\" e \"Ajustar\", e com outro texto \
             a aprovação não vale."
        }
        ("plan.next", Locale::EnUs) => {
            "Then ask the approval question in the order of explaining from the response style, with \
             the exact text \"{question}\". The options are \"{option}\" and \"Adjust\", and with \
             another text the approval does not count."
        }
        ("plan.wave_loop", Locale::PtBr) => {
            "As ondas {waves} dependem umas das outras em círculo, e nenhuma pode começar. Corte uma \
             das dependências."
        }
        ("plan.wave_loop", Locale::EnUs) => {
            "Waves {waves} depend on each other in a circle, and none of them can start. Cut one of \
             the dependencies."
        }
        ("plan.file_outside_git", Locale::PtBr) => {
            "A tarefa {task} cita {path}, que o git não guarda: um agente noutra sessão ou noutra \
             máquina não o vê."
        }
        ("plan.file_outside_git", Locale::EnUs) => {
            "Task {task} cites {path}, which git does not track: an agent in another session or on \
             another machine cannot see it."
        }
        ("plan.item_without_task", Locale::PtBr) => "Nenhuma tarefa diz que cobre o item {code}. Se ele não vira código, diga por quê.",
        ("plan.item_without_task", Locale::EnUs) => "No task says it covers item {code}. If it does not become code, say why.",
        // O dono de cada item combinado: o item novo depois da aprovação nasce
        // com dono.
        ("plan.owner_missing", Locale::PtBr) => {
            "O item novo do tipo {type} não tem dono, e a spec já foi aprovada. Todo requisito \
             acordado tem dono, e nada foi gravado. Dê o dono pelos arquivos, com `applies_to` e os \
             arquivos das tarefas que o cobrem ou vão cobrir, como \
             `\"applies_to\":{\"files\":[\"src/a.rs\"]}`. Quando ele vale para todas as tarefas, use \
             `\"applies_to\":{\"files\":[\"**\"]}`."
        }
        ("plan.owner_missing", Locale::EnUs) => {
            "The new {type} item has no owner, and the spec is already approved. Every agreed \
             requirement has an owner, and nothing was written. Give the owner by the files, with \
             `applies_to` and the files of the tasks that cover it or will cover it, as in \
             `\"applies_to\":{\"files\":[\"src/a.rs\"]}`. When it holds for every task, use \
             `\"applies_to\":{\"files\":[\"**\"]}`."
        }
        ("plan.contract_without_criterion", Locale::PtBr) => "Nenhum critério cita o contrato {code}: nada prova que ele foi cumprido.",
        ("plan.contract_without_criterion", Locale::EnUs) => "No criterion cites contract {code}: nothing proves it was met.",
        ("plan.task_without_file", Locale::PtBr) => {
            "A tarefa {task} mexe em código e não diz em que arquivo. Nomeie o arquivo na tarefa. \
             A tarefa que não mexe em arquivo nenhum diz isso no texto dela."
        }
        ("plan.task_without_file", Locale::EnUs) => {
            "Task {task} changes code and does not say which file. Name the file in the task. \
             A task that changes no file says so in its own text."
        }
        ("plan.task_could_name_a_skill", Locale::PtBr) => "A tarefa {task} não nomeia skill, e a skill {skill} serve para ela. Nomeie-a na tarefa.",
        ("plan.task_could_name_a_skill", Locale::EnUs) => "Task {task} names no skill, and skill {skill} fits it. Name it in the task.",
        ("plan.dependency_only_files.one", Locale::PtBr) => {
            "A tarefa {task} espera a tarefa {waits_for}, e as duas mexem em {files}. Se a espera é só por esse arquivo, passe-o para uma das duas e tire a dependência. Aí as ondas delas saem juntas."
        }
        ("plan.dependency_only_files.one", Locale::EnUs) => {
            "Task {task} waits for task {waits_for}, and both touch {files}. If that file is the only reason to wait, move it into one of them and drop the dependency. Their waves then go out together."
        }
        ("plan.dependency_only_files.many", Locale::PtBr) => {
            "A tarefa {task} espera a tarefa {waits_for}, e as duas mexem em {files}. Se a espera é só por esses arquivos, passe-os para uma das duas e tire a dependência. Aí as ondas delas saem juntas."
        }
        ("plan.dependency_only_files.many", Locale::EnUs) => {
            "Task {task} waits for task {waits_for}, and both touch {files}. If those files are the only reason to wait, move them into one of them and drop the dependency. Their waves then go out together."
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
        ("discard.archive", Locale::PtBr) => "guarda a pasta da spec, que continua no índice e na página do projeto, marcada como descartada",
        ("discard.archive", Locale::EnUs) => "archives the spec folder, which stays in the index and on the project page, marked as discarded",
        ("discard.delete", Locale::PtBr) => "apaga a pasta da spec e tira a linha dela do índice",
        ("discard.delete", Locale::EnUs) => "deletes the spec folder and drops its line from the index",
        ("discard.yes", Locale::PtBr) => "sim",
        ("discard.yes", Locale::EnUs) => "yes",
        ("discard.no", Locale::PtBr) => "não",
        ("discard.no", Locale::EnUs) => "no",
        ("discard.reason", Locale::PtBr) => "descartada pelo usuário",
        ("discard.reason", Locale::EnUs) => "discarded by the user",
        ("discard.confirm_mismatch", Locale::PtBr) => "O código não é o deste descarte: peça a primeira chamada de novo. Nada saiu.",
        ("discard.confirm_mismatch", Locale::EnUs) => "That code is not this discard's: ask for the first call again. Nothing was removed.",
        ("discard.incomplete", Locale::PtBr) => "O descarte não terminou: a pasta da spec não saiu do lugar, ou a linha dela no índice não mudou.",
        ("discard.incomplete", Locale::EnUs) => "The discard did not finish: the spec folder did not move, or its index line did not change.",
        ("discard.done", Locale::PtBr) => "A spec está descartada.",
        ("discard.done", Locale::EnUs) => "The spec is discarded.",

        // A retomada de uma spec (`commands/flow/resume.rs`).
        ("resume.line", Locale::PtBr) => "Retomada: spec {spec}, fase {phase}; último passo: {last}; próximo: {next}.",
        ("resume.line", Locale::EnUs) => "Resume: spec {spec}, phase {phase}; last step: {last}; next: {next}.",
        ("resume.none", Locale::PtBr) => "nenhum",
        ("resume.none", Locale::EnUs) => "none",
        ("resume.unbound", Locale::PtBr) => {
            "Esta conversa não está ligada à spec: o que se diz aqui não entra nela. Rode `mustard-rt run resume` (ou `/mustard:continue`) para ligá-la."
        }
        ("resume.unbound", Locale::EnUs) => {
            "This conversation is not bound to the spec: what is said here does not enter it. Run `mustard-rt run resume` (or `/mustard:continue`) to bind it."
        }
        ("resume.no_spec", Locale::PtBr) => {
            "Nenhuma spec em andamento neste projeto. Para começar uma obra, abra a spec com `mustard-rt run open`."
        }
        ("resume.no_spec", Locale::EnUs) => "No spec in progress in this project. To start a piece of work, open the spec with `mustard-rt run open`.",
        ("resume.wave", Locale::PtBr) => "onda {n}",
        ("resume.wave", Locale::EnUs) => "wave {n}",
        ("resume.next.survey", Locale::PtBr) => "A spec está no levantamento: rode o levantamento e grave a resposta de cada ponto.",
        ("resume.next.survey", Locale::EnUs) => "The spec is in the survey: run the survey and record the answer to each point.",
        ("resume.next.plan", Locale::PtBr) => {
            "O plano está gravado: confira o plano. Depois, faça a pergunta de aprovação na ordem \
             de explicar do estilo de resposta, com o texto exato \"{question}\". As opções são \
             \"{option}\" e \"Ajustar\", e com outro texto a aprovação não vale."
        }
        ("resume.next.plan", Locale::EnUs) => {
            "The plan is recorded: check the plan. Then ask the approval question in the order of \
             explaining from the response style, with the exact text \"{question}\". The options \
             are \"{option}\" and \"Adjust\", and with another text the approval does not count."
        }
        ("resume.next.running", Locale::PtBr) => "A spec está aprovada: rode a próxima rodada de ondas.",
        ("resume.next.running", Locale::EnUs) => "The spec is approved: run the next round of waves.",
        ("resume.next.closed", Locale::PtBr) => "A spec está fechada: abra o pull request.",
        ("resume.next.closed", Locale::EnUs) => "The spec is closed: open the pull request.",
        ("resume.next.pr_open", Locale::PtBr) => "O pull request está aberto: espere a revisão e faça o merge quando o usuário pedir.",
        ("resume.next.pr_open", Locale::EnUs) => "The pull request is open: wait for the review and merge when the user asks.",
        ("resume.next.delivered", Locale::PtBr) => "A spec foi entregue: não há passo seguinte.",
        ("resume.next.delivered", Locale::EnUs) => "The spec was delivered: there is no next step.",
        ("resume.next.discarded", Locale::PtBr) => "A spec foi descartada: não há passo seguinte.",
        ("resume.next.discarded", Locale::EnUs) => "The spec was discarded: there is no next step.",
        // O próximo passo em palavras do dia a dia, para quem lê o painel: o
        // que o comando do passo faz pela spec agora.
        ("resume.close_refused", Locale::PtBr) => {
            "O último fechamento desta spec caiu por `{reason}`. Não há mais onda nem tarefa a rodar. Não rode outra rodada: ela sairia sem nada. Rode o fechamento de novo para ver o motivo inteiro. As saídas são três. Abra uma tarefa nova para consertar. Ou ajuste com o usuário o que a conferência do projeto cobra. Ou feche de novo, com o que caiu já resolvido."
        }
        ("resume.close_refused", Locale::EnUs) => {
            "This spec's last close failed with `{reason}`. No wave or task is left to run. Do not run another round: it would go out with nothing. Run the close again to see the whole reason. There are three ways out. Open a new task to fix it. Or settle with the user what the project's check demands. Or close again, with what failed resolved."
        }
        ("resume.step.survey", Locale::PtBr) => "Seguir o levantamento e responder os pontos em aberto",
        ("resume.step.survey", Locale::EnUs) => "Continue the survey and answer the open points",
        ("resume.step.plan", Locale::PtBr) => "Conferir o plano e pedir a aprovação",
        ("resume.step.plan", Locale::EnUs) => "Check the plan and ask for approval",
        ("resume.step.closed", Locale::PtBr) => "Abrir o pull request da spec",
        ("resume.step.closed", Locale::EnUs) => "Open the spec's pull request",
        ("resume.step.stuck_one", Locale::PtBr) => "Decidir o que fazer com a onda {waves}, parada no limite de consertos",
        ("resume.step.stuck_one", Locale::EnUs) => "Decide what to do with wave {waves}, stopped at the fix limit",
        ("resume.step.stuck_many", Locale::PtBr) => "Decidir o que fazer com as ondas {waves}, paradas no limite de consertos",
        ("resume.step.stuck_many", Locale::EnUs) => "Decide what to do with waves {waves}, stopped at the fix limit",
        ("resume.step.cycle", Locale::PtBr) => "Desfazer o ciclo de dependência entre as ondas {waves}",
        ("resume.step.cycle", Locale::EnUs) => "Break the dependency loop between waves {waves}",
        ("resume.step.dispatch_one", Locale::PtBr) => "Despachar a onda {waves}",
        ("resume.step.dispatch_one", Locale::EnUs) => "Dispatch wave {waves}",
        ("resume.step.dispatch_many", Locale::PtBr) => "Despachar as ondas {waves}",
        ("resume.step.dispatch_many", Locale::EnUs) => "Dispatch waves {waves}",
        ("resume.step.backlog", Locale::PtBr) => "Montar ondas com as tarefas prontas do backlog e despachá-las",
        ("resume.step.backlog", Locale::EnUs) => "Form waves from the backlog's ready tasks and dispatch them",
        ("resume.step.wait_one", Locale::PtBr) => "Esperar a onda {waves} voltar",
        ("resume.step.wait_one", Locale::EnUs) => "Wait for wave {waves} to come back",
        ("resume.step.wait_many", Locale::PtBr) => "Esperar as ondas {waves} voltarem",
        ("resume.step.wait_many", Locale::EnUs) => "Wait for waves {waves} to come back",
        ("resume.step.missing", Locale::PtBr) => "Destravar a onda {wave}, que não tem como sair",
        ("resume.step.missing", Locale::EnUs) => "Unblock wave {wave}, which cannot go out",
        ("resume.step.backlog_held", Locale::PtBr) => "Destravar as tarefas presas no backlog",
        ("resume.step.backlog_held", Locale::EnUs) => "Unblock the tasks stuck in the backlog",
        ("resume.step.review_open", Locale::PtBr) => "Esperar o veredito da revisão final",
        ("resume.step.review_open", Locale::EnUs) => "Wait for the final review's verdict",
        ("resume.step.close", Locale::PtBr) => "Fechar a spec e pedir a revisão final",
        ("resume.step.close", Locale::EnUs) => "Close the spec and ask for the final review",
        ("resume.step.close_again", Locale::PtBr) => "Fechar a spec e refazer a revisão final, anulada pela reabertura",
        ("resume.step.close_again", Locale::EnUs) => "Close the spec and redo the final review the reopening voided",
        ("resume.step.close_approved", Locale::PtBr) => "Fechar a spec, já aprovada na revisão final",
        ("resume.step.close_approved", Locale::EnUs) => "Close the spec, already approved by the final review",

        // O fechamento de uma spec (`commands/flow/close.rs`).
        ("close.validation_inputs_changed", Locale::PtBr) => {
            "Código ou configuração mudou durante a validação final. Confira a mudança e repita o fechamento sobre a versão atual."
        }
        ("close.validation_inputs_changed", Locale::EnUs) => {
            "Code or configuration changed during final validation. Inspect the change and close again against the current version."
        }
        ("close.repair_next", Locale::PtBr) => {
            "Conserto final registrado. Rode `mustard-rt run round` para obter o envio autorizado de conserto; depois integre e repita o fechamento. A onda volta ao conserto pelo critério dela que falhou, ou porque a saída cita um arquivo da obra que ela declarou ou entregou."
        }
        ("close.repair_next", Locale::EnUs) => {
            "Final repair recorded. Run `mustard-rt run round` for the authorized repair dispatch, then integrate and close again. A wave returns to repair through its own failing criterion, or because the output cites a work file it declared or delivered."
        }
        // O lint ou a suíte do fechamento que caiu sem citar arquivo de onda
        // da obra: o que a saída citou, e que nenhuma onda reabriu.
        ("close.cited_outside", Locale::PtBr) => {
            "A saída cita {files}, e nenhum desses arquivos é da obra: a branch não os mudou. Nenhuma onda reabriu. \
             Mostre a saída ao usuário e pergunte o que fazer com esses arquivos; resolvido isso, repita o fechamento."
        }
        ("close.cited_outside", Locale::EnUs) => {
            "The output cites {files}, and none of these files belongs to the work: the branch did not change them. No wave \
             reopened. Show the output to the user and ask what to do with these files; once that is settled, close again."
        }
        ("close.cited_nothing", Locale::PtBr) => {
            "A saída não cita arquivo nenhum do projeto, e por isso nenhum da obra. Nenhuma onda reabriu. \
             Mostre a saída ao usuário e pergunte o que fazer; resolvido isso, repita o fechamento."
        }
        ("close.cited_nothing", Locale::EnUs) => {
            "The output cites no project file, so none from the work. No wave reopened. \
             Show the output to the user and ask what to do; once that is settled, close again."
        }
        ("close.cited_unowned", Locale::PtBr) => {
            "A saída cita arquivo que a branch mudou, mas que nenhuma onda declarou nem entregou. Nenhuma onda reabriu. \
             Mostre a saída ao usuário e pergunte o que fazer; resolvido isso, repita o fechamento."
        }
        ("close.cited_unowned", Locale::EnUs) => {
            "The output cites a file the branch changed, but no wave declared or delivered it. No wave reopened. \
             Show the output to the user and ask what to do; once that is settled, close again."
        }
        // A marca de conserto que o lint, a suíte ou a análise de declarações
        // sem uso do fechamento gravaram sem citar arquivo da onda, solta no
        // começo do fechamento ou da rodada.
        ("close.stale_mark_released", Locale::PtBr) => {
            "A reprovação da onda {wave} pelo lint, pela suíte ou pela análise de código sem uso do fechamento saiu. A saída \
             gravada nela não cita arquivo que a branch mudou e que a onda declarou ou entregou. Sem essa prova, a onda não \
             volta ao conserto."
        }
        ("close.stale_mark_released", Locale::EnUs) => {
            "The rejection of wave {wave} by the closing lint, suite or unused-code analysis was released. Its recorded output \
             cites no file the branch changed and the wave declared or delivered. Without that proof, the wave does not return \
             to repair."
        }
        ("close.not_running", Locale::PtBr) => "A spec está na fase {phase} e não está em execução: só fecha o que estava correndo.",
        ("close.not_running", Locale::EnUs) => "The spec is in the {phase} phase and is not running: only work in flight closes.",
        ("close.wave_without_commit", Locale::PtBr) => "A onda {wave} não tem commit: refaça a onda {wave} antes de fechar.",
        ("close.wave_without_commit", Locale::EnUs) => "Wave {wave} has no commit: redo wave {wave} before closing.",
        ("close.wave_rejected", Locale::PtBr) => "A última revisão da onda {wave} reprovou: refaça a onda {wave} antes de fechar.",
        ("close.wave_rejected", Locale::EnUs) => "The last review of wave {wave} rejected it: redo wave {wave} before closing.",
        ("close.request_not_delivered", Locale::PtBr) => {
            "O pedido {code} chegou depois da última entrega e nenhuma onda o entregou: \
             leve-o para uma onda antes de fechar."
        }
        ("close.request_not_delivered", Locale::EnUs) => {
            "Request {code} arrived after the last delivery and no wave delivered it: \
             take it into a wave before closing."
        }
        ("close.backlog_not_empty", Locale::PtBr) => {
            "O backlog ainda tem as tarefas {tasks}, que nenhuma onda entregou. Rode a rodada de novo \
             para formar o lote delas antes de fechar, ou retire da spec a que não vai mais ser feita."
        }
        ("close.backlog_not_empty", Locale::EnUs) => {
            "The backlog still holds tasks {tasks}, which no wave delivered. Run the round again to \
             form their batch before closing, or remove from the spec the one that will no longer be done."
        }
        ("close.criterion_failed", Locale::PtBr) => "A verificação do critério {code} não passou: {output}",
        ("close.criterion_failed", Locale::EnUs) => "The verification of criterion {code} did not pass: {output}",
        ("close.criterion_ran_no_test", Locale::PtBr) => {
            "A verificação do critério {code} saiu verde sem rodar teste nenhum: `{command}` diz que rodou \
             {count} testes. Grave a versão nova do critério com a verificação certa e feche de novo."
        }
        ("close.criterion_ran_no_test", Locale::EnUs) => {
            "The verification of criterion {code} came out green without running any test: `{command}` says \
             it ran {count} tests. Record the criterion's new version with the right verification and close again."
        }
        ("close.criterion_missing_test", Locale::PtBr) => {
            "A verificação do critério {code} cita o teste {name}, que não aparece em nenhum arquivo do \
             projeto. Escreva esse teste ou grave a versão nova do critério com o nome certo, e feche de novo."
        }
        ("close.criterion_missing_test", Locale::EnUs) => {
            "The verification of criterion {code} names the test {name}, which appears in no file of the \
             project. Write that test or record the criterion's new version with the right name, and close again."
        }
        ("close.build_failed", Locale::PtBr) => {
            "A compilação do projeto (`{command}`), rodada em ambiente limpo como o servidor a roda, \
             não passou, e a spec não fechou: {output}"
        }
        ("close.build_failed", Locale::EnUs) => {
            "The project build (`{command}`), run in a clean environment the way the server runs it, \
             did not pass, and the spec did not close: {output}"
        }
        ("close.lint_from_the_base", Locale::PtBr) => {
            "O `{command}` do projeto falhou, e só em arquivos que esta obra não tocou: {files}. A base já os acusava na abertura da spec, então o erro não é da obra e não trava o fechamento. Foi isto que o comando disse:\n{output}"
        }
        ("close.lint_from_the_base", Locale::EnUs) => {
            "The project's `{command}` failed, and only in files this work never touched: {files}. The base already flagged them when the spec opened, so the error is not the work's and does not block the close. This is what the command said:\n{output}"
        }
        ("close.lint_failed", Locale::PtBr) => "O lint do projeto (`{command}`) não passou, e a spec não fechou: {output}",
        ("close.lint_failed", Locale::EnUs) => "The project lint (`{command}`) did not pass, and the spec did not close: {output}",
        ("close.suite_failed", Locale::PtBr) => {
            "A suíte do projeto (`{command}`), rodada em ambiente limpo como o servidor a roda, não \
             passou, e a spec não fechou: {output}"
        }
        ("close.suite_failed", Locale::EnUs) => {
            "The project suite (`{command}`), run in a clean environment the way the server runs it, \
             did not pass, and the spec did not close: {output}"
        }
        // A compilação, o lint ou a suíte do fechamento que caiu num projeto
        // que declara arquivos locais: a cópia limpa nunca os recebe.
        ("close.ran_without_local_files", Locale::PtBr) => {
            "O comando rodou na cópia limpa, sem os arquivos locais que o `mustard.json` lista em \
             `localFiles`: {files}. O git os ignora, e a esteira do servidor também não os tem. O \
             comando que depende de um deles cai lá como caiu aqui."
        }
        ("close.ran_without_local_files", Locale::EnUs) => {
            "The command ran in the clean copy, without the local files `mustard.json` lists in \
             `localFiles`: {files}. Git ignores them, and the server pipeline does not have them \
             either. A command that depends on one of them fails there as it failed here."
        }
        // O comando do servidor que o `mustard.json` não declara: o
        // fechamento não o roda, e avisa que não promete o que o servidor
        // vai dizer.
        ("close.server_command_not_declared", Locale::PtBr) => {
            "O `mustard.json` não declara `{key}`: o fechamento não rodou esse comando, e não promete \
             que o servidor passa. Declare nele o comando que o servidor roda, ao pé da letra."
        }
        ("close.server_command_not_declared", Locale::EnUs) => {
            "`mustard.json` does not declare `{key}`: the close did not run that command, and does not \
             promise the server passes. Declare there the command the server runs, word for word."
        }
        ("close.review_copy_dirty", Locale::PtBr) => {
            "A cópia do revisor `{copy}` tem mudança ({files}), que um revisor anterior deixou, e o \
             fechamento não revisa por cima dela. Confira o que é e descarte com \
             `git worktree remove --force {copy}`; depois feche de novo."
        }
        ("close.review_copy_dirty", Locale::EnUs) => {
            "The reviewer's copy `{copy}` has changes ({files}) a previous reviewer left, and the close \
             does not review on top of them. Check what they are and discard with \
             `git worktree remove --force {copy}`; then close again."
        }
        ("close.review_copy_failed", Locale::PtBr) => "A cópia do revisor `{copy}` não pôde ser criada, e a spec não fechou: {detail}",
        ("close.review_copy_failed", Locale::EnUs) => "The reviewer's copy `{copy}` could not be created, and the spec did not close: {detail}",
        ("close.clean_copy_failed", Locale::PtBr) => "A cópia limpa `{copy}`, em que a compilação, o lint, a suíte e os critérios rodam, não pôde ser criada, e a spec não fechou: {detail}",
        ("close.clean_copy_failed", Locale::EnUs) => "The clean copy `{copy}`, where the build, the lint, the suite and the criteria run, could not be created, and the spec did not close: {detail}",
        // As cópias da obra que o fechamento ou o descarte não conseguiram
        // tirar. A obra segue: o aviso só diz o que ficou e como tirar.
        ("close.review_copy_kept", Locale::PtBr) => {
            "Estas cópias da obra ficaram no disco: {copies}. O git disse: {detail}. Apague cada uma \
             com `git worktree remove --force <cópia>` e depois rode `git worktree prune`."
        }
        ("close.review_copy_kept", Locale::EnUs) => {
            "These copies of the work stayed on disk: {copies}. Git said: {detail}. Delete each one \
             with `git worktree remove --force <copy>`, then run `git worktree prune`."
        }
        ("close.code_not_kept", Locale::PtBr) => {
            "Estas cópias da obra ficaram no disco: {copies}. O código que elas têm além do commit não \
             pôde ser guardado antes de apagar. Motivo: {detail}. Resolva o motivo e rode de novo; nada \
             delas foi apagado."
        }
        ("close.code_not_kept", Locale::EnUs) => {
            "These copies of the work stayed on disk: {copies}. The code they hold beyond the commit \
             could not be kept before wiping. Reason: {detail}. Fix the reason and run again; none of \
             them was wiped."
        }
        ("close.build_output_retained", Locale::PtBr) => "Cache `{folder}` conservado: {bytes} bytes, dentro do teto de 15 GB.",
        ("close.build_output_retained", Locale::EnUs) => "Cache `{folder}` retained: {bytes} bytes, within the 15 GB ceiling.",
        ("close.build_output_unsafe", Locale::PtBr) => {
            "A pasta de compilação `{folder}`, declarada no `mustard.json`, ficou no disco. Ela precisa \
             ser uma pasta dentro do projeto, fora do `.git` e diferente da raiz."
        }
        ("close.build_output_unsafe", Locale::EnUs) => {
            "The build folder `{folder}`, declared in `mustard.json`, stayed on disk. It must be a \
             folder inside the project, outside `.git` and other than the root."
        }
        ("close.build_output_not_ignored", Locale::PtBr) => {
            "A pasta de compilação `{folder}`, declarada no `mustard.json`, ficou no disco. O git não \
             a ignora ou guarda arquivo dela. Acerte o `.gitignore` ou tire a pasta da lista."
        }
        ("close.build_output_not_ignored", Locale::EnUs) => {
            "The build folder `{folder}`, declared in `mustard.json`, stayed on disk. Git does not \
             ignore it or tracks a file in it. Fix the `.gitignore` or take the folder off the list."
        }
        ("close.build_output_failed", Locale::PtBr) => {
            "A pasta de compilação `{folder}` ficou no disco: {detail}. Apague-a quando nada estiver \
             compilando."
        }
        ("close.build_output_failed", Locale::EnUs) => "The build folder `{folder}` stayed on disk: {detail}. Delete it when nothing is building.",
        ("close.final_review", Locale::PtBr) => {
            "A máquina passou. Antes do pull request, despache o agente de teste dedicado, \
             `mustard-review`. Mande a ele o comando em `review.read`, e ele lê o próprio pedido. \
             Ele grava o veredito na spec com `mustard-rt run write verdict`. \
             Quando ele voltar, conclua o fechamento com o mesmo comando: \
             `mustard-rt run close --spec {spec}`. Rode-o em segundo plano e espere o aviso de fim. \
             Com o veredito aprovado e nada mudado desde a máquina, o comando só confere o veredito \
             e fecha a spec. Se algo mudou, a máquina roda de novo. Aí a suíte inteira pode passar \
             dos 10 minutos que o terminal espera por um comando. Se o veredito não estiver na spec, \
             mande o agente gravá-lo de novo pela ferramenta."
        }
        ("close.final_review", Locale::EnUs) => {
            "The machine passed. Before the pull request, dispatch the dedicated test agent, \
             `mustard-review`. Send it the command in `review.read`, and it reads its own request. \
             It records its verdict in the spec with `mustard-rt run write verdict`. \
             When it comes back, finish the close with the same command: \
             `mustard-rt run close --spec {spec}`. Run it in the background and wait for the notice \
             that it ended. With the verdict approved and nothing changed since the machine, the \
             command only checks the verdict and closes the spec. If something changed, the machine \
             runs again. Then the whole suite can take longer than the 10 minutes the terminal waits \
             for a command. If the verdict is not in the spec, have the agent record it again \
             through the tool."
        }
        // O pedido de revisão final ficou para trás de itens gravados depois
        // dele: o fechamento gravou o pedido novo, que os lista.
        ("close.review_request_renewed", Locale::PtBr) => {
            "O pedido da revisão final foi refeito: a spec ganhou {items} depois do envio {send}, \
             e o pedido anterior não mostrava isso. O veredito do pedido anterior não aprova a obra. \
             O revisor lê o pedido novo pelo comando em `review.read`."
        }
        ("close.review_request_renewed", Locale::EnUs) => {
            "The final review request was rebuilt: the spec gained {items} after send {send}, and \
             the previous request did not show them. A verdict on the previous request does not \
             approve the work. The reviewer reads the new request through the command in \
             `review.read`."
        }
        ("close.next", Locale::PtBr) => "Depois, abra o pull request: `{command}`.",
        ("close.next", Locale::EnUs) => "Then open the pull request: `{command}`.",
        // A revisão final aprovou com achados: eles viraram tarefas, e o
        // pull request espera a entrega delas e uma revisão final nova.
        ("close.final_leftovers", Locale::PtBr) => {
            "A revisão final aprovou a obra com achados, que viraram as tarefas {tasks} no backlog. \
             A spec não fechou: antes do pull request, rode a rodada, que as despacha: `{command}`. \
             Depois da entrega, o fechamento pede uma revisão final nova."
        }
        ("close.final_leftovers", Locale::EnUs) => {
            "The final review approved the work with findings, which became tasks {tasks} in the \
             backlog. The spec did not close: before the pull request, run the round, which dispatches \
             them: `{command}`. After the delivery, the close asks for a new final review."
        }
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
        ("close.unowned_item", Locale::PtBr) => "O item {code} ({title}) não tem dono e nenhuma onda o levou: fica fora do código.",
        ("close.unowned_item", Locale::EnUs) => "Item {code} ({title}) has no owner and no wave carried it: it stays out of the code.",
        // O caminho de volta: um teste que a obra criou ou mudou e que
        // nenhum critério cita na prova dele — o fechamento avisa, com o
        // nome do teste e o arquivo, em vez de deixar o teste sem dono.
        ("close.unowned_test", Locale::PtBr) => "O teste {name}, em {file}, não tem critério que o cite na verificação dele: fica sem cobertura.",
        ("close.unowned_test", Locale::EnUs) => {
            "The test {name}, in {file}, has no criterion citing it in its verification: it stays without coverage."
        }

        // A rodada de ondas (`commands/flow/round.rs`).
        ("round.bad_report", Locale::PtBr) => "O relatório da rodada não se entende: {detail}. Nada foi gravado.",
        ("round.bad_report", Locale::EnUs) => "The round report cannot be read: {detail}. Nothing was recorded.",
        ("round.line_field", Locale::PtBr) => {
            "Uma linha `<{line}>` do relatório não traz o campo `{field}`: complete a linha e rode a \
             rodada de novo. Nada foi gravado."
        }
        ("round.line_field", Locale::EnUs) => {
            "A `<{line}>` line of the report lacks the `{field}` field: complete the line and run the \
             round again. Nothing was recorded."
        }
        ("round.merge_conflict", Locale::PtBr) => {
            "A entrega da onda {wave} conflita com o repositório principal nestes trechos: {conflicts}. \
             Nada dela foi gravado. Resolva na cópia {copy}: leve-a ao commit atual com \
             `git -C {copy} checkout --merge --detach {head}`, acerte os trechos marcados e rode a \
             rodada de novo. A entrega que a onda {wave} gravou segue na spec, e a rodada a assume."
        }
        ("round.merge_conflict", Locale::EnUs) => {
            "Wave {wave}'s delivery conflicts with the main repository in these hunks: {conflicts}. \
             Nothing of it was recorded. Resolve it in the copy {copy}: bring it to the current commit \
             with `git -C {copy} checkout --merge --detach {head}`, fix the marked hunks and run the \
             round again. The delivery wave {wave} recorded stays in the spec, and the round takes it over."
        }
        ("round.copy_failed", Locale::PtBr) => {
            "A cópia da onda {wave} não pôde ser criada: {detail}. A onda não saiu nesta rodada; \
             corrija e rode a rodada de novo."
        }
        ("round.copy_failed", Locale::EnUs) => {
            "Wave {wave}'s copy could not be created: {detail}. The wave did not go out this round; \
             fix it and run the round again."
        }
        // Um arquivo da lista de arquivos locais do projeto não foi copiado
        // para a cópia. Não trava nada: a cópia sai sem a versão dele da pasta
        // principal, e o aviso diz qual.
        ("round.local_file_missing", Locale::PtBr) => {
            "O arquivo local `{file}`, da lista `localFiles` do `mustard.json`, não foi copiado para a \
             cópia `{copy}`. Ou ele não existe no repositório principal, ou não é um caminho relativo \
             dentro do projeto. Ou o git não o ignora, ou o disco recusou a cópia. O que o git não \
             ignora chega à cópia pelo próprio git, na versão do commit. A cópia saiu sem a versão \
             dele da pasta principal."
        }
        ("round.local_file_missing", Locale::EnUs) => {
            "The local file `{file}`, from the `localFiles` list in `mustard.json`, was not copied into \
             the copy `{copy}`. Either it is missing from the main repository, or it is not a relative \
             path inside the project. Or git does not ignore it, or the disk refused the copy. A file \
             git does not ignore reaches the copy through git itself, in the commit's version. The \
             copy went out without the main folder's version of it."
        }
        // A recusa do upsert a um item de `--local-files` que o git não
        // ignora: nada é gravado.
        ("round.local_file_tracked", Locale::PtBr) => {
            "O arquivo `{file}` não entra em `localFiles`: o git não o ignora. A cópia de cada onda já \
             recebe pelo git, na versão do commit, todo arquivo que ele não ignora. Copiá-lo da pasta \
             principal por cima trocaria essa versão pela de lá. Só vai à lista o arquivo que o git \
             ignora, como o `.env`. Nada foi gravado."
        }
        ("round.local_file_tracked", Locale::EnUs) => {
            "The file `{file}` does not go into `localFiles`: git does not ignore it. Each wave's copy \
             already gets every file git does not ignore through git, in the commit's version. \
             Copying it over from the main folder would swap that version for the one there. Only a \
             file git ignores, like `.env`, goes into the list. Nothing was written."
        }
        // O gasto da obra inteira (`apps/rt/src/commands/spec_events/pages/copy.rs`).
        ("round.spend.line", Locale::PtBr) => {
            "Gasto total: {waves} tokens de onda + {caller} tokens de quem despachou = {total} tokens \
             ({turns} turnos por tarefa)."
        }
        ("round.spend.line", Locale::EnUs) => {
            "Total spend: {waves} wave tokens + {caller} orchestrator tokens = {total} tokens \
             ({turns} turns per task)."
        }
        // O teto de tokens do pedido de uma onda (`packages/core/src/domain/wave_prompt.rs`).
        // A rodada divide sozinha a onda que tem mais de uma tarefa; a recusa
        // fica para a que nem a primeira tarefa sozinha cabe.
        ("wave_prompt.token_cap", Locale::PtBr) => {
            "O pedido da onda {wave} tem {tokens} tokens, acima do teto de {cap}. Nem a primeira tarefa \
             sozinha cabe no teto. Divida essa tarefa ou encurte o texto que vai em todo pedido."
        }
        ("wave_prompt.token_cap", Locale::EnUs) => {
            "Wave {wave}'s request has {tokens} tokens, above the {cap} cap. Not even the first task alone \
             fits under the cap. Split that task or shorten the text every request carries."
        }
        // O bloco em destaque do começo do pedido que leva tarefas que
        // voltaram com o resumo de um agente que encheu a conversa
        // (`packages/core/src/domain/wave_prompt/summary.rs`).
        ("wave_prompt.summary.title", Locale::PtBr) => "Trabalho já começado",
        ("wave_prompt.summary.title", Locale::EnUs) => "Work already started",
        ("wave_prompt.summary.report_read", Locale::PtBr) => {
            "Leia o relato histórico {code} com `mustard-rt run read item-{code} {root}--spec {spec}` antes de continuar. A onda saiu do plano e este relato não foi integrado: confirme no código os fatos e o trabalho alegado antes de reutilizá-los."
        }
        ("wave_prompt.summary.report_read", Locale::EnUs) => {
            "Read historical report {code} with `mustard-rt run read item-{code} {root}--spec {spec}` before continuing. The wave left the plan and this report was not integrated: confirm its facts and claimed work in the code before reusing them."
        }
        ("wave_prompt.summary.read", Locale::PtBr) => {
            "Estas tarefas voltaram de um agente que encheu a conversa, e o resumo dele é {code}. \
             Antes de qualquer outro passo, leia-o com `mustard-rt run read item-{code} {root}--spec {spec}`. \
             A entrega é recusada sem essa leitura. O que o resumo dá como feito e decidido vale: não \
             confira de novo nem refaça a escolha. Se você também entregar antes do fim, o seu resumo \
             substitui este."
        }
        ("wave_prompt.summary.read", Locale::EnUs) => {
            "These tasks came back from an agent whose conversation filled up, and its summary is {code}. \
             Before any other step, read it with `mustard-rt run read item-{code} {root}--spec {spec}`. \
             The delivery is refused without that reading. What the summary gives as done and decided \
             holds: do not check it again or redo the choice. If you also deliver before the end, your \
             summary replaces this one."
        }
        // A linha sob a tarefa cujo arquivo mudou no git depois do texto dela
        // (`packages/core/src/domain/wave_prompt/changed.rs`).
        ("wave_prompt.task_changed", Locale::PtBr) => {
            "O git mudou o arquivo desta tarefa depois que o texto dela foi escrito (commits: {commits}; \
             arquivos: {files}). Confira no código antes de mudar. Se o que ela pede já está feito, diga \
             na entrega que já estava feita e não mude nada."
        }
        ("wave_prompt.task_changed", Locale::EnUs) => {
            "Git changed this task's file after its text was written (commits: {commits}; files: {files}). \
             Check the code before changing anything. If what it asks is already done, say in the delivery \
             that it was already done and change nothing."
        }
        // O que um agente deixou preso, encerrado no início da sessão, em
        // cada rodada e no fechamento (`apps/rt/src/commands/flow/stuck.rs`).
        ("stuck.found", Locale::PtBr) => "Processos possivelmente abandonados: {list}. Nenhum foi encerrado automaticamente.",
        ("stuck.found", Locale::EnUs) => "Possibly abandoned processes: {list}. None were terminated automatically.",
        ("stuck.reason.waiting_loop", Locale::PtBr) => "laço de espera",
        ("stuck.reason.waiting_loop", Locale::EnUs) => "waiting loop",
        ("stuck.reason.deleted_copy", Locale::PtBr) => "cópia de onda apagada",
        ("stuck.reason.deleted_copy", Locale::EnUs) => "deleted wave copy",
        ("stuck.reason.idle_copy", Locale::PtBr) => "cópia sem onda em andamento",
        ("stuck.reason.idle_copy", Locale::EnUs) => "copy with no wave running",
        // As cópias de obra que sobraram no disco, na retomada e no
        // fechamento, e a obra em execução que perdeu a branch dela.
        ("copies.left", Locale::PtBr) => {
            "Cópias de obra sobraram no disco, somando {total}: {list}. Tire-as com `mustard-rt run clean --apply`."
        }
        ("copies.left", Locale::EnUs) => {
            "Work copies were left on disk, {total} in all: {list}. Remove them with `mustard-rt run clean --apply`."
        }
        ("copies.branch_missing", Locale::PtBr) => {
            "A obra está marcada como em execução, e a branch {branch} já não existe. Ninguém a retoma assim: feche a obra ou corte a branch de novo. A cópia dela já conta como sobra."
        }
        ("copies.branch_missing", Locale::EnUs) => {
            "This work is marked as running, and branch {branch} no longer exists. Nobody can resume it: close the work or cut the branch again. Its copy already counts as leftover."
        }
        // O bloco de retomada, no aviso antes de compactar (`PreCompact`) e
        // no início da sessão depois do resumo.
        ("conversation_size.block", Locale::PtBr) => {
            "Retomada da obra: spec {spec}, fase {phase}. Ondas entregues: {delivered} no total. Em andamento: \
             {running}. Voltas gravadas à espera da rodada: {returned}. Paradas no limite de \
             consertos: {stuck}. Falta: {missing}. Gravado depois da última rodada: {recorded}. \
             Próximo comando: {command}. {next}"
        }
        ("conversation_size.block", Locale::EnUs) => {
            "Work resume: spec {spec}, phase {phase}. Delivered waves: {delivered} in all. In flight: \
             {running}. Returns recorded, waiting for the round: {returned}. Stopped at the fix \
             limit: {stuck}. Missing: {missing}. Recorded after the last round: {recorded}. Next \
             command: {command}. {next}"
        }
        ("conversation_size.copy", Locale::PtBr) => "{wave} (cópia {copy})",
        ("conversation_size.copy", Locale::EnUs) => "{wave} (copy {copy})",
        ("conversation_size.replan", Locale::PtBr) => "{wave} (pede mudança de plano, ainda sem o clique do usuário)",
        ("conversation_size.replan", Locale::EnUs) => "{wave} (asks for a plan change, still without the user's click)",
        ("conversation_size.more", Locale::PtBr) => "e mais {count}",
        ("conversation_size.more", Locale::EnUs) => "and {count} more",
        ("conversation_size.precompact", Locale::PtBr) => {
            "Esta conversa vai ser compactada agora; depois do resumo, o início da sessão traz de \
             volta, sozinho, o bloco de retomada. {block}"
        }
        ("conversation_size.precompact", Locale::EnUs) => {
            "This conversation is about to be compacted; after the summary, the session start \
             brings the resume block back on its own. {block}"
        }
        // O aviso a quem conduz, quando a conversa principal passa de um
        // degrau de tamanho: limpar ou compactar, com o bloco de retomada
        // pronto para colar numa janela limpa.
        ("conversation_size.notice", Locale::PtBr) => {
            "[Mustard] Contexto atingiu {tokens} mil tokens. Termine o passo atual e use `/compact` ou `/clear`. {line}"
        }
        ("conversation_size.notice", Locale::EnUs) => {
            "[Mustard] Context reached {tokens} thousand tokens. Finish the current step and use `/compact` or `/clear`. {line}"
        }
        // O mesmo aviso com onda de agente vivo: limpar a conversa para os
        // agentes, e a volta delas vem antes.
        ("conversation_size.notice_live", Locale::PtBr) => {
            "[Mustard] Contexto atingiu {tokens} mil tokens. Há {count} onda(s) com agente vivo ({waves}): espere a volta delas antes de `/clear`, que para esses agentes. Depois, termine o passo atual e use `/compact` ou `/clear`. {line}"
        }
        ("conversation_size.notice_live", Locale::EnUs) => {
            "[Mustard] Context reached {tokens} thousand tokens. There are {count} wave(s) with a live agent ({waves}): wait for them to return before `/clear`, which stops those agents. Then finish the current step and use `/compact` or `/clear`. {line}"
        }
        // A retomada depois de `/clear`: as ondas cujo agente a limpeza parou
        // e a linha que as reenvia.
        ("resume.lost_by_clear", Locale::PtBr) => {
            "A limpeza da conversa parou o agente das ondas {waves}. Na próxima rodada, mande {paused} no relatório, e a rodada as reenvia na mesma cópia."
        }
        ("resume.lost_by_clear", Locale::EnUs) => {
            "Clearing the conversation stopped the agent of waves {waves}. On the next round, send {paused} in the report, and the round resends them in the same copy."
        }
        // A entrada e os resumos herdados precedem o horizonte de trabalho,
        // e o aviso diz a janela do modelo, que só serve de teto ao horizonte.
        ("conversation_size.wave_signal", Locale::PtBr) => {
            "[Mustard] Sinal de conclusão: contexto total, {now} mil tokens; entrada e resumos herdados, {baseline} mil; trabalho, {counted} mil. {basis} O limite é {total_limit} mil tokens de contexto. Termine a tarefa em curso, execute os testes mínimos dela, resuma na entrega e volte ao orquestrador. Não comece outra tarefa. A tarefa em curso continua liberada."
        }
        ("conversation_size.wave_signal", Locale::EnUs) => {
            "[Mustard] Wrap-up signal: total context, {now} thousand tokens; initial input and inherited summaries, {baseline} thousand; work, {counted} thousand. {basis} The limit is {total_limit} thousand context tokens. Finish the task in progress, run its minimal tests, summarize in the delivery and return to the orchestrator. Do not start another task. The task in progress remains executable."
        }
        // A janela que serve de teto ao sinal: a do modelo da onda,
        // conhecida pela tabela da API, ou nenhuma, e então o sinal fica sem
        // teto. O tamanho dela vai por extenso: 1 milhão, ou o número de mil.
        ("conversation_size.window_known", Locale::PtBr) => {
            "Janela do modelo {model}: {window}; o teto do sinal é {share}% dela."
        }
        ("conversation_size.window_known", Locale::EnUs) => {
            "Window of model {model}: {window}; the signal is capped at {share}% of it."
        }
        // A janela que o registro da conversa declara, sem o nome do modelo,
        // que a amostra de uso do Codex não traz.
        ("conversation_size.window_declared", Locale::PtBr) => "Janela do modelo declarada pela conversa: {window}; o teto do sinal é {share}% dela.",
        ("conversation_size.window_declared", Locale::EnUs) => "Model window declared by the conversation: {window}; the signal is capped at {share}% of it.",
        ("conversation_size.window_million", Locale::PtBr) => "1 milhão de tokens",
        ("conversation_size.window_million", Locale::EnUs) => "1 million tokens",
        ("conversation_size.window_thousands", Locale::PtBr) => "{n} mil tokens",
        ("conversation_size.window_thousands", Locale::EnUs) => "{n} thousand tokens",
        ("conversation_size.window_unknown", Locale::PtBr) => "Janela do modelo desconhecida: o sinal fica sem teto.",
        ("conversation_size.window_unknown", Locale::EnUs) => "Model window unknown: the signal has no cap.",
        ("conversation_size.wave_continue", Locale::PtBr) => {
            "[Mustard] Fim de tarefa: contexto total, {now} mil tokens; entrada e resumos herdados, {baseline} mil; trabalho, {counted} mil. O limite é {total_limit} mil tokens de contexto. A maior tarefa acrescentou {largest} mil. Siga para a próxima tarefa."
        }
        ("conversation_size.wave_continue", Locale::EnUs) => {
            "[Mustard] Task finished: total context, {now} thousand tokens; initial input and inherited summaries, {baseline} thousand; work, {counted} thousand. The limit is {total_limit} thousand context tokens. The largest task added {largest} thousand. Go on to the next task."
        }
        // A resposta do passo que termina a última tarefa da onda: não há
        // próxima, e a ordem é entregar.
        ("conversation_size.tasks_finished", Locale::PtBr) => {
            "[Mustard] Todas as tarefas da onda terminaram. Grave a entrega e devolva o relatório a quem \
             conduz."
        }
        ("conversation_size.tasks_finished", Locale::EnUs) => {
            "[Mustard] Every task of the wave is finished. Record the delivery and return the report to \
             the conductor."
        }
        // A mesma medida quando outra tarefa não cabe: a ordem de entregar,
        // que fecha a trava, e como escrever o resumo, em cinco blocos fixos.
        // O texto da entrega é o único lugar do resumo.
        ("conversation_size.wave_deliver", Locale::PtBr) => {
            "[Mustard] Fim de tarefa: contexto total, {now} mil tokens; entrada e resumos herdados, {baseline} mil; trabalho, {counted} mil. O limite é {total_limit} mil tokens de contexto. A maior tarefa acrescentou {largest} \
             mil: não comece outra tarefa. Termine a tarefa em curso e execute os testes mínimos dela, mesmo acima do limite. Grave a entrega, com as tarefas não começadas em `undone`. O texto \
             dela é o resumo, e um agente novo continua só por ele. Escreva cinco blocos, nesta \
             ordem. Estado: se compila, cada teste que rodou, com o comando e o resultado, e o que \
             não rodou. Feito: cada tarefa concluída numa frase. Decidido: cada escolha que vale \
             para as tarefas que sobram, com o motivo, sem alternativa aberta. Fatos: o que você \
             descobriu e o próximo vai precisar, com a fonte e o comando que confere. Dúvidas: o que \
             você não sabe; sem nenhuma, escreva \"nenhuma\". Se você continuou outro resumo, o seu \
             o substitui: traga dele o que ainda vale. Daqui em diante, só leia e grave na spec. \
             Rode os comandos declarados de compilar, testar e verificar. Devolva o relatório a quem conduz."
        }
        ("conversation_size.wave_deliver", Locale::EnUs) => {
            "[Mustard] Task finished: total context, {now} thousand tokens; initial input and inherited summaries, {baseline} thousand; work, {counted} thousand. The limit is {total_limit} thousand context tokens. The \
             largest task added {largest} thousand: do not start another task. Finish the task in progress and run its minimal tests, even above the limit. Record the delivery, with \
             the tasks not started in `undone`. Its text is the summary, and a new agent continues \
             from it alone. Write five blocks, in this order. State: whether it builds, each test \
             that ran, with the command and the result, and what did not run. Done: each finished \
             task in one sentence. Decided: each choice that holds for the remaining tasks, with the \
             reason, with no open alternative. Facts: what you found out and the next one will \
             need, with the source and the command that checks it. Doubts: what you do not know; \
             with none, write \"none\". If you continued another summary, yours replaces it: bring \
             from it what still holds. From here on, only read and write the spec. \
             Run the declared build, test and lint commands. Return the report to the conductor."
        }
        // A recusa ao agente de onda depois da ordem de entregar.
        ("conversation_size.wave_locked", Locale::PtBr) => {
            "[Mustard] A ordem de entregar já saiu. Só passam ler e gravar na spec (`mustard-rt run \
             read` e `run write`){build}, inclusive encadeados quando cada parte for permitida. Substituições e comandos alheios continuam bloqueados. \
             A devolução do relatório a quem conduz também passa. Grave a entrega, com as tarefas \
             não começadas em `undone`, e pare."
        }
        ("conversation_size.wave_locked", Locale::EnUs) => {
            "[Mustard] The order to deliver is out. Only reading and writing the spec (`mustard-rt \
             run read` and `run write`){build} pass, including composed commands when every part is allowed. Nested substitutions and unrelated commands remain blocked. Handing the report back to whoever leads passes too. Record the delivery, \
             with the tasks not started in `undone`, and stop."
        }
        // O que a recusa acrescenta quando o projeto declara o comando de
        // compilar.
        ("conversation_size.wave_locked_build", Locale::PtBr) => " e `{command}`",
        ("conversation_size.wave_locked_build", Locale::EnUs) => " and `{command}`",
        ("round.files_diverged", Locale::PtBr) => {
            "A cópia da onda {wave} mudou {changed} arquivo(s) e a entrega citou {declared}: ficou de \
             fora {missing}. Todos entraram no commit mesmo assim."
        }
        ("round.files_diverged", Locale::EnUs) => {
            "Wave {wave}'s copy changed {changed} file(s) and the delivery cited {declared}: {missing} \
             was left out. All of it went into the commit anyway."
        }
        ("round.usage_missing", Locale::PtBr) => {
            "O arquivo de conversa do agente da onda {wave} não foi achado entre os que a \
             plataforma grava para esta sessão. O consumo dela não entrou na página, mas a entrega \
             ficou gravada assim mesmo."
        }
        ("round.usage_missing", Locale::EnUs) => {
            "The conversation file of wave {wave}'s agent was not found among the ones the \
             platform records for this session. Its usage did not reach the page, but the delivery \
             was written anyway."
        }
        // A suíte e o lint que o projeto declara, rodados pela rodada no
        // repositório principal antes do commit, com o fim da saída.
        ("round.tests_failed", Locale::PtBr) => {
            "A suíte do projeto (`{command}`) não passou no repositório principal, e a rodada não \
             comitou nada: {output}"
        }
        ("round.tests_failed", Locale::EnUs) => {
            "The project suite (`{command}`) did not pass in the main repository, and the round \
             committed nothing: {output}"
        }
        ("round.lint_failed", Locale::PtBr) => {
            "O lint do projeto (`{command}`) não passou no repositório principal, e a rodada não \
             comitou nada: {output}"
        }
        ("round.lint_failed", Locale::EnUs) => {
            "The project lint (`{command}`) did not pass in the main repository, and the round \
             committed nothing: {output}"
        }
        // A conferência depois da onda, antes do commit: as importações novas
        // contra o padrão do projeto e os restos do que a onda tirou.
        ("round.after_wave", Locale::PtBr) => {
            "A conferência depois da onda achou o que consertar, e só esta onda ficou fora do commit. \
             Mande-a de volta ao agente dela, na mesma cópia. Ele conserta, grava a entrega de novo, \
             e você roda a rodada outra vez. Os avisos não seguram a rodada."
        }
        ("round.after_wave", Locale::EnUs) => {
            "The after-wave check found things to fix, and only this wave was left out of the commit. \
             Send it back to its agent, in the same copy. The agent fixes it, records the delivery again, \
             and you run the round once more. Warnings do not hold the round."
        }
        // O teto das rodadas de conserto abre a recusa da conferência depois
        // da onda, a da compilação, do lint ou da suíte que caem e a da
        // verificação de um critério que cai: a frase serve a todas, e o que
        // vem abaixo diz o que caiu.
        ("round.after_wave.limit", Locale::PtBr) => {
            "A onda {waves} já passou por {max} rodadas de conserto, e a rodada ainda recusa a entrega. \
             Ela ficou fora do commit. Mostre ao usuário o que vem abaixo e faça a pergunta de `question`. \
             Se o usuário disser que o achado é falso positivo, mande a linha `FALSE_POSITIVE` na rodada, \
             com `wave` e o motivo em `reason`. A rodada aceita a onda como está e grava o motivo na spec."
        }
        ("round.after_wave.limit", Locale::EnUs) => {
            "Wave {waves} has already gone through {max} fix rounds, and the round still refuses the \
             delivery. It was left out of the commit. Show the user what follows and ask the question in \
             `question`. If the user says the finding is a false positive, send the `FALSE_POSITIVE` line \
             in the round, with `wave` and the reason in `reason`. The round accepts the wave as it is and \
             records the reason in the spec."
        }
        ("round.after_wave.question", Locale::PtBr) => {
            "A onda {waves} ainda tem o que consertar depois de {max} rodadas de conserto. Mudar o desenho \
             do projeto por um pedido, revisar o plano da onda, tirá-la do plano ou aceitá-la como falso \
             positivo?"
        }
        ("round.after_wave.question", Locale::EnUs) => {
            "Wave {waves} still has things to fix after {max} fix rounds. Change the project design \
             through a request, revise the wave's plan, take it out of the plan, or accept it as a false \
             positive?"
        }
        ("round.after_wave.warnings", Locale::PtBr) => "A conferência depois da onda só deixou avisos, e a rodada seguiu.",
        ("round.after_wave.warnings", Locale::EnUs) => "The after-wave check left only warnings, and the round went on.",
        ("round.after_wave.wave", Locale::PtBr) => "Onda {wave}, rodada de conserto {round} de {max}:",
        ("round.after_wave.wave", Locale::EnUs) => "Wave {wave}, fix round {round} of {max}:",
        ("round.after_wave.wave_warnings", Locale::PtBr) => "Onda {wave}, só avisos:",
        ("round.after_wave.wave_warnings", Locale::EnUs) => "Wave {wave}, warnings only:",
        // A onda recusada cujo agente se foi com o Claude Code que a mandou:
        // o conserto vai a um agente novo, e não ao que fez a onda.
        ("round.after_wave.new_agent", Locale::PtBr) => {
            "O Claude Code que mandou a onda {wave} fechou, e o agente dela não existe mais. Despache um \
             agente novo com o título `{title}`: o Mustard junta o pedido e o trecho dela."
        }
        ("round.after_wave.new_agent", Locale::EnUs) => {
            "The Claude Code that sent wave {wave} closed, and its agent no longer exists. Dispatch a new \
             agent with the title `{title}`: Mustard adds its request and its section."
        }
        ("round.after_wave.import", Locale::PtBr) => {
            "`{file}` linha {line} importa `{target}`: {from} importando {to} vai contra a regra \
             {rule_from} importa {rule_to}, seguida em {along} de {total} importações. Leve essa chamada \
             para um arquivo de {rule_from}."
        }
        ("round.after_wave.import", Locale::EnUs) => {
            "`{file}` line {line} imports `{target}`: {from} importing {to} goes against the rule \
             {rule_from} imports {rule_to}, followed in {along} of {total} imports. Move that call into a \
             {rule_from} file."
        }
        ("round.after_wave.weak", Locale::PtBr) => {
            "`{file}` linha {line} importa `{target}`: {from} importando {to} vai contra o costume \
             {rule_from} importa {rule_to}, seguido em {along} de {total} importações. É só um aviso."
        }
        ("round.after_wave.weak", Locale::EnUs) => {
            "`{file}` line {line} imports `{target}`: {from} importing {to} goes against the habit \
             {rule_from} imports {rule_to}, followed in {along} of {total} imports. It is only a warning."
        }
        ("round.after_wave.cycle", Locale::PtBr) => "`{file}` linha {line} importa `{target}` e fecha um ciclo novo de importações. É só um aviso.",
        ("round.after_wave.cycle", Locale::EnUs) => "`{file}` line {line} imports `{target}` and closes a new import cycle. It is only a warning.",
        ("round.after_wave.leftover", Locale::PtBr) => {
            concat!(
                "`{file}` linha {line} ainda cita `{name}`, que a onda tirou de `{from}`. Tire a citação ou \
                 troque pelo nome novo. Ache e leia o código pelo mapa: ",
                map_commands_pt!(),
                "."
            )
        }
        ("round.after_wave.leftover", Locale::EnUs) => {
            concat!(
                "`{file}` line {line} still cites `{name}`, which the wave took out of `{from}`. Remove the \
                 citation or use the new name. Find and read the code through the map: ",
                map_commands_en!(),
                "."
            )
        }
        ("round.after_wave.orphan", Locale::PtBr) => {
            "`{name}` em `{file}` linha {line} ficou sem uso fora de teste, porque a onda tirou quem a \
             chamava no mapa sintático. Confira usos externos, contratos e registros antes de remover código; este achado é um aviso."
        }
        ("round.after_wave.orphan", Locale::EnUs) => {
            "`{name}` in `{file}` line {line} lost its last use outside tests, because the wave removed its \
             caller in the syntax map. Check external uses, contracts and registrations before removing code; this finding is a warning."
        }
        ("round.after_wave.unused_candidate", Locale::PtBr) => {
            "`{name}` em `{file}` linha {line}: o scan não encontrou uso do código novo fora de teste. \
             Ausência de ligação sintática não prova ausência de uso. Confira contratos, registros e usos externos; aviso, sem bloqueio."
        }
        ("round.after_wave.unused_candidate", Locale::EnUs) => {
            "`{name}` in `{file}` line {line}: the scan found no use of the new code outside tests. \
             A missing syntax relation does not prove missing use. Check contracts, registrations and external uses; warning only."
        }
        ("round.after_wave.analysis_unknown", Locale::PtBr) => {
            "A análise de código novo sem uso está indisponível: falta o mapa da base ou o mapa atual. Não há prova de ausência de uso."
        }
        ("round.after_wave.analysis_unknown", Locale::EnUs) => {
            "New unused code analysis is unavailable: the baseline or current map is missing. Absence of use has not been proved."
        }
        ("round.after_wave.unused", Locale::PtBr) => {
            "`{name}` em `{file}` linha {line} é código novo sem uso fora de teste. Dê a ele um uso no \
             programa, ou tire a declaração e o teste só dela."
        }
        ("round.after_wave.unused", Locale::EnUs) => {
            "`{name}` in `{file}` line {line} is new code with no use outside tests. Give it a use in the \
             program, or remove the declaration and the tests that only cover it."
        }
        ("round.after_wave.unused_test", Locale::PtBr) => {
            "`{file}` linha {line} é um teste que só chama `{name}`, código novo sem uso fora de teste. \
             Tire o teste junto com ela."
        }
        ("round.after_wave.unused_test", Locale::EnUs) => {
            "`{file}` line {line} is a test that only calls `{name}`, new code with no use outside tests. \
             Remove the test along with it."
        }
        ("round.development_build_failed", Locale::PtBr) => {
            "O commit saiu, mas a versão em construção do Mustard não compilou, e a sessão segue no \
             programa compilado anterior: {output}"
        }
        ("round.development_build_failed", Locale::EnUs) => {
            "The commit landed, but the development build of Mustard did not compile, and the session \
             stays on the previous compiled program: {output}"
        }
        ("round.commit_carried_more", Locale::PtBr) => {
            "O commit levou arquivo que a entrega não listou: {extra}, em {repo}. Isso acontece \
             quando um gancho de pre-commit do projeto acrescenta ao commit tudo o que está \
             alterado na pasta. O commit foi desfeito, e nada no disco foi tocado. O trabalho da \
             onda e a sua mudança local seguem onde estavam. Guarde a sua mudança, rode a rodada \
             de novo e devolva a mudança depois:\n{stash}\n{restore}"
        }
        ("round.commit_carried_more", Locale::EnUs) => {
            "The commit carried a file the delivery did not list: {extra}, in {repo}. This happens \
             when a project pre-commit hook adds everything changed in the folder to the commit. \
             The commit was undone and nothing on disk was touched. The wave's work and your local \
             change are where they were. Stash your change, run the round again, and restore the \
             change afterwards:\n{stash}\n{restore}"
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
            "A verificação nova do critério {code} saiu verde sem rodar teste nenhum: o nome do teste não \
             casa. Peça a verificação certa antes de fechar."
        }
        ("round.proof_ran_no_test", Locale::EnUs) => {
            "The new verification of criterion {code} came out green without running any test: the test name \
             does not match. Ask for the right verification before closing."
        }
        ("round.proof_missing_test", Locale::PtBr) => {
            "A verificação nova do critério {code} cita o teste {name}, que não aparece em nenhum arquivo do \
             projeto. Peça esse teste ou a verificação certa antes de fechar."
        }
        ("round.proof_missing_test", Locale::EnUs) => {
            "The new verification of criterion {code} names the test {name}, which appears in no file of the \
             project. Ask for that test or the right verification before closing."
        }
        ("round.size.line", Locale::PtBr) => "onda {wave}: +{added} -{removed}, {files} arquivos",
        ("round.size.line", Locale::EnUs) => "wave {wave}: +{added} -{removed}, {files} files",
        ("round.not_approved", Locale::PtBr) => "A spec está na fase {phase} e ainda não foi aprovada: nenhuma onda sai antes do sim do usuário.",
        ("round.not_approved", Locale::EnUs) => {
            "The spec is in the {phase} phase and is not approved yet: no wave goes out before the user says yes."
        }
        ("round.closed", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}: ela já fechou, e a rodada não despacha onda nova \
             numa spec fechada. Para um pedido novo nela, rode `mustard-rt run reopen --spec {spec} \
             --reason \"<motivo>\"`: ela volta à execução, já aprovada, na mesma branch. Se o \
             servidor reprovou o pull request, o conserto vai pelo mesmo comando com `--fix`."
        }
        ("round.closed", Locale::EnUs) => {
            "The spec {spec} is in the {phase} phase: it has already closed, and the round sends no \
             new wave on a closed spec. For a new request on it, run `mustard-rt run reopen --spec \
             {spec} --reason \"<reason>\"`: it goes back to running, already approved, on the same \
             branch. If the server failed the pull request, the repair goes through the same \
             command with `--fix`."
        }
        ("round.finished", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}: ela já foi entregue na base, com o merge feito, \
             ou descartada. A rodada não despacha onda nela. Ela não volta: o pedido novo sobre \
             ela abre uma spec nova com `mustard-rt run open`. Nada foi gravado."
        }
        ("round.finished", Locale::EnUs) => {
            "The spec {spec} is in the {phase} phase: it has already been delivered to the base, \
             already merged, or discarded. The round sends no wave on it. It does not come \
             back: a new request on it opens a new spec with `mustard-rt run open`. Nothing was \
             written."
        }
        ("round.commit_too_long", Locale::PtBr) => "O {part} da mensagem do commit tem {chars} caracteres e o teto é {max}.",
        ("round.commit_too_long", Locale::EnUs) => "The commit message {part} has {chars} characters and the cap is {max}.",
        ("round.commit_forbidden", Locale::PtBr) => "A mensagem do commit traz {found}, que ela nunca leva. Tire e peça a rodada de novo.",
        ("round.commit_forbidden", Locale::EnUs) => {
            "The commit message carries {found}, which it never carries. Take it out and run the round again."
        }
        ("round.commit_looks_like_sha", Locale::PtBr) => {
            "O campo `commit` chegou como {found}, com cara de código de commit. Ali vai o título da \
             mensagem, em palavras e curto: o agente não comita, quem comita é a rodada."
        }
        ("round.commit_looks_like_sha", Locale::EnUs) => {
            "The `commit` field arrived as {found}, looking like a commit's code. That field takes the \
             message's title, in words and short: the agent does not commit, the round does."
        }
        ("round.commit_text_removed", Locale::PtBr) => {
            "Do resumo do commit da onda {wave} saiu {removed}, que um commit nunca leva. O commit saiu como \"{title}\"."
        }
        ("round.commit_text_removed", Locale::EnUs) => {
            "The commit summary of wave {wave} lost {removed}, which a commit never carries. The commit went out as \"{title}\"."
        }
        ("round.commit_neutral_one", Locale::PtBr) => "Atualiza {file}",
        ("round.commit_neutral_one", Locale::EnUs) => "Update {file}",
        ("round.commit_neutral_many", Locale::PtBr) => "Atualiza {count} arquivos",
        ("round.commit_neutral_many", Locale::EnUs) => "Update {count} files",
        ("round.formatter_missing", Locale::PtBr) => "O projeto usa {name} e ele não foi achado: os arquivos da rodada ficaram sem formatar.",
        ("round.formatter_missing", Locale::EnUs) => "The project uses {name} and it was not found: the round's files were left unformatted.",
        ("round.replan", Locale::PtBr) => {
            "A onda {wave} propôs mudar o plano aprovado e ficou fora do commit. Leia a entrega e \
             pergunte ao usuário com opções \"{yes}\"/\"{no}\" e cabeçalho {code}, explicando o efeito \
             da mudança. Após o aceite, revise as tarefas pendentes ({tasks}) antes de repetir a \
             rodada; o clique não resolve acesso nem reescreve tarefa. Recusar mantém o plano \
             aprovado. Quando a mensagem do usuário já recusa, use \
             `<PLAN_REFUSED>{\"wave\":{wave},\"reason\":\"…\"}</PLAN_REFUSED>` no relatório da rodada. \
             Com ela ou com o \"{no}\", a rodada comita o que a onda mudou, com a `REVIEWED` de \
             sempre. As tarefas voltam soltas ao backlog. Aceitar é só pelo clique."
        }
        ("round.replan", Locale::EnUs) => {
            "Wave {wave} proposed changing the approved plan and was held outside the commit. Read \
             the delivery and ask the user with options \"{yes}\"/\"{no}\" and header {code}, explaining \
             the effect of the change. After acceptance, revise pending tasks ({tasks}) before \
             repeating the round; a click does not resolve access or rewrite a task. Declining keeps \
             the approved plan. When the user's message already declines, use \
             `<PLAN_REFUSED>{\"wave\":{wave},\"reason\":\"…\"}</PLAN_REFUSED>` in the round report. \
             With it or with \"{no}\", the round commits what the wave changed, with the usual \
             `REVIEWED`. The tasks return to the backlog unheld. Accepting takes the click."
        }
        ("round.plan_refused.click", Locale::PtBr) => "O usuário recusou a mudança no clique em \"Recusar\".",
        ("round.plan_refused.click", Locale::EnUs) => "The user declined the change by clicking \"Decline\".",
        ("round.plan_refused.commit", Locale::PtBr) => "parte feita antes da mudança recusada",
        ("round.plan_refused.commit", Locale::EnUs) => "work done before the declined change",
        ("round.no_tasks", Locale::PtBr) => "nenhuma",
        ("round.no_tasks", Locale::EnUs) => "none",
        ("round.replan_needs_undone", Locale::PtBr) => {
            "A entrega da onda {wave} muda o plano e não diz quais tarefas ficaram por fazer. \
             Grave a entrega de novo com `\"undone\":[\"<código>\"]`, com cada tarefa não feita, \
             ou com `\"undone\":[]` se fez todas. Tarefas da onda: {tasks}. Nada foi gravado."
        }
        ("round.replan_needs_undone", Locale::EnUs) => {
            "Wave {wave}'s delivery changes the plan and does not say which tasks were left undone. \
             Record the delivery again with `\"undone\":[\"<task code>\"]`, listing each task not done, \
             or with `\"undone\":[]` if it did them all. The wave's tasks: {tasks}. Nothing was \
             recorded."
        }
        ("round.undone_not_in_wave", Locale::PtBr) => {
            "A tarefa {code} não é da onda {wave}, e `undone` só leva tarefa da própria onda. \
             Tarefas da onda: {tasks}. Nada foi gravado."
        }
        ("round.undone_not_in_wave", Locale::EnUs) => {
            "Task {code} is not in wave {wave}, and `undone` only takes tasks of the wave itself. \
             The wave's tasks: {tasks}. Nothing was recorded."
        }
        ("round.started_work_undone", Locale::PtBr) => {
            "A onda {wave} devolve {tasks} como não feitas, mas a cópia mudou depois do último passo \
             de término. Conclua a tarefa começada, grave o passo dela com `mustard-rt run write step` \
             e entregue de novo. Nada foi gravado."
        }
        ("round.started_work_undone", Locale::EnUs) => {
            "Wave {wave} gives back {tasks} as not done, but the copy changed after the last task-end \
             step. Finish the started task, record its step with `mustard-rt run write step`, and \
             deliver again. Nothing was recorded."
        }
        ("round.done_without_step", Locale::PtBr) => {
            "A onda {wave} dá como feitas as tarefas {tasks} sem o passo de término de cada uma. \
             Grave o passo de cada uma com `mustard-rt run write step` e entregue de novo. Nada foi \
             gravado."
        }
        ("round.done_without_step", Locale::EnUs) => {
            "Wave {wave} gives tasks {tasks} as done without the task-end step of each. Record each \
             one's step with `mustard-rt run write step` and deliver again. Nothing was recorded."
        }
        // A reprovação de quem conduz a obra (`<REJECTED>` no relatório): a
        // volta fica fora do commit, e a onda espera um agente novo, que
        // recebe o motivo.
        ("round.rejected", Locale::PtBr) => {
            "Você reprovou a volta da onda {wave} com o motivo \"{reason}\". A cópia ficou como \
             estava, e a onda espera um agente novo. Despache um agente novo com o título `{title}`: \
             o Mustard junta o pedido e o seu motivo."
        }
        ("round.rejected", Locale::EnUs) => {
            "You rejected wave {wave}'s return with the reason \"{reason}\". The copy stayed as it \
             was, and the wave waits for a new agent. Dispatch a new agent with the title `{title}`: \
             Mustard adds its request and your reason."
        }
        ("round.rejected_working", Locale::PtBr) => {
            "Você reprovou a volta da onda {wave} com o motivo \"{reason}\", e o agente novo dela já \
             trabalha na cópia. A rodada assume a volta nova quando ele a gravar."
        }
        ("round.rejected_working", Locale::EnUs) => {
            "You rejected wave {wave}'s return with the reason \"{reason}\", and its new agent is \
             already working in the copy. The round takes the new return once it records it."
        }
        ("round.rejected_fix", Locale::PtBr) => "Quem conduz a obra reprovou a entrega da onda {wave}, com este motivo: {reason}",
        ("round.rejected_fix", Locale::EnUs) => "Whoever runs the work rejected wave {wave}'s delivery, for this reason: {reason}",
        ("round.rejected_without_return", Locale::PtBr) => {
            "A linha `<REJECTED>` aponta a onda {wave}, que não tem volta gravada à espera da \
             rodada. Só se reprova a entrega que o agente gravou e a rodada ainda não assumiu. Tire \
             a linha e rode a rodada de novo. Nada foi gravado."
        }
        ("round.rejected_without_return", Locale::EnUs) => {
            "The `<REJECTED>` line points at wave {wave}, which has no recorded return waiting for \
             the round. Only a delivery the agent recorded and the round has not taken yet can be \
             rejected. Drop the line and run the round again. Nothing was recorded."
        }
        ("round.rejected_committed", Locale::PtBr) => {
            "A linha `<REJECTED>` aponta a onda {wave}, cuja volta já entrou no commit {sha}, e a \
             rodada não desfaz commit. Tire a linha e grave o que falta como tarefa nova. Nada foi \
             gravado."
        }
        ("round.rejected_committed", Locale::EnUs) => {
            "The `<REJECTED>` line points at wave {wave}, whose return is already in commit {sha}, \
             and the round does not undo a commit. Drop the line and record what is missing as a new \
             task. Nothing was recorded."
        }
        ("round.returned_change", Locale::PtBr) => "Mudança de plano aceita na volta da onda {wave}: {change}",
        ("round.returned_change", Locale::EnUs) => "Plan change accepted when wave {wave} came back: {change}",
        ("round.unmet_joined", Locale::PtBr) => "Item não cumprido na volta da onda {wave} — {code}: {text}",
        ("round.unmet_joined", Locale::EnUs) => "Item not met when wave {wave} came back — {code}: {text}",
        ("round.unmet_covered", Locale::PtBr) => {
            "Item não cumprido na volta da onda {wave} — {code}, que a tarefa {task}, ainda por entregar, já cobre: {text}"
        }
        ("round.unmet_covered", Locale::EnUs) => {
            "Item not met when wave {wave} came back — {code}, which task {task}, still to deliver, already covers: {text}"
        }
        ("round.unmet_review_joined", Locale::PtBr) => "Item não cumprido na revisão final — {code}: {text}",
        ("round.unmet_review_joined", Locale::EnUs) => "Item not met in the final review — {code}: {text}",
        ("round.unmet_review_covered", Locale::PtBr) => {
            "Item não cumprido na revisão final — {code}, que a tarefa {task}, ainda por entregar, já cobre: {text}"
        }
        ("round.unmet_review_covered", Locale::EnUs) => {
            "Item not met in the final review — {code}, which task {task}, still to deliver, already covers: {text}"
        }
        ("round.unmet_judged", Locale::PtBr) => {
            "O Jev julgou que terminar a tarefa termina o que o item pede, com chance {chance}; por isso ele não \
             virou tarefa nova."
        }
        ("round.unmet_judged", Locale::EnUs) => {
            "Jev judged that finishing the task finishes what the item asks, with chance {chance}; so it did not \
             become a new task."
        }
        ("round.leftover_joined", Locale::PtBr) => {
            concat!("Sobra da onda {wave}, nos mesmos arquivos — {title}: {detail} (Pelo mapa: ", map_commands_pt!(), ".)")
        }
        ("round.leftover_joined", Locale::EnUs) => {
            concat!("Leftover from wave {wave}, on the same files — {title}: {detail} (By the map: ", map_commands_en!(), ".)")
        }
        ("round.leftover_classified", Locale::PtBr) => {
            concat!("Erro da volta da onda {wave} — {title}: {detail} (Pelo mapa: ", map_commands_pt!(), ".)")
        }
        ("round.leftover_classified", Locale::EnUs) => {
            concat!("Error from wave {wave}'s return — {title}: {detail} (By the map: ", map_commands_en!(), ".)")
        }
        // Os mesmos textos da sobra, quando quem a apontou é a revisão final,
        // que não tem onda.
        ("round.leftover_review_joined", Locale::PtBr) => {
            concat!("Achado da revisão final, nos mesmos arquivos — {title}: {detail} (Pelo mapa: ", map_commands_pt!(), ".)")
        }
        ("round.leftover_review_joined", Locale::EnUs) => {
            concat!("Finding from the final review, on the same files — {title}: {detail} (By the map: ", map_commands_en!(), ".)")
        }
        ("round.leftover_review_classified", Locale::PtBr) => {
            concat!("Achado da revisão final — {title}: {detail} (Pelo mapa: ", map_commands_pt!(), ".)")
        }
        ("round.leftover_review_classified", Locale::EnUs) => {
            concat!("Finding from the final review — {title}: {detail} (By the map: ", map_commands_en!(), ".)")
        }
        ("round.leftover_priority", Locale::PtBr) => "Sobra da onda {wave}: erro achado na obra sai antes das tarefas antigas.",
        ("round.leftover_priority", Locale::EnUs) => "Leftover from wave {wave}: an error found in the work goes before the old tasks.",
        ("round.leftover_review_priority", Locale::PtBr) => "Achado da revisão final: erro achado na obra sai antes das tarefas antigas.",
        ("round.leftover_review_priority", Locale::EnUs) => "Finding from the final review: an error found in the work goes before the old tasks.",
        ("triage.same_work_open", Locale::PtBr) => {
            "A tarefa aberta {task} — {title} tem o mesmo sentido: nada foi gravado. Grave a versão nova dela, com `replaces` {task}, juntando esta atividade; o pedido em `rewrite` dá o texto. Se for outro trabalho, grave de novo com `distinct` e o motivo."
        }
        ("triage.same_work_open", Locale::EnUs) => {
            "The open task {task} — {title} has the same meaning: nothing was recorded. Record its new version, with `replaces` {task}, joining this activity; the request in `rewrite` gives the text. If it is other work, record again with `distinct` and the reason."
        }
        ("triage.same_work_covered", Locale::PtBr) => {
            "A tarefa aberta {task} — {title} já cobre esta atividade: terminá-la a termina, e nada foi gravado. Se for outro trabalho, grave de novo com `distinct` e o motivo."
        }
        ("triage.same_work_covered", Locale::EnUs) => {
            "The open task {task} — {title} already covers this activity: finishing it finishes this one, and nothing was recorded. If it is other work, record again with `distinct` and the reason."
        }
        ("round.jev_pending.removed_name", Locale::PtBr) => {
            "A onda {wave} cita um nome que ela tirou, e o Jev ainda não disse se é a peça tirada. A pergunta sai com a trava do git solta, e a rodada seguinte aplica a resposta. Rode `mustard-rt run round` de novo; com `reserves` na resposta, mande antes o recibo da reserva."
        }
        ("round.jev_pending.removed_name", Locale::EnUs) => {
            "Wave {wave} cites a name it removed, and Jev has not yet said whether it is the removed piece. The question goes out with the git lock released, and the next round applies the answer. Run `mustard-rt run round` again; with `reserves` in the answer, send the reserve's receipt first."
        }
        ("round.jev_pending.no_change", Locale::PtBr) => {
            "A onda {wave} voltou sem mudança, e o Jev ainda não classificou o relato dela. A pergunta sai com a trava do git solta, e a rodada seguinte aplica a resposta. Rode `mustard-rt run round` de novo; com `reserves` na resposta, mande antes o recibo da reserva."
        }
        ("round.jev_pending.no_change", Locale::EnUs) => {
            "Wave {wave} came back without changes, and Jev has not yet classified its report. The question goes out with the git lock released, and the next round applies the answer. Run `mustard-rt run round` again; with `reserves` in the answer, send the reserve's receipt first."
        }
        ("round.jev_pending.change_target", Locale::PtBr) => {
            "A onda {wave} pede uma mudança de plano que não cita tarefa, e o Jev ainda não disse qual tarefa ela trava. A pergunta sai com as travas do git e das cópias soltas, e a rodada seguinte aplica a resposta. Rode `mustard-rt run round` de novo; com `reserves` na resposta, mande antes o recibo da reserva."
        }
        ("round.jev_pending.change_target", Locale::EnUs) => {
            "Wave {wave} asks for a plan change that cites no task, and Jev has not yet said which task it holds. The question goes out with the git and copies locks released, and the next round applies the answer. Run `mustard-rt run round` again; with `reserves` in the answer, send the reserve's receipt first."
        }
        ("round.jev_pending.unmet_items", Locale::PtBr) => {
            "A onda {wave} não cumpriu um item combinado, e o Jev ainda não disse para onde ele vai. A pergunta sai com as travas do git e das cópias soltas, e a rodada seguinte aplica a resposta. Rode `mustard-rt run round` de novo; com `reserves` na resposta, mande antes o recibo da reserva."
        }
        ("round.jev_pending.unmet_items", Locale::EnUs) => {
            "Wave {wave} did not meet an agreed item, and Jev has not yet said where it goes. The question goes out with the git and copies locks released, and the next round applies the answer. Run `mustard-rt run round` again; with `reserves` in the answer, send the reserve's receipt first."
        }
        ("round.jev_pending.leftovers", Locale::PtBr) => {
            "A onda {wave} voltou com sobras, e o Jev ainda não disse para onde cada uma vai. A pergunta sai com as travas do git e das cópias soltas, e a rodada seguinte aplica a resposta. Rode `mustard-rt run round` de novo; com `reserves` na resposta, mande antes o recibo da reserva."
        }
        ("round.jev_pending.leftovers", Locale::EnUs) => {
            "Wave {wave} came back with leftovers, and Jev has not yet said where each one goes. The question goes out with the git and copies locks released, and the next round applies the answer. Run `mustard-rt run round` again; with `reserves` in the answer, send the reserve's receipt first."
        }
        ("round.jev_pending.review_unmet", Locale::PtBr) => {
            "A revisão final não cumpriu um item combinado, e o Jev ainda não disse para onde ele vai: o veredito fica fora desta chamada. A pergunta sai com as travas do git e das cópias soltas, e a chamada seguinte aplica a resposta. Rode o mesmo comando de novo; com `reserves` na resposta, mande antes o recibo da reserva."
        }
        ("round.jev_pending.review_unmet", Locale::EnUs) => {
            "The final review did not meet an agreed item, and Jev has not yet said where it goes: the verdict stays out of this call. The question goes out with the git and copies locks released, and the next call applies the answer. Run the same command again; with `reserves` in the answer, send the reserve's receipt first."
        }
        ("round.skill_pick_reserve", Locale::PtBr) => {
            "A onda {wave} não saiu nesta rodada. A escolha das skills de uma tarefa dela espera a decisão do Jev. Com `reserves` na resposta, mande antes o recibo da reserva. Ela sai na rodada seguinte com as skills escolhidas."
        }
        ("round.skill_pick_reserve", Locale::EnUs) => {
            "Wave {wave} did not go out in this round. The skill choice of one of its tasks waits for Jev's decision. With `reserves` in the answer, send the reserve's receipt first. It goes out in the next round with the chosen skills."
        }
        ("round.items_jev_pending", Locale::PtBr) => {
            "A onda {wave} não saiu nesta rodada. A escolha dos itens do pedido dela espera a decisão do Jev. Com `reserves` na resposta, mande antes o recibo da reserva. Ela sai na rodada seguinte."
        }
        ("round.items_jev_pending", Locale::EnUs) => {
            "Wave {wave} did not go out in this round. The choice of the items of its request waits for Jev's decision. With `reserves` in the answer, send the reserve's receipt first. It goes out in the next round."
        }
        ("triage.request_new_spec", Locale::PtBr) => {
            "O Jev leu este pedido como obra fora do objetivo desta spec: nada foi gravado. Abra uma spec nova com ele, pelo texto em `request`. Se o usuário disser que é desta spec, grave de novo com `same_spec` e a palavra dele."
        }
        ("triage.request_new_spec", Locale::EnUs) => {
            "Jev read this request as work outside this spec's goal: nothing was recorded. Open a new spec with it, from the text in `request`. If the user says it belongs to this spec, record it again with `same_spec` and their words."
        }
        ("triage.request_pending", Locale::PtBr) => {
            "O Jev leu este pedido como trabalho para depois, fora do objetivo desta spec. Ele foi gravado na lista de pendências do projeto, como {pending}, e não na spec."
        }
        ("triage.request_pending", Locale::EnUs) => {
            "Jev read this request as work for later, outside this spec's goal. It was recorded on the project's pending list, as {pending}, and not in the spec."
        }
        ("triage.request_unsure", Locale::PtBr) => {
            "O Jev não teve certeza do lugar deste pedido: esta spec, uma spec nova ou a lista de pendências do projeto. O pedido foi gravado aqui; confirme com o usuário."
        }
        ("triage.request_unsure", Locale::EnUs) => {
            "Jev was not sure where this request belongs: this spec, a new spec or the project's pending list. It was recorded here; confirm with the user."
        }
        ("triage.request_reserve", Locale::PtBr) => {
            "O Jev não respondeu a tempo sobre este pedido: nada foi gravado ainda. Lance o agente de análise da sessão com o pedido em `reserve`. Mande o recibo por `mustard-rt run knowledge --complete-session <recibo.json>`. Depois grave o pedido de novo, igual, com `reserve` e o `request_id` dele."
        }
        ("triage.request_reserve", Locale::EnUs) => {
            "Jev did not answer in time about this request: nothing was recorded yet. Launch the session's analysis agent with the request in `reserve`. Send its receipt with `mustard-rt run knowledge --complete-session <receipt.json>`. Then record the request again, unchanged, with `reserve` set to its `request_id`."
        }
        ("triage.task_reserve", Locale::PtBr) => {
            "O Jev não respondeu a tempo sobre esta tarefa: nada foi gravado ainda. Lance o agente de análise da sessão com o pedido em `reserve`. Mande o recibo por `mustard-rt run knowledge --complete-session <recibo.json>`. Depois grave a tarefa de novo, igual, com `reserve` e o `request_id` dele."
        }
        ("triage.task_reserve", Locale::EnUs) => {
            "Jev did not answer in time about this task: nothing was recorded yet. Launch the session's analysis agent with the request in `reserve`. Send its receipt with `mustard-rt run knowledge --complete-session <receipt.json>`. Then record the task again, unchanged, with `reserve` set to its `request_id`."
        }
        ("triage.reserve_pass", Locale::PtBr) => {
            "O Jev não respondeu a tempo: as decisões em `reserves` esperam a reserva dele. Lance o agente de análise da sessão com cada pedido. Mande cada recibo por `mustard-rt run knowledge --complete-session <recibo.json>`. Depois rode `{command}` de novo: o recibo decide no lugar do Jev."
        }
        ("triage.reserve_pass", Locale::EnUs) => {
            "Jev did not answer in time: the decisions in `reserves` wait for its reserve. Launch the session's analysis agent with each request. Send each receipt with `mustard-rt run knowledge --complete-session <receipt.json>`. Then run `{command}` again: the receipt decides in place of the Jev."
        }
        ("triage.next_grouping", Locale::PtBr) => {
            "O modelo pequeno da sessão ainda não leu esta lista. Entregue o pedido de `grouping` ao agente de análise da sessão. Mande o recibo por `mustard-rt run knowledge --complete-session <recibo.json>` e rode `run triage` de novo. O Jev ordena cada assunto, e a resposta mostra a lista antes de qualquer gravação."
        }
        ("triage.next_grouping", Locale::EnUs) => {
            "The session's small model has not read this list yet. Hand the `grouping` request to the session's analysis agent. Send its receipt with `mustard-rt run knowledge --complete-session <receipt.json>` and run `run triage` again. The Jev orders each subject, and the answer shows the list before anything is recorded."
        }
        ("round.formation_pending", Locale::PtBr) => {
            "O classificador espera a leitura da lista para formar as próximas ondas. Entregue o pedido de `formation` ao agente de análise da sessão. Mande o recibo por `mustard-rt run knowledge --complete-session <recibo.json>` e rode a rodada de novo. Até lá, só as ondas que ele já formou saem da lista."
        }
        ("round.formation_pending", Locale::EnUs) => {
            "The classifier waits for the list to be read before forming the next waves. Hand the `formation` request to the session's analysis agent. Send its receipt with `mustard-rt run knowledge --complete-session <receipt.json>` and run the round again. Until then, only the waves it already formed leave the list."
        }
        ("triage.wave_too_big", Locale::PtBr) => {
            "A onda \"{objective}\" lê {tokens} tokens de arquivos, acima do limite de uma onda. O agente entrega uma parte e devolve o resumo para a próxima."
        }
        ("triage.wave_too_big", Locale::EnUs) => {
            "The wave \"{objective}\" reads {tokens} tokens of files, above the limit of one wave. The agent delivers a part and returns the summary for the next one."
        }
        ("triage.order_unanswered", Locale::PtBr) => {
            "{missing} de {requests} pedidos de ordem ao Jev ficaram sem resposta usável. Esses assuntos ficam na ordem do modelo pequeno da sessão. A tarefa sem nota de complexidade vai no modelo da configuração."
        }
        ("triage.order_unanswered", Locale::EnUs) => {
            "{missing} of {requests} order requests to the Jev got no usable answer. Those subjects keep the order of the session's small model. A task without a complexity grade goes on the configured model."
        }
        ("triage.held", Locale::PtBr) => "Classificadas, mas fora da gravação, as tarefas de onda com agente trabalhando: {tasks}.",
        ("triage.held", Locale::EnUs) => "Classified but left out of the write, the tasks of a wave whose agent is still working: {tasks}.",
        ("triage.joined_line", Locale::PtBr) => "Junta {code} — {title}: {text}",
        ("triage.joined_line", Locale::EnUs) => "Joins {code} — {title}: {text}",
        ("triage.removed_joined", Locale::PtBr) => "Mesmo sentido de {code}, que recebe este trabalho na versão nova.",
        ("triage.removed_joined", Locale::EnUs) => "Same meaning as {code}, which takes this work in its new version.",
        ("triage.next_dry", Locale::PtBr) => "Nada foi gravado. Confira os assuntos, a ordem e as junções; para gravar, rode de novo com `--apply`.",
        ("triage.next_dry", Locale::EnUs) => "Nothing was recorded. Check the subjects, the order and the merges; to record, run again with `--apply`.",
        ("triage.next_applied", Locale::PtBr) => {
            "Gravado. Delegue cada pedido de `rewrite` ao agente de análise da sessão e grave a versão nova que o recibo devolve."
        }
        ("triage.next_applied", Locale::EnUs) => {
            "Recorded. Delegate each `rewrite` request to the session's analysis agent and record the new version its receipt returns."
        }
        ("round.leftover_repeated", Locale::PtBr) => {
            "Sobra da onda {wave} que repete a tarefa {task}, ainda por entregar: {detail}"
        }
        ("round.leftover_repeated", Locale::EnUs) => {
            "Leftover from wave {wave} that repeats task {task}, still to deliver: {detail}"
        }
        ("round.leftover_judged", Locale::PtBr) => {
            "O Jev julgou que a sobra descreve o mesmo trabalho da tarefa, com chance {chance}; por isso ela \
             não virou tarefa nova."
        }
        ("round.leftover_judged", Locale::EnUs) => {
            "Jev judged that the leftover describes the same work as the task, with chance {chance}; so it did \
             not become a new task."
        }
        ("round.leftover_marked", Locale::PtBr) => "Sobra da onda {wave}: {title}",
        ("round.leftover_marked", Locale::EnUs) => "Leftover from wave {wave}: {title}",
        ("round.leftover_review_marked", Locale::PtBr) => "Achado da revisão final: {title}",
        ("round.leftover_review_marked", Locale::EnUs) => "Finding from the final review: {title}",
        // A sobra só em arquivo que o git ignora: mudança local, que não vira
        // tarefa e fica como nota para o usuário.
        ("round.leftover_local", Locale::PtBr) => {
            "Sobra da onda {wave} só em arquivo que o git ignora ({files}). É mudança local: nenhuma onda a entrega e \
             nenhum commit a leva, por isso não virou tarefa. Cabe ao usuário fazer, nesta máquina — {title}: {detail}"
        }
        ("round.leftover_local", Locale::EnUs) => {
            "Leftover from wave {wave} only in files git ignores ({files}). It is a local change: no wave delivers it \
             and no commit carries it, so it did not become a task. It is up to the user to make it, on this machine \
             — {title}: {detail}"
        }
        ("round.leftover_review_local", Locale::PtBr) => {
            "Achado da revisão final só em arquivo que o git ignora ({files}). É mudança local: nenhuma onda a entrega \
             e nenhum commit a leva, por isso não virou tarefa. Cabe ao usuário fazer, nesta máquina — {title}: {detail}"
        }
        ("round.leftover_review_local", Locale::EnUs) => {
            "Finding from the final review only in files git ignores ({files}). It is a local change: no wave delivers \
             it and no commit carries it, so it did not become a task. It is up to the user to make it, on this \
             machine — {title}: {detail}"
        }
        ("round.wave_left_after_return", Locale::PtBr) => {
            "A onda {wave} saiu do ar por ficar sem tarefa. O agente dela já tinha gravado a volta ao pedido {send}, \
             mas a última tarefa foi removida antes de a rodada assumi-la. A volta fica só na história. A fila, a \
             retomada e a rodada deixam de listar a onda."
        }
        ("round.wave_left_after_return", Locale::EnUs) => {
            "Wave {wave} left the air for having no task. Its agent had already recorded the return to request \
             {send}, but the last task was removed before the round took it over. The return stays in the history \
             only. The queue, the resume and the round no longer list the wave."
        }
        ("round.wave_left_without_tasks", Locale::PtBr) => {
            "A onda {wave} saiu do ar por ficar sem tarefa: o pedido {send} segue sem volta, e nenhuma tarefa viva \
             aponta a onda. A fila, a retomada e a rodada deixam de listá-la."
        }
        ("round.wave_left_without_tasks", Locale::EnUs) => {
            "Wave {wave} left the air for having no task: request {send} has no return, and no live task points to \
             the wave. The queue, the resume and the round no longer list it."
        }
        ("round.leftover_without_files", Locale::PtBr) => {
            "Sobra sem arquivo declarado: grave uma versão nova da tarefa com os arquivos e as \
             dependências antes de ela sair numa onda."
        }
        ("round.leftover_without_files", Locale::EnUs) => {
            "Leftover without a declared file: write a new version of the task with its files and \
             dependencies before it goes out in a wave."
        }
        ("round.tasks_returned", Locale::PtBr) => {
            "A onda {wave} não fez as tarefas {tasks}, e elas voltaram ao backlog, com a mudança \
             de plano anotada quando houve. Se a mudança altera uma decisão ou o que a tarefa pede, \
             grave a decisão e reescreva a tarefa antes da próxima rodada."
        }
        ("round.tasks_returned", Locale::EnUs) => {
            "Wave {wave} did not do tasks {tasks}, and they went back to the backlog, with the plan \
             change noted when there was one. If the change alters a decision or what the task asks, \
             record the decision and rewrite the task before the next round."
        }
        // A prova que cita outro item que não um critério entra assim mesmo:
        // vale para o único critério que a tarefa citada cobre, ou fica como
        // do item citado.
        ("round.proof_rerouted", Locale::PtBr) => {
            "A prova citou {item}, que não é critério. Ela vale para {criterion}, o único critério \
             que essa tarefa cobre. Da próxima vez, cite o critério."
        }
        ("round.proof_rerouted", Locale::EnUs) => {
            "The proof cited {item}, which is not a criterion. It counts for {criterion}, the only \
             criterion that task covers. Next time, cite the criterion."
        }
        ("round.proof_kept", Locale::PtBr) => {
            "A prova citou {item}, do tipo {type}, que não é critério nem leva a um critério só. Ela \
             fica gravada como prova desse item e não muda critério nenhum. Para valer num critério, \
             cite o critério."
        }
        ("round.proof_kept", Locale::EnUs) => {
            "The proof cited {item}, a {type}, which is not a criterion and does not lead to a single \
             one. It stays recorded as that item's proof and changes no criterion. To count for a \
             criterion, cite the criterion."
        }
        ("round.resend_moved", Locale::PtBr) => {
            "A onda {wave} recomeça numa cópia livre: a cópia do envio anterior, {copy}, também é de \
             outra onda, e nenhuma delas a apaga."
        }
        ("round.resend_moved", Locale::EnUs) => {
            "Wave {wave} starts again in a free copy: the previous send's copy, {copy}, is also \
             another wave's, and neither wave wipes it."
        }
        ("round.resend_no_copy", Locale::PtBr) => {
            "A onda {wave} não foi reenviada: a cópia do envio anterior é também de outra onda, e não \
             há cópia livre. Rode a rodada de novo quando uma onda terminar."
        }
        ("round.resend_no_copy", Locale::EnUs) => {
            "Wave {wave} was not resent: the previous send's copy is also another wave's, and no copy \
             is free. Run the round again when a wave finishes."
        }
        ("round.resend_gone", Locale::PtBr) => {
            "A onda {wave} recomeça numa cópia nova: a do envio anterior, {copy}, não é mais uma cópia do projeto."
        }
        ("round.resend_gone", Locale::EnUs) => {
            "Wave {wave} starts again in a new copy: the previous send's copy, {copy}, is no longer a copy of the project."
        }
        ("round.resend_gone_no_copy", Locale::PtBr) => {
            "A onda {wave} não foi reenviada: a cópia do envio anterior, {copy}, não é mais uma cópia do projeto e outra \
             não pôde ser preparada. Corrija o que o aviso da cópia diz e rode a rodada de novo."
        }
        ("round.resend_gone_no_copy", Locale::EnUs) => {
            "Wave {wave} was not resent: the previous send's copy, {copy}, is no longer a copy of the project and another \
             could not be prepared. Fix what the copy warning says and run the round again."
        }
        ("round.code_kept", Locale::PtBr) => {
            "O código que a onda {wave} deixou sem commit na cópia {copy} ficou guardado na ref `{ref}`. \
             Para trazê-lo de volta, rode `git cherry-pick --no-commit {ref}` na cópia que continua o \
             trabalho."
        }
        ("round.code_kept", Locale::EnUs) => {
            "The code wave {wave} left uncommitted in the copy {copy} is kept under the ref `{ref}`. \
             To bring it back, run `git cherry-pick --no-commit {ref}` in the copy that carries on the \
             work."
        }
        ("round.code_kept_slot", Locale::PtBr) => {
            "O código que a cópia {copy} tinha sem commit ficou guardado na ref `{ref}`. Para trazê-lo \
             de volta, rode `git cherry-pick --no-commit {ref}` na cópia que continua o trabalho."
        }
        ("round.code_kept_slot", Locale::EnUs) => {
            "The uncommitted code the copy {copy} had is kept under the ref `{ref}`. To bring it back, \
             run `git cherry-pick --no-commit {ref}` in the copy that carries on the work."
        }
        ("round.copy_not_cleaned", Locale::PtBr) => {
            "A cópia da onda {wave} não foi limpa: o código dela não pôde ser guardado antes ({detail}). \
             A vaga segue com ela; resolva e rode a rodada de novo."
        }
        ("round.copy_not_cleaned", Locale::EnUs) => {
            "Wave {wave}'s copy was not wiped: its code could not be kept first ({detail}). The slot \
             stays with it; fix that and run the round again."
        }
        ("round.held_return", Locale::PtBr) => "A volta da onda {wave} ficou fora desta rodada, e só ela: o resto seguiu. {hint}",
        ("round.held_return", Locale::EnUs) => "Wave {wave}'s return was left out of this round, and only it: the rest went on. {hint}",
        ("round.project_hook_refused", Locale::PtBr) => {
            "O gancho `{hook}` deste projeto barrou o commit da rodada. A barreira é do projeto, e o Mustard não passa por ela. Ele reprovou: {files}. Foi isto que ele disse:\n{detail}\nNada foi gravado. Corrija o que ele apontou e rode a rodada de novo."
        }
        ("round.project_hook_refused", Locale::EnUs) => {
            "This project's `{hook}` hook blocked the round's commit. The check is the project's, and Mustard does not bypass it. It rejected: {files}. This is what it said:\n{detail}\nNothing was recorded. Fix what it pointed at and run the round again."
        }
        ("round.project_hook_no_file", Locale::PtBr) => "nenhum arquivo que ele tenha citado pelo nome",
        ("round.project_hook_no_file", Locale::EnUs) => "no file it named",
        ("round.git_refused", Locale::PtBr) => {
            "O git recusou: {detail}\nNada do que ele recusou foi gravado. Corrija o que o git \
             apontou, ou peça ao agente que grave a entrega corrigida, e rode a rodada de novo."
        }
        ("round.git_refused", Locale::EnUs) => {
            "Git refused: {detail}\nNothing it refused was recorded. Fix what git pointed \
             out, or have the agent record the corrected delivery, and run the round again."
        }
        // O commit da rodada e a cópia de cada onda nascem do ramo do checkout:
        // fora do ramo da spec, a rodada não junta, não comita e não abre vaga.
        // O caminho da entrega que nenhum commit leva: a onda grava de novo.
        ("round.invalid_path", Locale::PtBr) => {
            "A entrega cita o caminho {file}, que nunca entra num commit: um caminho absoluto, que sobe \
             de pasta ou que fica dentro de `node_modules`. Só esta onda ficou de fora. O agente grava a \
             entrega de novo sem ele."
        }
        ("round.invalid_path", Locale::EnUs) => {
            "The delivery names the path {file}, which never goes into a commit: an absolute path, one \
             that climbs out of the folder, or one inside `node_modules`. Only this wave was left out. \
             The agent records the delivery again without it."
        }
        ("round.wrong_branch", Locale::PtBr) => {
            "O checkout está no ramo {current}, e a spec é do ramo {branch}. A rodada não comita \
             nem abre cópia fora dele. Volte ao ramo da spec com `git switch {branch}` e rode a rodada \
             de novo."
        }
        ("round.wrong_branch", Locale::EnUs) => {
            "The checkout is on branch {current}, and the spec belongs to branch {branch}. The round \
             neither commits nor opens copies off it. Go back to the spec branch with \
             `git switch {branch}` and run the round again."
        }
        // A resposta da rodada não traz o pedido da onda: traz o comando que
        // o lê, e é o agente quem o roda.
        ("round.next", Locale::PtBr) => {
            "Despache cada entrada de `dispatch` a um agente `mustard-wave`, com o modelo do campo \
             `model` dela e a descrição do campo `description`. O prompt tem duas linhas: o título \
             `# <spec> — onda <wave>`, com o número do campo `wave`, e o comando do campo `read`. Esse \
             comando traz o pedido e as regras."
        }
        ("round.next", Locale::EnUs) => {
            "Dispatch each `dispatch` entry to a `mustard-wave` agent, on the model in its `model` \
             field and with the description in its `description` field. The prompt has two lines: the title `# <spec> — wave <wave>`, with the number in \
             the `wave` field, and the command in the `read` field. That command brings the request and \
             the rules."
        }
        // O pedido de publicar e copiar a página fica por extenso só no
        // arquivo, e a resposta leva a linha curta.
        ("round.next.copy_file", Locale::PtBr) => "Leia `{path}` e siga as instruções de lá.",
        ("round.next.copy_file", Locale::EnUs) => "Read `{path}` and follow the instructions there.",
        ("round.report", Locale::PtBr) => {
            "Ao terminar, o agente grava com `mustard-rt run write delivered`; o revisor, com \
             `mustard-rt run write verdict`. Sem volta, mande o agente gravá-la de novo pela ferramenta. Nunca a monte a partir da prosa dele. \
             Com a volta gravada, a rodada pede o `mustard-review` da onda. Lance-o com o título e o comando devolvidos; \
             leia só o veredito e os arquivos apontados. Sem ele, veja o resumo com `mustard-rt run review-wave --wave <n>` e rode a rodada com \
             `--report '<REVIEWED>{\"wave\":1,\"note\":\"…\"}</REVIEWED>'`, sem impressão nem linha de uso. O binário sabe qual volta \
             você viu e mede o consumo nas conversas. Para recusar, use `<REJECTED>{\"wave\":1,\"reason\":\"…\"}</REJECTED>`. \
             A recusa preserva a cópia e passa o motivo a um agente novo. Commit usa `commit` da entrega."
        }
        ("round.report", Locale::EnUs) => {
            "On finishing, the agent records with `mustard-rt run write delivered`; the reviewer, with \
             `mustard-rt run write verdict`. Missing return: have the agent record it again through the tool. Never assemble it from its prose. \
             With the return recorded, the round asks for the wave's `mustard-review`. Launch it with the title and command it returns; \
             read only the verdict and the files it points at. Without it, see the summary with `mustard-rt run review-wave --wave <n>` and run the round with \
             `--report '<REVIEWED>{\"wave\":1,\"note\":\"…\"}</REVIEWED>'`, with no fingerprint or usage line. The binary knows which \
             return you saw and measures usage from the conversations. To reject, use `<REJECTED>{\"wave\":1,\"reason\":\"…\"}</REJECTED>`. \
             Rejection preserves the copy and sends the reason to a new agent. Commits use the delivery’s `commit`."
        }
        ("round.review.hint", Locale::PtBr) => {
            "Leia o resumo: os arquivos com as linhas postas e tiradas, o que saiu do plano (`outside_scope`, \
             `tasks_without_step`) e os passos e provas da onda (`steps`, `proofs`). O diff inteiro vem com `--diff`. \
             Confira regras/skills e testes. Para aceitar, rode a rodada com \
             `--report '<REVIEWED>{\"wave\":{wave},\"note\":\"…\"}</REVIEWED>'`: o aceite vale para o diff que você viu, sem copiar nada. \
             Para recusar, use `<REJECTED>{\"wave\":{wave},\"reason\":\"…\"}</REJECTED>`. \
             Não peça aprovação ao usuário para uma revisão técnica já autorizada."
        }
        ("round.review.hint", Locale::EnUs) => {
            "Read the summary: files with lines added and removed, what left the plan (`outside_scope`, \
             `tasks_without_step`) and the wave's steps and proofs (`steps`, `proofs`). The whole diff comes with `--diff`. \
             Check project rules/skills and tests. To accept, run the round with \
             `--report '<REVIEWED>{\"wave\":{wave},\"note\":\"…\"}</REVIEWED>'`: the acceptance holds for the diff you saw, with nothing to copy. \
             To reject, use `<REJECTED>{\"wave\":{wave},\"reason\":\"…\"}</REJECTED>`. \
             Technical review does not require another user approval."
        }
        ("round.review.required", Locale::PtBr) => {
            "A onda {wave} aguarda revisão antes do commit. Rode `mustard-rt run review-wave --wave {wave} --spec {spec}`, \
             leia o resumo e aceite na rodada com `<REVIEWED>{\"wave\":{wave},\"note\":\"…\"}</REVIEWED>` ou recuse com `REJECTED`. \
             O aceite vale para o diff visto: se ele mudou depois, veja de novo. A entrega gravada não aprova código."
        }
        ("round.review.required", Locale::EnUs) => {
            "Wave {wave} awaits review before commit. Run `mustard-rt run review-wave --wave {wave} --spec {spec}`, \
             read the summary and accept in the round with `<REVIEWED>{\"wave\":{wave},\"note\":\"…\"}</REVIEWED>` or reject with `REJECTED`. \
             Acceptance holds for the diff seen: if it changed since, view it again. A recorded delivery does not approve code."
        }
        ("round.wave_review.required", Locale::PtBr) => {
            "A onda {wave} passou nas conferências e espera o revisor dela antes do commit. Lance o `mustard-review` \
             com o despacho de duas linhas: `{title}` e `{read}`. Ele grava um veredito curto; rode a rodada de novo \
             e leia só o resultado, o motivo e os arquivos apontados. O diff inteiro fica com `run review-wave --wave {wave}`, a pedido."
        }
        ("round.wave_review.required", Locale::EnUs) => {
            "Wave {wave} passed the checks and awaits its reviewer before commit. Launch `mustard-review` \
             with the two-line dispatch: `{title}` and `{read}`. It records a short verdict; run the round again \
             and read only the result, the reason and the files it points at. The whole diff stays in `run review-wave --wave {wave}`, on request."
        }
        ("round.wave_review.title", Locale::PtBr) => "{spec} — revisão da onda {wave}",
        ("round.wave_review.title", Locale::EnUs) => "{spec} — review of wave {wave}",
        ("round.wave_review.request", Locale::PtBr) => {
            "Revise a volta da onda {wave} sem corrigir nada. Leia só o que este pedido traz; abra um arquivo da cópia `{copy}` \
             só para conferir um ponto do diff. Responda com um veredito curto, gravado por `{command}`: \
             `{\"wave\":{wave},\"result\":\"approved\",\"text\":\"…\"}` aprova; \
             `{\"wave\":{wave},\"result\":\"rejected\",\"text\":\"o motivo em poucas frases\",\"files\":[\"…\"]}` reprova e aponta os arquivos. \
             Só o defeito crítico reprova. O arquivo marcado com `*` saiu do plano."
        }
        ("round.wave_review.request", Locale::EnUs) => {
            "Review wave {wave}'s return without fixing anything. Read only what this request brings; open a file of the copy `{copy}` \
             only to check a point of the diff. Answer with a short verdict, recorded by `{command}`: \
             `{\"wave\":{wave},\"result\":\"approved\",\"text\":\"…\"}` approves; \
             `{\"wave\":{wave},\"result\":\"rejected\",\"text\":\"the reason in a few sentences\",\"files\":[\"…\"]}` rejects and points at the files. \
             Only a critical defect rejects. A file marked `*` left the plan."
        }
        ("round.wave_review.tasks", Locale::PtBr) => "Tarefas da onda",
        ("round.wave_review.tasks", Locale::EnUs) => "Tasks of the wave",
        ("round.wave_review.covered", Locale::PtBr) => "Critérios e pedidos que elas cobrem",
        ("round.wave_review.covered", Locale::EnUs) => "Criteria and requests they cover",
        ("round.wave_review.rules", Locale::PtBr) => "Regras e decisões da onda",
        ("round.wave_review.rules", Locale::EnUs) => "Rules and decisions of the wave",
        ("round.wave_review.skills", Locale::PtBr) => "Skills que foram com as tarefas",
        ("round.wave_review.skills", Locale::EnUs) => "Skills sent with the tasks",
        ("round.wave_review.proofs", Locale::PtBr) => "Provas e passos que passaram",
        ("round.wave_review.proofs", Locale::EnUs) => "Proofs and steps that passed",
        ("round.wave_review.diff", Locale::PtBr) => "Diff da onda contra a base dela",
        ("round.wave_review.diff", Locale::EnUs) => "The wave's diff against its base",
        ("round.wave_review.left_out", Locale::PtBr) => {
            "Ficaram de fora do diff, pelo tamanho: {files}. Leia-os na cópia só se o veredito depender deles."
        }
        ("round.wave_review.left_out", Locale::EnUs) => {
            "Left out of the diff for size: {files}. Read them in the copy only if the verdict depends on them."
        }
        ("round.report.checks", Locale::PtBr) => {
            "A rodada junta e comita sem compilar nem rodar prova. O fechamento compila e valida \
             o lint, a suíte completa e os critérios da spec. Não rode a suíte completa nem lint por conta própria."
        }
        ("round.report.checks", Locale::EnUs) => {
            "The round joins and commits without building or running proofs. Closing builds and validates \
             lint, the full suite and the spec criteria. Do not run the full suite or lint on your own."
        }
        ("round.waiting", Locale::PtBr) => {
            "Nada novo a despachar nem a revisar: as ondas {waves} já têm agente trabalhando. \
             Espere a volta delas."
        }
        ("round.waiting", Locale::EnUs) => {
            "Nothing new to dispatch or review: waves {waves} already have an agent working. \
             Wait for them to come back."
        }
        // O que segura a tarefa pronta que a rodada deixou na fila: o detalhe
        // de cada motivo, no `held` da resposta, e as frases do próximo passo.
        ("round.queued.capacity", Locale::PtBr) => {
            "o limite é de {limit} ondas compilando ao mesmo tempo ({origin}), e as {occupied} vagas estão com as ondas {waves}"
        }
        ("round.queued.capacity", Locale::EnUs) => {
            "the limit is {limit} waves compiling at once ({origin}), and the {occupied} slots are held by waves {waves}"
        }
        ("round.queued.no_slots", Locale::PtBr) => {
            "o limite é de {limit} ondas compilando ao mesmo tempo ({origin}), e com esse número nenhuma onda sai"
        }
        ("round.queued.no_slots", Locale::EnUs) => {
            "the limit is {limit} waves compiling at once ({origin}), and with that number no wave goes out"
        }
        ("round.queued.origin_config", Locale::PtBr) => "`maxCompilingWaves` do `mustard.json`",
        ("round.queued.origin_config", Locale::EnUs) => "`maxCompilingWaves` in `mustard.json`",
        ("round.queued.origin_default", Locale::PtBr) => "padrão do Mustard, sem `maxCompilingWaves` no `mustard.json`",
        ("round.queued.origin_default", Locale::EnUs) => "Mustard default, with no `maxCompilingWaves` in `mustard.json`",
        ("round.queued.file", Locale::PtBr) => "arquivo {files}, reservado pela onda {wave}",
        ("round.queued.file", Locale::EnUs) => "file {files}, held by wave {wave}",
        ("round.queued.assembly", Locale::PtBr) => "pronta e com vaga, mas fora desta montagem. A próxima rodada confere de novo",
        ("round.queued.assembly", Locale::EnUs) => "ready with a free slot, but left out of this assembly. The next round checks again",
        ("round.queued.reads", Locale::PtBr) => "lê {files}, que a onda {wave} muda, pelo mapa do scan",
        ("round.queued.reads", Locale::EnUs) => "reads {files}, which wave {wave} changes, per the scan map",
        ("round.queued.read_by", Locale::PtBr) => "muda {files}, que a onda {wave} lê, pelo mapa do scan",
        ("round.queued.read_by", Locale::EnUs) => "changes {files}, which wave {wave} reads, per the scan map",
        ("round.queued.judged", Locale::PtBr) => "o Jev julgou {chance}% de chance de ela mudar o mesmo que uma onda em andamento",
        ("round.queued.judged", Locale::EnUs) => "Jev judged a {chance}% chance that it changes the same as a running wave",
        ("round.queued.behind", Locale::PtBr) => "reservada atrás de {task}, que também ficou e divide {files} com ela",
        ("round.queued.behind", Locale::EnUs) => "held behind {task}, which also stayed and shares {files} with it",
        ("round.queued.behind_lot", Locale::PtBr) => "no mesmo assunto de {task}, que também ficou",
        ("round.queued.behind_lot", Locale::EnUs) => "in the same subject as {task}, which also stayed",
        ("round.queued.following", Locale::PtBr) => {
            "vai no mesmo sentido da onda {wave} e espera a volta dela. Sai na próxima onda, com o resumo dela quando houver"
        }
        ("round.queued.following", Locale::EnUs) => {
            "goes in the same direction as wave {wave} and waits for it to return. It leaves in the next wave, with that wave's summary when there is one"
        }
        ("round.queued.arrived", Locale::PtBr) => "chegou ao backlog nesta rodada e entra na montagem da próxima",
        ("round.queued.arrived", Locale::EnUs) => "reached the backlog this round and joins the next round's assembly",
        ("round.queued.jev_pending", Locale::PtBr) => {
            "espera a decisão do Jev sobre ela; com `reserves` na resposta, mande antes o recibo da reserva"
        }
        ("round.queued.jev_pending", Locale::EnUs) => {
            "waits for Jev's decision on it; with `reserves` in the answer, send the reserve's receipt first"
        }
        ("round.queued.broken_dependency", Locale::PtBr) => {
            "depende de {dependencies}, que não existe mais nesta spec; grave uma versão dela com `replaces` e sem essa dependência"
        }
        ("round.queued.broken_dependency", Locale::EnUs) => {
            "depends on {dependencies}, which no longer exists in this spec; write a version of it with `replaces` and without that dependency"
        }
        ("round.queued.line_held", Locale::PtBr) => "{task} ficou: {detail}.",
        ("round.queued.line_held", Locale::EnUs) => "{task} stayed: {detail}.",
        ("round.queued.line_capacity", Locale::PtBr) => "Sem vaga para {tasks}: {detail}. A fila anda quando uma onda voltar.",
        ("round.queued.line_capacity", Locale::EnUs) => "No slot for {tasks}: {detail}. The queue moves when a wave returns.",
        ("round.queued.line_file", Locale::PtBr) => "{task} espera o {detail}.",
        ("round.queued.line_file", Locale::EnUs) => "{task} waits for the {detail}.",
        ("round.queued.line_other", Locale::PtBr) => "Também ficaram na fila: {tasks}; o motivo de cada uma está em `held`.",
        ("round.queued.line_other", Locale::EnUs) => "Also left in the queue: {tasks}; each one's reason is in `held`.",
        ("round.close", Locale::PtBr) => "Todas as ondas estão entregues e aprovadas: feche a spec com `{command}`.",
        ("round.close", Locale::EnUs) => "Every wave is delivered and approved: close the spec with `{command}`.",
        ("round.review_open", Locale::PtBr) => {
            "Todas as ondas estão entregues, mas a revisão final segue aberta, sem o veredito do revisor. \
             Espere o revisor gravar o veredito e rode a rodada de novo, que o assume."
        }
        ("round.review_open", Locale::EnUs) => {
            "Every wave is delivered, but the final review is still open, without the reviewer's verdict. \
             Wait for the reviewer to record the verdict, then run the round again to take it over."
        }
        ("round.backlog_left", Locale::PtBr) => {
            "Toda onda planejada terminou, mas o backlog ainda tem as tarefas {tasks}, prontas para \
             virar lote. A obra não fecha com tarefa no backlog. Rode a rodada de novo com `{command}`."
        }
        ("round.backlog_left", Locale::EnUs) => {
            "Every planned wave is done, but the backlog still holds tasks {tasks}, ready to become a \
             batch. The work does not close with a task in the backlog. Run the round again with `{command}`."
        }
        ("round.backlog_needs_planning", Locale::PtBr) => {
            "As tarefas {tasks} do backlog esperam plano. Estão presas sem arquivo nenhum e não saem numa \
             onda até ganhar arquivos e dependências. Leia cada uma pelo item e grave uma versão nova, com \
             `replaces`, os arquivos e as dependências e sem o `blocked_by`."
        }
        ("round.backlog_needs_planning", Locale::EnUs) => {
            "Backlog tasks {tasks} wait for planning. They are held without any file and do not go out in a \
             wave until they get files and dependencies. Read each one by its item and write a new version, \
             with `replaces`, the files and the dependencies and without `blocked_by`."
        }
        ("round.backlog_stuck", Locale::PtBr) => {
            "Nada a despachar e nada em andamento, mas o backlog ainda tem as tarefas {tasks}, presas. \
             Nenhuma tem todas as dependências entregues e cobre algum item. Mostre ao usuário o que as segura \
             antes de fechar."
        }
        ("round.backlog_stuck", Locale::EnUs) => {
            "Nothing to dispatch and nothing in flight, but the backlog still holds tasks {tasks}, stuck: \
             none has every dependency delivered and covers an item. Show the user what holds them before closing."
        }
        // O aviso da rodada: uma chamada dela ao Jev falhou, e a rodada seguiu
        // sem ele. O remédio sai pelo motivo da falha.
        ("round.terminal_inside_a_copy", Locale::PtBr) => {
            "Este terminal está dentro de uma cópia de onda: `{folder}`. A rodada apaga as cópias dela, e o shell que perde a pasta em que está não roda mais nada. Saia para a pasta principal do projeto antes da próxima rodada."
        }
        ("round.terminal_inside_a_copy", Locale::EnUs) => {
            "This terminal stands inside a wave copy: `{folder}`. The round deletes its copies, and a shell that loses the folder it stands in runs nothing more. Move to the project's main folder before the next round."
        }
        ("round.assembly_by_files", Locale::PtBr) => {
            "A montagem perguntou ao Jev por {candidates} tarefas e nenhuma onda saiu da resposta dele. As ondas desta rodada saíram pela regra de arquivos."
        }
        ("round.assembly_by_files", Locale::EnUs) => {
            "The assembly asked Jev about {candidates} tasks and no wave came out of its answer. This round's waves were formed by the file rule."
        }
        ("round.jev_failed", Locale::PtBr) => {
            "O Jev falhou nesta rodada ({reason}), e a rodada seguiu sem ele, pelas regras nativas. {remedy}"
        }
        ("round.jev_failed", Locale::EnUs) => {
            "Jev failed in this round ({reason}), and the round went on without it, by the native rules. {remedy}"
        }
        ("round.jev_remedy_key", Locale::PtBr) => "O serviço recusou a chave: configure uma chave válida.",
        ("round.jev_remedy_key", Locale::EnUs) => "The service refused the key: configure a valid key.",
        ("round.jev_remedy_missing_key", Locale::PtBr) => "O projeto não tem chave do Jev: configure uma para a rodada perguntar a ele.",
        ("round.jev_remedy_missing_key", Locale::EnUs) => "The project has no Jev key: configure one for the round to ask it.",
        ("round.jev_remedy_key_in_git", Locale::PtBr) => {
            "A chave do Jev não vale: o git guarda o arquivo dela ou não o ignora. Tire o arquivo do git e troque a chave."
        }
        ("round.jev_remedy_key_in_git", Locale::EnUs) => {
            "The Jev key does not count: git tracks its file or does not ignore it. Take the file out of git and change the key."
        }
        ("round.jev_remedy_credit", Locale::PtBr) => {
            "A conta do serviço está sem crédito: ponha crédito nela; a chave vale, e trocá-la não resolve."
        }
        ("round.jev_remedy_credit", Locale::EnUs) => {
            "The service account has no credit: add credit to it; the key is valid, and changing it does not help."
        }
        ("round.jev_remedy_size", Locale::PtBr) => {
            "A pergunta passou do teto de tamanho e nem saiu; a chave e o crédito não estão em causa."
        }
        ("round.jev_remedy_size", Locale::EnUs) => {
            "The question went over the size cap and was never sent; the key and the credit are not at fault."
        }
        ("round.jev_remedy_wait", Locale::PtBr) => {
            "O serviço não deu resposta útil desta vez: lento, ocupado, fora de alcance ou com erro. A rodada \
             seguinte pergunta de novo. A chave e o crédito não estão em causa."
        }
        ("round.jev_remedy_wait", Locale::EnUs) => {
            "The service gave no usable answer this time: slow, busy, unreachable or failing. The next round \
             asks again. The key and the credit are not at fault."
        }
        ("round.task_without_covers", Locale::PtBr) => {
            "As tarefas {tasks} do backlog não cobrem item nenhum (`covers`). A onda leva como critérios os \
             itens que as tarefas dela cobrem. Por isso elas ficam sem onda, e as que dependem delas esperam. \
             Grave uma versão de cada uma, com `replaces` e os itens em `covers`, para entrarem na montagem."
        }
        ("round.task_without_covers", Locale::EnUs) => {
            "Backlog tasks {tasks} cover no item (`covers`). A wave carries the items its tasks cover as its \
             criteria. So they stay out of the waves, and the tasks that depend on them wait. Record a version \
             of each one, with `replaces` and the items in `covers`, so they enter the assembly."
        }
        // O aviso da rodada e do fechamento: os agentes do projeto estavam
        // com outro texto e voltaram ao deste programa, ou não puderam voltar.
        ("round.agents_refreshed", Locale::PtBr) => {
            "Os agentes {files} estavam com um texto diferente do deste Mustard e foram regravados. \
             O Claude Code leva alguns segundos para reler o arquivo."
        }
        ("round.agents_refreshed", Locale::EnUs) => {
            "The agents {files} had a text different from this Mustard's and were rewritten. \
             Claude Code takes a few seconds to read the file again."
        }
        ("round.agents_not_refreshed", Locale::PtBr) => {
            "Os agentes em .claude/agents/mustard/ não puderam ser conferidos nem regravados: {detail}. \
             O agente abre com o texto que estiver lá, que pode ser de outra versão. \
             Rode `mustard-rt run upsert` para regravá-los."
        }
        ("round.agents_not_refreshed", Locale::EnUs) => {
            "The agents in .claude/agents/mustard/ could not be checked or rewritten: {detail}. \
             The agent opens with the text that is there, which may be from another version. \
             Run `mustard-rt run upsert` to rewrite them."
        }
        ("round.fix_push", Locale::PtBr) => {
            "A onda de conserto está entregue e comitada na branch da obra, que continua fechada: \
             empurre o conserto para o servidor com `{command}`."
        }
        ("round.fix_push", Locale::EnUs) => {
            "The fix wave is delivered and committed on the work's branch, which stays closed: \
             push the repair to the server with `{command}`."
        }
        ("round.fix_reason", Locale::PtBr) => "o servidor reprovou os testes do pull request",
        ("round.fix_reason", Locale::EnUs) => "the server failed the pull request's tests",
        ("round.missing", Locale::PtBr) => {
            "Nada a despachar nem a revisar, e a onda {wave} ainda não está entregue e aprovada. \
             Mostre ao usuário o que a segura antes de fechar."
        }
        ("round.missing", Locale::EnUs) => {
            "Nothing to dispatch or review, and wave {wave} is not delivered and approved yet. \
             Show the user what holds it before closing."
        }
        ("round.fix_limit", Locale::PtBr) => {
            "A onda {wave} foi reprovada {count} vezes seguidas, e o limite é de {max} rodadas de \
             conserto. A rodada não a manda de novo, nem as ondas que dependem dela, e o resto \
             segue. Mostre ao usuário os vereditos {verdicts} e faça a pergunta desta onda em \
             `stopped`. As saídas são duas. Uma é revisar o plano dela, e a versão nova zera a \
             conta. A outra é tirá-la do plano, com a onda e as tarefas dela, e os vereditos dela \
             deixam de contar."
        }
        ("round.fix_limit", Locale::EnUs) => {
            "Wave {wave} was rejected {count} times in a row, and the limit is {max} fix rounds. \
             The round does not send it again, nor the waves that depend on it, and the rest goes \
             on. Show the user the verdicts {verdicts} and ask this wave's question in `stopped`. \
             There are two ways out. One is to revise its plan, and the new version resets the \
             count. The other is to take it out of the plan, with its wave and tasks, and its \
             verdicts stop counting."
        }
        ("round.fix_limit.question", Locale::PtBr) => {
            "A onda {wave} foi reprovada de novo depois de {max} rodadas de conserto. Revisar o plano dela ou tirá-la do plano?"
        }
        ("round.fix_limit.question", Locale::EnUs) => {
            "Wave {wave} was rejected again after {max} fix rounds. Revise its plan or take it out of the plan?"
        }
        // A retomada: a onda pausada ou órfã sai de novo com o pedido
        // anterior, mais os passos gravados e o aviso de ver o que mudou na
        // cópia; e a onda viva sem sinal por 40 minutos vira aviso.
        ("round.resume.steps", Locale::PtBr) => "Passos já gravados desta onda:",
        ("round.resume.steps", Locale::EnUs) => "Steps already recorded for this wave:",
        // O reenvio da onda que parou no meio de uma tarefa que não é a
        // primeira: o bloco dela vem logo depois desta frase.
        ("round.resume.task", Locale::PtBr) => {
            "A tarefa em curso, que continua daqui, vem inteira abaixo. As seguintes chegam na resposta \
             do passo, como antes."
        }
        ("round.resume.task", Locale::EnUs) => {
            "The task in progress, which goes on from here, comes whole below. The next ones arrive in \
             the answer to the step, as before."
        }
        ("round.resume.notice", Locale::PtBr) => "Comece vendo o que mudou na cópia.",
        ("round.resume.notice", Locale::EnUs) => "Start by seeing what changed in the copy.",
        // O reenvio da onda órfã cujo agente deixou código na cópia que a
        // rodada zerou: as refs que o guardam vêm logo depois desta frase,
        // uma por linha.
        ("round.resume.kept", Locale::PtBr) => {
            "O agente anterior desta onda parou no meio. O que ele deixou na cópia ficou guardado nas \
             refs abaixo, a mais nova primeiro, cada uma com os arquivos que traz. Confira cada ref com \
             `git show --stat <ref>`. Traga de volta o que serve com `git cherry-pick --no-commit <ref>`, \
             na cópia ou na pasta que a linha diz, e continue dali. O resto só se descarta com o motivo \
             gravado num passo."
        }
        ("round.resume.kept", Locale::EnUs) => {
            "The previous agent of this wave stopped midway. What it left in the copy is kept under the \
             refs below, newest first, each with the files it carries. Check each ref with \
             `git show --stat <ref>`. Bring back what still serves with `git cherry-pick --no-commit <ref>`, \
             in the copy or in the folder the line names, and carry on from there. The rest is only \
             discarded with the reason recorded in a step."
        }
        ("round.resume.kept_in", Locale::PtBr) => "- `{ref}`, na pasta `{folder}` da cópia: {files}",
        ("round.resume.kept_in", Locale::EnUs) => "- `{ref}`, in the copy folder `{folder}`: {files}",
        ("round.copies_full", Locale::PtBr) => {
            "As {copies} cópias da obra estão ocupadas, e as ondas {waves} já voltaram e guardam a delas até o commit: \
             o despacho espera. Revise e reporte essas ondas para liberar a cópia."
        }
        ("round.copies_full", Locale::EnUs) => {
            "All {copies} copies of the work are taken, and waves {waves} already came back and keep theirs until the commit: \
             dispatch waits. Review and report those waves to free the copy."
        }
        // A onda cujo pedido passava do teto de tokens saiu com as tarefas
        // que cabem; as outras voltaram ao backlog.
        ("round.request_split", Locale::PtBr) => {
            "O pedido da onda {wave} passava do teto de {cap} tokens. Ela saiu com {kept} das {total} \
             tarefas, e {tasks} voltaram ao backlog para a rodada seguinte."
        }
        ("round.request_split", Locale::EnUs) => {
            "Wave {wave}'s request was above the {cap} token cap. It went out with {kept} of its {total} \
             tasks, and {tasks} went back to the backlog for the next round."
        }
        ("round.resume.silent", Locale::PtBr) => {
            "O agente da onda {wave} começou e está sem sinal de vida há mais de 40 minutos. Pare-o \
             e mande `<PAUSED>{\"wave\":{wave}}</PAUSED>` na próxima rodada, para reenviar a onda."
        }
        ("round.resume.silent", Locale::EnUs) => {
            "Wave {wave}'s agent started and has had no sign of life for more than 40 minutes. Stop \
             it and send `<PAUSED>{\"wave\":{wave}}</PAUSED>` on the next round, to resend the wave."
        }
        ("plan.finding.label", Locale::PtBr) => "achado do plano",
        ("plan.finding.label", Locale::EnUs) => "plan finding",
        ("approve_spec.open_points", Locale::PtBr) => "Pontos do levantamento ainda abertos ({count}): {points}",
        ("approve_spec.open_points", Locale::EnUs) => "Survey points still open ({count}): {points}",
        ("open.choose_kind", Locale::PtBr) => "Falta o tipo. Pergunte ao usuário o tipo da branch, como feature ou fix. Nada foi criado.",
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
             espere a resposta. O objetivo da spec é uma frase que diz o que ele pediu: a dele, ou a \
             que você sugeriu e ele aprovou. Grave-o como o primeiro `context`, com `origin` na \
             mensagem em que ele fez o pedido. O card, os critérios de aceite e os documentos antigos \
             que vierem junto vão logo depois, como `context`, com o mesmo `origin`."
        }
        // Só quando a abertura trouxe as mensagens de antes para a spec.
        ("open.next_goal.earlier", Locale::PtBr) => {
            "As mensagens que o usuário digitou antes da abertura já estão na spec, como mensagens \
             dele, com os números em `earlier_messages`. O `origin` do objetivo pode ser uma delas."
        }
        ("open.next_goal.earlier", Locale::EnUs) => {
            "The messages the user typed before the opening are already in the spec, as their \
             messages, with the numbers in `earlier_messages`. The goal's `origin` can be one of them."
        }
        ("open.earlier_messages_missing", Locale::PtBr) => {
            "A conversa desta sessão não foi achada neste projeto, e as mensagens que o usuário digitou \
             antes da abertura ficaram fora da spec. O `origin` do objetivo vai na resposta dele à pergunta."
        }
        ("open.earlier_messages_missing", Locale::EnUs) => {
            "This session's conversation was not found in this project, and the messages the user typed \
             before the opening stayed out of the spec. The goal's `origin` goes on their answer to the question."
        }
        ("open.next_goal", Locale::EnUs) => {
            "Spec {spec} was born on branch {branch}. Ask the user the question in `question` and \
             wait for the answer. The spec's goal is one sentence saying what they asked for: theirs, \
             or the one you suggested and they approved. Record it as the first `context`, with \
             `origin` on the message where they made the request. The card, the acceptance criteria \
             and the old documents that come along go right after it, as `context`, with the same \
             `origin`."
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
        ("open.branch_taken", Locale::PtBr) => "A branch {branch} já existe. Escolha outro nome, ou retome a spec dela. Nada foi criado.",
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
        ("open.git_failed", Locale::PtBr) => "O git recusou criar a branch {branch}: {detail}. Nada foi gravado na spec.",
        ("open.git_failed", Locale::EnUs) => "Git refused to create branch {branch}: {detail}. Nothing was written to the spec.",
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
        ("open.pending_project.one", Locale::PtBr) => "1 pendência do projeto esperando. {list} Quer resolver alguma nesta obra?",
        ("open.pending_project.one", Locale::EnUs) => "1 project pending item waiting. {list} Do you want to work on any of them in this unit?",
        ("open.pending_project.many", Locale::PtBr) => "{count} pendências do projeto esperando. {list} Quer resolver alguma nesta obra?",
        ("open.pending_project.many", Locale::EnUs) => {
            "{count} project pending items waiting. {list} Do you want to work on any of them in this unit?"
        }
        // O trecho de onde a lista inteira está: com o endereço da página do
        // projeto, quando ele já foi gravado, ou o comando que a mostra,
        // senão. `{url}` vem de quem chama.
        ("open.pending_project.list_page", Locale::PtBr) => "A lista inteira está na página do projeto: {url}.",
        ("open.pending_project.list_page", Locale::EnUs) => "The whole list is on the project page: {url}.",
        ("open.pending_project.list_command", Locale::PtBr) => "As abertas saem com `mustard-rt run pending`.",
        ("open.pending_project.list_command", Locale::EnUs) => "The open ones come from `mustard-rt run pending`.",
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
             passaram. A ordem do fluxo roda os critérios antes da integração, e integrar agora \
             integra trabalho que ninguém conferiu."
        }
        ("pr.qa_pending", Locale::EnUs) => {
            "Not every criterion of `{spec}` has a passing run: {passed} of {criteria} passed. The \
             flow runs the criteria before integration, and integrating now integrates work \
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
        ("pr.submodules.stuck", Locale::EnUs) => "The main pull request #{pr} stays a draft: {reason}. The next check tries again.",
        ("pr.pointer_commit", Locale::PtBr) => "chore(submódulo): atualiza o ponteiro",
        ("pr.pointer_commit", Locale::EnUs) => "chore(submodule): update the pointer",
        // O corpo do pull request aberto que ficou como estava, porque alguém
        // o editou depois do último corpo gerado ou o provedor não deixou lê-lo.
        ("pr.body_kept", Locale::PtBr) => {
            "O corpo do pull request #{pr} ficou como estava ({reason}). O Mustard só refaz o corpo \
             que segue igual ao último que ele gerou, e a edição de quem mexeu nele fica. Para o \
             Mustard voltar a refazê-lo, apague a descrição no provedor."
        }
        ("pr.body_kept", Locale::EnUs) => {
            "The body of pull request #{pr} stayed as it was ({reason}). Mustard only rewrites a body \
             still equal to the last one it generated, and the edit of whoever changed it stays. For \
             Mustard to rewrite it again, clear the description on the provider."
        }
        // O corpo do pull request aberto que ficou como estava porque não traz
        // a marca do corpo gerado: é de antes dela ou foi escrito à mão.
        ("pr.body_unmarked", Locale::PtBr) => {
            "O corpo do pull request #{pr} ficou como estava ({reason}). Ele não traz a marca que o \
             Mustard põe no fim do corpo que gera. É de antes dela ou foi escrito à mão. Sem essa \
             marca, não há como saber se alguém o editou. Para o Mustard voltar a refazê-lo, apague \
             a descrição no provedor."
        }
        ("pr.body_unmarked", Locale::EnUs) => {
            "The body of pull request #{pr} stayed as it was ({reason}). It lacks the mark Mustard \
             puts at the end of the body it generates. It predates that mark or was written by hand. \
             Without the mark, there is no telling whether someone edited it. For Mustard to rewrite \
             it again, clear the description on the provider."
        }
        // A esteira do pull request lida depois de abri-lo e na retomada da
        // spec com ele aberto, uma frase por resposta do provedor.
        ("pr.checks.passed", Locale::PtBr) => {
            "A esteira do pull request #{pr} passou: as verificações do provedor terminaram sem falha."
        }
        ("pr.checks.passed", Locale::EnUs) => {
            "The pipeline of pull request #{pr} passed: the provider's checks finished without a failure."
        }
        ("pr.checks.running", Locale::PtBr) => {
            "A esteira do pull request #{pr} ainda está rodando. A retomada da spec lê de novo, e o \
             merge espera o resultado."
        }
        ("pr.checks.running", Locale::EnUs) => {
            "The pipeline of pull request #{pr} is still running. Resuming the spec reads it again, \
             and the merge waits for the result."
        }
        ("pr.checks.failed", Locale::PtBr) => {
            "A esteira do pull request #{pr} reprovou: uma verificação do provedor terminou em falha."
        }
        ("pr.checks.failed", Locale::EnUs) => {
            "The pipeline of pull request #{pr} failed: a check of the provider finished in failure."
        }
        ("pr.checks.fix", Locale::PtBr) => {
            "Leia no provedor o que falhou e rode `mustard-rt run reopen --spec {spec} --fix --reason \
             <o que o servidor informou>`: a porta de conserto abre a onda de correção na mesma branch."
        }
        ("pr.checks.fix", Locale::EnUs) => {
            "Read on the provider what failed and run `mustard-rt run reopen --spec {spec} --fix \
             --reason <what the server reported>`: the repair door opens the fix wave on the same \
             branch."
        }
        ("pr.checks.absent", Locale::PtBr) => {
            "O pull request #{pr} ainda não tem verificação nenhuma do provedor: a esteira não \
             começou, ou o projeto não tem uma."
        }
        ("pr.checks.absent", Locale::EnUs) => {
            "Pull request #{pr} has no check from the provider yet: the pipeline has not started, or \
             the project has none."
        }
        ("pr.checks.unreadable", Locale::PtBr) => {
            "A esteira do pull request #{pr} não pôde ser lida ({reason}). O merge pergunta antes de \
             juntar sem ela."
        }
        ("pr.checks.unreadable", Locale::EnUs) => {
            "The pipeline of pull request #{pr} could not be read ({reason}). The merge asks before \
             merging without it."
        }
        // A recusa do merge de uma spec que voltou à execução depois do
        // fechamento: o pull request dela leva a versão sem o ajuste até a
        // spec fechar de novo.
        ("pr.merge_reopened", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}: ela foi reaberta e ainda não fechou de novo. O \
             pull request #{pr} leva a versão sem o ajuste. Nada foi juntado, e o provedor nem foi \
             perguntado. Termine o ajuste e feche a spec pela rodada (`mustard-rt run round --dispatch --spec \
             {spec}`). Depois do fechamento, o `pr-open` que ele aponta atualiza o mesmo pull \
             request, e o merge segue."
        }
        ("pr.merge_reopened", Locale::EnUs) => {
            "The spec {spec} is in the {phase} phase: it was reopened and has not closed again. \
             Pull request #{pr} carries the version without the change. Nothing was merged, and \
             the provider was not even asked. Finish the change and close the spec through the \
             round (`mustard-rt run round --dispatch --spec {spec}`). After the close, the `pr-open` it points \
             to updates the same pull request, and the merge goes on."
        }
        // A recusa do merge de uma spec que ainda não fechou e nunca voltou do
        // fechamento: a fase dela e o passo que falta, sem falar em reabertura.
        ("pr.merge_not_closed", Locale::PtBr) => {
            "A spec {spec} está na fase {phase} e ainda não fechou, e o pull request #{pr} só entra na \
             base depois do fechamento dela. Nada foi juntado, e o provedor nem foi perguntado. O passo \
             que falta agora é `{command}`; dele o fluxo segue até o fechamento pela rodada, e o merge \
             vem depois."
        }
        ("pr.merge_not_closed", Locale::EnUs) => {
            "The spec {spec} is in the {phase} phase and has not closed yet, and pull request #{pr} \
             only goes into the base after it closes. Nothing was merged, and the provider was not \
             even asked. The step missing now is `{command}`; from it the flow goes on to the close \
             through the round, and the merge comes after."
        }
        // A recusa do merge de uma spec entregue ou descartada: ela saiu do
        // fluxo, e nenhum passo dela leva o pull request à base.
        ("pr.merge_settled", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}: ela já saiu do fluxo, entregue na base ou descartada. \
             Nenhum passo dela leva o pull request #{pr} à base. Nada foi juntado, e o provedor nem \
             foi perguntado. Pedido novo sobre ela é obra nova, pelo `mustard-rt run open`."
        }
        ("pr.merge_settled", Locale::EnUs) => {
            "The spec {spec} is in the {phase} phase: it has already left the flow, delivered to the \
             base or discarded. No step of it takes pull request #{pr} into the base. Nothing was \
             merged, and the provider was not even asked. A new request about it is new work, through \
             `mustard-rt run open`."
        }
        // As dicas e os avisos do merge do Mustard e da lista de pull requests
        // abertos: cada um sai no idioma do texto do projeto.
        ("pr.list_from_unit", Locale::PtBr) => {
            "A lista de pull requests abertos se pede de uma base, não de dentro de uma unidade. Vá \
             para `{base}` (`git checkout {base}`) e rode `mustard-rt run pr-review` de novo."
        }
        ("pr.list_from_unit", Locale::EnUs) => {
            "The list of open pull requests is asked from a base, not from inside a unit. Switch to \
             `{base}` (`git checkout {base}`) and run `mustard-rt run pr-review` again."
        }
        ("pr.list_from_unit_no_base", Locale::PtBr) => {
            "A lista de pull requests abertos se pede de uma base, não de dentro de uma unidade. Vá \
             para a branch em que esta unidade entra e rode `mustard-rt run pr-review` de novo."
        }
        ("pr.list_from_unit_no_base", Locale::EnUs) => {
            "The list of open pull requests is asked from a base, not from inside a unit. Switch to \
             the branch this unit integrates into and run `mustard-rt run pr-review` again."
        }
        ("pr.confirm_running", Locale::PtBr) => {
            "As verificações do próprio provedor para `{unit}` ainda estão rodando. Nada foi juntado, \
             então o resultado delas ainda tem o que barrar."
        }
        ("pr.confirm_running", Locale::EnUs) => {
            "The provider's own checks for `{unit}` are still running. Nothing was merged, so their \
             verdict still has something to stop."
        }
        ("pr.confirm_failed", Locale::PtBr) => {
            "As verificações do próprio provedor para `{unit}` não voltaram verdes (falharam ou foram \
             canceladas). Nada foi juntado."
        }
        ("pr.confirm_failed", Locale::EnUs) => {
            "The provider's own checks for `{unit}` did not come back green (failed or cancelled). \
             Nothing was merged."
        }
        ("pr.confirm_unreadable", Locale::PtBr) => {
            "As verificações do próprio provedor para `{unit}` não puderam ser lidas ({checks}). Nada \
             foi juntado."
        }
        ("pr.confirm_unreadable", Locale::EnUs) => "The provider's own checks for `{unit}` could not be read ({checks}). Nothing was merged.",
        ("pr.confirm_no_verdict", Locale::PtBr) => "`{unit}` não tem veredito de revisão gravado. Nada foi juntado.",
        ("pr.confirm_no_verdict", Locale::EnUs) => "`{unit}` carries no recorded review verdict. Nothing was merged.",
        ("pr.confirm_verdict", Locale::PtBr) => "A última revisão de `{unit}` voltou `{verdict}`. Nada foi juntado.",
        ("pr.confirm_verdict", Locale::EnUs) => "The last review of `{unit}` came back `{verdict}`. Nothing was merged.",
        ("pr.merge_wait_checks", Locale::PtBr) => {
            "Espere as verificações terminarem e rode `pr merge` de novo, ou rode de novo com \
             `--confirm` para juntar sem esperar."
        }
        ("pr.merge_wait_checks", Locale::EnUs) => {
            "Wait for the checks to finish and run `pr merge` again, or re-run with `--confirm` to \
             merge without waiting."
        }
        ("pr.merge_checks_failed", Locale::PtBr) => {
            "Rode `mustard-rt run reopen --spec {unit} --fix --reason <o que o servidor informou>`. A \
             porta de conserto abre a onda de correção dentro da spec fechada, comita na mesma branch \
             e envia, sem reabrir a obra. Ou rode de novo com `--confirm` para juntar assim mesmo."
        }
        ("pr.merge_checks_failed", Locale::EnUs) => {
            "Run `mustard-rt run reopen --spec {unit} --fix --reason <what the server reported>`. The \
             repair door opens the fix wave inside the closed spec, commits on the same branch and \
             pushes, without reopening the work. Or re-run with `--confirm` to merge anyway."
        }
        ("pr.merge_checks_unreadable", Locale::PtBr) => {
            "O provedor não respondeu. Confira se a ferramenta dele está instalada e autenticada e rode \
             `pr merge` de novo. Com `--confirm`, o merge sai sem a resposta dele."
        }
        ("pr.merge_checks_unreadable", Locale::EnUs) => {
            "The provider did not answer. Check that its tooling is installed and authenticated, then \
             run `pr merge` again. `--confirm` merges without it."
        }
        ("pr.merge_no_verdict", Locale::PtBr) => {
            "Pergunte ao operador e rode de novo com `--confirm` para juntar assim mesmo. O veredito \
             lido aqui é o que `mustard-rt run round` grava a cada onda."
        }
        ("pr.merge_no_verdict", Locale::EnUs) => {
            "Ask the operator, then re-run with `--confirm` to merge anyway. The verdict read here is \
             the one `mustard-rt run round` records for each wave."
        }
        ("pr.merge_provider_refused", Locale::PtBr) => {
            "Nada foi podado, e a unidade segue como estava. Resolva a recusa (conflitos, rascunho, \
             verificações obrigatórias) e rode `pr merge` de novo."
        }
        ("pr.merge_provider_refused", Locale::EnUs) => {
            "Nothing was pruned, and the unit is untouched. Resolve the refusal (conflicts, draft \
             state, required checks) and run `pr merge` again."
        }
        ("pr.promotion_merged", Locale::PtBr) => {
            "`{head}` é uma base, não uma unidade: a promoção termina no merge e não há poda a fazer. \
             Atualize as bases locais com `git fetch origin <base>:<base>`. Nenhuma branch foi \
             apagada."
        }
        ("pr.promotion_merged", Locale::EnUs) => {
            "`{head}` is a base, not a unit: the promotion ends at the merge and there is nothing to \
             prune. Update the local bases with `git fetch origin <base>:<base>`. No branch was \
             deleted."
        }
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
        ("message.by_hand", Locale::PtBr) => "Testar à mão: {tasks}.",
        ("message.by_hand", Locale::EnUs) => "Test by hand: {tasks}.",
        ("reopen.reason_missing", Locale::PtBr) => {
            "A volta ao levantamento precisa do motivo: passe `--reason` com uma frase dizendo por \
             que a spec volta. O motivo fica gravado no evento da volta. Nada foi gravado."
        }
        ("reopen.reason_missing", Locale::EnUs) => {
            "Going back to the survey needs a reason: pass `--reason` with one sentence saying why \
             the spec goes back. The reason is written into the event. Nothing was written."
        }
        ("reopen.settled", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}. A spec entregue na base, com o merge feito, e a \
             descartada não voltam, porque o que elas decidiram já saiu. Abra uma spec nova com \
             `mustard-rt run open`. Nada foi gravado."
        }
        ("reopen.settled", Locale::EnUs) => {
            "The spec {spec} is in the phase {phase}. A spec delivered to the base, already merged, \
             and a discarded one do not come back, because what they decided is already out. Open \
             a new spec with `mustard-rt run open`. Nothing was written."
        }
        ("reopen.reopened", Locale::PtBr) => {
            "A spec {spec} voltou à execução, já aprovada, na mesma branch. O motivo ficou gravado, \
             e nada do que foi decidido é perguntado de novo. Grave o pedido novo com \
             `mustard-rt run write request --spec {spec}` e, dele, as tarefas novas. As ondas delas \
             saem pela rodada depois que o usuário aprovar a mudança."
        }
        ("reopen.reopened", Locale::EnUs) => {
            "The spec {spec} is back to running, already approved, on the same branch. The reason \
             is on the record, and nothing already decided is asked again. Write the new request \
             with `mustard-rt run write request --spec {spec}` and, from it, the new tasks. Their \
             waves go out through the round after the user approves the change."
        }
        // O fim do passo da reabertura, pelo pull request da spec: o mesmo,
        // como está no provedor, que só o merge do Mustard segura até o
        // fechamento, ou o fechado sem merge, que dá lugar a outro.
        ("reopen.same_pr", Locale::PtBr) => {
            "Depois vem o fechamento de novo, e o pull request continua o mesmo, como está no \
             provedor. Até lá, o merge pelo Mustard recusa; pelo botão do provedor, ele juntaria na \
             base a versão sem o ajuste."
        }
        ("reopen.same_pr", Locale::EnUs) => {
            "Then comes the close again, and the pull request stays the same, as it is on the \
             provider. Until then, Mustard's merge refuses; through the provider's button, it would \
             take the version without the change into the base."
        }
        ("reopen.pr_closed", Locale::PtBr) => {
            "O pull request #{pr} dela está fechado, sem merge. Depois vem o fechamento de novo, e o \
             `pr-open` que ele aponta abre outro pull request, na mesma branch."
        }
        ("reopen.pr_closed", Locale::EnUs) => {
            "Its pull request #{pr} is closed, not merged. Then comes the close again, and the \
             `pr-open` it points to opens another pull request, on the same branch."
        }
        ("reopen.fix_not_red", Locale::PtBr) => {
            "A porta de conserto da spec {spec} não abriu. Ela pede a spec com o pull request \
             aberto, na branch dela, e o vermelho do servidor relatado pelo provedor. A spec está \
             na fase {phase} sem isso. Para um pedido novo, rode o `mustard-rt run reopen` sem \
             `--fix`. Nada foi gravado."
        }
        ("reopen.fix_not_red", Locale::EnUs) => {
            "The fix door of the spec {spec} did not open. It needs the spec with its pull request \
             open, on its branch, and the server's red reported by the provider. The spec is in \
             the phase {phase} without that. For a new request, run `mustard-rt run reopen` \
             without `--fix`. Nothing was written."
        }
        // O aviso da reabertura de uma spec com o pull request aberto: o merge
        // que não deu para conferir.
        ("reopen.merge_unchecked", Locale::PtBr) => {
            "Não deu para conferir no provedor se o pull request da spec {spec} já entrou na base \
             ({reason}). A spec voltou à execução mesmo assim. Se alguém já fez o merge, o ajuste \
             novo iria para uma branch já juntada: confira no provedor antes de seguir."
        }
        ("reopen.merge_unchecked", Locale::EnUs) => {
            "The provider could not be asked whether the pull request of the spec {spec} is already \
             merged ({reason}). The spec went back to running anyway. If someone already merged it, \
             the new change would go to a branch already merged: check on the provider before \
             going on."
        }
        // O levantamento lembra o tipo de trabalho gravado: o comando sai sem
        // o tipo e roda como veio.
        ("reopen.next", Locale::PtBr) => {
            "A spec {spec} voltou ao levantamento, e o motivo ficou gravado. Rode `mustard-rt run \
             grill --spec {spec}` para montar os pontos: os novos convivem com o que já foi \
             decidido, e nada do que estava gravado foi apagado."
        }
        ("reopen.next", Locale::EnUs) => {
            "The spec {spec} is back in the survey, and the reason is on the record. Run \
             `mustard-rt run grill --spec {spec}` to build the points: the new ones live alongside \
             what was already decided, and nothing written was erased."
        }
        ("reopen.already", Locale::PtBr) => {
            "A spec {spec} já está em levantamento, e nada foi gravado. Rode `mustard-rt run grill \
             --spec {spec}` para montar os pontos."
        }
        ("reopen.already", Locale::EnUs) => {
            "The spec {spec} is already under survey, and nothing was written. Run `mustard-rt run \
             grill --spec {spec}` to build the points."
        }
        ("reopen.fix_opened", Locale::PtBr) => {
            "O servidor reprovou os testes do pull request {pr}, e a onda de conserto {wave} está \
             aberta na spec {spec}, que continua fechada. Rode `mustard-rt run round --dispatch --spec \
             {spec}`: a onda de conserto sai, e o conserto é comitado na mesma branch. Depois \
             rode este mesmo passo de novo para empurrar."
        }
        ("reopen.fix_opened", Locale::EnUs) => {
            "The server failed the tests of pull request {pr}, and the fix wave {wave} is open in \
             the spec {spec}, which stays closed. Run `mustard-rt run round --dispatch --spec {spec}`: the \
             fix wave goes out, and the repair is committed on the same branch. Then run this \
             very step again to push it."
        }
        ("reopen.fix_waiting", Locale::PtBr) => {
            "A onda de conserto {wave} da spec {spec} já está aberta e ainda não entregou, e nada \
             foi gravado. Rode `mustard-rt run round --dispatch --spec {spec}` para levá-la até o commit; \
             depois rode este mesmo passo de novo para empurrar."
        }
        ("reopen.fix_waiting", Locale::EnUs) => {
            "The fix wave {wave} of the spec {spec} is already open and has not delivered yet, and \
             nothing was written. Run `mustard-rt run round --dispatch --spec {spec}` to take it to the \
             commit; then run this very step again to push it."
        }
        ("reopen.fix_pushed", Locale::PtBr) => {
            "O conserto da onda {wave} foi empurrado para a branch {branch}: o servidor roda os \
             testes do pull request {pr} de novo. A obra não foi reaberta e nenhuma spec nova \
             nasceu; o que foi consertado ficou gravado na spec {spec}."
        }
        ("reopen.fix_pushed", Locale::EnUs) => {
            "The repair of wave {wave} was pushed to the branch {branch}: the server runs the \
             tests of pull request {pr} again. The work was not reopened and no new spec was \
             born; what was repaired is on the record in the spec {spec}."
        }
        ("reopen.fix_push_failed", Locale::PtBr) => {
            "O git recusou empurrar a branch {branch}: {error}. O conserto continua comitado na \
             branch, e nada foi gravado; resolva o que o git disse e rode este passo de novo."
        }
        ("reopen.fix_push_failed", Locale::EnUs) => {
            "Git refused to push the branch {branch}: {error}. The repair is still committed on \
             the branch, and nothing was written; resolve what git said and run this step again."
        }
        ("reopen.fix_wave_text", Locale::PtBr) => {
            "Conserte o que o servidor reprovou no pull request {pr}: {reason}. A obra continua \
             fechada; o conserto sai na mesma branch."
        }
        ("reopen.fix_wave_text", Locale::EnUs) => {
            "Repair what the server failed on pull request {pr}: {reason}. The work stays closed; \
             the repair goes out on the same branch."
        }
        ("reopen.fix_wave_done", Locale::PtBr) => "O servidor roda os testes do pull request {pr} de novo e eles voltam verdes.",
        ("reopen.fix_wave_done", Locale::EnUs) => "The server runs the tests of pull request {pr} again and they come back green.",
        ("reopen.fix_note", Locale::PtBr) => {
            "A onda de conserto {wave} consertou o que o servidor reprovou no pull request {pr}, e \
             o commit dela foi empurrado para a branch {branch}."
        }
        ("reopen.fix_note", Locale::EnUs) => {
            "The fix wave {wave} repaired what the server failed on pull request {pr}, and its \
             commit was pushed to the branch {branch}."
        }
        // Gravações atualizam o estado local; exportar exige ação explícita.
        ("request.new_waves", Locale::PtBr) => {
            concat!(
                "Pedido gravado. Grave o que ele gerou, as tarefas novas, sem `wave`: elas entram no \
                 backlog, e o programa as junta em ondas na hora de despachar. Ache e leia o código pelo \
                 mapa antes de escrever cada tarefa: ",
                map_commands_pt!(),
                ". O painel local acompanha as gravações. Publicação externa só por `/mustard:pages` quando \
                 solicitada. A spec e a branch continuam as mesmas, e não há nova aprovação."
            )
        }
        ("request.new_waves", Locale::EnUs) => {
            concat!(
                "Request recorded. Record what it generated, the new tasks, without `wave`: they go into \
                 the backlog, and the program groups them into waves when it dispatches. Find and read \
                 the code through the map before writing each task: ",
                map_commands_en!(),
                ". The local panel follows the writes. External publication requires `/mustard:pages` when \
                 requested. The spec and the branch stay the same, and there is no new approval."
            )
        }
        ("request.adjust_waves", Locale::PtBr) => {
            concat!(
                "Pedido gravado. Grave o que ele gerou: as versões novas das tarefas que mudam, com \
                 `replaces`. Repita o `wave` da versão antiga quando ela já está numa onda. Ache e leia \
                 o código pelo mapa antes de reescrever cada tarefa: ",
                map_commands_pt!(),
                ". O painel local acompanha as gravações. Publicação externa só por `/mustard:pages` quando \
                 solicitada. A spec e a branch continuam as mesmas, e não há nova aprovação."
            )
        }
        ("request.adjust_waves", Locale::EnUs) => {
            concat!(
                "Request recorded. Record what it generated: the new versions of the tasks that change, \
                 with `replaces`. Repeat the `wave` of the old version when it is already in a wave. \
                 Find and read the code through the map before rewriting each task: ",
                map_commands_en!(),
                ". The local panel follows the writes. External publication requires `/mustard:pages` when \
                 requested. The spec and the branch stay the same, and there is no new approval."
            )
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
        crate::platform::i18n::tests::assert_part_unchanged(include_str!("flow.rs"), super::PREFIXES, 381, 0x0191_9f94_c087_1774);
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
            ("open.next_goal.earlier", &[][..]),
            ("open.earlier_messages_missing", &[][..]),
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
        for (key, slots, points_to) in [("retired.wait_round", &["{command}"][..], "mustard-rt run round")] {
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

    /// Os títulos e as instruções fixas do pedido de uma onda, e o que a
    /// conferência do plano acha, saem do catálogo nos dois idiomas, com as
    /// vagas que o montador preenche.
    #[test]
    fn i18n_translates_wave_prompt_keys() {
        for (key, slots) in [
            ("plan.not_ready", &["{count}"][..]),
            ("plan.next", &["{question}", "{option}"][..]),
            ("page.purge_pending", &["{codes}", "{spec}"][..]),
            ("plan.wave_loop", &["{waves}"][..]),
            ("plan.file_outside_git", &["{task}", "{path}"][..]),
            ("plan.item_without_task", &["{code}"][..]),
            ("plan.owner_missing", &["{type}"][..]),
            ("plan.contract_without_criterion", &["{code}"][..]),
            ("plan.task_without_file", &["{task}"][..]),
            ("plan.dependency_only_files.one", &["{task}", "{waits_for}", "{files}"][..]),
            ("plan.dependency_only_files.many", &["{task}", "{waits_for}", "{files}"][..]),
            ("plan.task_could_name_a_skill", &["{task}", "{skill}"][..]),
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
            ("resume.unbound", &[][..]),
            ("resume.wave", &["{n}"][..]),
            ("resume.next.survey", &[][..]),
            ("resume.next.plan", &["{question}", "{option}"][..]),
            ("resume.next.running", &[][..]),
            ("resume.next.closed", &[][..]),
            ("resume.next.pr_open", &[][..]),
            ("resume.next.delivered", &[][..]),
            ("resume.next.discarded", &[][..]),
            ("resume.close_refused", &["{reason}"][..]),
            ("resume.step.survey", &[][..]),
            ("resume.step.plan", &[][..]),
            ("resume.step.closed", &[][..]),
            ("resume.step.stuck_one", &["{waves}"][..]),
            ("resume.step.stuck_many", &["{waves}"][..]),
            ("resume.step.cycle", &["{waves}"][..]),
            ("resume.step.dispatch_one", &["{waves}"][..]),
            ("resume.step.dispatch_many", &["{waves}"][..]),
            ("resume.step.backlog", &[][..]),
            ("resume.step.wait_one", &["{waves}"][..]),
            ("resume.step.wait_many", &["{waves}"][..]),
            ("resume.step.missing", &["{wave}"][..]),
            ("resume.step.backlog_held", &[][..]),
            ("resume.step.review_open", &[][..]),
            ("resume.step.close", &[][..]),
            ("resume.step.close_again", &[][..]),
            ("resume.step.close_approved", &[][..]),
            ("close.not_running", &["{phase}"][..]),
            ("close.wave_without_commit", &["{wave}"][..]),
            ("close.wave_rejected", &["{wave}"][..]),
            ("close.request_not_delivered", &["{code}"][..]),
            ("close.backlog_not_empty", &["{tasks}"][..]),
            ("close.criterion_failed", &["{code}", "{output}"][..]),
            ("close.criterion_ran_no_test", &["{code}", "{command}", "{count}"][..]),
            ("close.criterion_missing_test", &["{code}", "{name}"][..]),
            ("close.build_failed", &["{command}", "{output}"][..]),
            ("close.lint_failed", &["{command}", "{output}"][..]),
            ("close.lint_from_the_base", &["{command}", "{files}", "{output}"][..]),
            ("close.suite_failed", &["{command}", "{output}"][..]),
            ("close.ran_without_local_files", &["{files}"][..]),
            ("close.cited_outside", &["{files}"][..]),
            ("close.cited_nothing", &[][..]),
            ("close.cited_unowned", &[][..]),
            ("close.server_command_not_declared", &["{key}"][..]),
            ("close.review_copy_dirty", &["{copy}", "{files}"][..]),
            ("close.review_copy_failed", &["{copy}", "{detail}"][..]),
            ("close.clean_copy_failed", &["{copy}", "{detail}"][..]),
            ("close.review_copy_kept", &["{copies}", "{detail}"][..]),
            ("close.code_not_kept", &["{copies}", "{detail}"][..]),
            ("close.build_output_unsafe", &["{folder}"][..]),
            ("close.build_output_not_ignored", &["{folder}"][..]),
            ("close.build_output_failed", &["{folder}", "{detail}"][..]),
            ("close.final_review", &["{spec}"][..]),
            ("close.review_request_renewed", &["{items}", "{send}"][..]),
            ("close.next", &["{command}"][..]),
            ("close.final_leftovers", &["{tasks}", "{command}"][..]),
            ("close.pending_destination", &["{id}", "{title}", "{spec}"][..]),
            ("close.unowned_item", &["{code}", "{title}"][..]),
            ("close.unowned_test", &["{name}", "{file}"][..]),
            ("round.bad_report", &["{detail}"][..]),
            ("round.line_field", &["{line}", "{field}"][..]),
            ("round.merge_conflict", &["{wave}", "{conflicts}", "{copy}", "{head}"][..]),
            ("round.copy_failed", &["{wave}", "{detail}"][..]),
            ("round.local_file_missing", &["{file}", "{copy}"][..]),
            ("round.local_file_tracked", &["{file}"][..]),
            ("pr.submodules.waiting", &["{pr}", "{paths}"][..]),
            ("pr.submodules.ready", &["{pr}", "{paths}"][..]),
            ("pr.submodules.stuck", &["{pr}", "{reason}"][..]),
            ("pr.pointer_commit", &[][..]),
            ("pr.merge_reopened", &["{spec}", "{phase}", "{pr}"][..]),
            ("pr.merge_not_closed", &["{spec}", "{phase}", "{pr}", "{command}"][..]),
            ("pr.merge_settled", &["{spec}", "{phase}", "{pr}"][..]),
            ("pr.list_from_unit", &["{base}"][..]),
            ("pr.list_from_unit_no_base", &[][..]),
            ("pr.confirm_running", &["{unit}"][..]),
            ("pr.confirm_failed", &["{unit}"][..]),
            ("pr.confirm_unreadable", &["{unit}", "{checks}"][..]),
            ("pr.confirm_no_verdict", &["{unit}"][..]),
            ("pr.confirm_verdict", &["{unit}", "{verdict}"][..]),
            ("pr.merge_wait_checks", &[][..]),
            ("pr.merge_checks_failed", &["{unit}"][..]),
            ("pr.merge_checks_unreadable", &[][..]),
            ("pr.merge_no_verdict", &[][..]),
            ("pr.merge_provider_refused", &[][..]),
            ("pr.promotion_merged", &["{head}"][..]),
            ("pr.body_kept", &["{pr}", "{reason}"][..]),
            ("pr.body_unmarked", &["{pr}", "{reason}"][..]),
            ("pr.checks.passed", &["{pr}"][..]),
            ("pr.checks.running", &["{pr}"][..]),
            ("pr.checks.failed", &["{pr}"][..]),
            ("pr.checks.fix", &["{spec}"][..]),
            ("pr.checks.absent", &["{pr}"][..]),
            ("pr.checks.unreadable", &["{pr}", "{reason}"][..]),
            ("reopen.same_pr", &[][..]),
            ("reopen.pr_closed", &["{pr}"][..]),
            ("stuck.found", &["{list}"][..]),
            ("stuck.reason.waiting_loop", &[][..]),
            ("stuck.reason.deleted_copy", &[][..]),
            ("stuck.reason.idle_copy", &[][..]),
            ("copies.left", &["{total}", "{list}"][..]),
            ("copies.branch_missing", &["{branch}"][..]),
            (
                "conversation_size.block",
                &["{spec}", "{phase}", "{delivered}", "{running}", "{returned}", "{stuck}", "{missing}", "{recorded}", "{command}", "{next}"][..],
            ),
            ("conversation_size.copy", &["{wave}", "{copy}"][..]),
            ("conversation_size.replan", &["{wave}"][..]),
            ("conversation_size.more", &["{count}"][..]),
            ("conversation_size.precompact", &["{block}"][..]),
            ("conversation_size.notice", &["{tokens}", "{line}"][..]),
            ("conversation_size.notice_live", &["{tokens}", "{count}", "{waves}", "{line}"][..]),
            ("resume.lost_by_clear", &["{waves}", "{paused}"][..]),
            ("conversation_size.wave_signal", &["{now}", "{baseline}", "{counted}", "{basis}", "{total_limit}"][..]),
            ("conversation_size.window_known", &["{model}", "{window}", "{share}"][..]),
            ("conversation_size.window_declared", &["{window}", "{share}"][..]),
            ("conversation_size.window_million", &[][..]),
            ("conversation_size.window_thousands", &["{n}"][..]),
            ("conversation_size.window_unknown", &[][..]),
            ("conversation_size.wave_continue", &["{now}", "{baseline}", "{counted}", "{total_limit}", "{largest}"][..]),
            ("conversation_size.wave_deliver", &["{now}", "{baseline}", "{counted}", "{total_limit}", "{largest}"][..]),
            ("conversation_size.tasks_finished", &[][..]),
            ("conversation_size.wave_locked", &["{build}"][..]),
            ("conversation_size.wave_locked_build", &["{command}"][..]),
            ("wave_prompt.summary.title", &[][..]),
            ("wave_prompt.summary.read", &["{code}", "{root}", "{spec}"][..]),
            ("wave_prompt.task_changed", &["{commits}", "{files}"][..]),
            ("round.file_unknown", &["{file}", "{wave}"][..]),
            ("round.commit_carried_more", &["{extra}", "{repo}", "{stash}", "{restore}"][..]),
            ("round.files_diverged", &["{wave}", "{changed}", "{declared}", "{missing}"][..]),
            ("round.usage_missing", &["{wave}"][..]),
            ("round.tests_failed", &["{command}", "{output}"][..]),
            ("round.lint_failed", &["{command}", "{output}"][..]),
            ("round.after_wave.limit", &["{waves}", "{max}"][..]),
            ("round.after_wave.question", &["{waves}", "{max}"][..]),
            ("round.after_wave.wave", &["{wave}", "{round}", "{max}"][..]),
            ("round.after_wave.wave_warnings", &["{wave}"][..]),
            ("round.after_wave.new_agent", &["{wave}", "{title}"][..]),
            ("round.after_wave.import", &["{file}", "{line}", "{target}", "{from}", "{to}", "{rule_from}", "{rule_to}", "{along}", "{total}"][..]),
            ("round.after_wave.weak", &["{file}", "{line}", "{target}", "{from}", "{to}", "{rule_from}", "{rule_to}", "{along}", "{total}"][..]),
            ("round.after_wave.cycle", &["{file}", "{line}", "{target}"][..]),
            ("round.after_wave.leftover", &["{file}", "{line}", "{name}", "{from}"][..]),
            ("round.after_wave.orphan", &["{file}", "{line}", "{name}"][..]),
            ("round.after_wave.unused", &["{file}", "{line}", "{name}"][..]),
            ("round.after_wave.unused_test", &["{file}", "{line}", "{name}"][..]),
            ("round.development_build_failed", &["{output}"][..]),
            ("round.proof_ran_no_test", &["{code}"][..]),
            ("round.proof_missing_test", &["{code}", "{name}"][..]),
            ("round.size.line", &["{wave}", "{added}", "{removed}", "{files}"][..]),
            ("round.not_approved", &["{phase}"][..]),
            ("round.closed", &["{spec}", "{phase}"][..]),
            ("round.finished", &["{spec}", "{phase}"][..]),
            ("round.commit_too_long", &["{part}", "{chars}", "{max}"][..]),
            ("round.commit_forbidden", &["{found}"][..]),
            ("round.commit_looks_like_sha", &["{found}"][..]),
            ("round.commit_text_removed", &["{wave}", "{removed}", "{title}"][..]),
            ("round.commit_neutral_one", &["{file}"][..]),
            ("round.commit_neutral_many", &["{count}"][..]),
            ("round.formatter_missing", &["{name}"][..]),
            ("round.replan", &["{wave}", "{yes}", "{no}", "{code}", "{tasks}"][..]),
            ("round.plan_refused.click", &[][..]),
            ("round.plan_refused.commit", &[][..]),
            ("round.no_tasks", &[][..]),
            ("round.replan_needs_undone", &["{wave}", "{tasks}"][..]),
            ("round.undone_not_in_wave", &["{wave}", "{code}", "{tasks}"][..]),
            ("round.started_work_undone", &["{wave}", "{tasks}"][..]),
            ("round.done_without_step", &["{wave}", "{tasks}"][..]),
            ("round.rejected", &["{wave}", "{reason}", "{title}"][..]),
            ("round.rejected_working", &["{wave}", "{reason}"][..]),
            ("round.rejected_fix", &["{wave}", "{reason}"][..]),
            ("round.rejected_without_return", &["{wave}"][..]),
            ("round.rejected_committed", &["{wave}", "{sha}"][..]),
            ("round.returned_change", &["{wave}", "{change}"][..]),
            ("round.unmet_joined", &["{wave}", "{code}", "{text}"][..]),
            ("round.unmet_covered", &["{wave}", "{code}", "{task}", "{text}"][..]),
            ("round.unmet_review_joined", &["{code}", "{text}"][..]),
            ("round.unmet_review_covered", &["{code}", "{task}", "{text}"][..]),
            ("round.unmet_judged", &["{chance}"][..]),
            ("round.leftover_joined", &["{wave}", "{title}", "{detail}"][..]),
            ("round.leftover_classified", &["{wave}", "{title}", "{detail}"][..]),
            ("round.leftover_priority", &["{wave}"][..]),
            ("round.leftover_review_joined", &["{title}", "{detail}"][..]),
            ("round.leftover_review_classified", &["{title}", "{detail}"][..]),
            ("round.leftover_review_priority", &[][..]),
            ("triage.same_work_open", &["{task}", "{title}"][..]),
            ("triage.same_work_covered", &["{task}", "{title}"][..]),
            ("round.jev_pending.removed_name", &["{wave}"][..]),
            ("round.jev_pending.no_change", &["{wave}"][..]),
            ("round.jev_pending.change_target", &["{wave}"][..]),
            ("round.jev_pending.unmet_items", &["{wave}"][..]),
            ("round.jev_pending.leftovers", &["{wave}"][..]),
            ("round.jev_pending.review_unmet", &[][..]),
            ("round.skill_pick_reserve", &["{wave}"][..]),
            ("round.items_jev_pending", &["{wave}"][..]),
            ("triage.request_new_spec", &[][..]),
            ("triage.request_pending", &["{pending}"][..]),
            ("triage.request_unsure", &[][..]),
            ("triage.request_reserve", &[][..]),
            ("triage.task_reserve", &[][..]),
            ("triage.reserve_pass", &["{command}"][..]),
            ("triage.held", &["{tasks}"][..]),
            ("triage.order_unanswered", &["{missing}", "{requests}"][..]),
            ("triage.wave_too_big", &["{objective}", "{tokens}"][..]),
            ("triage.joined_line", &["{code}", "{title}", "{text}"][..]),
            ("triage.removed_joined", &["{code}"][..]),
            ("round.leftover_repeated", &["{wave}", "{task}", "{detail}"][..]),
            ("round.leftover_judged", &["{chance}"][..]),
            ("round.leftover_marked", &["{wave}", "{title}"][..]),
            ("round.leftover_review_marked", &["{title}"][..]),
            ("round.leftover_local", &["{wave}", "{files}", "{title}", "{detail}"][..]),
            ("round.leftover_review_local", &["{files}", "{title}", "{detail}"][..]),
            ("round.wave_left_without_tasks", &["{wave}", "{send}"][..]),
            ("round.wave_left_after_return", &["{wave}", "{send}"][..]),
            ("round.queued.broken_dependency", &["{dependencies}"][..]),
            ("round.queued.following", &["{wave}"][..]),
            ("round.leftover_without_files", &[][..]),
            ("round.tasks_returned", &["{wave}", "{tasks}"][..]),
            ("round.proof_rerouted", &["{item}", "{criterion}"][..]),
            ("round.proof_kept", &["{item}", "{type}"][..]),
            ("round.held_return", &["{wave}", "{hint}"][..]),
            ("round.code_kept", &["{wave}", "{copy}", "{ref}"][..]),
            ("round.code_kept_slot", &["{copy}", "{ref}"][..]),
            ("round.copy_not_cleaned", &["{wave}", "{detail}"][..]),
            ("round.resend_moved", &["{wave}", "{copy}"][..]),
            ("round.resend_no_copy", &["{wave}"][..]),
            ("round.resend_gone", &["{wave}", "{copy}"][..]),
            ("round.resend_gone_no_copy", &["{wave}", "{copy}"][..]),
            ("round.git_refused", &["{detail}"][..]),
            ("round.project_hook_refused", &["{hook}", "{files}", "{detail}"][..]),
            ("round.project_hook_no_file", &[][..]),
            ("round.wrong_branch", &["{current}", "{branch}"][..]),
            ("round.invalid_path", &["{file}"][..]),
            ("round.next", &[][..]),
            ("round.next.copy_file", &["{path}"][..]),
            ("round.report", &[][..]),
            ("round.report.checks", &[][..]),
            ("round.review.hint", &["{wave}"][..]),
            ("round.review.required", &["{wave}", "{spec}"][..]),
            ("round.wave_review.required", &["{wave}", "{title}", "{read}"][..]),
            ("round.wave_review.title", &["{spec}", "{wave}"][..]),
            ("round.wave_review.request", &["{wave}", "{copy}", "{command}"][..]),
            ("round.wave_review.tasks", &[][..]),
            ("round.wave_review.covered", &[][..]),
            ("round.wave_review.rules", &[][..]),
            ("round.wave_review.skills", &[][..]),
            ("round.wave_review.proofs", &[][..]),
            ("round.wave_review.diff", &[][..]),
            ("round.wave_review.left_out", &["{files}"][..]),
            ("round.waiting", &["{waves}"][..]),
            ("round.close", &["{command}"][..]),
            ("round.review_open", &[][..]),
            ("round.missing", &["{wave}"][..]),
            ("round.backlog_left", &["{tasks}", "{command}"][..]),
            ("round.backlog_needs_planning", &["{tasks}"][..]),
            ("round.backlog_stuck", &["{tasks}"][..]),
            ("round.task_without_covers", &["{tasks}"][..]),
            ("round.jev_failed", &["{reason}", "{remedy}"][..]),
            ("round.assembly_by_files", &["{candidates}"][..]),
            ("round.terminal_inside_a_copy", &["{folder}"][..]),
            ("round.jev_remedy_key", &[][..]),
            ("round.jev_remedy_missing_key", &[][..]),
            ("round.jev_remedy_key_in_git", &[][..]),
            ("round.jev_remedy_credit", &[][..]),
            ("round.jev_remedy_size", &[][..]),
            ("round.jev_remedy_wait", &[][..]),
            ("round.agents_refreshed", &["{files}"][..]),
            ("round.agents_not_refreshed", &["{detail}"][..]),
            ("round.fix_limit", &["{wave}", "{count}", "{max}", "{verdicts}"][..]),
            ("round.fix_limit.question", &["{wave}", "{max}"][..]),
            ("round.resume.steps", &[][..]),
            ("round.resume.notice", &[][..]),
            ("round.resume.kept", &[][..]),
            ("round.resume.kept_in", &["{ref}", "{folder}", "{files}"][..]),
            ("round.resume.task", &[][..]),
            ("round.resume.silent", &["{wave}"][..]),
            ("round.copies_full", &["{copies}", "{waves}"][..]),
            ("round.request_split", &["{wave}", "{cap}", "{kept}", "{total}", "{tasks}"][..]),
            ("wave_prompt.token_cap", &["{wave}", "{tokens}", "{cap}"][..]),
            ("page.findings.heading", &[][..]),
            ("prompt.title", &["{spec}", "{n}"][..]),
            ("prompt.languages", &["{text}", "{code}"][..]),
            ("prompt.final.title", &["{spec}"][..]),
            ("prompt.final.fixed", &[][..]),
            ("prompt.read.final", &["{root}", "{spec}"][..]),
            ("prompt.read.wave", &["{root}", "{spec}", "{wave}"][..]),
            ("prompt.part.delivers", &[][..]),
            ("prompt.part.read", &[][..]),
            ("prompt.part.do", &[][..]),
            ("prompt.part.obey", &[][..]),
            ("prompt.part.return", &[][..]),
            ("prompt.part.work", &[][..]),
            ("prompt.step.read", &["{part}"][..]),
            ("prompt.step.tasks", &[][..]),
            ("prompt.step.first", &["{item}"][..]),
            ("prompt.step.attends", &["{item}"][..]),
            ("prompt.step.user_message", &[][..]),
            ("prompt.task.heading", &["{n}", "{total}", "{item}"][..]),
            ("prompt.task.skill", &["{name}", "{path}"][..]),
            ("prompt.task.skill_cut", &["{name}", "{path}"][..]),
            ("prompt.step.file", &["{files}"][..]),
            ("prompt.step.files", &["{files}"][..]),
            ("prompt.step.read_before", &["{hints}"][..]),
            ("prompt.step.deliver", &["{part}"][..]),
            ("prompt.obey.no_lessons", &[][..]),
            ("prompt.return.loose", &[][..]),
            ("prompt.return.step", &["{root}", "{spec}", "{wave}"][..]),
            ("prompt.return.delivered", &["{root}", "{spec}", "{wave}"][..]),
            ("prompt.copy.first", &["{copy}"][..]),
            ("prompt.dispatch.as_is", &[][..]),
            ("prompt.task.tested_by", &["{file}", "{tests}"][..]),
            ("prompt.part.waves", &[][..]),
            ("prompt.part.each_delivered", &[][..]),
            ("prompt.part.agreed", &[][..]),
            ("prompt.part.criteria", &[][..]),
            ("prompt.skill.stale", &[][..]),
            ("prompt.execution.copy", &["{copy}", "{root}"][..]),
            ("prompt.review.copy", &["{copy}", "{root}", "{commit}"][..]),
            ("prompt.review.cleanup", &["{copy}"][..]),
            ("prompt.execution.prepare", &["{command}"][..]),
            ("prompt.execution.prepare_new", &["{command}"][..]),
            ("prompt.execution.prepare_reused", &["{command}", "{files}"][..]),
            ("prompt.execution.prepare_same", &["{command}"][..]),
            ("prompt.execution.prepare_more", &["{n}", "{diff}"][..]),
            ("prompt.review.local_files", &["{files}", "{root}"][..]),
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
        let plugin = crate::manifest_dir::manifest_dir().join("../../plugin");
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
            let texts = hints.map(|key| (key, translate(key, lang))).into_iter().chain([("session map", crate::platform::seeds::session_map(lang))]);
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

    /// O próximo passo da rodada que despacha manda o agente ler o próprio
    /// pedido pelo comando da resposta, nos dois idiomas, em frases que
    /// passam na conferência de escrita.
    #[test]
    fn the_dispatch_step_sends_the_read_command_and_reads_clearly() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = translate("round.next", lang);
            assert!(text.contains("`read`"), "{lang:?}: {text}");
            let report = crate::domain::clarity::measure(text, &[], Some(lang));
            assert!(report.passed, "{lang:?}: {report:?}");
        }
    }

    /// O passo do fechamento que despacha o revisor final manda o agente ler o
    /// próprio pedido pelo comando do campo `review.read`, e não fala mais do
    /// pedido inteiro no campo `review.prompt`, nos dois idiomas. Diz também
    /// o que a segunda chamada faz: com o veredito aprovado e nada mudado, só
    /// confere o veredito e fecha a spec, e a máquina roda de novo só se algo
    /// mudou. A frase que manda o comando tem 13 palavras em português e 12 em
    /// inglês, e o passo inteiro passa na conferência de escrita, com a lacuna
    /// trocada por uma palavra.
    #[test]
    fn the_final_review_step_sends_the_read_command_and_reads_clearly() {
        for (lang, sentence_words, second_call, rerun) in [
            (Locale::PtBr, 13, "o comando só confere o veredito e fecha a spec.", "Se algo mudou, a máquina roda de novo."),
            (Locale::EnUs, 12, "the command only checks the verdict and closes the spec.", "If something changed, the machine runs again."),
        ] {
            let text = translate("close.final_review", lang).replace("{spec}", "teste");
            assert!(text.contains("review.read"), "{lang:?}: não manda o comando do campo: {text}");
            assert!(!text.contains("review.prompt"), "{lang:?}: ainda fala do pedido inteiro: {text}");
            assert!(text.contains(second_call), "{lang:?}: não diz o que a segunda chamada faz: {text}");
            assert!(text.contains(rerun), "{lang:?}: não diz que a máquina só roda de novo se algo mudou: {text}");
            let sentence =
                text.split(". ").find(|sentence| sentence.contains("review.read")).unwrap_or_else(|| panic!("{lang:?}: nenhuma frase cita o campo: {text}"));
            assert_eq!(sentence.split_whitespace().count(), sentence_words, "{lang:?}: {sentence}");
            let report = crate::domain::clarity::measure(&text, &[], Some(lang));
            assert!(report.passed, "{lang:?}: {report:?}");
        }
    }

    /// O texto do modo solo mandava ler o pedido "abaixo", que a resposta da
    /// rodada não traz mais, e nenhum código o usava: a chave saiu do
    /// catálogo nos dois idiomas, e o próximo passo que despacha fala do
    /// comando de leitura, não de um pedido dentro da resposta.
    #[test]
    fn the_solo_mode_text_left_the_catalog() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            assert_eq!(translate("round.next.solo", lang), "<missing-key>", "{lang:?}");
            let next = translate("round.next", lang);
            assert!(!next.contains("abaixo") && !next.contains("below"), "{lang:?}: {next}");
        }
    }

    /// A conferência depois da onda fala nos dois idiomas com frases que
    /// passam na conferência de escrita, com cada lacuna trocada por uma
    /// palavra.
    #[test]
    fn the_after_wave_check_texts_read_clearly() {
        let keys = [
            "round.after_wave",
            "round.after_wave.limit",
            "round.after_wave.question",
            "round.after_wave.warnings",
            "round.after_wave.wave",
            "round.after_wave.wave_warnings",
            "round.after_wave.import",
            "round.after_wave.weak",
            "round.after_wave.cycle",
            "round.after_wave.leftover",
            "round.after_wave.orphan",
            "round.after_wave.unused",
            "round.after_wave.unused_test",
        ];
        let words = [
            ("{waves}", "3"),
            ("{wave}", "3"),
            ("{max}", "2"),
            ("{round}", "1"),
            ("{file}", "src/order.service.ts"),
            ("{line}", "12"),
            ("{target}", "src/order.controller.ts"),
            ("{from}", "service"),
            ("{to}", "controller"),
            ("{rule_from}", "controller"),
            ("{rule_to}", "service"),
            ("{along}", "24"),
            ("{total}", "25"),
            ("{name}", "old_total"),
        ];
        for lang in [Locale::PtBr, Locale::EnUs] {
            for key in keys {
                let text = words.iter().fold(translate(key, lang).to_string(), |text, (slot, word)| text.replace(slot, word));
                assert!(!text.contains('{'), "{key} {lang:?}: {text}");
                let report = crate::domain::clarity::measure(&text, &[], Some(lang));
                assert!(report.passed, "{key} {lang:?}: {report:?}");
            }
        }
    }
}
