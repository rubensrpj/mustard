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
pub(super) const PREFIXES: &[&str] = &["open", "plan", "round", "close", "resume", "reopen", "discard", "request", "message", "pr", "approve_spec", "retired", "banner"];

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
            "Depois, faça a pergunta de aprovação, com \"Aprovar\" e \"Ajustar\"."
        }
        ("plan.next", Locale::EnUs) => "Then ask the approval question, with \"Approve\" and \"Adjust\".",
        ("plan.held", Locale::PtBr) => {
            "Depois, rode `mustard-rt run plan` de novo: com a página sem item retido, ele manda \
             publicar e fazer a pergunta de aprovação."
        }
        ("plan.held", Locale::EnUs) => {
            "Then run `mustard-rt run plan` again: with no item withheld from the page, it orders \
             the publish and the approval question."
        }
        ("plan.copy", Locale::PtBr) => {
            "A última publicação falhou: mande junto o comando de `copy` para o usuário abrir a página."
        }
        ("plan.copy", Locale::EnUs) => {
            "The last publish failed: send the `copy` command along so the user can open the page."
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
            "A tarefa {task} está na onda {wave} e o texto dela não casa com o texto dessa onda; \
             casa melhor com a onda {best}. Mova a tarefa ou reescreva o texto da onda."
        }
        ("plan.task_wrong_wave", Locale::EnUs) => {
            "Task {task} sits in wave {wave} and its text does not match that wave's text; \
             it matches wave {best} better. Move the task or rewrite the wave's text."
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
            "O plano está gravado: confira o plano, publique a página da spec e faça a pergunta de aprovação."
        }
        ("resume.next.plan", Locale::EnUs) => {
            "The plan is recorded: check the plan, publish the spec page and ask the approval question."
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
        ("close.bad_report", Locale::PtBr) => {
            "O relatório da última rodada não se entende: {detail}. Nada foi gravado."
        }
        ("close.bad_report", Locale::EnUs) => {
            "The last round's report cannot be read: {detail}. Nothing was recorded."
        }
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
        ("close.next", Locale::PtBr) => "Depois, abra o pull request.",
        ("close.next", Locale::EnUs) => "Then open the pull request.",

        // A rodada de ondas (`commands/flow/round.rs`).
        ("round.bad_report", Locale::PtBr) => {
            "O relatório da rodada não se entende: {detail}. Nada foi gravado."
        }
        ("round.bad_report", Locale::EnUs) => {
            "The round report cannot be read: {detail}. Nothing was recorded."
        }
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
        ("round.git_refused", Locale::PtBr) => "O git recusou o commit da rodada: {detail}",
        ("round.git_refused", Locale::EnUs) => "Git refused the round's commit: {detail}",
        ("round.next", Locale::PtBr) => {
            "Despache os pedidos desta rodada: cada onda ao agente `wave` e cada revisão ao agente \
             `review`. Quando voltarem, rode a rodada de novo com o relatório: `--report \
             '{\"waves\":[{\"wave\":1,\"delivered\":\"…\",\"files\":[\"…\"],\"verdict\":{…}}]}'`, com o \
             `verdict` tirado da linha `VERDICT` da revisão e, quando a onda disser que o plano não \
             funciona, o `replan` da linha `DELIVERED`."
        }
        ("round.next", Locale::EnUs) => {
            "Dispatch this round's requests: each wave to the `wave` agent and each review to \
             the `review` agent. When they come back, run the round again with the report: `--report \
             '{\"waves\":[{\"wave\":1,\"delivered\":\"…\",\"files\":[\"…\"],\"verdict\":{…}}]}'`, with the \
             `verdict` taken from the review's `VERDICT` line and, when the wave says its plan does \
             not work, the `replan` from the `DELIVERED` line."
        }
        ("plan.finding.label", Locale::PtBr) => "achado do plano",
        ("plan.finding.label", Locale::EnUs) => "plan finding",
        ("approve_spec.open_points", Locale::PtBr) => "{count} pontos do levantamento ainda abertos: {points}",
        ("approve_spec.open_points", Locale::EnUs) => "{count} survey points still open: {points}",
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
        ("open.ask_goal", Locale::PtBr) => "Qual o objetivo, numa frase?",
        ("open.ask_goal", Locale::EnUs) => "What is the goal, in one sentence?",
        ("open.next_goal", Locale::PtBr) => {
            "A spec {spec} nasceu na branch {branch}. Faça ao usuário a pergunta de `question` e \
             espere a resposta: ela vira o objetivo da spec, palavra por palavra, gravada como o \
             primeiro `context`, com `origin` na mensagem dele."
        }
        ("open.next_goal", Locale::EnUs) => {
            "Spec {spec} was born on branch {branch}. Ask the user the question in `question` and \
             wait for the answer: it becomes the spec's goal, word for word, recorded as the first \
             `context`, with `origin` on their message."
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
        ("retired.spec_draft", Locale::PtBr) => {
            "O `spec-draft` saiu do fluxo. Abra a spec com `mustard-rt run open`, que cria a branch e \
             a spec com o mesmo nome. Nada foi criado."
        }
        ("retired.spec_draft", Locale::EnUs) => {
            "`spec-draft` has left the flow. Open the spec with `mustard-rt run open`, which creates \
             the branch and the spec with the same name. Nothing was created."
        }
        ("retired.pipeline_door", Locale::PtBr) => {
            "O `emit-pipeline {kind}` não cria nem avança mais uma spec. Abra a spec com `mustard-rt \
             run open`; as fases passam pelos comandos do fluxo novo. Nada foi gravado."
        }
        ("retired.pipeline_door", Locale::EnUs) => {
            "`emit-pipeline {kind}` no longer creates or advances a spec. Open the spec with \
             `mustard-rt run open`; the phases go through the new flow's commands. Nothing was \
             written."
        }
        ("retired.wait_close", Locale::PtBr) => {
            "O `{command}` não grava mais na spec. Os critérios vão rodar, e a spec vai fechar, pelo \
             `mustard-rt run close`, que ainda não existe nesta versão. Nada foi gravado."
        }
        ("retired.wait_close", Locale::EnUs) => {
            "`{command}` no longer writes to the spec. The criteria will run, and the spec will \
             close, through `mustard-rt run close`, which does not exist in this version yet. \
             Nothing was written."
        }
        ("retired.wait_round", Locale::PtBr) => {
            "O `{command}` não grava mais o veredito na spec. O veredito de cada onda vai ser \
             gravado pelo `mustard-rt run round`, que ainda não existe nesta versão. Nada foi \
             gravado."
        }
        ("retired.wait_round", Locale::EnUs) => {
            "`{command}` no longer records the verdict in the spec. Each wave's verdict will be \
             recorded by `mustard-rt run round`, which does not exist in this version yet. Nothing \
             was written."
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
        ("message.too_long", Locale::PtBr) => {
            "O {part} da mensagem tem {chars} caracteres e o limite é {max}. Escreva outro: o \
             corte automático mentiria sobre o que a mensagem diz. Nada foi enviado."
        }
        ("message.too_long", Locale::EnUs) => {
            "The message {part} has {chars} characters and the limit is {max}. Write another one: \
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
        ("retired.approve_spec", Locale::PtBr) => {
            "O `approve-spec` não aprova nem avança mais uma spec. Quem aprova é o usuário, na \
             pergunta \"Aprovar esta spec?\": ele escolhe \"Aprovar\", e a testemunha grava a spec \
             como aprovada. Nada foi gravado."
        }
        ("retired.approve_spec", Locale::EnUs) => {
            "`approve-spec` no longer approves or advances a spec. The user approves it in the \
             question \"Approve this spec?\": they choose \"Approve\", and the witness records the \
             spec as approved. Nothing was written."
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
            87,
            0x5446_ae01_ac8d_c8f0,
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

    /// As recusas dos comandos antigos que saíram do fluxo saem do catálogo
    /// nos dois idiomas, cada uma com as vagas que o chamador preenche, e
    /// todas dizem por qual comando esperar, ou para qual ir. A da aprovação
    /// não tem comando para onde ir: ela diz a pergunta de aprovação.
    #[test]
    fn i18n_translates_retired_keys() {
        let (pt, en) = (translate("retired.approve_spec", Locale::PtBr), translate("retired.approve_spec", Locale::EnUs));
        assert!(pt.contains(translate("approval.question", Locale::PtBr)), "{pt}");
        assert!(en.contains(translate("approval.question", Locale::EnUs)), "{en}");
        for (key, slots, points_to) in [
            ("retired.spec_draft", &[][..], "mustard-rt run open"),
            ("retired.pipeline_door", &["{kind}"][..], "mustard-rt run open"),
            ("retired.wait_close", &["{command}"][..], "mustard-rt run close"),
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
            ("plan.next", &[][..]),
            ("plan.held", &[][..]),
            ("page.publish", &["{milestone}"][..]),
            ("page.hold", &["{codes}"][..]),
            ("page.after_purge", &["{milestone}"][..]),
            ("page.not_rebuilt", &[][..]),
            ("plan.copy", &[][..]),
            ("plan.wave_loop", &["{waves}"][..]),
            ("plan.depends_on_missing", &["{wave}", "{on}"][..]),
            ("plan.task_without_wave", &["{task}", "{wave}"][..]),
            ("plan.shared_file", &["{waves}", "{files}", "{chain}"][..]),
            ("plan.file_outside_git", &["{task}", "{path}"][..]),
            ("plan.item_without_task", &["{code}"][..]),
            ("plan.contract_without_criterion", &["{code}"][..]),
            ("plan.task_without_file", &["{task}", "{files}"][..]),
            ("plan.task_wrong_wave", &["{task}", "{wave}", "{best}"][..]),
            ("plan.task_could_name_a_skill", &["{task}", "{skill}"][..]),
            ("plan.skill_to_be_born", &["{task}"][..]),
            ("plan.no_suggestion", &[][..]),
            ("plan.finding.label", &[][..]),
            ("discard.preview", &["{spec}", "{branch}", "{remote}", "{what}", "{token}"][..]),
            ("discard.archive", &[][..]),
            ("discard.delete", &[][..]),
            ("discard.yes", &[][..]),
            ("discard.no", &[][..]),
            ("discard.reason", &[][..]),
            ("discard.confirm_mismatch", &[][..]),
            ("discard.incomplete", &[][..]),
            ("resume.line", &["{spec}", "{phase}", "{last}", "{next}"][..]),
            ("resume.none", &[][..]),
            ("resume.wave", &["{n}"][..]),
            ("resume.next.survey", &[][..]),
            ("resume.next.plan", &[][..]),
            ("resume.next.running", &[][..]),
            ("resume.next.closed", &[][..]),
            ("resume.next.pr_open", &[][..]),
            ("resume.next.delivered", &[][..]),
            ("resume.next.discarded", &[][..]),
            ("close.bad_report", &["{detail}"][..]),
            ("close.not_running", &["{phase}"][..]),
            ("close.wave_without_commit", &["{wave}"][..]),
            ("close.wave_rejected", &["{wave}"][..]),
            ("close.request_not_delivered", &["{code}"][..]),
            ("close.criterion_failed", &["{code}", "{output}"][..]),
            ("close.next", &[][..]),
            ("round.bad_report", &["{detail}"][..]),
            ("round.not_approved", &["{phase}"][..]),
            ("round.delivered_too_long", &["{wave}", "{chars}", "{max}"][..]),
            ("round.commit_too_long", &["{part}", "{chars}", "{max}"][..]),
            ("round.commit_forbidden", &["{found}"][..]),
            ("round.formatter_missing", &["{name}"][..]),
            ("round.replan", &["{wave}", "{change}", "{question}", "{yes}", "{no}"][..]),
            ("round.git_refused", &["{detail}"][..]),
            ("round.next", &[][..]),
            ("page.findings.heading", &[][..]),
            ("prompt.title", &["{spec}", "{n}"][..]),
            ("prompt.fixed", &[][..]),
            ("prompt.review.title", &["{spec}", "{n}"][..]),
            ("prompt.review.fixed", &[][..]),
            ("prompt.part.defects", &[][..]),
            ("prompt.part.specification", &[][..]),
            ("prompt.part.agreed", &[][..]),
            ("prompt.part.wave", &[][..]),
            ("prompt.part.criteria", &[][..]),
            ("prompt.part.lessons", &[][..]),
            ("prompt.part.skills", &[][..]),
            ("prompt.part.delivered", &[][..]),
            ("prompt.skill.stale", &[][..]),
            ("prompt.skill.read", &[][..]),
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
}
