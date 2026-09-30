//! O arquivo de eventos da spec: as recusas e os avisos da gravação e da
//! leitura, o banco de lições e o índice das specs.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["spec_events", "lessons", "spec_index"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Recusas e avisos do arquivo de eventos da spec (`domain::spec_events`,
        // comandos `run write` e `run read`). As vagas vêm do chamador.
        ("spec_events.not_an_object", Locale::PtBr) => {
            "Os campos do evento precisam vir num objeto JSON, como {\"text\": \"…\"}, e o que \
             veio não serve: {detail}. Nada foi gravado."
        }
        ("spec_events.not_an_object", Locale::EnUs) => {
            "The event's fields must come as one JSON object, like {\"text\": \"…\"}, and what \
             came does not parse: {detail}. Nothing was written."
        }
        ("spec_events.unknown_type", Locale::PtBr) => {
            "O tipo {type} não existe no arquivo da spec. Nada foi gravado. Tipos aceitos: {types}."
        }
        ("spec_events.unknown_type", Locale::EnUs) => {
            "The spec file has no {type} event type. Nothing was written. Accepted types: {types}."
        }
        ("spec_events.missing_field", Locale::PtBr) => {
            "Campos obrigatórios do evento {type} que faltaram ou vieram vazios: {field}. Nada foi \
             gravado."
        }
        ("spec_events.missing_field", Locale::EnUs) => {
            "Required fields of the {type} event that are missing or empty: {field}. Nothing was \
             written."
        }
        ("spec_events.task_declaration_missing", Locale::PtBr) => {
            "A tarefa precisa declarar: {missing}. Nada foi gravado."
        }
        ("spec_events.task_declaration_missing", Locale::EnUs) => {
            "The task must declare: {missing}. Nothing was written."
        }
        ("spec_events.task_declaration_what", Locale::PtBr) => "o que ela faz",
        ("spec_events.task_declaration_what", Locale::EnUs) => "what it does",
        ("spec_events.task_declaration_files", Locale::PtBr) => "os arquivos que toca",
        ("spec_events.task_declaration_files", Locale::EnUs) => "the files it touches",
        ("spec_events.task_declaration_depends_on", Locale::PtBr) => "de quais tarefas depende",
        ("spec_events.task_declaration_depends_on", Locale::EnUs) => "which tasks it depends on",
        ("spec_events.task_declaration_title", Locale::PtBr) => {
            "um título curto, de até 70 caracteres, com o que ela entrega (como \"Fechamento \
             confere cada critério contra o código\")"
        }
        ("spec_events.task_declaration_title", Locale::EnUs) => {
            "a short title, of up to 70 characters, saying what it delivers (like \"Closing checks \
             each criterion against the code\")"
        }
        ("spec_events.task_declaration_covers", Locale::PtBr) => {
            "os itens que ela cobre, em `covers`, pelo número (a onda leva esses itens como critérios)"
        }
        ("spec_events.task_declaration_covers", Locale::EnUs) => {
            "the items it covers, in `covers`, by number (the wave takes them as its criteria)"
        }
        ("spec_events.task_uncovered_items", Locale::PtBr) => "Itens que nenhuma tarefa cobre ainda: {items}.",
        ("spec_events.task_uncovered_items", Locale::EnUs) => "Items no task covers yet: {items}.",
        ("spec_events.task_uncovered_more", Locale::PtBr) => "e mais {n}",
        ("spec_events.task_uncovered_more", Locale::EnUs) => "and {n} more",
        // A forma fixa de um item que descreve o trabalho: título, parte do
        // usuário e parte do agente, e a conferência de escrita das duas
        // primeiras.
        ("spec_events.item_form_missing", Locale::PtBr) => {
            "Falta ao item: {missing}. Nada foi gravado."
        }
        ("spec_events.item_form_missing", Locale::EnUs) => "The item is missing: {missing}. Nothing was written.",
        ("spec_events.item_part_title", Locale::PtBr) => "o título curto em `title`, com até 70 caracteres",
        ("spec_events.item_part_title", Locale::EnUs) => "a short title in `title`, up to 70 characters",
        ("spec_events.item_part_user", Locale::PtBr) => "a parte do usuário em `text`, com o porquê",
        ("spec_events.item_part_user", Locale::EnUs) => "the user's part in `text`, with the why",
        ("spec_events.item_part_agent", Locale::PtBr) => {
            "a parte do agente em `agent`, com arquivos, linhas, comandos e o que testar"
        }
        ("spec_events.item_part_agent", Locale::EnUs) => {
            "the agent's part in `agent`, with files, lines, commands and what to test"
        }
        ("spec_events.item_part_user_cites", Locale::PtBr) => {
            "a parte do usuário sem {found}, porque crase, caminho e código de item vão para `agent`"
        }
        ("spec_events.item_part_user_cites", Locale::EnUs) => {
            "the user's part without {found}, because backticks, paths and item codes go in `agent`"
        }
        ("spec_events.item_unclear", Locale::PtBr) => {
            "O título e a parte do usuário passam pela conferência de escrita das respostas. Ela \
             achou: {defects}. Reescreva e grave de novo. Nada foi gravado."
        }
        ("spec_events.item_unclear", Locale::EnUs) => {
            "The title and the user's part go through the writing check of responses. It found: \
             {defects}. Rewrite them and write again. Nothing was written."
        }
        ("spec_events.task_depends_on_unknown", Locale::PtBr) => {
            "A tarefa {task} depende de {depends_on}, que não existe nesta spec. Nada foi gravado."
        }
        ("spec_events.task_depends_on_unknown", Locale::EnUs) => {
            "Task {task} depends on {depends_on}, which does not exist in this spec. Nothing was \
             written."
        }
        ("spec_events.task_dependency_cycle", Locale::PtBr) => {
            "As tarefas dependem umas das outras em círculo: {cycle}. Nada foi gravado."
        }
        ("spec_events.task_dependency_cycle", Locale::EnUs) => {
            "The tasks depend on each other in a circle: {cycle}. Nothing was written."
        }
        ("spec_events.agreed_items_missing", Locale::PtBr) => {
            "O veredito final precisa responder por todos os requisitos acordados vigentes: faltou {missing}. \
             O revisor grava o veredito de novo, com todos eles. Nada foi gravado."
        }
        ("spec_events.agreed_items_missing", Locale::EnUs) => {
            "The final verdict must answer for all the vigent agreed requirements: {missing} is missing. \
             The reviewer writes the verdict again, with all of them. Nothing was written."
        }
        ("spec_events.delivery_agreed_missing", Locale::PtBr) => {
            "A entrega da onda {wave} precisa responder, em `agreed`, por cada item combinado que o pedido \
             dela levou: faltou {missing}. Grave a entrega de novo com cada um em `agreed`, pelo código em \
             `item` e com `met`: `true` se foi cumprido, ou `false` com o que falta em `text`. Nada foi \
             gravado."
        }
        ("spec_events.delivery_agreed_missing", Locale::EnUs) => {
            "The delivery of wave {wave} must answer, in `agreed`, for each agreed item its request carried: \
             {missing} is missing. Record the delivery again with each one in `agreed`, by its code in \
             `item` and with `met`: `true` if it was met, or `false` with what is missing in `text`. \
             Nothing was written."
        }
        ("spec_events.criterion_form_missing", Locale::PtBr) => {
            "O critério precisa declarar a forma dele, uma das cinco do padrão. São elas: a que vale \
             sempre, a disparada por um acontecimento e a que só vale enquanto um estado durar. As \
             outras duas são a que só vale se um recurso existir e a que trata um acontecimento \
             indesejado. Nada foi gravado."
        }
        ("spec_events.criterion_form_missing", Locale::EnUs) => {
            "The criterion must declare its form, one of the pattern's five. They are: the kind that \
             always holds, the kind triggered by an event, and the kind that only holds while a \
             state lasts. The other two are the kind that only holds if a resource exists and the \
             kind that handles an unwanted event. Nothing was written."
        }
        ("spec_events.proof_not_a_command", Locale::PtBr) => {
            "A prova do critério {criterion} precisa ser uma linha de comando, a que demonstra o \
             critério, e veio {found}. Junte as provas do mesmo critério num comando só e repita a \
             rodada. Nada foi gravado."
        }
        ("spec_events.proof_not_a_command", Locale::EnUs) => {
            "Criterion {criterion}'s proof must be a command line, the one that demonstrates the \
             criterion, and {found} arrived instead. Merge the proofs of the same criterion into a \
             single command and run the round again. Nothing was written."
        }
        ("spec_events.proof_program_unknown", Locale::PtBr) => {
            "A prova do critério precisa ser um comando que roda, e o primeiro termo dela, {term}, \
             não é um programa que o shell ache. Escreva no lugar o comando que demonstra o \
             critério. A conferência que só se faz à mão vai para o texto do critério, e não para \
             a prova. Nada foi gravado."
        }
        ("spec_events.proof_program_unknown", Locale::EnUs) => {
            "A criterion's proof must be a command that runs, and its first term, {term}, is not a \
             program the shell can find. Write the command that demonstrates the criterion \
             instead. A check that can only be done by hand goes in the criterion's text, not in \
             the proof. Nothing was written."
        }
        ("spec_events.proof_chained_by_semicolon", Locale::PtBr) => {
            "A prova liga comandos por ponto e vírgula, e só o último decide o resultado: {found}. \
             Ligue os comandos por && e a prova falha se qualquer um falhar. Nada foi gravado."
        }
        ("spec_events.proof_chained_by_semicolon", Locale::EnUs) => {
            "The proof chains commands with a semicolon, and only the last one decides the result: \
             {found}. Join the commands with && and the proof fails if any of them fails. Nothing \
             was written."
        }
        ("spec_events.proof_search_not_negated", Locale::PtBr) => {
            "A prova é uma busca sozinha, {found}, e uma busca sai com sucesso quando acha texto. \
             Se ela deve sair vazia, escreva ! na frente: a prova passa quando nada é achado. Se \
             ela deve achar o texto, acrescente -q. Nada foi gravado."
        }
        ("spec_events.proof_search_not_negated", Locale::EnUs) => {
            "The proof is a lone search, {found}, and a search succeeds when it finds text. If it \
             must come out empty, put ! in front: the proof then passes when nothing is found. If \
             it must find the text, add -q. Nothing was written."
        }
        ("spec_events.invalid_value", Locale::PtBr) => {
            "O campo {field} do evento {type} precisa ser {expected}. Nada foi gravado."
        }
        ("spec_events.invalid_value", Locale::EnUs) => {
            "The {field} field of the {type} event must be {expected}. Nothing was written."
        }
        ("spec_events.wrong_count", Locale::PtBr) => {
            "O campo {field} do evento {type} leva de {min} a {max} itens, e vieram {count}. Nada \
             foi gravado."
        }
        ("spec_events.wrong_count", Locale::EnUs) => {
            "The {field} field of the {type} event takes {min} to {max} items, and {count} came. \
             Nothing was written."
        }
        ("spec_events.fact_without_source", Locale::PtBr) => {
            "O fato {fact} do ponto não tem fonte. Diga de onde ele saiu: o arquivo e a linha, o \
             comando com o resultado, ou o número da mensagem do usuário. Nada foi gravado."
        }
        ("spec_events.fact_without_source", Locale::EnUs) => {
            "Fact {fact} of the point has no source. Say where it came from: the file and line, \
             the command with its result, or the number of the user's message. Nothing was \
             written."
        }
        ("spec_events.cited_file_missing", Locale::PtBr) => {
            "O fato {fact} cita {path}, e esse arquivo não existe. Confira o caminho antes de \
             afirmar. Nada foi gravado."
        }
        ("spec_events.cited_file_missing", Locale::EnUs) => {
            "Fact {fact} cites {path}, and that file does not exist. Check the path before stating \
             it. Nothing was written."
        }
        ("spec_events.cited_line_missing", Locale::PtBr) => {
            "O fato {fact} cita a linha {line} de {path}, mas o arquivo tem {lines} linhas. Confira \
             a linha antes de afirmar. Nada foi gravado."
        }
        ("spec_events.cited_line_missing", Locale::EnUs) => {
            "Fact {fact} cites line {line} of {path}, but the file has {lines} lines. Check the \
             line before stating it. Nothing was written."
        }
        ("spec_events.name_elsewhere", Locale::PtBr) => {
            "O fato {fact} cita `{name}`, que o mapa não acha em {path}, e sim em {found}."
        }
        ("spec_events.name_elsewhere", Locale::EnUs) => {
            "Fact {fact} cites `{name}`, which the map does not find in {path}, but in {found}."
        }
        ("spec_events.name_unknown", Locale::PtBr) => {
            "O fato {fact} cita `{name}`, e o mapa do projeto não conhece esse nome. Confira antes \
             de afirmar."
        }
        ("spec_events.name_unknown", Locale::EnUs) => {
            "Fact {fact} cites `{name}`, and the project map does not know that name. Check it \
             before stating it."
        }
        ("spec_events.names_unchecked", Locale::PtBr) => {
            "Sem o mapa do projeto, os nomes citados não foram conferidos. Rode `mustard-rt run scan`."
        }
        ("spec_events.names_unchecked", Locale::EnUs) => {
            "Without the project map, the cited names were not checked. Run `mustard-rt run scan`."
        }
        ("spec_events.deferred_unknown_pending", Locale::PtBr) => {
            "A pendência {pending} não existe na lista. Crie-a com `mustard-rt run pending --add` e \
             grave o pedido adiado com o número que ela receber. Nada foi gravado."
        }
        ("spec_events.deferred_unknown_pending", Locale::EnUs) => {
            "Pending item {pending} is not on the list. Create it with `mustard-rt run pending --add` \
             and record the deferred request with the number it gets. Nothing was written."
        }
        ("spec_events.deferred_closed_pending", Locale::PtBr) => {
            "A pendência {pending} já está fechada ou descartada. Um pedido adiado aponta uma \
             pendência aberta: crie outra com `mustard-rt run pending --add`. Nada foi gravado."
        }
        ("spec_events.deferred_closed_pending", Locale::EnUs) => {
            "Pending item {pending} is already closed or dropped. A deferred request points to an \
             open item: create another one with `mustard-rt run pending --add`. Nothing was written."
        }
        ("spec_events.goal_origin_not_user", Locale::PtBr) => {
            "O primeiro `context` da spec {spec} é o objetivo, e ele aponta em `origin` a mensagem \
             do usuário que o define. Grave o objetivo em `text` e, em `origin`, o número dessa \
             mensagem; {origin} não é uma mensagem do usuário. Nada foi gravado."
        }
        ("spec_events.goal_origin_not_user", Locale::EnUs) => {
            "The first `context` of spec {spec} is the goal, and it points in `origin` to the user \
             message that defines it. Put the goal in `text` and that message's number in `origin`; \
             {origin} is not a user message. Nothing was written."
        }
        ("spec_events.survey_open", Locale::PtBr) => {
            "A spec {spec} ainda tem pontos abertos no levantamento ({count}): {points}. Feche cada um, \
             com a resposta ou com \"não se aplica\" e o motivo, antes de passar para o plano. Nada \
             foi gravado."
        }
        ("spec_events.survey_open", Locale::EnUs) => {
            "Spec {spec} still has open survey points ({count}): {points}. Close each one, with the \
             answer or with \"not applicable\" and the reason, before moving on to the plan. Nothing \
             was written."
        }
        ("spec_events.survey_not_started", Locale::PtBr) => {
            "A spec {spec} ainda não teve levantamento: rode `mustard-rt run grill` antes de passar \
             para o plano. Nada foi gravado."
        }
        ("spec_events.survey_not_started", Locale::EnUs) => {
            "Spec {spec} has had no survey yet: run `mustard-rt run grill` before moving on to the \
             plan. Nothing was written."
        }
        ("spec_events.survey_gaps_unrecorded", Locale::PtBr) => {
            "A spec {spec} ainda tem {count} lacunas do tipo de trabalho sem ponto: {gaps}. Rode \
             `mustard-rt run grill` e grave os pontos que ele lista antes de passar para o plano. \
             Nada foi gravado."
        }
        ("spec_events.survey_gaps_unrecorded", Locale::EnUs) => {
            "Spec {spec} still has {count} work-type gaps without a point: {gaps}. Run `mustard-rt \
             run grill` and record the points it lists before moving on to the plan. Nothing was \
             written."
        }
        ("spec_events.point_already_open", Locale::PtBr) => {
            "O ponto {code}, no bloco {block}, já está aberto com esta mesma lacuna. Responda a ele \
             em vez de gravar outro: dois pontos abertos pedindo a mesma resposta fariam a mesma \
             pergunta duas vezes. Nada foi gravado."
        }
        ("spec_events.point_already_open", Locale::EnUs) => {
            "Point {code}, in the block {block}, is already open with this very gap. Answer it \
             instead of recording another: two open points asking for the same answer would ask \
             the same question twice. Nothing was written."
        }
        ("spec_events.point_not_open", Locale::PtBr) => {
            "O ponto {id} não está aberto, e só um ponto aberto pode ser fechado. Abertos agora: \
             {open}. Nada foi gravado."
        }
        ("spec_events.point_not_open", Locale::EnUs) => {
            "Point {id} is not open, and only an open point can be closed. Open now: {open}. Nothing \
             was written."
        }
        ("spec_events.closing_point_open", Locale::PtBr) => {
            "Um ponto que fecha outro (`closes`) leva a situação `closed` ou `not_applicable`, nunca \
             `open`. Nada foi gravado."
        }
        ("spec_events.closing_point_open", Locale::EnUs) => {
            "A point that closes another (`closes`) takes the status `closed` or `not_applicable`, \
             never `open`. Nothing was written."
        }
        ("spec_events.not_applicable_reason", Locale::PtBr) => {
            "Um ponto marcado \"não se aplica\" leva o motivo em `reason`. Nada foi gravado."
        }
        ("spec_events.not_applicable_reason", Locale::EnUs) => {
            "A point marked \"not applicable\" takes the reason in `reason`. Nothing was written."
        }
        ("spec_events.open_point_removed", Locale::PtBr) => {
            "O ponto {code} está aberto e não sai com `remove`: feche-o com um ponto que o aponte em \
             `closes`, com a resposta ou o motivo. Nada foi gravado."
        }
        ("spec_events.open_point_removed", Locale::EnUs) => {
            "Point {code} is open and does not leave with `remove`: close it with a point that names \
             it in `closes`, with the answer or the reason. Nothing was written."
        }
        ("spec_events.purge_excerpt_not_found", Locale::PtBr) => {
            "O item {code} não traz o trecho a expurgar: nem o que o pedido indica em `excerpt`, nem \
             texto com cara de segredo. Diga o trecho exato em `excerpt`. Nada foi gravado."
        }
        ("spec_events.purge_excerpt_not_found", Locale::EnUs) => {
            "Item {code} does not carry the excerpt to purge: neither the one the request names in \
             `excerpt` nor text that looks like a secret. Name the exact excerpt in `excerpt`. \
             Nothing was written."
        }
        ("spec_events.closing_point_last_record", Locale::PtBr) => {
            "O ponto {code} fecha um ponto cujo texto original já saiu, e é o único registro dele: \
             não sai com `remove`. Para tirar um dado sensível dele, use `purge`, que só oculta o \
             trecho. Nada foi gravado."
        }
        ("spec_events.closing_point_last_record", Locale::EnUs) => {
            "Point {code} closes a point whose original text is already gone, and it is the only \
             record of it: it does not leave with `remove`. To take sensitive data out of it, use \
             `purge`, which only hides the excerpt. Nothing was written."
        }
        ("spec_events.delivered_too_long", Locale::PtBr) => {
            "O entregou tem {chars} caracteres, e o teto é {max}. Ele volta para a janela principal: \
             conte o que mudou, sem repetir o pedido. Nada foi gravado."
        }
        ("spec_events.delivered_too_long", Locale::EnUs) => {
            "The delivery note has {chars} characters, and the cap is {max}. It goes back to the main \
             window: say what changed, without repeating the request. Nothing was written."
        }
        ("spec_events.no_open_send", Locale::PtBr) => {
            "Não há envio aberto para a onda {wave}. Nada foi gravado."
        }
        ("spec_events.no_open_send", Locale::EnUs) => {
            "There is no open send for wave {wave}. Nothing was written."
        }
        ("spec_events.no_open_review", Locale::PtBr) => {
            "Não há pedido de revisão aberto nesta spec. Nada foi gravado."
        }
        ("spec_events.no_open_review", Locale::EnUs) => {
            "There is no open review request in this spec. Nothing was written."
        }
        ("spec_events.verdict_missing", Locale::PtBr) => {
            "O pedido de revisão segue aberto, e o revisor ainda não gravou o veredito. Peça a ele \
             que o grave com `mustard-rt run write verdict` e rode o fechamento de novo. Nenhum \
             pedido de revisão novo foi aberto."
        }
        ("spec_events.verdict_missing", Locale::EnUs) => {
            "The review request is still open, and the reviewer has not written the verdict yet. \
             Ask them to write it with `mustard-rt run write verdict` and run the close again. No \
             new review request was opened."
        }
        ("spec_events.leftover_field_missing", Locale::PtBr) => {
            "Falta o campo {field} numa sobra (leftovers) da entrega. Nada foi gravado."
        }
        ("spec_events.leftover_field_missing", Locale::EnUs) => {
            "A leftover (leftovers) of the delivery lacks the field {field}. Nothing was written."
        }
        ("spec_events.report_carries_return_line", Locale::PtBr) => {
            "A entrega e o veredito moram na spec: o agente os grava com mustard-rt run write. O \
             relatório leva só as linhas `USAGE`, `PAUSED` e `ANALYSIS`."
        }
        ("spec_events.report_carries_return_line", Locale::EnUs) => {
            "The delivery and the verdict live in the spec: the agent writes them with mustard-rt \
             run write. The report carries only the `USAGE`, `PAUSED` and `ANALYSIS` lines."
        }
        ("spec_events.return_missing", Locale::PtBr) => {
            "A onda {wave} terminou sem gravar a entrega: peça ao agente que a grave com `mustard-rt \
             run write delivered` e rode a rodada de novo com a linha `USAGE` dela."
        }
        ("spec_events.return_missing", Locale::EnUs) => {
            "Wave {wave} ended without writing its delivery: ask the agent to write it with \
             `mustard-rt run write delivered` and run the round again with its `USAGE` line."
        }
        ("spec_events.return_needs_commit", Locale::PtBr) => {
            "A cópia da onda {wave} mudou arquivo, e a entrega que ela gravou não traz o resumo do \
             commit (`commit`). Peça ao agente que grave a entrega de novo, com o campo, e rode a \
             rodada de novo. Nada foi gravado."
        }
        ("spec_events.return_needs_commit", Locale::EnUs) => {
            "The copy of wave {wave} changed files, and the delivery it wrote lacks the commit \
             summary (`commit`). Ask the agent to write the delivery again, with the field, and \
             run the round again. Nothing was written."
        }
        ("spec_events.unknown_target", Locale::PtBr) => {
            "O evento {id} não existe nesta spec. Nada foi gravado."
        }
        ("spec_events.unknown_target", Locale::EnUs) => {
            "Event {id} does not exist in this spec. Nothing was written."
        }
        ("spec_events.unknown_code", Locale::PtBr) => {
            "O item {code} não existe nesta spec. Confira o código na página ou no read. Nada foi \
             gravado."
        }
        ("spec_events.unknown_code", Locale::EnUs) => {
            "Item {code} does not exist in this spec. Check the code on the page or in read. \
             Nothing was written."
        }
        ("spec_events.target_other_type", Locale::PtBr) => {
            "O item {item} existe, mas é do tipo {found}, e este campo pede um item do tipo \
             {expected}. Use o código de um {expected}, que a página e o read mostram. Nada foi \
             gravado."
        }
        ("spec_events.target_other_type", Locale::EnUs) => {
            "Item {item} exists, but it is a {found}, and this field asks for an item of type \
             {expected}. Use the code of a {expected}, shown on the page and in read. Nothing was \
             written."
        }
        ("spec_events.binary_only_field", Locale::PtBr) => {
            "O campo {field} é gravado só pelo binário e não pode vir no --json. Para apontar um \
             item pelo código, use replaces ou os alvos de remove e purge. Nada foi gravado."
        }
        ("spec_events.binary_only_field", Locale::EnUs) => {
            "The {field} field is written only by the binary and cannot come in --json. To point \
             at an item by its code, use replaces or the targets of remove and purge. Nothing was \
             written."
        }
        ("spec_events.unknown_field", Locale::PtBr) => {
            "O tipo {type} não tem o campo {field}: um campo que o tipo não declara nunca é lido \
             por nada. Os campos deste tipo são: {fields}. Nada foi gravado."
        }
        ("spec_events.unknown_field", Locale::EnUs) => {
            "Type {type} has no {field} field: a field the type does not declare is never read by \
             anything. This type's fields are: {fields}. Nothing was written."
        }
        ("spec_events.replaces_other_type", Locale::PtBr) => {
            "O evento {id} é do tipo {found}, e a versão nova veio como {type}; ela precisa ser do \
             mesmo tipo. Nada foi gravado."
        }
        ("spec_events.replaces_other_type", Locale::EnUs) => {
            "Event {id} is a {found}, and the new version came as {type}; it must have the same \
             type. Nothing was written."
        }
        ("spec_events.replaces_superseded", Locale::PtBr) => {
            "O evento {id} já foi substituído, e a versão vigente do item é {current}. Uma versão \
             nova substitui só a vigente: releia-a e grave a versão nova por cima dela, com \
             replaces apontando para ela. Nada foi gravado."
        }
        ("spec_events.replaces_superseded", Locale::EnUs) => {
            "Event {id} was already replaced, and the item's current version is {current}. A new \
             version replaces only the current one: read it again and write the new version over \
             it, with replaces pointing at it. Nothing was written."
        }
        ("spec_events.replaces_removed", Locale::PtBr) => {
            "O evento {id} é de uma tarefa que a remoção {by} tirou. Para refazer o trabalho, \
             grave uma tarefa nova, sem replaces. Nada foi gravado."
        }
        ("spec_events.replaces_removed", Locale::EnUs) => {
            "Event {id} belongs to a task that removal {by} took out. To redo the work, write a \
             new task, without replaces. Nothing was written."
        }
        ("spec_events.depends_outside_wave", Locale::PtBr) => {
            "A tarefa vai para a onda {wave}, que ainda não saiu, e depende de {missing}. Essas \
             tarefas não estão nela, numa onda entregue nem numa onda que ela espera. A rodada \
             soltaria a onda antes delas. Grave a tarefa sem wave: o backlog a põe numa onda \
             depois das dependências. Nada foi gravado."
        }
        ("spec_events.depends_outside_wave", Locale::EnUs) => {
            "The task goes to wave {wave}, which has not gone out yet, and depends on {missing}. \
             Those tasks are not in it, in a delivered wave, or in a wave it waits for. The round \
             would send the wave before them. Write the task without wave: the backlog puts it in \
             a wave after its dependencies. Nothing was written."
        }
        ("spec_events.filter_matches_nothing", Locale::PtBr) => {
            "Nenhum evento {type} entre {from} e {to}. Nada foi gravado."
        }
        ("spec_events.filter_matches_nothing", Locale::EnUs) => {
            "No {type} event between {from} and {to}. Nothing was written."
        }
        ("spec_events.unknown_block", Locale::PtBr) => "O bloco {block} não existe. Blocos: {blocks}.",
        ("spec_events.unknown_block", Locale::EnUs) => "There is no {block} block. Blocks: {blocks}.",
        ("spec_events.bad_spec_name", Locale::PtBr) => {
            "{spec} não serve como nome de spec: use um nome sem barra e sem \"..\"."
        }
        ("spec_events.bad_spec_name", Locale::EnUs) => {
            "{spec} cannot name a spec: use a name with no slash and no \"..\"."
        }
        ("spec_events.no_spec_file", Locale::PtBr) => "A spec {spec} ainda não tem arquivo de eventos.",
        ("spec_events.no_spec_file", Locale::EnUs) => "The spec {spec} has no event file yet.",
        ("spec_events.spec_not_open", Locale::PtBr) => {
            "A spec {spec} não foi aberta: não há arquivo de eventos nem estado, e uma gravação não \
             faz a spec nascer. Abra a spec com `mustard-rt run open`, que cria a branch e a spec \
             com o mesmo nome. Nada foi gravado."
        }
        ("spec_events.spec_not_open", Locale::EnUs) => {
            "The spec {spec} was never opened: there is no event file and no state, and a write \
             does not bring a spec into being. Open the spec with `mustard-rt run open`, which \
             creates the branch and the spec with the same name. Nothing was written."
        }
        ("spec_events.phase_change_refused", Locale::PtBr) => {
            "Esta gravação na spec {spec}, da fase {from} para {to}, não passa por esta porta, e \
             nada foi gravado. A aprovação nasce só quando o usuário escolhe \"Aprovar\" na \
             pergunta \"Aprovar esta spec?\", pela testemunha. As fases depois dela, e a branch e \
             a base, só pelo binário."
        }
        ("spec_events.phase_change_refused", Locale::EnUs) => {
            "This write to the spec {spec}, from the phase {from} to {to}, does not go through \
             this door, and nothing was written. The approval is born only when the user chooses \
             \"Approve\" in the question \"Approve this spec?\", through the witness. The phases \
             after it, and the branch and the base, go only through the binary."
        }
        ("spec_events.state_by_flow_only", Locale::PtBr) => {
            "O estado da spec {spec} não é gravado pelo `run write`, e nada foi gravado. Ele é \
             gravado pelos comandos do fluxo e pela testemunha da aprovação, quando o usuário \
             escolhe \"Aprovar\" na pergunta \"Aprovar esta spec?\"."
        }
        ("spec_events.state_by_flow_only", Locale::EnUs) => {
            "The state of the spec {spec} is not written by `run write`, and nothing was written. \
             The flow's commands write it, and so does the approval witness, when the user \
             chooses \"Approve\" in the question \"Approve this spec?\"."
        }
        ("spec_events.binary_only_type", Locale::PtBr) => {
            "O tipo {type} da spec {spec} não é gravado pelo `run write`, nem tirado ou revisto por \
             ele. Nada foi gravado. O binário grava a execução dos critérios no fechamento, o \
             veredito oficial, o envio, a entrega oficial de cada onda e o commit pela rodada. Ele \
             grava também a resposta do assistente, no fim de cada resposta. A onda grava só a \
             própria volta, com `run write delivered`, enquanto o envio dela está aberto. O revisor \
             grava só o próprio veredito, com `run write verdict`, enquanto o pedido de revisão \
             está aberto. Fora disso, a rodada ou o fechamento assume a volta."
        }
        ("spec_events.binary_only_type", Locale::EnUs) => {
            "The type {type} of the spec {spec} is not written, removed or revised by `run write`. \
             Nothing was written. The binary writes the criteria runs at the close, the official \
             verdict, the send, each wave's official delivery and the commit through the round. It \
             also writes the assistant's response at the end of each answer. A wave writes only \
             its own return, with `run write delivered`, while its send is open. The reviewer \
             writes only its own verdict, with `run write verdict`, while the review request is \
             open. Otherwise, the round or the close takes the return over."
        }
        ("spec_events.user_message_by_hook", Locale::PtBr) => {
            "Na spec {spec}, o `run write` não grava, não tira e não revê dois tipos de mensagem. Um \
             é a resposta a uma pergunta com opções: a mensagem com `witness`, de qualquer autor. O \
             outro é a fala do usuário, que só o gancho grava. Nada foi gravado: a resposta chega \
             pela testemunha, e a fala, pelo gancho da entrada."
        }
        ("spec_events.user_message_by_hook", Locale::EnUs) => {
            "In the spec {spec}, `run write` does not write, remove or revise two kinds of message. \
             One is the answer to a question with options: the message with `witness`, from any \
             author. The other is the user's speech, which only the hook records. Nothing was \
             written: the answer arrives through the witness, and the speech, through the entry \
             hook."
        }
        ("spec_events.binary_author", Locale::PtBr) => {
            "O autor `binary` fica para as gravações de dentro do binário, e nada foi gravado: o \
             `run write` grava com o autor de quem escreve, `assistant` (o padrão) ou `user`."
        }
        ("spec_events.binary_author", Locale::EnUs) => {
            "The author `binary` is kept for the writes made inside the binary, and nothing was \
             written: `run write` records the author who writes, `assistant` (the default) or `user`."
        }
        ("spec_events.wave_by_backlog", Locale::PtBr) => {
            "A onda não é gravada pelo `run write`, e nada foi gravado. A onda nasce do backlog, e é \
             o programa que monta o lote na hora de despachar. Grave só a tarefa, sem `wave`, com o \
             que ela faz (`text`), os arquivos (`files`) e as tarefas de que depende \
             (`depends_on`). Na versão nova de uma tarefa que já está numa onda, repita o `wave` da \
             versão que ela substitui."
        }
        ("spec_events.wave_by_backlog", Locale::EnUs) => {
            "A wave is not written by `run write`, and nothing was written. The wave is born from \
             the backlog, and the program puts the batch together when it dispatches. Write only \
             the task, without `wave`, with what it does (`text`), the files (`files`) and the \
             tasks it depends on (`depends_on`). In the new version of a task that is already in a \
             wave, repeat the `wave` of the version it replaces."
        }
        ("spec_events.request_on_closed_spec", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}: ela já fechou, e o pedido novo só entra nela \
             depois de reaberta. Rode `mustard-rt run reopen --spec {spec} --reason \"<motivo>\"`: \
             ela volta à execução, já aprovada, na mesma branch e com o mesmo pull request; depois \
             grave o pedido de novo. Nada foi gravado."
        }
        ("spec_events.request_on_closed_spec", Locale::EnUs) => {
            "The spec {spec} is in the {phase} phase: it has already closed, and a new request only \
             goes into it after it is reopened. Run `mustard-rt run reopen --spec {spec} --reason \
             \"<reason>\"`: it goes back to running, already approved, on the same branch and with \
             the same pull request; then write the request again. Nothing was written."
        }
        ("spec_events.work_on_finished_spec", Locale::PtBr) => {
            "A spec {spec} está na fase {phase}, e o `run write {type}` não grava nela. A spec \
             entregue na base, com o merge feito, e a descartada não voltam. Pedido novo sobre ela \
             é obra nova: abra uma spec nova com `mustard-rt run open` e grave nela. Nada foi \
             gravado."
        }
        ("spec_events.work_on_finished_spec", Locale::EnUs) => {
            "The spec {spec} is in the {phase} phase, and `run write {type}` does not write to it. \
             A spec delivered to the base, already merged, and a discarded one do not come back. A \
             new request on it is new work: open a new spec with `mustard-rt run open` and write \
             there. Nothing was written."
        }
        ("spec_events.old_format_spec", Locale::PtBr) => {
            "A spec {spec} está no formato antigo: o `spec.md` dela traz a seção \"Critérios de \
             Aceitação\", ou a pasta tem `meta.json` e nenhum `spec.ndjson`. O binário não grava \
             nela. Abra uma spec nova com `mustard-rt run open`. Nada foi gravado."
        }
        ("spec_events.old_format_spec", Locale::EnUs) => {
            "Spec {spec} is in the old format: its `spec.md` carries the \"Acceptance Criteria\" \
             section, or the folder has a `meta.json` and no `spec.ndjson`. The binary does not \
             write to it. Open a new spec with `mustard-rt run open`. Nothing was written."
        }
        ("spec_events.io_failed", Locale::PtBr) => "Não consegui usar o arquivo da spec: {detail}.",
        ("spec_events.io_failed", Locale::EnUs) => "Could not use the spec file: {detail}.",
        ("spec_events.skipped_line", Locale::PtBr) => {
            "A linha {line} do spec.ndjson não se entende e foi pulada; o resto do arquivo foi lido."
        }
        ("spec_events.skipped_line", Locale::EnUs) => {
            "Line {line} of spec.ndjson could not be understood and was skipped; the rest of the \
             file was read."
        }
        ("spec_events.duplicate_id", Locale::PtBr) => {
            "A linha {line} do spec.ndjson repete o número {id}, que já apareceu antes, e foi \
             pulada."
        }
        ("spec_events.duplicate_id", Locale::EnUs) => {
            "Line {line} of spec.ndjson repeats number {id}, already used above, and was skipped."
        }
        ("spec_events.kind.text", Locale::PtBr) => "um texto",
        ("spec_events.kind.text", Locale::EnUs) => "a text",
        ("spec_events.kind.int", Locale::PtBr) => "um número inteiro",
        ("spec_events.kind.int", Locale::EnUs) => "a whole number",
        ("spec_events.kind.bool", Locale::PtBr) => "true ou false",
        ("spec_events.kind.bool", Locale::EnUs) => "true or false",
        ("spec_events.kind.object", Locale::PtBr) => "um objeto JSON",
        ("spec_events.kind.object", Locale::EnUs) => "a JSON object",
        ("spec_events.kind.ints", Locale::PtBr) => "uma lista de números inteiros",
        ("spec_events.kind.ints", Locale::EnUs) => "a list of whole numbers",
        ("spec_events.kind.texts", Locale::PtBr) => "uma lista de textos",
        ("spec_events.kind.texts", Locale::EnUs) => "a list of texts",
        ("spec_events.kind.objects", Locale::PtBr) => "uma lista de objetos JSON",
        ("spec_events.kind.objects", Locale::EnUs) => "a list of JSON objects",
        ("spec_events.kind.list", Locale::PtBr) => "uma lista",
        ("spec_events.kind.list", Locale::EnUs) => "a list",
        ("spec_events.kind.one_of", Locale::PtBr) => "uma destas palavras: {values}",
        ("spec_events.kind.one_of", Locale::EnUs) => "one of these words: {values}",
        ("spec_events.kind.many_of", Locale::PtBr) => "uma lista só com estas palavras: {values}",
        ("spec_events.kind.many_of", Locale::EnUs) => "a list with only these words: {values}",
        ("spec_events.kind.one_of_numbers", Locale::PtBr) => "um destes números: {values}",
        ("spec_events.kind.one_of_numbers", Locale::EnUs) => "one of these numbers: {values}",
        ("spec_events.kind.text_or_object", Locale::PtBr) => "um texto ou um objeto JSON",
        ("spec_events.kind.text_or_object", Locale::EnUs) => "a text or a JSON object",
        ("spec_events.kind.time", Locale::PtBr) => "uma data e hora como 2026-09-11T21:03",
        ("spec_events.kind.time", Locale::EnUs) => "a date and time like 2026-09-11T21:03",
        ("spec_events.kind.ref", Locale::PtBr) => {
            "o número de um evento ou o código de um item, como `MSTD-RULE-0002`"
        }
        ("spec_events.kind.ref", Locale::EnUs) => "an event number or an item code, like `MSTD-RULE-0002`",
        ("spec_events.kind.refs", Locale::PtBr) => {
            "uma lista de números de evento ou de códigos de item, como `MSTD-RULE-0002`"
        }
        ("spec_events.kind.refs", Locale::EnUs) => {
            "a list of event numbers or item codes, like `MSTD-RULE-0002`"
        }

        // O índice das specs (`io::spec_index`, o comando `run index` e a
        // conferência do `doctor`). As vagas vêm do chamador.
        ("spec_index.write_warning", Locale::PtBr) => {
            "O evento foi gravado, mas a linha da spec no índice não foi refeita: {detail}. Rode \
             `mustard-rt run index` para refazer o índice."
        }
        ("spec_index.write_warning", Locale::EnUs) => {
            "The event was written, but the spec's line in the index was not rebuilt: {detail}. \
             Run `mustard-rt run index` to rebuild the index."
        }
        ("spec_index.missing", Locale::PtBr) => {
            "O índice das specs (.claude/spec/index.ndjson) não existe, e há {count} spec(s) com \
             arquivo de eventos. Rode `mustard-rt run index` para refazê-lo."
        }
        ("spec_index.missing", Locale::EnUs) => {
            "The spec index (.claude/spec/index.ndjson) does not exist, and {count} spec(s) have an \
             event file. Run `mustard-rt run index` to rebuild it."
        }
        ("spec_index.diverged", Locale::PtBr) => {
            "O índice das specs difere dos arquivos de eventos em {count} linha(s): {specs}. Rode \
             `mustard-rt run index` para refazê-lo."
        }
        ("spec_index.diverged", Locale::EnUs) => {
            "The spec index differs from the event files in {count} line(s): {specs}. Run \
             `mustard-rt run index` to rebuild it."
        }
        ("spec_index.stale_search", Locale::PtBr) => {
            "{count} linha(s) dos arquivos de eventos e do banco de lições estão sem o campo \
             search. Rode `mustard-rt run index` para pô-lo."
        }
        ("spec_index.stale_search", Locale::EnUs) => {
            "{count} line(s) of the event files and the lesson bank have no search field. Run \
             `mustard-rt run index` to fill it."
        }
        ("spec_events.spec_required", Locale::PtBr) => {
            "Falta a spec: o tipo {type} é gravado no arquivo de eventos de uma spec. Passe \
             `--spec <nome>`. Nada foi gravado."
        }
        ("spec_events.spec_required", Locale::EnUs) => {
            "The spec is missing: a {type} event is written to a spec's event file. Pass \
             `--spec <name>`. Nothing was written."
        }
        ("spec_events.no_current_spec", Locale::PtBr) => {
            "Nenhuma spec atual: nem `MUSTARD_ACTIVE_SPEC`, nem a branch, nem a sessão apontam \
             uma spec. Passe `--spec <nome>`."
        }
        ("spec_events.no_current_spec", Locale::EnUs) => {
            "No current spec: neither `MUSTARD_ACTIVE_SPEC`, the branch nor the session names \
             one. Pass `--spec <name>`."
        }

        // Refusals of the lessons bank (`domain::lessons`, `run write lesson`).
        // The slots come from the caller.
        ("lessons.unknown_lesson", Locale::PtBr) => {
            "A lição {id} não existe no banco de lições. Nada foi gravado."
        }
        ("lessons.unknown_lesson", Locale::EnUs) => {
            "Lesson {id} does not exist in the lesson bank. Nothing was written."
        }
        ("lessons.origin_missing", Locale::PtBr) => {
            "A lição precisa dizer onde nasceu, em found_in: `spec`, `branch` e `commit`, ou \
             `source` (o arquivo de onde ela veio). Nada foi gravado."
        }
        ("lessons.origin_missing", Locale::EnUs) => {
            "The lesson must say where it was born, in found_in: `spec`, `branch` and `commit`, \
             or `source` (the file it came from). Nothing was written."
        }
        ("lessons.repeated", Locale::PtBr) => {
            "O texto desta lição repete o da lição {id}, já guardada no banco: \"{text}\". Para \
             mudar a que existe, grave a versão nova com `\"replaces\": {id}`. Nada foi gravado."
        }
        ("lessons.repeated", Locale::EnUs) => {
            "The text of this lesson repeats the one of lesson {id}, already in the bank: \"{text}\". \
             To change the one that exists, write its new version with `\"replaces\": {id}`. \
             Nothing was written."
        }
        ("lessons.too_long", Locale::PtBr) => {
            "A lição tem {lines} linhas, e o teto é {max}. Resuma o texto e grave de novo. Nada foi \
             gravado."
        }
        ("lessons.too_long", Locale::EnUs) => {
            "The lesson has {lines} lines, and the limit is {max}. Shorten the text and write it \
             again. Nothing was written."
        }
        ("lessons.unclear", Locale::PtBr) => {
            "A lição é um resumo do assistente no jeito de escrever do projeto, e a conferência de \
             escrita do fim da resposta achou: {defects}. Reescreva o texto e grave de novo. Nada \
             foi gravado."
        }
        ("lessons.unclear", Locale::EnUs) => {
            "A lesson is the assistant's summary, written the project's way, and the writing check \
             of the end of a response found: {defects}. Rewrite the text and write it again. \
             Nothing was written."
        }
        // A lição que vale para o projeto (o defeito e a regra do projeto) não
        // entra no banco, que não vai ao git: vira ajuste no código, com teste.
        // Com uma spec aberta, a recusa manda gravar a tarefa nela; sem spec
        // aberta, a recusa não fala em spec.
        ("lessons.defect_by_task", Locale::PtBr) => {
            "O defeito que pode se repetir não entra no banco de lições, que fica só nesta máquina \
             e não vai ao git. Ele vira conserto no código, com o teste que falha se o defeito \
             voltar. O teste vai ao git no commit da obra. Grave a tarefa desse conserto com \
             `mustard-rt run write task`, na spec {spec}. Nada foi gravado."
        }
        ("lessons.defect_by_task", Locale::EnUs) => {
            "A defect that can happen again does not go in the lesson bank, which stays on this \
             machine only and never goes to git. It becomes a fix in the code, with the test that \
             fails if the defect comes back. The test goes to git in the commit of the work. Write \
             the task of that fix with `mustard-rt run write task`, in the spec {spec}. Nothing \
             was written."
        }
        ("lessons.defect_in_code", Locale::PtBr) => {
            "O defeito que pode se repetir não entra no banco de lições, que fica só nesta máquina \
             e não vai ao git. Ele vira conserto no código, com o teste que falha se o defeito \
             voltar. O teste vai ao git no commit. Nada foi gravado."
        }
        ("lessons.defect_in_code", Locale::EnUs) => {
            "A defect that can happen again does not go in the lesson bank, which stays on this \
             machine only and never goes to git. It becomes a fix in the code, with the test that \
             fails if the defect comes back. The test goes to git in the commit. Nothing was \
             written."
        }
        ("lessons.rule_by_task", Locale::PtBr) => {
            "A regra do projeto não entra no banco de lições, que fica só nesta máquina e não vai \
             ao git. Em outra máquina, ela some. Ela vira teste no código da obra, que falha se a \
             regra for quebrada. O teste vai ao git no commit da obra. Grave a tarefa desse teste \
             com `mustard-rt run write task`, na spec {spec}. Nada foi gravado."
        }
        ("lessons.rule_by_task", Locale::EnUs) => {
            "A project rule does not go in the lesson bank, which stays on this machine only and \
             never goes to git. On another machine, it is gone. It becomes a test in the code of \
             the work, one that fails if the rule is broken. The test goes to git in the commit of \
             the work. Write the task of that test with `mustard-rt run write task`, in the spec \
             {spec}. Nothing was written."
        }
        ("lessons.rule_in_code", Locale::PtBr) => {
            "A regra do projeto não entra no banco de lições, que fica só nesta máquina e não vai \
             ao git. Em outra máquina, ela some. Ela vira teste no código, que falha se a regra \
             for quebrada. O teste vai ao git no commit. Nada foi gravado."
        }
        ("lessons.rule_in_code", Locale::EnUs) => {
            "A project rule does not go in the lesson bank, which stays on this machine only and \
             never goes to git. On another machine, it is gone. It becomes a test in the code, one \
             that fails if the rule is broken. The test goes to git in the commit. Nothing was \
             written."
        }
        // As regras que a limpeza da instalação tira dos arquivos de instrução
        // viram um item só da lista de pendências, com o texto de cada uma e o
        // arquivo de onde saiu.
        ("lessons.rules_left.title", Locale::PtBr) => {
            "Regras que a instalação tirou dos arquivos de instrução ({count}): cada uma vira \
             teste ou sai"
        }
        ("lessons.rules_left.title", Locale::EnUs) => {
            "Rules the install took out of the instruction files ({count}): each becomes a test or \
             goes"
        }
        ("lessons.rules_left.detail", Locale::PtBr) => {
            "Um Mustard antigo escreveu estas regras entre as marcas dele nos arquivos de \
             instrução, e a instalação as tirou de lá. Elas não voltam para o arquivo nem vão ao \
             banco de lições, que não vai ao git. Transforme cada uma num teste no código, que \
             falha se a regra for quebrada, ou descarte a que já não vale. {rules}"
        }
        ("lessons.rules_left.detail", Locale::EnUs) => {
            "An older Mustard wrote these rules between its marks in the instruction files, and \
             the install took them out. They go back neither to the file nor to the lesson bank, \
             which never goes to git. Turn each one into a test in the code, one that fails if \
             the rule is broken, or drop the one that no longer holds. {rules}"
        }
        ("lessons.rules_left.rule", Locale::PtBr) => "{n}) {text} (saiu de {sources})",
        ("lessons.rules_left.rule", Locale::EnUs) => "{n}) {text} (taken from {sources})",
        // O que o scan aponta para enxugar o banco de lições (`run scan`).
        ("lessons.scan_merge", Locale::PtBr) => {
            "Junte cada grupo de lições parecidas numa lição só, resumida no jeito de escrever do \
             projeto. Grave-a com `mustard-rt run write lesson`, com `\"replaces\"` apontando as \
             lições do grupo: {groups}."
        }
        ("lessons.scan_merge", Locale::EnUs) => {
            "Merge each group of similar lessons into one lesson, summarized the project's way. \
             Write it with `mustard-rt run write lesson`, with `\"replaces\"` naming the group's \
             lessons: {groups}."
        }
        ("lessons.scan_retire", Locale::PtBr) => {
            "Retire as lições que já não valem, porque citam um caminho que o projeto já não tem, \
             com `mustard-rt run write lesson --json '{\"targets\":[…],\"reason\":\"…\"}'`: \
             {lessons}."
        }
        ("lessons.scan_retire", Locale::EnUs) => {
            "Retire the lessons that no longer hold, because they cite a path the project no \
             longer has, with `mustard-rt run write lesson --json '{\"targets\":[…],\"reason\":\"…\"}'`: \
             {lessons}."
        }
        ("lessons.scan_untouched", Locale::PtBr) => "O scan não mudou o banco de lições.",
        ("lessons.scan_untouched", Locale::EnUs) => "The scan did not change the lesson bank.",
        ("spec_index.no_specs", Locale::PtBr) => {
            "Nenhuma spec tem arquivo de eventos: não há índice a conferir."
        }
        ("spec_index.no_specs", Locale::EnUs) => "No spec has an event file: there is no index to check.",
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
            include_str!("events.rs"),
            super::PREFIXES,
            118,
            0xe610_ff7c_9597_4d85,
        );
    }

    /// As recusas e os avisos do arquivo de eventos da spec saem do catálogo
    /// nos dois idiomas, cada um com as vagas que o chamador preenche.
    #[test]
    fn i18n_translates_spec_event_keys() {
        for (key, slots) in [
            ("spec_events.not_an_object", &["{detail}"][..]),
            ("spec_events.unknown_type", &["{type}", "{types}"][..]),
            ("spec_events.missing_field", &["{type}", "{field}"][..]),
            ("spec_events.task_declaration_missing", &["{missing}"][..]),
            ("spec_events.task_declaration_what", &[][..]),
            ("spec_events.task_declaration_files", &[][..]),
            ("spec_events.task_declaration_depends_on", &[][..]),
            ("spec_events.task_declaration_title", &[][..]),
            ("spec_events.task_declaration_covers", &[][..]),
            ("spec_events.task_uncovered_items", &["{items}"][..]),
            ("spec_events.task_uncovered_more", &["{n}"][..]),
            ("spec_events.item_form_missing", &["{missing}"][..]),
            ("spec_events.item_part_title", &[][..]),
            ("spec_events.item_part_user", &[][..]),
            ("spec_events.item_part_agent", &[][..]),
            ("spec_events.item_part_user_cites", &["{found}"][..]),
            ("spec_events.item_unclear", &["{defects}"][..]),
            ("spec_events.task_depends_on_unknown", &["{task}", "{depends_on}"][..]),
            ("spec_events.task_dependency_cycle", &["{cycle}"][..]),
            ("spec_events.agreed_items_missing", &["{missing}"][..]),
            ("spec_events.delivery_agreed_missing", &["{wave}", "{missing}"][..]),
            ("spec_events.criterion_form_missing", &[][..]),
            ("spec_events.proof_not_a_command", &["{criterion}", "{found}"][..]),
            ("spec_events.proof_program_unknown", &["{term}"][..]),
            ("spec_events.proof_chained_by_semicolon", &["{found}"][..]),
            ("spec_events.proof_search_not_negated", &["{found}"][..]),
            ("spec_events.invalid_value", &["{type}", "{field}", "{expected}"][..]),
            ("spec_events.wrong_count", &["{type}", "{field}", "{min}", "{max}", "{count}"][..]),
            ("spec_events.fact_without_source", &["{fact}"][..]),
            ("spec_events.cited_file_missing", &["{fact}", "{path}"][..]),
            ("spec_events.cited_line_missing", &["{fact}", "{path}", "{line}", "{lines}"][..]),
            ("spec_events.name_elsewhere", &["{fact}", "{name}", "{path}", "{found}"][..]),
            ("spec_events.name_unknown", &["{fact}", "{name}"][..]),
            ("spec_events.names_unchecked", &[][..]),
            ("spec_events.goal_origin_not_user", &["{spec}", "{origin}"][..]),
            ("spec_events.survey_open", &["{spec}", "{count}", "{points}"][..]),
            ("spec_events.survey_not_started", &["{spec}"][..]),
            ("spec_events.survey_gaps_unrecorded", &["{spec}", "{count}", "{gaps}"][..]),
            ("spec_events.point_already_open", &["{code}", "{block}"][..]),
            ("spec_events.point_not_open", &["{id}", "{open}"][..]),
            ("spec_events.closing_point_open", &[][..]),
            ("spec_events.not_applicable_reason", &[][..]),
            ("spec_events.open_point_removed", &["{code}"][..]),
            ("spec_events.purge_excerpt_not_found", &["{code}"][..]),
            ("spec_events.closing_point_last_record", &["{code}"][..]),
            ("spec_events.delivered_too_long", &["{chars}", "{max}"][..]),
            ("spec_events.no_open_send", &["{wave}"][..]),
            ("spec_events.no_open_review", &[][..]),
            ("spec_events.verdict_missing", &[][..]),
            ("spec_events.leftover_field_missing", &["{field}"][..]),
            ("spec_events.report_carries_return_line", &[][..]),
            ("spec_events.return_missing", &["{wave}"][..]),
            ("spec_events.return_needs_commit", &["{wave}"][..]),
            ("approve_spec.open_points", &["{count}", "{points}"][..]),
            ("spec_events.deferred_unknown_pending", &["{pending}"][..]),
            ("spec_events.deferred_closed_pending", &["{pending}"][..]),
            ("request.new_waves", &[][..]),
            ("request.adjust_waves", &[][..]),
            ("spec_events.unknown_target", &["{id}"][..]),
            ("spec_events.unknown_code", &["{code}"][..]),
            ("spec_events.target_other_type", &["{item}", "{found}", "{expected}"][..]),
            ("spec_events.binary_only_field", &["{field}"][..]),
            ("spec_events.unknown_field", &["{type}", "{field}", "{fields}"][..]),
            ("spec_events.replaces_other_type", &["{id}", "{found}", "{type}"][..]),
            ("spec_events.replaces_superseded", &["{id}", "{current}"][..]),
            ("spec_events.replaces_removed", &["{id}", "{by}"][..]),
            ("spec_events.depends_outside_wave", &["{wave}", "{missing}"][..]),
            ("spec_events.filter_matches_nothing", &["{type}", "{from}", "{to}"][..]),
            ("spec_events.unknown_block", &["{block}", "{blocks}"][..]),
            ("spec_events.bad_spec_name", &["{spec}"][..]),
            ("spec_events.no_spec_file", &["{spec}"][..]),
            ("spec_events.spec_not_open", &["{spec}"][..]),
            ("spec_events.phase_change_refused", &["{spec}", "{from}", "{to}"][..]),
            ("spec_events.state_by_flow_only", &["{spec}"][..]),
            ("spec_events.binary_only_type", &["{type}", "{spec}"][..]),
            ("spec_events.binary_author", &[][..]),
            ("spec_events.wave_by_backlog", &[][..]),
            ("spec_events.request_on_closed_spec", &["{spec}", "{phase}"][..]),
            ("spec_events.work_on_finished_spec", &["{spec}", "{phase}", "{type}"][..]),
            ("spec_events.user_message_by_hook", &["{spec}"][..]),
            ("spec_events.old_format_spec", &["{spec}"][..]),
            ("spec_events.no_current_spec", &[][..]),
            ("spec_events.io_failed", &["{detail}"][..]),
            ("spec_events.skipped_line", &["{line}"][..]),
            ("spec_events.duplicate_id", &["{line}", "{id}"][..]),
            ("spec_events.kind.text", &[][..]),
            ("spec_events.kind.int", &[][..]),
            ("spec_events.kind.bool", &[][..]),
            ("spec_events.kind.object", &[][..]),
            ("spec_events.kind.ints", &[][..]),
            ("spec_events.kind.texts", &[][..]),
            ("spec_events.kind.objects", &[][..]),
            ("spec_events.kind.list", &[][..]),
            ("spec_events.kind.one_of", &["{values}"][..]),
            ("spec_events.kind.many_of", &["{values}"][..]),
            ("spec_events.kind.one_of_numbers", &["{values}"][..]),
            ("spec_events.kind.text_or_object", &[][..]),
            ("spec_events.kind.time", &[][..]),
            ("spec_events.kind.ref", &[][..]),
            ("spec_events.kind.refs", &[][..]),
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

    /// The spec index advisories and the lessons bank refusals come from the
    /// catalog in both languages, each with the slots the caller fills.
    #[test]
    fn i18n_translates_spec_index_and_lesson_keys() {
        for (key, slots) in [
            ("spec_index.write_warning", &["{detail}"][..]),
            ("spec_index.missing", &["{count}"][..]),
            ("spec_index.diverged", &["{count}", "{specs}"][..]),
            ("spec_index.stale_search", &["{count}"][..]),
            ("spec_index.no_specs", &[][..]),
            ("spec_events.spec_required", &["{type}"][..]),
            ("lessons.unknown_lesson", &["{id}"][..]),
            ("lessons.origin_missing", &[][..]),
            ("lessons.repeated", &["{id}", "{text}"][..]),
            ("lessons.too_long", &["{lines}", "{max}"][..]),
            ("lessons.unclear", &["{defects}"][..]),
            ("lessons.defect_by_task", &["{spec}"][..]),
            ("lessons.defect_in_code", &[][..]),
            ("lessons.rule_by_task", &["{spec}"][..]),
            ("lessons.rule_in_code", &[][..]),
            ("lessons.rules_left.title", &["{count}"][..]),
            ("lessons.rules_left.detail", &["{rules}"][..]),
            ("lessons.rules_left.rule", &["{n}", "{text}", "{sources}"][..]),
            ("lessons.scan_merge", &["{groups}"][..]),
            ("lessons.scan_retire", &["{lessons}"][..]),
            ("lessons.scan_untouched", &[][..]),
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

    /// A recusa da versão nova de uma tarefa removida passa na conferência de
    /// escrita das respostas, nos dois idiomas, com cada vaga trocada por uma
    /// palavra.
    #[test]
    fn the_removed_task_refusal_reads_clearly() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = translate("spec_events.replaces_removed", lang).replace("{id}", "12").replace("{by}", "15");
            let report = crate::domain::clarity::measure(&text, &[], Some(lang));
            assert!(report.passed, "{lang:?}: {report:?}");
        }
    }

    /// A recusa da tarefa que depende de outra fora da onda dela passa na
    /// conferência de escrita das respostas, nos dois idiomas, com cada vaga
    /// trocada por uma palavra.
    #[test]
    fn the_dependency_outside_the_wave_refusal_reads_clearly() {
        for (lang, word) in [(Locale::PtBr, "tarefas"), (Locale::EnUs, "tasks")] {
            let text = translate("spec_events.depends_outside_wave", lang).replace("{wave}", "3").replace("{missing}", word);
            let report = crate::domain::clarity::measure(&text, &[], Some(lang));
            assert!(report.passed, "{lang:?}: {report:?}");
        }
    }

    /// A recusa da lição acima do teto de linhas passa na conferência de
    /// escrita das respostas, nos dois idiomas, com cada vaga trocada por uma
    /// palavra.
    #[test]
    fn the_lesson_line_limit_refusal_reads_clearly() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = translate("lessons.too_long", lang).replace("{lines}", "16").replace("{max}", "15");
            let report = crate::domain::clarity::measure(&text, &[], Some(lang));
            assert!(report.passed, "{lang:?}: {report:?}");
        }
    }
}
