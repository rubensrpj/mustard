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
    "retired", "banner", "stuck", "conversation_size", "wave_prompt",
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
            "O plano tem {count} coisas a corrigir antes da pergunta de aprovação. Nada foi gravado."
        }
        ("plan.not_ready", Locale::EnUs) => {
            "The plan has {count} things to fix before the approval question. Nothing was written."
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
        ("plan.item_without_task", Locale::PtBr) => {
            "Nenhuma tarefa diz que cobre o item {code}. Se ele não vira código, diga por quê."
        }
        ("plan.item_without_task", Locale::EnUs) => {
            "No task says it covers item {code}. If it does not become code, say why."
        }
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
        ("plan.contract_without_criterion", Locale::PtBr) => {
            "Nenhum critério cita o contrato {code}: nada prova que ele foi cumprido."
        }
        ("plan.contract_without_criterion", Locale::EnUs) => {
            "No criterion cites contract {code}: nothing proves it was met."
        }
        ("plan.task_without_file", Locale::PtBr) => {
            "A tarefa {task} mexe em código e não diz em que arquivo. Nomeie o arquivo na tarefa. \
             A tarefa que não mexe em arquivo nenhum diz isso no texto dela."
        }
        ("plan.task_without_file", Locale::EnUs) => {
            "Task {task} changes code and does not say which file. Name the file in the task. \
             A task that changes no file says so in its own text."
        }
        ("plan.task_could_name_a_skill", Locale::PtBr) => {
            "A tarefa {task} não nomeia skill, e a skill {skill} serve para ela. Nomeie-a na tarefa."
        }
        ("plan.task_could_name_a_skill", Locale::EnUs) => {
            "Task {task} names no skill, and skill {skill} fits it. Name it in the task."
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
            "O plano está gravado: confira o plano e publique a página da spec. Depois, faça a \
             pergunta de aprovação na ordem de explicar do estilo de resposta, com o texto exato \
             \"{question}\". As opções são \"{option}\" e \"Ajustar\", e com outro texto a aprovação \
             não vale."
        }
        ("resume.next.plan", Locale::EnUs) => {
            "The plan is recorded: check the plan and publish the spec page. Then ask the approval \
             question in the order of explaining from the response style, with the exact text \
             \"{question}\". The options are \"{option}\" and \"Adjust\", and with another text the \
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
        ("close.backlog_not_empty", Locale::PtBr) => {
            "O backlog ainda tem as tarefas {tasks}, que nenhuma onda entregou. Rode a rodada de novo \
             para formar o lote delas antes de fechar, ou retire da spec a que não vai mais ser feita."
        }
        ("close.backlog_not_empty", Locale::EnUs) => {
            "The backlog still holds tasks {tasks}, which no wave delivered. Run the round again to \
             form their batch before closing, or remove from the spec the one that will no longer be done."
        }
        ("close.criterion_failed", Locale::PtBr) => {
            "A verificação do critério {code} não passou: {output}"
        }
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
        ("close.lint_failed", Locale::PtBr) => {
            "O lint do projeto (`{command}`) não passou, e a spec não fechou: {output}"
        }
        ("close.lint_failed", Locale::EnUs) => {
            "The project lint (`{command}`) did not pass, and the spec did not close: {output}"
        }
        ("close.suite_failed", Locale::PtBr) => {
            "A suíte do projeto (`{command}`), rodada em ambiente limpo como o servidor a roda, não \
             passou, e a spec não fechou: {output}"
        }
        ("close.suite_failed", Locale::EnUs) => {
            "The project suite (`{command}`), run in a clean environment the way the server runs it, \
             did not pass, and the spec did not close: {output}"
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
        ("close.review_copy_failed", Locale::PtBr) => {
            "A cópia do revisor `{copy}` não pôde ser criada, e a spec não fechou: {detail}"
        }
        ("close.review_copy_failed", Locale::EnUs) => {
            "The reviewer's copy `{copy}` could not be created, and the spec did not close: {detail}"
        }
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
        ("close.build_output_failed", Locale::EnUs) => {
            "The build folder `{folder}` stayed on disk: {detail}. Delete it when nothing is building."
        }
        ("close.final_review", Locale::PtBr) => {
            "A máquina passou. Antes do pull request, despache o agente de teste dedicado, \
             `mustard-review`. Mande a ele o comando em `review.read`, e ele lê o próprio pedido. \
             Ele grava o veredito na spec com `mustard-rt run write verdict`. \
             Quando ele voltar, feche de novo: `mustard-rt run close --spec {spec}`. \
             Rode-o em segundo plano e espere o aviso de fim. A suíte inteira pode passar dos 10 \
             minutos que o terminal espera por um comando. Se o veredito não estiver na spec, mande o \
             agente gravá-lo de novo pela ferramenta."
        }
        ("close.final_review", Locale::EnUs) => {
            "The machine passed. Before the pull request, dispatch the dedicated test agent, \
             `mustard-review`. Send it the command in `review.read`, and it reads its own request. \
             It records its verdict in the spec with `mustard-rt run write verdict`. \
             When it comes back, close again: \
             `mustard-rt run close --spec {spec}`. Run it in the background and wait for the notice \
             that it ended. The whole suite can take longer than the 10 minutes the terminal waits \
             for a command. If the verdict is not in the spec, have the agent record it again \
             through the tool."
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
        // O caminho de volta: um teste que a obra criou ou mudou e que
        // nenhum critério cita na prova dele — o fechamento avisa, com o
        // nome do teste e o arquivo, em vez de deixar o teste sem dono.
        ("close.unowned_test", Locale::PtBr) => {
            "O teste {name}, em {file}, não tem critério que o cite na verificação dele: fica sem cobertura."
        }
        ("close.unowned_test", Locale::EnUs) => {
            "The test {name}, in {file}, has no criterion citing it in its verification: it stays without coverage."
        }

        // A rodada de ondas (`commands/flow/round.rs`).
        ("round.bad_report", Locale::PtBr) => {
            "O relatório da rodada não se entende: {detail}. Nada foi gravado."
        }
        ("round.bad_report", Locale::EnUs) => {
            "The round report cannot be read: {detail}. Nothing was recorded."
        }
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
        ("wave_prompt.token_cap", Locale::PtBr) => {
            "O pedido da onda {wave} tem {tokens} tokens, acima do teto de {cap}. Divida o lote em dois."
        }
        ("wave_prompt.token_cap", Locale::EnUs) => {
            "Wave {wave}'s request has {tokens} tokens, above the {cap} cap. Split the batch in two."
        }
        // O bloco em destaque do começo do pedido da onda que continua o
        // resumo de outra (`packages/core/src/domain/wave_prompt/summary.rs`).
        ("wave_prompt.summary.title", Locale::PtBr) => "Trabalho já começado",
        ("wave_prompt.summary.title", Locale::EnUs) => "Work already started",
        ("wave_prompt.summary.read", Locale::PtBr) => {
            "Esta onda continua o trabalho de um agente que parou no limite, e o resumo dele é {code}. \
             Antes de qualquer outro passo, leia-o com `mustard-rt run read item-{code} {root}--spec {spec}` \
             e faça só o que falta. A entrega é recusada sem essa leitura."
        }
        ("wave_prompt.summary.read", Locale::EnUs) => {
            "This wave continues the work of an agent that stopped at the limit, and its summary is {code}. \
             Before any other step, read it with `mustard-rt run read item-{code} {root}--spec {spec}` \
             and do only what is left. The delivery is refused without that reading."
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
        ("stuck.ended", Locale::PtBr) => "Processo(s) preso(s) encerrado(s): {list}.",
        ("stuck.ended", Locale::EnUs) => "Stuck process(es) ended: {list}.",
        ("stuck.reason.waiting_loop", Locale::PtBr) => "laço de espera",
        ("stuck.reason.waiting_loop", Locale::EnUs) => "waiting loop",
        ("stuck.reason.deleted_copy", Locale::PtBr) => "cópia de onda apagada",
        ("stuck.reason.deleted_copy", Locale::EnUs) => "deleted wave copy",
        ("stuck.reason.idle_copy", Locale::PtBr) => "cópia sem onda em andamento",
        ("stuck.reason.idle_copy", Locale::EnUs) => "copy with no wave running",
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
        ("conversation_size.replan", Locale::PtBr) => {
            "{wave} (pede mudança de plano, ainda sem o clique do usuário)"
        }
        ("conversation_size.replan", Locale::EnUs) => {
            "{wave} (asks for a plan change, still without the user's click)"
        }
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
            "[Mustard] Esta conversa passou de {tokens} mil tokens. Termine o que está em curso e \
             limpe a conversa com `/clear` ou compacte com `/compact`. Depois de compactar, o bloco \
             de retomada volta sozinho; numa janela limpa, cole este bloco: {block}"
        }
        ("conversation_size.notice", Locale::EnUs) => {
            "[Mustard] This conversation passed {tokens} thousand tokens. Finish what is in progress \
             and clear the conversation with `/clear` or compact it with `/compact`. After \
             compacting, the resume block comes back on its own; in a clean window, paste this \
             block: {block}"
        }
        // O aviso ao agente de onda que passou do limite da conversa, sem o
        // resumo que ele leu: terminar a tarefa em curso e gravar o que falta.
        ("conversation_size.wave_limit", Locale::PtBr) => {
            "[Mustard] A sua conversa está em {now} mil tokens; sem o resumo da onda anterior, \
             {counted} mil, acima do limite de {limit} mil. Termine a tarefa em curso, a que você \
             está lendo ou mudando agora, com o build passando, e grave o passo dela. Não comece \
             outra tarefa. Grave a entrega como o pedido manda. Nela, diga o que você fez e o que \
             aprendeu do código, que poupa leitura a quem continuar. Ponha em `undone` as tarefas \
             que você não começou. Um agente novo faz o que falta na rodada seguinte."
        }
        ("conversation_size.wave_limit", Locale::EnUs) => {
            "[Mustard] Your conversation is at {now} thousand tokens; without the previous wave's \
             summary, {counted} thousand, over the limit of {limit} thousand. Finish the task in \
             progress, the one you are reading or changing now, with the build passing, and record \
             its step. Do not start another task. Record the delivery as the request says. In it, \
             say what you did and what you learned from the code, which saves reading for whoever \
             continues. Put the tasks you did not start in `undone`. A new agent does what is left \
             in the next round."
        }
        // O aviso ao agente de onda que pede de novo linhas que leu há pouco
        // e que não mudaram: elas estão acima, na conversa.
        ("conversation_size.reread", Locale::PtBr) => {
            "[Mustard] Você já leu as linhas {from} a {to} de {file} há cerca de {ago} mil tokens, e \
             elas não mudaram desde então. O texto delas está acima, na sua conversa: use-o em vez de \
             ler de novo. Só se não o achar lá, repita o mesmo pedido e a leitura passa."
        }
        ("conversation_size.reread", Locale::EnUs) => {
            "[Mustard] You already read lines {from} to {to} of {file} about {ago} thousand tokens ago, \
             and they have not changed since. Their text is above, in your conversation: use it instead \
             of reading again. Only if you cannot find it there, repeat the same request and the read \
             goes through."
        }
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
        ("round.build_failed", Locale::PtBr) => {
            "O repositório principal não compilou com `{command}`, e a rodada não comitou nada: {output}"
        }
        ("round.build_failed", Locale::EnUs) => {
            "The main repository did not build with `{command}`, and the round committed nothing: {output}"
        }
        // A conferência depois da onda, antes do commit: as importações novas
        // contra o padrão do projeto e os restos do que a onda tirou.
        ("round.after_wave", Locale::PtBr) => {
            "A conferência depois da onda achou o que consertar, e a rodada não comitou nada. Mande cada \
             onda abaixo de volta ao agente dela, na mesma cópia. Ele conserta, grava a entrega de novo, \
             e você roda a rodada outra vez. Os avisos não seguram a rodada."
        }
        ("round.after_wave", Locale::EnUs) => {
            "The after-wave check found things to fix, and the round committed nothing. Send each wave \
             below back to its agent, in the same copy. The agent fixes them, records the delivery again, \
             and you run the round once more. Warnings do not hold the round."
        }
        ("round.after_wave.limit", Locale::PtBr) => {
            "A onda {waves} já passou por {max} rodadas de conserto, e a conferência depois da onda ainda \
             recusa. A rodada não comitou nada. Mostre ao usuário a lista abaixo e faça a pergunta de \
             `question`."
        }
        ("round.after_wave.limit", Locale::EnUs) => {
            "Wave {waves} has already gone through {max} fix rounds, and the after-wave check still \
             refuses. The round committed nothing. Show the user the list below and ask the question in \
             `question`."
        }
        ("round.after_wave.question", Locale::PtBr) => {
            "A onda {waves} ainda tem o que consertar depois de {max} rodadas de conserto. Mudar o desenho \
             do projeto por um pedido, revisar o plano da onda ou tirá-la do plano?"
        }
        ("round.after_wave.question", Locale::EnUs) => {
            "Wave {waves} still has things to fix after {max} fix rounds. Change the project design \
             through a request, revise the wave's plan, or take it out of the plan?"
        }
        ("round.after_wave.warnings", Locale::PtBr) => {
            "A conferência depois da onda só deixou avisos, e a rodada seguiu."
        }
        ("round.after_wave.warnings", Locale::EnUs) => "The after-wave check left only warnings, and the round went on.",
        ("round.after_wave.wave", Locale::PtBr) => "Onda {wave}, rodada de conserto {round} de {max}:",
        ("round.after_wave.wave", Locale::EnUs) => "Wave {wave}, fix round {round} of {max}:",
        ("round.after_wave.wave_warnings", Locale::PtBr) => "Onda {wave}, só avisos:",
        ("round.after_wave.wave_warnings", Locale::EnUs) => "Wave {wave}, warnings only:",
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
        ("round.after_wave.cycle", Locale::PtBr) => {
            "`{file}` linha {line} importa `{target}` e fecha um ciclo novo de importações. É só um aviso."
        }
        ("round.after_wave.cycle", Locale::EnUs) => {
            "`{file}` line {line} imports `{target}` and closes a new import cycle. It is only a warning."
        }
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
             chamava. Tire a declaração e o teste só dela."
        }
        ("round.after_wave.orphan", Locale::EnUs) => {
            "`{name}` in `{file}` line {line} lost its last use outside tests, because the wave removed its \
             caller. Remove the declaration and the tests that only cover it."
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
        // A onda que cresce além do que a tarefa pede, ou traz testes demais
        // para os critérios (`apps/rt/src/commands/flow/round/size_check.rs`).
        ("round.size.over", Locale::PtBr) => {
            "A onda {wave} pôs {added} linhas e tirou {removed}: o código cresceu {growth}. O limite é \
             {limit}, o maior entre 600 e três vezes a mediana de {median} linhas postas por onda. Corte o \
             que a tarefa não exige e mande o resto para `leftovers` da entrega."
        }
        ("round.size.over", Locale::EnUs) => {
            "Wave {wave} added {added} lines and removed {removed}: the code grew by {growth}. The limit is \
             {limit}, the larger of 600 and three times the median of {median} lines added per wave. Cut \
             what the task does not require and send the rest to the delivery's `leftovers`."
        }
        ("round.size.tests", Locale::PtBr) => {
            "A onda {wave} traz {tests} testes novos para {covered} critérios e regras cobertos. O limite é \
             {limit}, o maior entre 6 e dois por critério ou regra. Corte o teste que repete outro e o que \
             a tarefa não exige, e mande o resto para `leftovers` da entrega."
        }
        ("round.size.tests", Locale::EnUs) => {
            "Wave {wave} brings {tests} new tests for {covered} criteria and rules covered. The limit is \
             {limit}, the larger of 6 and two per criterion or rule. Cut the test that repeats another and \
             what the task does not require, and send the rest to the delivery's `leftovers`."
        }
        ("round.criterion_proof_failed", Locale::PtBr) => {
            "A verificação do critério {code} não executou ou não passou, e a rodada não comitou nada: \
             `{command}` — {output}"
        }
        ("round.criterion_proof_failed", Locale::EnUs) => {
            "Criterion {code}'s verification did not run or did not pass, and the round committed nothing: \
             `{command}` — {output}"
        }
        ("round.criterion_ran_no_test", Locale::PtBr) => {
            "A verificação do critério {code} saiu verde sem rodar teste nenhum: `{command}` diz que rodou \
             {count} testes. A rodada não comitou nada. Grave a versão nova do critério com a verificação \
             certa e rode a rodada de novo."
        }
        ("round.criterion_ran_no_test", Locale::EnUs) => {
            "The verification of criterion {code} came out green without running any test: `{command}` says \
             it ran {count} tests. The round committed nothing. Record the criterion's new version with the \
             right verification and run the round again."
        }
        ("round.criterion_missing_test", Locale::PtBr) => {
            "A verificação do critério {code} cita o teste {name}, que não aparece em nenhum arquivo do \
             projeto. A rodada não comitou nada. Escreva esse teste ou grave a versão nova do critério com o \
             nome certo, e rode a rodada de novo."
        }
        ("round.criterion_missing_test", Locale::EnUs) => {
            "The verification of criterion {code} names the test {name}, which appears in no file of the \
             project. The round committed nothing. Write that test or record the criterion's new version with \
             the right name, and run the round again."
        }
        ("round.development_build_failed", Locale::PtBr) => {
            "O commit saiu, mas a versão em construção do Mustard não compilou, e a sessão segue no \
             programa compilado anterior: {output}"
        }
        ("round.development_build_failed", Locale::EnUs) => {
            "The commit landed, but the development build of Mustard did not compile, and the session \
             stays on the previous compiled program: {output}"
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
        ("round.commit.scope.one", Locale::PtBr) => "onda-{waves}",
        ("round.commit.scope.one", Locale::EnUs) => "wave-{waves}",
        ("round.commit.scope.many", Locale::PtBr) => "ondas-{waves}",
        ("round.commit.scope.many", Locale::EnUs) => "waves-{waves}",
        ("round.commit.line", Locale::PtBr) => "- onda {wave}: {summary}",
        ("round.commit.line", Locale::EnUs) => "- wave {wave}: {summary}",
        ("round.commit.fixes", Locale::PtBr) => "(conserta: onda {waves})",
        ("round.commit.fixes", Locale::EnUs) => "(fixes: wave {waves})",
        ("round.size.line", Locale::PtBr) => "onda {wave}: +{added} -{removed}, {tests} testes, {files} arquivos",
        ("round.size.line", Locale::EnUs) => "wave {wave}: +{added} -{removed}, {tests} tests, {files} files",
        ("round.not_approved", Locale::PtBr) => {
            "A spec está na fase {phase} e ainda não foi aprovada: nenhuma onda sai antes do sim do usuário."
        }
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
        ("round.commit_looks_like_sha", Locale::PtBr) => {
            "O campo `commit` chegou como {found}, com cara de código de commit. Ali vai o título da \
             mensagem, em palavras e curto: o agente não comita, quem comita é a rodada."
        }
        ("round.commit_looks_like_sha", Locale::EnUs) => {
            "The `commit` field arrived as {found}, looking like a commit's code. That field takes the \
             message's title, in words and short: the agent does not commit, the round does."
        }
        ("round.formatter_missing", Locale::PtBr) => {
            "O projeto usa {name} e ele não foi achado: os arquivos da rodada ficaram sem formatar."
        }
        ("round.formatter_missing", Locale::EnUs) => {
            "The project uses {name} and it was not found: the round's files were left unformatted."
        }
        ("round.replan", Locale::PtBr) => {
            "A onda {wave} diz que a mudança de plano dela troca uma decisão do usuário. A \
             rodada não segue com ela sem o sim dele. Leia na volta da onda qual é a mudança e \
             qual decisão ela troca. Faça ao usuário a pergunta com opções, com \"{yes}\" e \
             \"{no}\", e ponha {code} no cabeçalho dela. É o cabeçalho que diz qual mudança o \
             clique decide. Escreva o enunciado pelo estilo de resposta: no máximo três frases \
             curtas. Nada de nome de arquivo, de símbolo nem de número de laboratório. Diga o \
             que muda para o usuário no sim e no não. Com o aceite, voltam à fila as tarefas \
             que a onda não fez: {tasks}. O sim é o clique em \"{yes}\": depois dele, repita a \
             rodada com o mesmo relatório."
        }
        ("round.replan", Locale::EnUs) => {
            "Wave {wave} says its plan change swaps a decision of the user's. The round does not \
             go on with it without their yes. Read in the wave's return what the change is and \
             which decision it swaps. Ask the user a question with options, with \"{yes}\" and \
             \"{no}\", and put {code} in its header. The header is what says which change the \
             click decides. Write the question by the answer style: at most three short \
             sentences. Use no file name, symbol or lab number. Say what changes for the user on \
             yes and on no. On acceptance, the tasks the wave did not do go back to the queue: \
             {tasks}. The yes is the click on \"{yes}\": after it, run the round again with the \
             same report."
        }
        ("round.replan_recorded", Locale::PtBr) => {
            "A onda {wave} mudou o plano sem trocar decisão do usuário, e a rodada seguiu sem \
             perguntar. A mudança: {change}. Conte-a na entrega, em \"O que eu decidi sozinho\"."
        }
        ("round.replan_recorded", Locale::EnUs) => {
            "Wave {wave} changed the plan without swapping a decision of the user's, and the round \
             went on without asking. The change: {change}. Tell it in the delivery, under \"What I \
             decided on my own\"."
        }
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
        ("round.returned_change", Locale::PtBr) => "Mudança de plano aceita na volta da onda {wave}: {change}",
        ("round.returned_change", Locale::EnUs) => "Plan change accepted when wave {wave} came back: {change}",
        ("round.leftover_joined", Locale::PtBr) => concat!(
            "Sobra da onda {wave}, nos mesmos arquivos — {title}: {detail} (Pelo mapa: ",
            map_commands_pt!(),
            ".)"
        ),
        ("round.leftover_joined", Locale::EnUs) => concat!(
            "Leftover from wave {wave}, on the same files — {title}: {detail} (By the map: ",
            map_commands_en!(),
            ".)"
        ),
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
        ("round.held_return", Locale::PtBr) => {
            "A volta da onda {wave} ficou fora desta rodada, e só ela: o resto seguiu. {hint}"
        }
        ("round.held_return", Locale::EnUs) => {
            "Wave {wave}'s return was left out of this round, and only it: the rest went on. {hint}"
        }
        ("round.git_refused", Locale::PtBr) => {
            "O git recusou o commit da rodada: {detail}\nNada foi gravado. Corrija o que o git \
             apontou, ou peça ao agente que grave a entrega corrigida, e rode a rodada de novo."
        }
        ("round.git_refused", Locale::EnUs) => {
            "Git refused the round's commit: {detail}\nNothing was recorded. Fix what git pointed \
             out, or have the agent record the corrected delivery, and run the round again."
        }
        // A resposta da rodada não traz o pedido da onda: traz o comando que
        // o lê, e é o agente quem o roda.
        ("round.next", Locale::PtBr) => {
            "Despache os pedidos desta rodada: cada onda ao agente `mustard-wave` e cada revisão ao \
             agente `mustard-review`. O pedido da onda não vem nesta resposta. Mande ao agente o \
             comando de `read`, e ele lê o próprio pedido."
        }
        ("round.next", Locale::EnUs) => {
            "Dispatch this round's requests: each wave to the `mustard-wave` agent and each review \
             to the `mustard-review` agent. The wave's request is not in this answer. Send the agent \
             the `read` command, and it reads its own request."
        }
        // O pedido de publicar e copiar a página fica por extenso só no
        // arquivo, e a resposta leva a linha curta.
        ("round.next.copy_file", Locale::PtBr) => "Leia `{path}` e siga as instruções de lá.",
        ("round.next.copy_file", Locale::EnUs) => "Read `{path}` and follow the instructions there.",
        ("round.report", Locale::PtBr) => {
            "Quando voltarem, cada agente já terá gravado a própria volta na spec, e a rodada a \
             assume. O de onda grava com `mustard-rt run write delivered`, o revisor com \
             `mustard-rt run write verdict`. Quando cada agente de onda terminar, rode a rodada de \
             novo com uma linha por onda, todas no mesmo `--report '…'`: `<USAGE>{\"wave\":1}</USAGE>`, \
             só com o número da onda. O consumo de cada onda e o seu a rodada mede nos arquivos de \
             conversa que a plataforma grava, nunca num número digitado. A rodada monta o commit do \
             `commit` de cada entrega. Quando a volta de um agente não estiver na spec, mande o \
             agente gravá-la de novo pela ferramenta. Nunca a monte a partir da prosa dele."
        }
        ("round.report", Locale::EnUs) => {
            "When they come back, each agent has already recorded its own return in the spec, and \
             the round takes it over. The wave agent records with `mustard-rt run write delivered`, \
             the reviewer with `mustard-rt run write verdict`. When each wave agent finishes, run \
             the round again with one line per wave, all in the same `--report '…'`: \
             `<USAGE>{\"wave\":1}</USAGE>`, with only the wave's number. The round measures each \
             wave's usage and yours from the conversation files the platform records, never from a \
             typed number. The round builds the commit from each delivery's `commit`. When an \
             agent's return is not in the spec, have the agent record it again through the tool. \
             Never assemble it from its prose."
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
        ("round.backlog_stuck", Locale::PtBr) => {
            "Nada a despachar e nada em andamento, mas o backlog ainda tem as tarefas {tasks}, presas. \
             Nenhuma tem todas as dependências entregues e cobre algum item. Mostre ao usuário o que as segura \
             antes de fechar."
        }
        ("round.backlog_stuck", Locale::EnUs) => {
            "Nothing to dispatch and nothing in flight, but the backlog still holds tasks {tasks}, stuck: \
             none has every dependency delivered and covers an item. Show the user what holds them before closing."
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
             espere a resposta. O objetivo da spec é uma frase que diz o que ele pediu: a dele, ou a \
             que você sugeriu e ele aprovou. Grave-o como o primeiro `context`, com `origin` na \
             mensagem dele. O card, os critérios de aceite e os documentos antigos que vierem junto \
             vão logo depois, como `context`, com o mesmo `origin`."
        }
        ("open.next_goal", Locale::EnUs) => {
            "Spec {spec} was born on branch {branch}. Ask the user the question in `question` and \
             wait for the answer. The spec's goal is one sentence saying what they asked for: theirs, \
             or the one you suggested and they approved. Record it as the first `context`, with \
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
        ("pr.submodules.stuck", Locale::EnUs) => {
            "The main pull request #{pr} stays a draft: {reason}. The next check tries again."
        }
        ("pr.pointer_commit", Locale::PtBr) => "chore(submódulo): atualiza o ponteiro",
        ("pr.pointer_commit", Locale::EnUs) => "chore(submodule): update the pointer",
        // A recusa do merge de uma spec que voltou à execução depois do
        // fechamento: o pull request dela leva a versão sem o ajuste até a
        // spec fechar de novo.
        ("pr.merge_reopened", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}: ela foi reaberta e ainda não fechou de novo. O \
             pull request #{pr} leva a versão sem o ajuste. Nada foi juntado, e o provedor nem foi \
             perguntado. Termine o ajuste e feche a spec pela rodada (`mustard-rt run round --spec \
             {spec}`). Depois do fechamento, o `pr-open` que ele aponta atualiza o mesmo pull \
             request, e o merge segue."
        }
        ("pr.merge_reopened", Locale::EnUs) => {
            "The spec {spec} is in the {phase} phase: it was reopened and has not closed again. \
             Pull request #{pr} carries the version without the change. Nothing was merged, and \
             the provider was not even asked. Finish the change and close the spec through the \
             round (`mustard-rt run round --spec {spec}`). After the close, the `pr-open` it points \
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
        ("pr.confirm_unreadable", Locale::EnUs) => {
            "The provider's own checks for `{unit}` could not be read ({checks}). Nothing was merged."
        }
        ("pr.confirm_no_verdict", Locale::PtBr) => {
            "`{unit}` não tem veredito de revisão gravado. Nada foi juntado."
        }
        ("pr.confirm_no_verdict", Locale::EnUs) => {
            "`{unit}` carries no recorded review verdict. Nothing was merged."
        }
        ("pr.confirm_verdict", Locale::PtBr) => {
            "A última revisão de `{unit}` voltou `{verdict}`. Nada foi juntado."
        }
        ("pr.confirm_verdict", Locale::EnUs) => {
            "The last review of `{unit}` came back `{verdict}`. Nothing was merged."
        }
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
        // O fim do passo da reabertura, pelo pull request da spec: o mesmo, ou
        // o fechado sem merge, que não vai para rascunho e dá lugar a outro.
        ("reopen.same_pr", Locale::PtBr) => "Depois vem o fechamento de novo, e o pull request continua o mesmo.",
        ("reopen.same_pr", Locale::EnUs) => "Then comes the close again, and the pull request stays the same.",
        ("reopen.pr_closed", Locale::PtBr) => {
            "O pull request #{pr} dela está fechado, sem merge, e por isso não foi posto em rascunho. \
             Depois vem o fechamento de novo, e o `pr-open` que ele aponta abre outro pull request, na \
             mesma branch."
        }
        ("reopen.pr_closed", Locale::EnUs) => {
            "Its pull request #{pr} is closed, not merged, so it was not put in draft. Then comes the \
             close again, and the `pr-open` it points to opens another pull request, on the same branch."
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
        // Os avisos da reabertura de uma spec com o pull request aberto: o
        // merge que não deu para conferir e o rascunho que o provedor recusou.
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
        ("reopen.draft_failed", Locale::PtBr) => {
            "O pull request da spec {spec} não foi posto em rascunho ({reason}) e ficou liberado. Um \
             merge pelo botão do provedor agora juntaria na base a versão sem o ajuste. Ponha-o em \
             rascunho pelo provedor, se puder; o merge do Mustard recusa até a spec fechar de novo."
        }
        ("reopen.draft_failed", Locale::EnUs) => {
            "The pull request of the spec {spec} was not put in draft ({reason}) and stays open to \
             merging. A merge through the provider's button now would take the version without the \
             change into the base. Put it in draft on the provider if you can; Mustard's merge \
             refuses until the spec closes again."
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
        ("reopen.fix_opened", Locale::PtBr) => {
            "O servidor reprovou os testes do pull request {pr}, e a onda de conserto {wave} está \
             aberta na spec {spec}, que continua fechada. Rode `mustard-rt run round --spec \
             {spec}`: a onda de conserto sai, e o conserto é comitado na mesma branch. Depois \
             rode este mesmo passo de novo para empurrar."
        }
        ("reopen.fix_opened", Locale::EnUs) => {
            "The server failed the tests of pull request {pr}, and the fix wave {wave} is open in \
             the spec {spec}, which stays closed. Run `mustard-rt run round --spec {spec}`: the \
             fix wave goes out, and the repair is committed on the same branch. Then run this \
             very step again to push it."
        }
        ("reopen.fix_waiting", Locale::PtBr) => {
            "A onda de conserto {wave} da spec {spec} já está aberta e ainda não entregou, e nada \
             foi gravado. Rode `mustard-rt run round --spec {spec}` para levá-la até o commit; \
             depois rode este mesmo passo de novo para empurrar."
        }
        ("reopen.fix_waiting", Locale::EnUs) => {
            "The fix wave {wave} of the spec {spec} is already open and has not delivered yet, and \
             nothing was written. Run `mustard-rt run round --spec {spec}` to take it to the \
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
        ("reopen.fix_wave_done", Locale::PtBr) => {
            "O servidor roda os testes do pull request {pr} de novo e eles voltam verdes."
        }
        ("reopen.fix_wave_done", Locale::EnUs) => {
            "The server runs the tests of pull request {pr} again and they come back green."
        }
        ("reopen.fix_note", Locale::PtBr) => {
            "A onda de conserto {wave} consertou o que o servidor reprovou no pull request {pr}, e \
             o commit dela foi empurrado para a branch {branch}."
        }
        ("reopen.fix_note", Locale::EnUs) => {
            "The fix wave {wave} repaired what the server failed on pull request {pr}, and its \
             commit was pushed to the branch {branch}."
        }
        // Só a última gravação que o pedido gera leva `--copy`: a página
        // recebe uma cópia só, já com tudo o que o pedido mudou.
        ("request.new_waves", Locale::PtBr) => {
            concat!(
                "Pedido gravado. Grave o que ele gerou, as tarefas novas, sem `wave`: elas entram no \
                 backlog, e o programa as junta em ondas na hora de despachar. Ache e leia o código pelo \
                 mapa antes de escrever cada tarefa: ",
                map_commands_pt!(),
                ". Passe `--copy` só na última dessas gravações: ela prepara uma cópia da página, já com \
                 tudo o que o pedido gerou. O pedido que não gera outra gravação leva `--copy` na própria \
                 gravação. A spec e a branch continuam as mesmas, e não há nova aprovação."
            )
        }
        ("request.new_waves", Locale::EnUs) => {
            concat!(
                "Request recorded. Record what it generated, the new tasks, without `wave`: they go into \
                 the backlog, and the program groups them into waves when it dispatches. Find and read \
                 the code through the map before writing each task: ",
                map_commands_en!(),
                ". Pass `--copy` only on the last of those writes: it prepares one copy of the page, \
                 already with everything the request generated. A request that generates no other write \
                 takes `--copy` on its own write. The spec and the branch stay the same, and there is no \
                 new approval."
            )
        }
        ("request.adjust_waves", Locale::PtBr) => {
            concat!(
                "Pedido gravado. Grave o que ele gerou: as versões novas das tarefas que mudam, com \
                 `replaces`. Repita o `wave` da versão antiga quando ela já está numa onda. Ache e leia \
                 o código pelo mapa antes de reescrever cada tarefa: ",
                map_commands_pt!(),
                ". Passe `--copy` só na última dessas gravações: ela prepara uma cópia da página, já com \
                 tudo o que o pedido gerou. O pedido que não gera outra gravação leva `--copy` na própria \
                 gravação. A spec e a branch continuam as mesmas, e não há nova aprovação."
            )
        }
        ("request.adjust_waves", Locale::EnUs) => {
            concat!(
                "Request recorded. Record what it generated: the new versions of the tasks that change, \
                 with `replaces`. Repeat the `wave` of the old version when it is already in a wave. \
                 Find and read the code through the map before rewriting each task: ",
                map_commands_en!(),
                ". Pass `--copy` only on the last of those writes: it prepares one copy of the page, \
                 already with everything the request generated. A request that generates no other write \
                 takes `--copy` on its own write. The spec and the branch stay the same, and there is no \
                 new approval."
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
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("flow.rs"),
            super::PREFIXES,
            216,
            0x2dd0_b6dc_c6da_4b76,
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

    /// Os títulos e as instruções fixas do pedido de uma onda, e o que a
    /// conferência do plano acha, saem do catálogo nos dois idiomas, com as
    /// vagas que o montador preenche.
    #[test]
    fn i18n_translates_wave_prompt_keys() {
        for (key, slots) in [
            ("plan.not_ready", &["{count}"][..]),
            ("plan.next", &["{question}", "{option}"][..]),
            ("page.copy.publish", &["{page}", "{template}", "{capabilities}", "{spec}", "{key}", "{milestone}"][..]),
            ("page.copy.batches", &["{page}", "{url}", "{key}"][..]),
            ("page.copy.record", &["{page}", "{spec}", "{record}"][..]),
            ("page.copy.record_project", &["{spec}", "{record}"][..]),
            ("page.copy.recorded", &["{page}"][..]),
            ("page.copy.old_page", &["{page}"][..]),
            ("page.migration.unrated", &["{tasks}", "{scale}", "{cap}"][..]),
            ("page.migration.over_cap", &["{wave}", "{points}", "{cap}"][..]),
            ("page.copy.new_address", &[][..]),
            ("page.copy.no_links", &[][..]),
            ("page.copy.failed", &[][..]),
            ("page.purge_pending", &["{codes}", "{spec}"][..]),
            ("plan.wave_loop", &["{waves}"][..]),
            ("plan.file_outside_git", &["{task}", "{path}"][..]),
            ("plan.item_without_task", &["{code}"][..]),
            ("plan.owner_missing", &["{type}"][..]),
            ("plan.contract_without_criterion", &["{code}"][..]),
            ("plan.task_without_file", &["{task}"][..]),
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
            ("close.backlog_not_empty", &["{tasks}"][..]),
            ("close.criterion_failed", &["{code}", "{output}"][..]),
            ("close.criterion_ran_no_test", &["{code}", "{command}", "{count}"][..]),
            ("close.criterion_missing_test", &["{code}", "{name}"][..]),
            ("close.lint_failed", &["{command}", "{output}"][..]),
            ("close.suite_failed", &["{command}", "{output}"][..]),
            ("close.server_command_not_declared", &["{key}"][..]),
            ("close.review_copy_dirty", &["{copy}", "{files}"][..]),
            ("close.review_copy_failed", &["{copy}", "{detail}"][..]),
            ("close.review_copy_kept", &["{copies}", "{detail}"][..]),
            ("close.code_not_kept", &["{copies}", "{detail}"][..]),
            ("close.build_output_unsafe", &["{folder}"][..]),
            ("close.build_output_not_ignored", &["{folder}"][..]),
            ("close.build_output_failed", &["{folder}", "{detail}"][..]),
            ("close.final_review", &["{spec}"][..]),
            ("close.next", &["{command}"][..]),
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
            ("reopen.same_pr", &[][..]),
            ("reopen.pr_closed", &["{pr}"][..]),
            ("stuck.ended", &["{list}"][..]),
            ("stuck.reason.waiting_loop", &[][..]),
            ("stuck.reason.deleted_copy", &[][..]),
            ("stuck.reason.idle_copy", &[][..]),
            (
                "conversation_size.block",
                &["{spec}", "{phase}", "{delivered}", "{running}", "{returned}", "{stuck}", "{missing}",
                    "{recorded}", "{command}", "{next}"][..],
            ),
            ("conversation_size.copy", &["{wave}", "{copy}"][..]),
            ("conversation_size.replan", &["{wave}"][..]),
            ("conversation_size.more", &["{count}"][..]),
            ("conversation_size.precompact", &["{block}"][..]),
            ("conversation_size.notice", &["{tokens}", "{block}"][..]),
            ("conversation_size.wave_limit", &["{now}", "{counted}", "{limit}"][..]),
            ("wave_prompt.summary.title", &[][..]),
            ("wave_prompt.summary.read", &["{code}", "{root}", "{spec}"][..]),
            ("wave_prompt.task_changed", &["{commits}", "{files}"][..]),
            ("round.file_unknown", &["{file}", "{wave}"][..]),
            ("round.files_diverged", &["{wave}", "{changed}", "{declared}", "{missing}"][..]),
            ("round.usage_missing", &["{wave}"][..]),
            ("round.build_failed", &["{command}", "{output}"][..]),
            ("round.after_wave.limit", &["{waves}", "{max}"][..]),
            ("round.after_wave.question", &["{waves}", "{max}"][..]),
            ("round.after_wave.wave", &["{wave}", "{round}", "{max}"][..]),
            ("round.after_wave.wave_warnings", &["{wave}"][..]),
            (
                "round.after_wave.import",
                &["{file}", "{line}", "{target}", "{from}", "{to}", "{rule_from}", "{rule_to}", "{along}", "{total}"][..],
            ),
            (
                "round.after_wave.weak",
                &["{file}", "{line}", "{target}", "{from}", "{to}", "{rule_from}", "{rule_to}", "{along}", "{total}"][..],
            ),
            ("round.after_wave.cycle", &["{file}", "{line}", "{target}"][..]),
            ("round.after_wave.leftover", &["{file}", "{line}", "{name}", "{from}"][..]),
            ("round.after_wave.orphan", &["{file}", "{line}", "{name}"][..]),
            ("round.after_wave.unused", &["{file}", "{line}", "{name}"][..]),
            ("round.after_wave.unused_test", &["{file}", "{line}", "{name}"][..]),
            ("round.size.over", &["{wave}", "{added}", "{removed}", "{growth}", "{limit}", "{median}"][..]),
            ("round.size.tests", &["{wave}", "{tests}", "{covered}", "{limit}"][..]),
            ("round.criterion_proof_failed", &["{code}", "{command}", "{output}"][..]),
            ("round.criterion_ran_no_test", &["{code}", "{command}", "{count}"][..]),
            ("round.criterion_missing_test", &["{code}", "{name}"][..]),
            ("round.development_build_failed", &["{output}"][..]),
            ("round.proof_ran_no_test", &["{code}"][..]),
            ("round.proof_missing_test", &["{code}", "{name}"][..]),
            ("round.commit.scope.one", &["{waves}"][..]),
            ("round.commit.scope.many", &["{waves}"][..]),
            ("round.commit.line", &["{wave}", "{summary}"][..]),
            ("round.commit.fixes", &["{waves}"][..]),
            ("round.size.line", &["{wave}", "{added}", "{removed}", "{tests}", "{files}"][..]),
            ("round.not_approved", &["{phase}"][..]),
            ("round.closed", &["{spec}", "{phase}"][..]),
            ("round.finished", &["{spec}", "{phase}"][..]),
            ("round.commit_too_long", &["{part}", "{chars}", "{max}"][..]),
            ("round.commit_forbidden", &["{found}"][..]),
            ("round.commit_looks_like_sha", &["{found}"][..]),
            ("round.formatter_missing", &["{name}"][..]),
            ("round.replan", &["{wave}", "{yes}", "{no}", "{code}", "{tasks}"][..]),
            ("round.replan_recorded", &["{wave}", "{change}"][..]),
            ("round.no_tasks", &[][..]),
            ("round.replan_needs_undone", &["{wave}", "{tasks}"][..]),
            ("round.undone_not_in_wave", &["{wave}", "{code}", "{tasks}"][..]),
            ("round.returned_change", &["{wave}", "{change}"][..]),
            ("round.leftover_joined", &["{wave}", "{title}", "{detail}"][..]),
            ("round.tasks_returned", &["{wave}", "{tasks}"][..]),
            ("round.held_return", &["{wave}", "{hint}"][..]),
            ("round.code_kept", &["{wave}", "{copy}", "{ref}"][..]),
            ("round.code_kept_slot", &["{copy}", "{ref}"][..]),
            ("round.copy_not_cleaned", &["{wave}", "{detail}"][..]),
            ("round.resend_moved", &["{wave}", "{copy}"][..]),
            ("round.resend_no_copy", &["{wave}"][..]),
            ("round.resend_gone", &["{wave}", "{copy}"][..]),
            ("round.resend_gone_no_copy", &["{wave}", "{copy}"][..]),
            ("round.git_refused", &["{detail}"][..]),
            ("round.next", &[][..]),
            ("round.next.copy_file", &["{path}"][..]),
            ("round.report", &[][..]),
            ("round.waiting", &["{waves}"][..]),
            ("round.close", &["{command}"][..]),
            ("round.review_open", &[][..]),
            ("round.missing", &["{wave}"][..]),
            ("round.backlog_left", &["{tasks}", "{command}"][..]),
            ("round.backlog_stuck", &["{tasks}"][..]),
            ("round.task_without_covers", &["{tasks}"][..]),
            ("round.fix_limit", &["{wave}", "{count}", "{max}", "{verdicts}"][..]),
            ("round.fix_limit.question", &["{wave}", "{max}"][..]),
            ("round.resume.steps", &[][..]),
            ("round.resume.notice", &[][..]),
            ("round.resume.silent", &["{wave}"][..]),
            ("page.findings.heading", &[][..]),
            ("prompt.title", &["{spec}", "{n}"][..]),
            ("prompt.languages", &["{text}", "{code}"][..]),
            ("prompt.final.title", &["{spec}"][..]),
            ("prompt.final.fixed", &[][..]),
            ("prompt.read.final", &["{root}", "{spec}"][..]),
            ("prompt.read.wave", &["{root}", "{spec}"][..]),
            ("prompt.part.delivers", &[][..]),
            ("prompt.part.read", &[][..]),
            ("prompt.part.do", &[][..]),
            ("prompt.part.obey", &[][..]),
            ("prompt.part.return", &[][..]),
            ("prompt.part.work", &[][..]),
            ("prompt.step.read", &["{part}"][..]),
            ("prompt.step.task", &["{item}"][..]),
            ("prompt.step.attends", &["{item}"][..]),
            ("prompt.step.read_task", &[][..]),
            ("prompt.step.read_task_attends", &[][..]),
            ("prompt.step.read_task_message", &[][..]),
            ("prompt.step.user_message", &[][..]),
            ("prompt.step.file", &["{files}"][..]),
            ("prompt.step.files", &["{files}"][..]),
            ("prompt.step.read_before", &["{hints}"][..]),
            ("prompt.step.suite", &["{command}"][..]),
            ("prompt.step.deliver", &["{part}"][..]),
            ("prompt.obey.no_lessons", &[][..]),
            ("prompt.return.loose", &[][..]),
            ("prompt.task.tested_by", &["{file}", "{tests}"][..]),
            ("prompt.part.waves", &[][..]),
            ("prompt.part.each_delivered", &[][..]),
            ("prompt.part.agreed", &[][..]),
            ("prompt.part.criteria", &[][..]),
            ("prompt.skill.stale", &[][..]),
            ("prompt.skill.read", &[][..]),
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
    /// pedido inteiro no campo `review.prompt`, nos dois idiomas. A frase que
    /// manda o comando tem 13 palavras em português e 12 em inglês, e o passo
    /// inteiro passa na conferência de escrita, com a lacuna trocada por uma
    /// palavra.
    #[test]
    fn the_final_review_step_sends_the_read_command_and_reads_clearly() {
        for (lang, sentence_words) in [(Locale::PtBr, 13), (Locale::EnUs, 12)] {
            let text = translate("close.final_review", lang).replace("{spec}", "teste");
            assert!(text.contains("review.read"), "{lang:?}: não manda o comando do campo: {text}");
            assert!(!text.contains("review.prompt"), "{lang:?}: ainda fala do pedido inteiro: {text}");
            let sentence = text
                .split(". ")
                .find(|sentence| sentence.contains("review.read"))
                .unwrap_or_else(|| panic!("{lang:?}: nenhuma frase cita o campo: {text}"));
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
            "round.size.over",
            "round.size.tests",
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
            ("{added}", "1900"),
            ("{removed}", "40"),
            ("{growth}", "1860"),
            ("{limit}", "900"),
            ("{median}", "300"),
            ("{tests}", "9"),
            ("{covered}", "2"),
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
