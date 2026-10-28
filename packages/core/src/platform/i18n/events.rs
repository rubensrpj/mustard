//! O arquivo de eventos da spec: as recusas e os avisos da gravação e da
//! leitura, o banco de lições, o índice das specs e o achado que vira tarefa
//! da spec.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::flow::{map_commands_en, map_commands_pt};
use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["spec_events", "lessons", "spec_index", "failures", "skills", "finding"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Recusas e avisos do arquivo de eventos da spec (`domain::spec_events`,
        // comandos `run write` e `run read`). As vagas vêm do chamador.
        ("spec_events.not_an_object", Locale::PtBr) => {
            "Os campos do evento precisam vir num objeto JSON, como {\"text\": \"…\"}, e o que \
             veio não serve: {detail}. Nada foi gravado. Texto longo vai cru pela entrada padrão, \
             com `--text -` ou `--agent -`. Várias gravações vão juntas no `run write --batch`, \
             uma linha JSON por item."
        }
        ("spec_events.not_an_object", Locale::EnUs) => {
            "The event's fields must come as one JSON object, like {\"text\": \"…\"}, and what \
             came does not parse: {detail}. Nothing was written. Long text goes raw through \
             standard input, with `--text -` or `--agent -`. Many writes go together through \
             `run write --batch`, one JSON line per item."
        }
        // A entrada crua e o lote do `run write` e do `run answer`.
        ("spec_events.raw_input_twice", Locale::PtBr) => {
            "Só um campo lê a entrada padrão: `--text -` ou `--agent -`. Dê o outro direto na \
             opção ou no `--json`. Nada foi gravado."
        }
        ("spec_events.raw_input_twice", Locale::EnUs) => {
            "Only one field reads standard input: `--text -` or `--agent -`. Give the other one \
             straight in its option or in `--json`. Nothing was written."
        }
        ("spec_events.raw_field_twice", Locale::PtBr) => {
            "O campo {field} veio no `--json` e na opção própria. Mande só um dos dois. Nada foi \
             gravado."
        }
        ("spec_events.raw_field_twice", Locale::EnUs) => {
            "The {field} field came both in `--json` and in its own option. Send only one of \
             them. Nothing was written."
        }
        ("spec_events.batch_empty", Locale::PtBr) => {
            "O lote chegou vazio. Mande pela entrada padrão uma linha JSON por item, cada uma com \
             o tipo em `type`."
        }
        ("spec_events.batch_empty", Locale::EnUs) => {
            "The batch came empty. Send one JSON line per item through standard input, each with \
             its type in `type`."
        }
        ("spec_events.batch_type_missing", Locale::PtBr) => {
            "Cada linha do lote traz o tipo do evento em `type`, como {\"type\": \"task\", …}. \
             Nada desta linha foi gravado."
        }
        ("spec_events.batch_type_missing", Locale::EnUs) => {
            "Each batch line carries the event type in `type`, like {\"type\": \"task\", …}. \
             Nothing from this line was written."
        }
        ("spec_events.batch_line_refused", Locale::PtBr) => {
            "Esta linha cita pelo rótulo {label} a linha {line} do lote, que não foi gravada. \
             Nada desta linha foi gravado. Corrija a linha {line} e mande as duas de novo."
        }
        ("spec_events.batch_line_refused", Locale::EnUs) => {
            "This line cites by the label {label} line {line} of the batch, which was not \
             written. Nothing from this line was written. Fix line {line} and send both again."
        }
        ("spec_events.unknown_type", Locale::PtBr) => {
            "O tipo {type} não existe no arquivo da spec. Nada foi gravado. Tipos aceitos: {types}."
        }
        ("spec_events.unknown_type", Locale::EnUs) => {
            "The spec file has no {type} event type. Nothing was written. Accepted types: {types}."
        }
        // A procura na spec (`run find`) e a leitura de uma onda de dentro da
        // cópia, que saem em texto.
        ("spec_events.find_more", Locale::PtBr) => "{shown} de {total}. Use mais uma palavra ou o filtro por tipo.",
        ("spec_events.find_more", Locale::EnUs) => "{shown} of {total}. Add a word or the type filter.",
        ("spec_events.find_none", Locale::PtBr) => "Não achei.",
        ("spec_events.find_none", Locale::EnUs) => "Nothing found.",
        ("spec_events.find_jev_off", Locale::PtBr) => "Jev fora: {reason}.",
        ("spec_events.find_jev_off", Locale::EnUs) => "Jev off: {reason}.",
        // Por que o Jev ficou fora da procura: o desligado diz o ajuste do
        // mustard.json que o liga.
        ("spec_events.find_off_test", Locale::PtBr) => "o programa de teste não chama o serviço",
        ("spec_events.find_off_test", Locale::EnUs) => "the test program does not call the service",
        ("spec_events.find_off_fallback", Locale::PtBr) => {
            "o Jev está desligado no projeto; para ligar, ponha true em ai.fallback no mustard.json"
        }
        ("spec_events.find_off_fallback", Locale::EnUs) => "Jev is off in the project; to turn it on, set ai.fallback to true in mustard.json",
        ("spec_events.find_off_purpose", Locale::PtBr) => {
            "a busca na spec está desligada; para ligar, ponha jev em judgement.{purpose}.filter no mustard.json"
        }
        ("spec_events.find_off_purpose", Locale::EnUs) => {
            "the spec search is off; to turn it on, set judgement.{purpose}.filter to jev in mustard.json"
        }
        ("spec_events.find_no_key", Locale::PtBr) => "sem chave",
        ("spec_events.find_no_key", Locale::EnUs) => "no key",
        ("spec_events.find_key_in_git", Locale::PtBr) => "a chave em jev.key não vale, porque o git guarda o mustard.json ou não o ignora",
        ("spec_events.find_key_in_git", Locale::EnUs) => "the key in jev.key does not count, because git tracks mustard.json or does not ignore it",
        // A procura que pediu a escolha ao Jev e ficou sem ela: ele falhou ou
        // passou do prazo. A escolha fica com quem procurou.
        ("spec_events.find_choice_left", Locale::PtBr) => "O Jev não escolheu a tempo: a escolha é sua, entre os itens acima.",
        ("spec_events.find_choice_left", Locale::EnUs) => "Jev did not choose in time: the choice is yours, among the items above.",
        ("spec_events.find_partial", Locale::PtBr) => "parcial",
        ("spec_events.find_partial", Locale::EnUs) => "partial",
        ("spec_events.find_user_message", Locale::PtBr) => "Mensagem do usuário",
        ("spec_events.find_user_message", Locale::EnUs) => "User message",
        ("spec_events.find_lesson", Locale::PtBr) => "Lição",
        ("spec_events.find_lesson", Locale::EnUs) => "Lesson",
        ("spec_events.find_pasted", Locale::PtBr) => "[texto colado, {kb} kB: leia por item-{code}]",
        ("spec_events.find_pasted", Locale::EnUs) => "[pasted text, {kb} kB: read item-{code}]",
        ("spec_events.find_files", Locale::PtBr) => "Arquivos: {files}",
        ("spec_events.find_files", Locale::EnUs) => "Files: {files}",
        // A leitura de quem conduz (`run read` sem `--all`): o teto do bloco,
        // a lista só das ondas abertas e a busca no painel em resumo.
        ("spec_events.read_more", Locale::PtBr) => "{shown} de {total}, os mais novos primeiro. Todos: --all.",
        ("spec_events.read_more", Locale::EnUs) => "{shown} of {total}, newest first. All of them: --all.",
        ("spec_events.read_open_waves", Locale::PtBr) => "{shown} de {total} ondas: só as abertas. Todas: --all.",
        ("spec_events.read_open_waves", Locale::EnUs) => "{shown} of {total} waves: only the open ones. All of them: --all.",
        ("spec_events.read_summary", Locale::PtBr) => {
            "Resumo por tipo do que o termo achou. Um item: item- e o código. Todos inteiros: --all."
        }
        ("spec_events.read_summary", Locale::EnUs) => {
            "Summary by type of what the term found. One item: item- plus its code. All of them whole: --all."
        }
        ("spec_events.no_wave_request", Locale::PtBr) => {
            "A onda {wave} não tem pedido gravado: ela ainda não saiu. Confira o número na wave-list."
        }
        ("spec_events.no_wave_request", Locale::EnUs) => {
            "Wave {wave} has no recorded request: it has not gone out yet. Check the number in the wave-list."
        }
        ("spec_events.no_review_request", Locale::PtBr) => {
            "O revisor final não tem pedido gravado: o fechamento ainda não o chamou."
        }
        ("spec_events.no_review_request", Locale::EnUs) => {
            "The final reviewer has no recorded request: the close has not called it yet."
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
        ("spec_events.task_has_dependents", Locale::PtBr) => {
            "Remover {removed} deixaria {dependents} dependendo de tarefa que não existe mais nesta spec. \
             Remova as dependentes no mesmo `targets`, ou grave antes uma versão delas sem essa dependência. \
             Nada foi gravado."
        }
        ("spec_events.task_has_dependents", Locale::EnUs) => {
            "Removing {removed} would leave {dependents} depending on a task that no longer exists in this \
             spec. Remove the dependents in the same `targets`, or first write a version of them without that \
             dependency. Nothing was written."
        }
        ("spec_events.task_dependency_cycle", Locale::PtBr) => {
            "As tarefas dependem umas das outras em círculo: {cycle}. Nada foi gravado."
        }
        ("spec_events.task_dependency_cycle", Locale::EnUs) => {
            "The tasks depend on each other in a circle: {cycle}. Nothing was written."
        }
        ("spec_events.agreed_items_missing", Locale::PtBr) => {
            "O veredito final responde pelos requisitos acordados que nenhuma tarefa terminada cobre: faltou {missing}. \
             O revisor grava o veredito de novo, com todos eles. Nada foi gravado."
        }
        ("spec_events.agreed_items_missing", Locale::EnUs) => {
            "The final verdict answers for the agreed requirements no finished task covers: {missing} is missing. \
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
        ("spec_events.delivery_read_missing", Locale::PtBr) => {
            "A entrega da onda {wave} foi recusada: o pedido lista item que ainda não foi lido, e faltou \
             {missing}. Leia cada um de dentro da cópia da onda. Use o comando de \"Como ler cada item\" \
             do pedido; `lesson-<número>` é a lição desse número. Depois grave a entrega de novo. Nada foi \
             gravado."
        }
        ("spec_events.delivery_read_missing", Locale::EnUs) => {
            "The delivery of wave {wave} was refused: the request lists an item that was not read yet, and \
             {missing} is missing. Read each one from inside the wave's copy. Use the command of \"How to \
             read each item\" in the request; `lesson-<number>` is the lesson of that number. Then record \
             the delivery again. Nothing was written."
        }
        ("spec_events.task_not_released", Locale::PtBr) => {
            "A tarefa {task} ainda não chegou: ela vem na resposta do fim da tarefa atual."
        }
        ("spec_events.task_not_released", Locale::EnUs) => {
            "The task {task} has not arrived yet: it comes in the answer to the end of the current task."
        }
        ("spec_events.delivery_proof_older_than_file", Locale::PtBr) => {
            "A entrega da onda {wave} foi recusada: {files} mudou depois do último passo de término de \
             tarefa. A prova verde desse passo não cobre o que vai no commit. Rode a prova da tarefa de \
             novo e, com ela verde, grave o passo da tarefa com `run write step`. Depois grave a entrega \
             de novo. Nada foi gravado."
        }
        ("spec_events.delivery_proof_older_than_file", Locale::EnUs) => {
            "The delivery of wave {wave} was refused: {files} changed after the last task-end step. The \
             green proof of that step does not cover what goes into the commit. Run the task's proof \
             again and, once it is green, record the task's step with `run write step`. Then record the \
             delivery again. Nothing was written."
        }
        ("spec_events.delivery_item_code_in_comment", Locale::PtBr) => {
            "A entrega da onda {wave} foi recusada: comentário acrescentado cita código de item da spec, \
             em {found}. O código do item fala da obra, e o comentário fica no código depois que ela \
             acaba. Escreva no comentário o comportamento, sem o código, ou tire o comentário. Depois \
             grave o passo da tarefa e a entrega de novo. Nada foi gravado."
        }
        ("spec_events.delivery_item_code_in_comment", Locale::EnUs) => {
            "The delivery of wave {wave} was refused: an added comment cites a spec item code, at \
             {found}. The item code speaks of the work, and the comment stays in the code after the \
             work ends. Write the behavior in the comment, without the code, or remove the comment. \
             Then record the task's step and the delivery again. Nothing was written."
        }
        ("spec_events.verdict_read_missing", Locale::PtBr) => {
            "O veredito foi recusado: o pedido da revisão lista item que ainda não foi lido, e faltou \
             {missing}. Leia cada um de dentro da cópia do revisor. Use o comando de \"Como ler cada \
             item\" do pedido. Depois grave o veredito de novo. Nada foi gravado."
        }
        ("spec_events.verdict_read_missing", Locale::EnUs) => {
            "The verdict was refused: the review request lists an item that was not read yet, and \
             {missing} is missing. Read each one from inside the reviewer's copy. Use the command of \
             \"How to read each item\" in the request. Then record the verdict again. Nothing was \
             written."
        }
        ("spec_events.verdict_leftovers_not_approved", Locale::PtBr) => {
            "O veredito foi recusado: os achados em `leftovers` só viram tarefas no veredito final \
             aprovado, e neste nada os leria, então se perderiam em silêncio. Tire o campo. Na \
             revisão de uma onda, reprove com o motivo em `text` e os arquivos em `files`. Na \
             revisão final, para reprovar, marque o item combinado afetado em `agreed` com \
             `met:false`, `text` e `files`, e ele vira tarefa no backlog; para aprovar, os \
             achados ficam em `leftovers`. Depois grave o veredito de novo. Nada foi gravado."
        }
        ("spec_events.verdict_leftovers_not_approved", Locale::EnUs) => {
            "The verdict was refused: findings in `leftovers` become tasks only in the approved \
             final verdict, and in this one nothing would read them, so they would be lost \
             silently. Remove the field. In a wave review, reject with the reason in `text` and \
             the files in `files`. In the final review, to reject, mark the affected agreed item \
             in `agreed` with `met:false`, `text` and `files`, and it becomes a backlog task; to \
             approve, the findings stay in `leftovers`. Then record the verdict again. Nothing \
             was written."
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
            "A prova precisa ser um comando que roda, e o primeiro termo dela, {term}, não é um \
             programa que o shell ache. Escreva no lugar o comando que demonstra o critério ou a \
             tarefa. A conferência que só se faz à mão vai para o texto, e não para a prova. Nada \
             foi gravado."
        }
        ("spec_events.proof_program_unknown", Locale::EnUs) => {
            "A proof must be a command that runs, and its first term, {term}, is not a program the \
             shell can find. Write the command that demonstrates the criterion or the task instead. \
             A check that can only be done by hand goes in the text, not in the proof. Nothing was \
             written."
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
        ("spec_events.proof_test_target_missing", Locale::PtBr) => {
            "A prova roda o alvo de teste {target} do pacote {package}, e o pacote não tem esse \
             alvo. O cargo recusa o comando antes de rodar teste nenhum. Os alvos de teste do \
             pacote são: {targets}. Cite um deles em --test, com o nome do teste como filtro depois \
             dele, ou tire o --test e deixe só o filtro. Nada foi gravado."
        }
        ("spec_events.proof_test_target_missing", Locale::EnUs) => {
            "The proof runs test target {target} of package {package}, and the package has no such \
             target. Cargo refuses the command before running any test. The package's test targets \
             are: {targets}. Name one of them in --test, with the test's name as a filter after it, \
             or drop --test and keep only the filter. Nothing was written."
        }
        ("spec_events.proof_test_target_missing_none", Locale::PtBr) => {
            "A prova roda o alvo de teste {target} do pacote {package}, e o pacote não tem alvo de \
             teste nenhum. O cargo recusa o comando antes de rodar teste nenhum. Tire o --test e \
             deixe só o nome do teste como filtro. Se a tarefa cria o alvo, declare o arquivo dele \
             na lista de arquivos, marcado como novo. Nada foi gravado."
        }
        ("spec_events.proof_test_target_missing_none", Locale::EnUs) => {
            "The proof runs test target {target} of package {package}, and the package has no test \
             target at all. Cargo refuses the command before running any test. Drop --test and \
             keep only the test's name as a filter, or declare the target's file in the task's \
             files, marked as new. Nothing was written."
        }
        ("spec_events.proof_path_missing", Locale::PtBr) => {
            "A prova cita o caminho {path}, que não existe no projeto. O comando falha quando a \
             onda o roda, ou passa sem provar nada. Corrija o caminho, ou declare o arquivo na \
             lista de arquivos da tarefa, marcado como novo, quando é a tarefa que o cria. Nada \
             foi gravado."
        }
        ("spec_events.proof_path_missing", Locale::EnUs) => {
            "The proof cites the path {path}, which does not exist in the project. The command \
             fails when the wave runs it, or passes without proving anything. Fix the path, or \
             declare the file in the task's files, marked as new, when the task creates it. \
             Nothing was written."
        }
        ("spec_events.invalid_value", Locale::PtBr) => {
            "O campo {field} do evento {type} precisa ser {expected}. Nada foi gravado."
        }
        ("spec_events.invalid_value", Locale::EnUs) => {
            "The {field} field of the {type} event must be {expected}. Nothing was written."
        }
        ("spec_events.empty_text", Locale::PtBr) => {
            "O campo {field} do evento {type} veio vazio. Escreva o valor ou tire o campo. Nada foi \
             gravado."
        }
        ("spec_events.empty_text", Locale::EnUs) => {
            "The {field} field of the {type} event came empty. Write its value or drop the field. \
             Nothing was written."
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
        ("spec_events.cited_file_missing.same_name", Locale::PtBr) => {
            "O fato {fact} cita {path}, e esse arquivo não existe. Com esse nome, o projeto tem \
             {found}: cite o caminho inteiro. Nada foi gravado."
        }
        ("spec_events.cited_file_missing.same_name", Locale::EnUs) => {
            "Fact {fact} cites {path}, and that file does not exist. With that name, the project \
             has {found}: cite the whole path. Nothing was written."
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
        ("spec_events.goal_title_too_long", Locale::PtBr) => {
            "O objetivo não tem `title`, e a primeira frase dele é o título do pull request: ela tem \
             {chars} caracteres e o limite é {max}. A frase: \"{sentence}\". Dê ao objetivo um \
             `title` de até {max} caracteres ou escreva uma primeira frase mais curta: o corte \
             automático mentiria sobre o que a mensagem diz. O resto do objetivo não foi medido. \
             Nada foi gravado."
        }
        ("spec_events.goal_title_too_long", Locale::EnUs) => {
            "The goal has no `title`, so its first sentence is the pull request title: it has \
             {chars} characters and the limit is {max}. The sentence: \"{sentence}\". Give the goal \
             a `title` of up to {max} characters or write a shorter first sentence: truncating would \
             misstate what the message says. The rest of the goal was not measured. Nothing was \
             written."
        }
        ("spec_events.goal_title_field_too_long", Locale::PtBr) => {
            "O `title` do objetivo tem {chars} caracteres e o limite é {max}. Ele é o título do pull \
             request e o nome da spec no painel e no resumo do projeto. O título: \"{title}\". \
             Escreva um `title` mais curto: o corte automático mentiria sobre o que a mensagem diz. \
             O texto do objetivo não foi medido. Nada foi gravado."
        }
        ("spec_events.goal_title_field_too_long", Locale::EnUs) => {
            "The goal's `title` has {chars} characters and the limit is {max}. It is the pull request \
             title and the spec's name in the panel and in the project summary. The title: \
             \"{title}\". Write a shorter `title`: truncating would misstate what the message says. \
             The goal's text was not measured. Nothing was written."
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
        ("spec_events.plan_without_task", Locale::PtBr) => {
            "A spec {spec} não tem tarefa nenhuma, e não há o que aprovar. Grave as tarefas e rode \
             `mustard-rt run plan` de novo. Nada foi gravado."
        }
        ("spec_events.plan_without_task", Locale::EnUs) => {
            "Spec {spec} has no task at all, so there is nothing to approve. Write the tasks and run \
             `mustard-rt run plan` again. Nothing was written."
        }
        ("spec_events.plan_not_passed", Locale::PtBr) => {
            "A spec {spec} não pode ser aprovada: o último plano não passou, ou o plano mudou depois \
             dele. Rode `mustard-rt run plan` e faça a pergunta de novo quando ele passar. Nada foi \
             gravado."
        }
        ("spec_events.plan_not_passed", Locale::EnUs) => {
            "Spec {spec} cannot be approved: the last plan did not pass, or the plan changed after \
             it. Run `mustard-rt run plan` and ask the question again once it passes. Nothing was \
             written."
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
        ("spec_events.point_without_facts", Locale::PtBr) => {
            "O ponto {code} não tem fato nenhum, e ponto sem fato não fecha com resposta (`result`). \
             Inclua os fatos conferidos no próprio `answer`, com `--facts` (lista JSON com `text` e `source`). \
             Ou feche-o como `not_applicable`, com o motivo. Nada foi gravado."
        }
        ("spec_events.point_without_facts", Locale::EnUs) => {
            "Point {code} has no fact, so it does not close with an answer (`result`). \
             Include verified facts in `answer` itself, with `--facts` (JSON array with `text` and `source`). \
             Or close it as `not_applicable`, with the reason. Nothing was written."
        }
        ("spec_events.open_point_removed", Locale::PtBr) => {
            "O ponto {code} está aberto e não sai com `remove`. Se ele não se aplica, feche-o com \
             `mustard-rt run answer --point {code} --not-applicable --reason \"<motivo>\"`. Se ele \
             tem resposta, troque o `--not-applicable` e o `--reason` por `--type` e `--json`. Nada \
             foi gravado."
        }
        ("spec_events.open_point_removed", Locale::EnUs) => {
            "Point {code} is open and does not leave with `remove`. If it does not apply, close it \
             with `mustard-rt run answer --point {code} --not-applicable --reason \"<reason>\"`. If \
             it has an answer, swap `--not-applicable` and `--reason` for `--type` and `--json`. \
             Nothing was written."
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
        ("spec_events.wave_agent_working", Locale::PtBr) => {
            "A onda {wave} está em andamento: o agente dela ainda trabalha, e esta remoção tiraria a \
             última tarefa dela. A onda sairia do ar e a entrega dele seria recusada. Espere a entrega \
             do agente para remover a tarefa. Para cancelar a onda de propósito, remova a própria onda: \
             as tarefas dela voltam ao backlog. Nada foi gravado."
        }
        ("spec_events.wave_agent_working", Locale::EnUs) => {
            "Wave {wave} is in progress: its agent is still working, and this removal would take out \
             its last task. The wave would leave the air and the agent's delivery would be refused. \
             Wait for the agent's delivery before removing the task. To cancel the wave on purpose, \
             remove the wave itself: its tasks go back to the backlog. Nothing was written."
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
            "O achado {finding} da volta (leftovers) não traz {field}. Todo achado diz o título, o \
             detalhe, a regra que quebrou (rule) e os lugares dela (places). Nada foi gravado."
        }
        ("spec_events.leftover_field_missing", Locale::EnUs) => {
            "Finding {finding} of the return (leftovers) lacks {field}: every finding states the \
             title, the detail, the rule it broke (rule) and its places (places). Nothing was \
             written."
        }
        ("spec_events.finding_place_missing", Locale::PtBr) => {
            "O achado {finding} cita o lugar {place}, que o projeto não tem. Confira o caminho e a \
             linha pelo mapa. Nada foi gravado."
        }
        ("spec_events.finding_place_missing", Locale::EnUs) => {
            "Finding {finding} cites the place {place}, which the project does not have. Check the \
             path and the line through the map. Nothing was written."
        }
        ("spec_events.finding_place_missing.same_name", Locale::PtBr) => {
            "O achado {finding} cita o lugar {place}, que o projeto não tem. Com esse nome, o \
             projeto tem {found}: cite o caminho inteiro. Nada foi gravado."
        }
        ("spec_events.finding_place_missing.same_name", Locale::EnUs) => {
            "Finding {finding} cites the place {place}, which the project does not have. With that \
             name, the project has {found}: cite the whole path. Nothing was written."
        }
        ("spec_events.finding_proof_missing", Locale::PtBr) => {
            "O achado {finding} não nomeia a prova no detalhe. A tarefa que ele vira cobre a regra \
             com a prova que a onda roda: o detalhe traz `Prova:` e o comando entre crases. Nada \
             foi gravado."
        }
        ("spec_events.finding_proof_missing", Locale::EnUs) => {
            "Finding {finding} names no proof in its detail. The task it becomes covers the rule \
             with the proof the wave runs: the detail carries `Proof:` and the command in \
             backticks. Nothing was written."
        }
        ("spec_events.task_without_proof", Locale::PtBr) => {
            "A tarefa cobre {covers}, e nenhum deles dá à onda uma prova que ela rode. Toda tarefa \
             cobre uma regra com prova. Vale o critério com a prova dele, ou o pedido cujo relato \
             traga `Prova:` e o comando entre crases. Grave antes o critério e cubra-o. Nada foi \
             gravado."
        }
        ("spec_events.task_without_proof", Locale::EnUs) => {
            "The task covers {covers}, and none of them gives the wave a proof it runs. Every task \
             covers a rule with a proof. That is a criterion with its proof, or a request whose \
             account carries `Proof:` and the command in backticks. Record the criterion first and \
             cover it. Nothing was written."
        }
        ("spec_events.delivery_place_untouched", Locale::PtBr) => {
            "A entrega da onda {wave} dá a tarefa {task} por feita sem mudar os lugares {places} da \
             regra dela. Mude cada lugar, ou devolva a tarefa em `undone`. O lugar conferido que não \
             precisa mudar vai em `checked_places`, com `place` e `reason`. Nada foi gravado."
        }
        ("spec_events.delivery_place_untouched", Locale::EnUs) => {
            "The delivery of wave {wave} gives task {task} as done without changing the places \
             {places} of its rule. Change every place, or return the task in `undone`. A checked \
             place that needs no change goes in `checked_places`, with `place` and `reason`. \
             Nothing was written."
        }
        // O achado que vira tarefa da spec: a regra que ele quebrou, os
        // lugares dela e a linha do achado que entra na tarefa da mesma regra,
        // com os comandos do mapa que acham os lugares da regra.
        ("finding.rule", Locale::PtBr) => "Regra que o achado quebrou: {rule}.",
        ("finding.rule", Locale::EnUs) => "Rule the finding broke: {rule}.",
        ("finding.places", Locale::PtBr) => "Lugares dela: {places}.",
        ("finding.places", Locale::EnUs) => "Its places: {places}.",
        ("finding.joined_by_rule", Locale::PtBr) => {
            concat!("Sobra da onda {wave}, da mesma regra — {title}: {detail} (Pelo mapa: ", map_commands_pt!(), ".)")
        }
        ("finding.joined_by_rule", Locale::EnUs) => {
            concat!("Leftover from wave {wave}, of the same rule — {title}: {detail} (By the map: ", map_commands_en!(), ".)")
        }
        ("finding.review_joined_by_rule", Locale::PtBr) => {
            concat!("Achado da revisão final, da mesma regra — {title}: {detail} (Pelo mapa: ", map_commands_pt!(), ".)")
        }
        ("finding.review_joined_by_rule", Locale::EnUs) => {
            concat!("Finding from the final review, of the same rule — {title}: {detail} (By the map: ", map_commands_en!(), ".)")
        }
        // O achado da revisão final sem regra escrita na spec, que vai ao
        // usuário como pergunta, e o aviso que manda fazê-la.
        ("finding.question", Locale::PtBr) => {
            "A revisão final achou: {title}. A regra que o achado diz, {rule}, não está escrita na spec. \
             Ela passa a valer nesta obra?"
        }
        ("finding.question", Locale::EnUs) => {
            "The final review found: {title}. The rule the finding names, {rule}, is not written in the \
             spec. Should it hold in this work?"
        }
        ("finding.questions_hint", Locale::PtBr) => {
            "A revisão final trouxe achados sem regra escrita na spec, e eles não viraram tarefa. Antes do \
             pull request, faça ao usuário a pergunta de cada um, em `questions`."
        }
        ("finding.questions_hint", Locale::EnUs) => {
            "The final review brought findings without a rule written in the spec, and they did not become \
             tasks. Before the pull request, ask the user the question of each one, in `questions`."
        }
        ("spec_events.report_carries_return_line", Locale::PtBr) => {
            "A entrega e o veredito moram na spec: o agente os grava com mustard-rt run write. O \
             relatório leva só as linhas `USAGE`, `PAUSED` e `REJECTED`."
        }
        ("spec_events.report_carries_return_line", Locale::EnUs) => {
            "The delivery and the verdict live in the spec: the agent writes them with mustard-rt \
             run write. The report carries only the `USAGE`, `PAUSED` and `REJECTED` lines."
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
        ("spec_events.unknown_block", Locale::PtBr) => "O bloco {block} não existe. Blocos: {blocks}. Tipos: {types}. Para qualquer item, use `mustard-rt run read item-<código>`.",
        ("spec_events.unknown_block", Locale::EnUs) => "There is no {block} block. Blocks: {blocks}. Types: {types}. Read any item by its code, for example `mustard-rt run read item-MSTD-RULE-0001`.",
        ("spec_events.another_session", Locale::PtBr) => {
            "Esta spec já tem registro da sessão {session}, e esta gravação veio da sessão {mine}. Duas conversas no mesmo projeto gravam na mesma spec. Confira com o usuário quem conduz esta obra antes de seguir."
        }
        ("spec_events.another_session", Locale::EnUs) => {
            "This spec already holds records from session {session}, and this write came from session {mine}. Two conversations in the same project write to the same spec. Check with the user who is driving this work before going on."
        }
        ("spec_events.block_alias", Locale::PtBr) => {
            "Você pediu `{asked}`, e o nome do bloco é `{block}`. A leitura é esta mesma. Na próxima vez, peça pelo nome."
        }
        ("spec_events.block_alias", Locale::EnUs) => {
            "You asked for `{asked}`, and the block is named `{block}`. This is that read. Next time, ask by the name."
        }
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
             versão que ela substitui; numa onda em andamento, omitir o `wave` também vale, e a \
             versão nova fica na onda da que ela substitui."
        }
        ("spec_events.wave_by_backlog", Locale::EnUs) => {
            "A wave is not written by `run write`, and nothing was written. The wave is born from \
             the backlog, and the program puts the batch together when it dispatches. Write only \
             the task, without `wave`, with what it does (`text`), the files (`files`) and the \
             tasks it depends on (`depends_on`). In the new version of a task that is already in a \
             wave, repeat the `wave` of the version it replaces; in a wave in progress, leaving out \
             `wave` works too, and the new version stays in the wave of the one it replaces."
        }
        ("spec_events.files_ignored_by_git", Locale::PtBr) => {
            "A tarefa só mexe em arquivo que o git ignora ({files}), e nada foi gravado. É mudança \
             local: nenhuma onda a entrega e nenhum commit a leva. Grave uma nota com o que o \
             usuário deve fazer nesta máquina, com `mustard-rt run write note`; se a tarefa também \
             mexe em arquivo versionado, declare esse arquivo em `files`."
        }
        ("spec_events.files_ignored_by_git", Locale::EnUs) => {
            "The task only touches files that git ignores ({files}), and nothing was written. It is \
             a local change: no wave delivers it and no commit carries it. Write a note with what \
             the user must do on this machine, with `mustard-rt run write note`; if the task also \
             touches a versioned file, declare that file in `files`."
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
        ("spec_events.legacy_spec", Locale::PtBr) => {
            "A spec {spec} é de uma versão anterior do Mustard e só pode ser lida. Abra uma spec nova \
             para continuar o trabalho."
        }
        ("spec_events.legacy_spec", Locale::EnUs) => {
            "The spec {spec} is from an earlier version of Mustard and can only be read. Open a new \
             spec to continue the work."
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
        ("spec_events.session_unbound", Locale::PtBr) => {
            "Sem `--spec`, o tipo {type} vai para a spec ligada a esta conversa, e esta \
             conversa não está ligada a nenhuma. Rode `mustard-rt run resume --spec <nome>` \
             (o `/mustard:continue` faz isso) ou passe `--spec <nome>`. Nada foi gravado."
        }
        ("spec_events.session_unbound", Locale::EnUs) => {
            "Without `--spec`, a {type} event goes to the spec bound to this conversation, and \
             this conversation is bound to none. Run `mustard-rt run resume --spec <name>` \
             (`/mustard:continue` does it) or pass `--spec <name>`. Nothing was written."
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
            "Regras que a instalação tirou dos arquivos de instrução ({count}, lote {mark}): cada \
             uma vira teste ou sai"
        }
        ("lessons.rules_left.title", Locale::EnUs) => {
            "Rules the install took out of the instruction files ({count}, batch {mark}): each \
             becomes a test or goes"
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
        ("lessons.scan_test_or_retire", Locale::PtBr) => {
            "Estes grupos são de lições de defeito ou de regras do projeto, e o banco recusa juntá-las: \
             {groups}. Transforme cada uma num teste no código, que falha se a regra for quebrada. \
             Ou retire do banco a que já não vale, com `mustard-rt run write lesson --json \
             '{\"targets\":[…],\"reason\":\"…\"}'`."
        }
        ("lessons.scan_test_or_retire", Locale::EnUs) => {
            "These groups hold defect lessons or project rules, and the bank refuses to merge \
             them: {groups}. Turn each one into a test in the code, one that fails if the rule is \
             broken. Or retire the one that no longer holds from the bank, with `mustard-rt run \
             write lesson --json '{\"targets\":[…],\"reason\":\"…\"}'`."
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

        // O resumo do projeto e as regras que a spec entregue deixa
        // (`io::project_summary`).
        ("spec_index.rules_title", Locale::PtBr) => "Regras da spec {spec}",
        ("spec_index.rules_title", Locale::EnUs) => "Rules from spec {spec}",
        ("spec_index.summary_failed", Locale::PtBr) => {
            "A spec {spec} foi entregue, mas o resumo do projeto não foi gravado: {detail}."
        }
        ("spec_index.summary_failed", Locale::EnUs) => {
            "Spec {spec} was delivered, but the project summary was not written: {detail}."
        }
        ("spec_index.legacy_summary_failed", Locale::PtBr) => {
            "O resumo das specs antigas não foi gravado: {detail}. A próxima abertura de spec \
             tenta de novo."
        }
        ("spec_index.rules_pending_failed", Locale::PtBr) => {
            "As regras do projeto que esperam a decisão do Jev não foram gravadas: {detail}. A \
             próxima abertura de spec tenta de novo."
        }
        ("spec_index.rules_pending_failed", Locale::EnUs) => {
            "The project rules waiting for the Jev's decision were not written: {detail}. The next \
             spec opening tries again."
        }
        ("spec_index.legacy_summary_failed", Locale::EnUs) => {
            "The summary of the old specs was not written: {detail}. The next spec opening tries \
             again."
        }
        // A falha que se repete vira a tarefa de criar a skill
        // (`shared::failure_skills`).
        ("failures.not_recorded", Locale::PtBr) => {
            "As falhas do projeto não foram gravadas: {detail}. A próxima rodada tenta de novo."
        }
        ("failures.not_recorded", Locale::EnUs) => "The project failures were not recorded: {detail}. The next round tries again.",
        ("failures.skill_task_title", Locale::PtBr) => "Criar a skill: {reason}",
        ("failures.skill_task_title", Locale::EnUs) => "Create the skill: {reason}",
        ("failures.skill_task_text", Locale::PtBr) => {
            "A mesma falha voltou {count} vezes. A skill nova ensina o que evita a volta dela."
        }
        ("failures.skill_task_text", Locale::EnUs) => {
            "The same failure came back {count} times. The new skill teaches what keeps it from coming back."
        }
        ("failures.skill_task_agent", Locale::PtBr) => {
            "A mesma falha voltou {count} vezes. Crie a skill que a evita, pelo skill-creator do bloco \
             desta tarefa. As ocorrências:\n{occurrences}"
        }
        ("failures.skill_task_agent", Locale::EnUs) => {
            "The same failure came back {count} times. Create the skill that prevents it, with the \
             skill-creator in this task's block. The occurrences:\n{occurrences}"
        }
        ("failures.occurrence", Locale::PtBr) => "- Spec {spec}, onda {wave}, {source}: {reason}",
        ("failures.occurrence", Locale::EnUs) => "- Spec {spec}, wave {wave}, {source}: {reason}",
        ("failures.occurrence_files", Locale::PtBr) => "Arquivos: {files}.",
        ("failures.occurrence_files", Locale::EnUs) => "Files: {files}.",
        ("failures.source.wave_review", Locale::PtBr) => "reprovada pelo revisor da onda",
        ("failures.source.wave_review", Locale::EnUs) => "rejected by the wave reviewer",
        ("failures.source.conductor", Locale::PtBr) => "reprovada por quem conduz a obra",
        ("failures.source.conductor", Locale::EnUs) => "rejected by the conductor",
        ("failures.source.final_review", Locale::PtBr) => "reprovada pela revisão final",
        ("failures.source.final_review", Locale::EnUs) => "rejected by the final review",
        ("skills.bad_name", Locale::PtBr) => {
            "O nome da skill `{name}` não serve de pasta. Use só letras, números, hífen e sublinhado."
        }
        ("skills.bad_name", Locale::EnUs) => {
            "The skill name `{name}` cannot be a folder. Use only letters, digits, hyphens and underscores."
        }
        ("skills.folder_missing", Locale::PtBr) => {
            "A skill {name} não está em `{reserved}`, a partir da raiz da cópia da onda, nem em \
             `.claude/skills/{name}/SKILL.md` do projeto. Grave o arquivo antes de gravar o resultado."
        }
        ("skills.folder_missing", Locale::EnUs) => {
            "The skill {name} is not at `{reserved}`, from the root of the wave copy, nor at \
             `.claude/skills/{name}/SKILL.md` in the project. Write the file before recording the result."
        }
        ("skills.not_hidden", Locale::PtBr) => "A skill {name} foi gravada, mas não ficou fora do git: {detail}.",
        ("skills.not_hidden", Locale::EnUs) => "The skill {name} was recorded, but it was not hidden from git: {detail}.",
        ("skills.not_backed_up", Locale::PtBr) => {
            "A skill {name} foi gravada, mas a cópia de segurança falhou: {detail}."
        }
        ("skills.not_backed_up", Locale::EnUs) => "The skill {name} was recorded, but its backup copy failed: {detail}.",
        // A vida da skill do Mustard depois de criada (`shared::skill_cycle`).
        ("skills.update_task_title", Locale::PtBr) => "Atualizar a skill {name}",
        ("skills.update_task_title", Locale::EnUs) => "Update the skill {name}",
        ("skills.update_task_text", Locale::PtBr) => {
            "A skill {name} pode ter ficado desatualizada. Arquivos: {files}."
        }
        ("skills.update_task_text", Locale::EnUs) => "The skill {name} may be out of date. Files: {files}.",
        ("skills.update_task_agent", Locale::PtBr) => {
            "Atualize a skill {name}, de `{path}`, pelo skill-creator do bloco desta tarefa. {why}\n\n\
             Teste com e sem a mudança na skill, nas tarefas em que ela entra. Com ganho, escreva a versão \
             nova inteira em `{reserved}`, a partir da raiz da sua cópia. Depois grave \
             `\"action\":\"updated\"`. A rodada a grava no lugar da skill quando assumir a volta. O \
             `SKILL.md` de `{path}` fica como está. Sem ganho, grave `\"action\":\"discarded\"`, com o \
             motivo em `text`."
        }
        ("skills.update_task_agent", Locale::EnUs) => {
            "Update the skill {name}, from `{path}`, with the skill-creator in this task's block. {why}\n\n\
             Test with and without the change to the skill, on the tasks it goes with. With a gain, write the \
             whole new version at `{reserved}`, from the root of your copy. Then record \
             `\"action\":\"updated\"`. The round writes it in the place of the skill when it takes the return. \
             The `SKILL.md` at `{path}` stays as it is. Without one, record `\"action\":\"discarded\"`, with \
             the reason in `text`."
        }
        ("skills.not_installed", Locale::PtBr) => {
            "A versão nova da skill {name} não foi gravada no lugar dela: {detail}. Ela segue em `{reserved}`."
        }
        ("skills.not_installed", Locale::EnUs) => {
            "The new version of the skill {name} was not written in its place: {detail}. It stays at `{reserved}`."
        }
        ("skills.update_why_change", Locale::PtBr) => {
            "O commit `{sha}` mudou {files}, que a skill cita, e o Jev deu chance de {chance} de a mudança \
             contradizer o que ela ensina. A mudança:\n\n```diff\n{diff}\n```"
        }
        ("skills.update_why_change", Locale::EnUs) => {
            "The commit `{sha}` changed {files}, which the skill cites, and the Jev gave a chance of {chance} \
             that the change contradicts what it teaches. The change:\n\n```diff\n{diff}\n```"
        }
        ("skills.update_why_gone", Locale::PtBr) => "O commit `{sha}` apagou {files}, que a skill cita.",
        ("skills.update_why_gone", Locale::EnUs) => "The commit `{sha}` deleted {files}, which the skill cites.",
        ("skills.update_why_listed", Locale::PtBr) => {
            "O commit `{sha}` mudou {files}, que a skill cita, desde a última avaliação dela. A mudança:\n\n```diff\n{diff}\n```"
        }
        ("skills.update_why_listed", Locale::EnUs) => {
            "The commit `{sha}` changed {files}, which the skill cites, since its last evaluation. The change:\n\n```diff\n{diff}\n```"
        }
        ("skills.update_why_failure", Locale::PtBr) => {
            "A onda {wave} da spec {spec} recebeu a skill e foi reprovada pela mesma falha que a motivou: {reason}"
        }
        ("skills.update_why_failure", Locale::EnUs) => {
            "Wave {wave} of spec {spec} got the skill and was rejected for the same failure that motivated it: {reason}"
        }
        ("skills.retired", Locale::PtBr) => {
            "A skill {name} saiu de `.claude/skills/`: {reason}. A cópia de segurança fica em `{backup}`."
        }
        ("skills.retired", Locale::EnUs) => {
            "The skill {name} left `.claude/skills/`: {reason}. The backup copy stays in `{backup}`."
        }
        ("skills.retired_reason.idle", Locale::PtBr) => {
            "foi candidata em {count} envios e nunca entrou no bloco de uma tarefa"
        }
        ("skills.retired_reason.idle", Locale::EnUs) => "it was a candidate in {count} sends and never entered a task block",
        ("skills.retired_reason.discarded", Locale::PtBr) => "foi descartada sem ganho em {count} testes seguidos",
        ("skills.retired_reason.discarded", Locale::EnUs) => "it was discarded without a gain in {count} tests in a row",
        ("skills.cycle_failed", Locale::PtBr) => {
            "As contas das skills do Mustard não foram gravadas: {detail}. A próxima rodada tenta de novo."
        }
        ("skills.cycle_failed", Locale::EnUs) => "The Mustard skills were not recorded: {detail}. The next round tries again.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::translate;

    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas, como o arquivo de impressões dela em `stamps/` guarda. Quem muda
    /// um texto de propósito regrava o arquivo pelo comando que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged("events", include_str!("events.rs"), super::PREFIXES);
    }

    /// As recusas e os avisos do arquivo de eventos da spec saem do catálogo
    /// nos dois idiomas, cada um com as vagas que o chamador preenche.
    #[test]
    fn i18n_translates_spec_event_keys() {
        for (key, slots) in [
            ("spec_events.not_an_object", &["{detail}"][..]),
            ("spec_events.raw_input_twice", &[][..]),
            ("spec_events.raw_field_twice", &["{field}"][..]),
            ("spec_events.batch_empty", &[][..]),
            ("spec_events.batch_type_missing", &[][..]),
            ("spec_events.batch_line_refused", &["{label}", "{line}"][..]),
            ("spec_events.unknown_type", &["{type}", "{types}"][..]),
            ("spec_events.find_more", &["{shown}", "{total}"][..]),
            ("spec_events.find_none", &[][..]),
            ("spec_events.find_jev_off", &["{reason}"][..]),
            ("spec_events.find_off_test", &[][..]),
            ("spec_events.find_off_fallback", &[][..]),
            ("spec_events.find_off_purpose", &["{purpose}"][..]),
            ("spec_events.find_no_key", &[][..]),
            ("spec_events.find_key_in_git", &[][..]),
            ("spec_events.find_choice_left", &[][..]),
            ("spec_events.find_partial", &[][..]),
            ("spec_events.find_user_message", &[][..]),
            ("spec_events.find_lesson", &[][..]),
            ("spec_events.find_pasted", &["{kb}", "{code}"][..]),
            ("spec_events.find_files", &["{files}"][..]),
            ("spec_events.read_more", &["{shown}", "{total}"][..]),
            ("spec_events.read_open_waves", &["{shown}", "{total}"][..]),
            ("spec_events.read_summary", &[][..]),
            ("spec_events.no_wave_request", &["{wave}"][..]),
            ("spec_events.no_review_request", &[][..]),
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
            ("spec_events.task_has_dependents", &["{removed}", "{dependents}"][..]),
            ("spec_events.task_dependency_cycle", &["{cycle}"][..]),
            ("spec_events.agreed_items_missing", &["{missing}"][..]),
            ("spec_events.delivery_agreed_missing", &["{wave}", "{missing}"][..]),
            ("spec_events.delivery_read_missing", &["{wave}", "{missing}"][..]),
            ("spec_events.task_not_released", &["{task}"][..]),
            ("spec_events.delivery_proof_older_than_file", &["{wave}", "{files}"][..]),
            ("spec_events.delivery_item_code_in_comment", &["{wave}", "{found}"][..]),
            ("spec_events.verdict_read_missing", &["{missing}"][..]),
            ("spec_events.criterion_form_missing", &[][..]),
            ("spec_events.proof_not_a_command", &["{criterion}", "{found}"][..]),
            ("spec_events.proof_program_unknown", &["{term}"][..]),
            ("spec_events.proof_chained_by_semicolon", &["{found}"][..]),
            ("spec_events.proof_search_not_negated", &["{found}"][..]),
            ("spec_events.proof_test_target_missing", &["{package}", "{target}", "{targets}"][..]),
            ("spec_events.proof_test_target_missing_none", &["{target}", "{package}"][..]),
            ("spec_events.proof_path_missing", &["{path}"][..]),
            ("spec_events.invalid_value", &["{type}", "{field}", "{expected}"][..]),
            ("spec_events.empty_text", &["{type}", "{field}"][..]),
            ("spec_events.wrong_count", &["{type}", "{field}", "{min}", "{max}", "{count}"][..]),
            ("spec_events.fact_without_source", &["{fact}"][..]),
            ("spec_events.cited_file_missing", &["{fact}", "{path}"][..]),
            ("spec_events.cited_file_missing.same_name", &["{fact}", "{path}", "{found}"][..]),
            ("spec_events.cited_line_missing", &["{fact}", "{path}", "{line}", "{lines}"][..]),
            ("spec_events.name_elsewhere", &["{fact}", "{name}", "{path}", "{found}"][..]),
            ("spec_events.name_unknown", &["{fact}", "{name}"][..]),
            ("spec_events.names_unchecked", &[][..]),
            ("spec_events.goal_origin_not_user", &["{spec}", "{origin}"][..]),
            ("spec_events.goal_title_too_long", &["{chars}", "{max}", "{sentence}"][..]),
            ("spec_events.goal_title_field_too_long", &["{chars}", "{max}", "{title}"][..]),
            ("spec_events.survey_open", &["{spec}", "{count}", "{points}"][..]),
            ("spec_events.survey_not_started", &["{spec}"][..]),
            ("spec_events.plan_without_task", &["{spec}"][..]),
            ("spec_events.plan_not_passed", &["{spec}"][..]),
            ("spec_events.survey_gaps_unrecorded", &["{spec}", "{count}", "{gaps}"][..]),
            ("spec_events.point_already_open", &["{code}", "{block}"][..]),
            ("spec_events.point_not_open", &["{id}", "{open}"][..]),
            ("spec_events.closing_point_open", &[][..]),
            ("spec_events.not_applicable_reason", &[][..]),
            ("spec_events.point_without_facts", &["{code}"][..]),
            ("spec_events.open_point_removed", &["{code}"][..]),
            ("spec_events.purge_excerpt_not_found", &["{code}"][..]),
            ("spec_events.closing_point_last_record", &["{code}"][..]),
            ("spec_events.delivered_too_long", &["{chars}", "{max}"][..]),
            ("spec_events.no_open_send", &["{wave}"][..]),
            ("spec_events.wave_agent_working", &["{wave}"][..]),
            ("spec_events.no_open_review", &[][..]),
            ("spec_events.verdict_missing", &[][..]),
            ("spec_events.leftover_field_missing", &["{finding}", "{field}"][..]),
            ("spec_events.finding_place_missing", &["{finding}", "{place}"][..]),
            ("spec_events.finding_place_missing.same_name", &["{finding}", "{place}", "{found}"][..]),
            ("spec_events.finding_proof_missing", &["{finding}"][..]),
            ("spec_events.task_without_proof", &["{covers}"][..]),
            ("spec_events.delivery_place_untouched", &["{wave}", "{task}", "{places}"][..]),
            ("finding.rule", &["{rule}"][..]),
            ("finding.places", &["{places}"][..]),
            ("finding.joined_by_rule", &["{wave}", "{title}", "{detail}"][..]),
            ("finding.review_joined_by_rule", &["{title}", "{detail}"][..]),
            ("finding.question", &["{title}", "{rule}"][..]),
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
            ("spec_events.unknown_block", &["{block}", "{blocks}", "{types}"][..]),
            ("spec_events.block_alias", &["{asked}", "{block}"][..]),
            ("spec_events.another_session", &["{session}", "{mine}"][..]),
            ("spec_events.bad_spec_name", &["{spec}"][..]),
            ("spec_events.no_spec_file", &["{spec}"][..]),
            ("spec_events.spec_not_open", &["{spec}"][..]),
            ("spec_events.phase_change_refused", &["{spec}", "{from}", "{to}"][..]),
            ("spec_events.state_by_flow_only", &["{spec}"][..]),
            ("spec_events.binary_only_type", &["{type}", "{spec}"][..]),
            ("spec_events.binary_author", &[][..]),
            ("spec_events.wave_by_backlog", &[][..]),
            ("spec_events.files_ignored_by_git", &["{files}"][..]),
            ("spec_events.request_on_closed_spec", &["{spec}", "{phase}"][..]),
            ("spec_events.work_on_finished_spec", &["{spec}", "{phase}", "{type}"][..]),
            ("spec_events.user_message_by_hook", &["{spec}"][..]),
            ("spec_events.old_format_spec", &["{spec}"][..]),
            ("spec_events.legacy_spec", &["{spec}"][..]),
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
            ("spec_events.session_unbound", &["{type}"][..]),
            ("lessons.unknown_lesson", &["{id}"][..]),
            ("lessons.origin_missing", &[][..]),
            ("lessons.repeated", &["{id}", "{text}"][..]),
            ("lessons.too_long", &["{lines}", "{max}"][..]),
            ("lessons.unclear", &["{defects}"][..]),
            ("lessons.defect_by_task", &["{spec}"][..]),
            ("lessons.defect_in_code", &[][..]),
            ("lessons.rule_by_task", &["{spec}"][..]),
            ("lessons.rule_in_code", &[][..]),
            ("lessons.rules_left.title", &["{count}", "{mark}"][..]),
            ("lessons.rules_left.detail", &["{rules}"][..]),
            ("lessons.rules_left.rule", &["{n}", "{text}", "{sources}"][..]),
            ("lessons.scan_merge", &["{groups}"][..]),
            ("lessons.scan_test_or_retire", &["{groups}"][..]),
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

    /// O título do item da limpeza da instalação passa na conferência de
    /// escrita das respostas, nos dois idiomas, com a contagem e a marca do
    /// lote no lugar das vagas.
    #[test]
    fn the_cleanup_item_title_reads_clearly() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = translate("lessons.rules_left.title", lang).replace("{count}", "2").replace("{mark}", "3f9a1c07");
            let report = crate::domain::clarity::measure(&text, &[], Some(lang));
            assert!(report.passed, "{lang:?}: {text}: {report:?}");
        }
    }

    /// A dica do scan para o grupo de defeitos ou de regras do projeto passa na
    /// conferência de escrita das respostas, nos dois idiomas, com cada vaga
    /// trocada por uma palavra, e traz o comando de retirada pronto.
    #[test]
    fn the_scan_hint_for_lessons_that_cannot_merge_reads_clearly() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let hint = translate("lessons.scan_test_or_retire", lang);
            assert!(hint.contains("mustard-rt run write lesson --json '{\"targets\":["), "{lang:?}: {hint}");
            let text = hint.replace("{groups}", "grupos");
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
