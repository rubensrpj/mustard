//! As páginas: a da spec e o `.md` dela (blocos, tipos, campos, valores, fases,
//! autores e o painel de medição), a lista dos itens sem dono, a do projeto, a
//! cópia para o banco de dados das páginas publicadas e as recusas do comando
//! `page`.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["page", "project"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // A cópia para o banco de dados das páginas publicadas, dita pelos
        // marcos (o plano, a rodada e o fechamento) e pela gravação de um
        // pedido que muda o plano (`commands/spec_events/pages/copy.rs`).
        ("page.copy.publish", Locale::PtBr) => {
            "A {page} ainda não foi publicada: publique o template `{template}` com a ferramenta \
             `Artifact`, passando em `capabilities` o valor `{capabilities}`, e grave o endereço com \
             `mustard-rt run write publish --spec {spec} --json '{\"page\":\"{key}\",\"milestone\":\"{milestone}\",\"ok\":true,\"template\":true,\"url\":\"…\"}'`, \
             com `\"ok\":false` e `\"reason\"` quando falhar."
        }
        ("page.copy.publish", Locale::EnUs) => {
            "The {page} is not published yet: publish the template `{template}` with the `Artifact` \
             tool, passing `{capabilities}` as `capabilities`, and record the address with \
             `mustard-rt run write publish --spec {spec} --json '{\"page\":\"{key}\",\"milestone\":\"{milestone}\",\"ok\":true,\"template\":true,\"url\":\"…\"}'`, \
             with `\"ok\":false` and a `\"reason\"` when it fails."
        }
        // A spec que já tinha a página inteira publicada por uma versão
        // antiga ganha o template num link novo; a antiga fica parada.
        ("page.copy.old_page", Locale::PtBr) => {
            "A página que uma versão antiga do Mustard publicou para esta spec fica parada como está, \
             como um retrato: publique o template como página nova, com link novo, sem mexer na \
             antiga, e a barra de status passa a mostrar o link novo."
        }
        ("page.copy.old_page", Locale::EnUs) => {
            "The page an older Mustard version published for this spec stays still as it is, like a \
             snapshot: publish the template as a new page, with a new link, without touching the old \
             one, and the status line starts showing the new link."
        }
        // A primeira cópia leva a spec inteira e fica com um agente separado.
        ("page.copy.agent", Locale::PtBr) => {
            "A primeira cópia para o banco da {page} leva a spec inteira e fica com um agente \
             separado, para esta conversa continuar leve: despache um agente com o texto entre « e », \
             com o endereço da página escrito nele, e espere a volta dele antes de seguir. «{order}»"
        }
        ("page.copy.agent", Locale::EnUs) => {
            "The first copy into the {page}'s database carries the whole spec and goes to a separate \
             agent, so this conversation stays light: dispatch an agent with the text between « and », \
             with the page's address written in it, and wait for it to come back before going on. \
             «{order}»"
        }
        // A migração de uma spec antiga: as notas de trabalho das tarefas das
        // ondas que ainda não saíram.
        ("page.migration.unrated", Locale::PtBr) => {
            "Esta spec veio de uma versão do Mustard sem a nota de trabalho das tarefas: grave uma \
             versão nova de cada tarefa das ondas que ainda não saíram, {tasks}, com a nota dela. \
             {scale} Depois some as notas de cada onda: a que passar de {cap} pontos volta para o \
             usuário aprovar a divisão dela antes de sair."
        }
        ("page.migration.unrated", Locale::EnUs) => {
            "This spec came from a Mustard version without the tasks' points: record a new version of \
             each task of the waves that have not gone out yet, {tasks}, with its points. {scale} \
             Then add up the points of each wave: the one over {cap} points goes back to the user to \
             approve its split before it goes out."
        }
        ("page.migration.over_cap", Locale::PtBr) => {
            "A onda {wave} ainda não saiu e soma {points} pontos, acima do teto de {cap}: ela volta \
             para o usuário aprovar a divisão dela antes de sair."
        }
        ("page.migration.over_cap", Locale::EnUs) => {
            "Wave {wave} has not gone out yet and adds up to {points} points, over the cap of {cap}: \
             it goes back to the user to approve its split before it goes out."
        }
        // O comando que ainda gera o `.md` e o `.html` da spec e da página do
        // projeto.
        ("page.deprecated", Locale::PtBr) => {
            "O `page --spec` foi descontinuado: as páginas da spec e do projeto agora são templates \
             que leem um banco de dados, e o `.md` e o `.html` gravados aqui não são mais publicados."
        }
        ("page.deprecated", Locale::EnUs) => {
            "`page --spec` is deprecated: the spec and project pages are now templates that read a \
             database, and the `.md` and `.html` written here are no longer published."
        }
        ("page.copy.batches", Locale::PtBr) => {
            "Copie para o banco de dados da {page}, no endereço {url}, os lotes {files}, nessa ordem: \
             cada arquivo é a lista `writes` de uma chamada da ferramenta `ArtifactData` com `action` \
             `batch`, e cada documento vai pelo `file_path` dele, sem você ler os itens. Depois grave a \
             cópia com `mustard-rt run write copy --spec {spec} --json '{record}'`."
        }
        ("page.copy.batches", Locale::EnUs) => {
            "Copy into the {page}'s database, at {url}, the batches {files}, in this order: each file is \
             the `writes` list of one `ArtifactData` call with `action` `batch`, and each document goes \
             by its `file_path`, without reading the items. Then record the copy with \
             `mustard-rt run write copy --spec {spec} --json '{record}'`."
        }
        ("page.copy.new_address", Locale::PtBr) => "o endereço que a publicação devolver",
        ("page.copy.new_address", Locale::EnUs) => "the address the publication returns",
        ("page.copy.no_links", Locale::PtBr) => {
            "Não escreva os endereços na resposta: eles ficam na barra de status."
        }
        ("page.copy.no_links", Locale::EnUs) => {
            "Never write the addresses in the reply: they live in the status line."
        }
        ("page.copy.failed", Locale::PtBr) => {
            "A cópia para o banco de dados das páginas não pôde ser preparada, e o motivo está em \
             `warnings`: não copie nada agora, porque a próxima cópia leva os mesmos itens."
        }
        ("page.copy.failed", Locale::EnUs) => {
            "The copy into the pages' database could not be prepared, and the reason is in `warnings`: \
             copy nothing now, since the next copy carries the same items."
        }
        ("page.purge_pending", Locale::PtBr) => {
            "Os itens {codes} guardam no arquivo um trecho com cara de segredo e ficam fora da cópia \
             para o banco de dados da página até serem expurgados: expurgue cada um com \
             `mustard-rt run write purge --spec {spec} --json '{\"targets\":[\"<código>\"],\"reason\":\"secret\"}'`."
        }
        ("page.purge_pending", Locale::EnUs) => {
            "Items {codes} keep in the file an excerpt that looks like a secret and stay out of the \
             copy into the page's database until they are purged: purge each one with \
             `mustard-rt run write purge --spec {spec} --json '{\"targets\":[\"<item>\"],\"reason\":\"secret\"}'`."
        }
        ("page.name.spec", Locale::PtBr) => "página da spec",
        ("page.name.spec", Locale::EnUs) => "spec page",
        ("page.name.project", Locale::PtBr) => "página do projeto",
        ("page.name.project", Locale::EnUs) => "project page",

        // A moldura de toda página: o menu lateral, a busca e os botões de
        // abrir e fechar. `{n}` e `{total}` são preenchidos pelo script da
        // página.
        ("page.sections", Locale::PtBr) => "Seções",
        ("page.sections", Locale::EnUs) => "Sections",
        ("page.search.placeholder", Locale::PtBr) => "Buscar texto ou código",
        ("page.search.placeholder", Locale::EnUs) => "Search text or code",
        ("page.search.label", Locale::PtBr) => "Buscar na página",
        ("page.search.label", Locale::EnUs) => "Search the page",
        ("page.open_all", Locale::PtBr) => "Abrir tudo",
        ("page.open_all", Locale::EnUs) => "Open all",
        ("page.close_all", Locale::PtBr) => "Fechar tudo",
        ("page.close_all", Locale::EnUs) => "Close all",
        ("page.not_found", Locale::PtBr) => "Nada encontrado. Tente outra palavra ou o código do item, como DEC-0142.",
        ("page.not_found", Locale::EnUs) => "Nothing found. Try another word or an item's code, like DEC-0142.",
        ("page.of", Locale::PtBr) => "{n} de {total}",
        ("page.of", Locale::EnUs) => "{n} of {total}",
        ("page.count.one", _) => "{n} item",
        ("page.count.many", Locale::PtBr) => "{n} itens",
        ("page.count.many", Locale::EnUs) => "{n} items",
        ("page.request", Locale::PtBr) => "pedido",
        ("page.request", Locale::EnUs) => "request",
        ("page.old_version", Locale::PtBr) => "versão antiga",
        ("page.old_version", Locale::EnUs) => "old version",

        // Os templates das páginas (`platform::page_templates`): o que eles
        // dizem enquanto leem o banco de dados da página, quando o banco
        // ainda está vazio, o filtro por tipo e o botão de baixar o `.md`.
        ("page.loading", Locale::PtBr) => "Lendo o banco de dados da página…",
        ("page.loading", Locale::EnUs) => "Reading the page's database…",
        ("page.no_data", Locale::PtBr) => {
            "Ainda não há dados: o Mustard ainda não copiou nada para o banco de dados desta página."
        }
        ("page.no_data", Locale::EnUs) => "No data yet: Mustard has not copied anything to this page's database.",
        ("page.filter.label", Locale::PtBr) => "Filtrar por tipo",
        ("page.filter.label", Locale::EnUs) => "Filter by type",
        ("page.filter.all", Locale::PtBr) => "Todos os tipos",
        ("page.filter.all", Locale::EnUs) => "All types",
        ("page.download", Locale::PtBr) => "Baixar .md",
        ("page.download", Locale::EnUs) => "Download .md",
        ("page.wave.full", Locale::PtBr) => "com o texto de cada item no lugar do código",
        ("page.wave.full", Locale::EnUs) => "with each item's text in place of its code",

        // A página e o `.md` de uma spec (`view::document`): os títulos das
        // seções e dos grupos, os nomes dos tipos, os rótulos dos campos e
        // dos valores.
        ("page.group.metrics", Locale::PtBr) => "Medição",
        ("page.group.metrics", Locale::EnUs) => "Measurement",
        ("page.group.state", Locale::PtBr) => "Fases e publicações",
        ("page.group.state", Locale::EnUs) => "Phases and publications",
        ("page.group.commits", _) => "Commits",
        ("page.group.criterion", Locale::PtBr) => "Critérios de aceite",
        ("page.group.criterion", Locale::EnUs) => "Acceptance criteria",
        ("page.group.day", Locale::PtBr) => "Dia {day}",
        ("page.group.day", Locale::EnUs) => "Day {day}",
        ("page.findings.tasks", Locale::PtBr) => "Sobre tarefas",
        ("page.findings.tasks", Locale::EnUs) => "About tasks",
        ("page.findings.waves", Locale::PtBr) => "Sobre ondas",
        ("page.findings.waves", Locale::EnUs) => "About waves",
        ("page.findings.criteria", Locale::PtBr) => "Sobre critérios",
        ("page.findings.criteria", Locale::EnUs) => "About criteria",
        ("page.findings.rules", Locale::PtBr) => "Sobre regras",
        ("page.findings.rules", Locale::EnUs) => "About rules",
        ("page.findings.decisions", Locale::PtBr) => "Sobre decisões",
        ("page.findings.decisions", Locale::EnUs) => "About decisions",
        ("page.findings.skills", Locale::PtBr) => "Sobre skills",
        ("page.findings.skills", Locale::EnUs) => "About skills",
        ("page.findings.others", Locale::PtBr) => "Outros",
        ("page.findings.others", Locale::EnUs) => "Others",
        ("page.block.agreed", Locale::PtBr) => "Combinado",
        ("page.block.agreed", Locale::EnUs) => "Agreed",
        ("page.block.specification", Locale::PtBr) => "Especificação",
        ("page.block.specification", Locale::EnUs) => "Specification",
        ("page.block.criteria", Locale::PtBr) => "Critérios",
        ("page.block.criteria", Locale::EnUs) => "Criteria",
        ("page.block.waves", Locale::PtBr) => "Ondas",
        ("page.block.waves", Locale::EnUs) => "Waves",
        ("page.block.review", Locale::PtBr) => "Revisão e QA",
        ("page.block.review", Locale::EnUs) => "Review and QA",
        ("page.block.progress", Locale::PtBr) => "Andamento",
        ("page.block.progress", Locale::EnUs) => "Progress",
        ("page.block.notes", Locale::PtBr) => "Anotações",
        ("page.block.notes", Locale::EnUs) => "Notes",
        ("page.findings.heading", Locale::PtBr) => "O que o plano achou",
        ("page.findings.heading", Locale::EnUs) => "What the plan found",
        ("page.block.conversation", Locale::PtBr) => "Conversa",
        ("page.block.conversation", Locale::EnUs) => "Conversation",

        ("page.kind.spec", _) => "spec",
        ("page.meta.spec", _) => "spec",

        // A lista dos itens sem dono (`page --spec <nome> --owners`), que o
        // usuário confere antes de os donos serem gravados.
        ("page.kind.owners", Locale::PtBr) => "donos para conferir",
        ("page.kind.owners", Locale::EnUs) => "owners to check",
        ("page.owners.heading", Locale::PtBr) => "Itens sem dono",
        ("page.owners.heading", Locale::EnUs) => "Items without an owner",
        ("page.owners.intro", Locale::PtBr) => {
            "Cada item combinado que ainda não tem dono, com o dono que ele recebe e a regra de onde o \
             dono veio. Nada foi gravado: confira, diga o que muda, e só então os donos são gravados. O \
             pedido de cada onda passa a levar só os itens dela e os do projeto."
        }
        ("page.owners.intro", Locale::EnUs) => {
            "Every agreed item that still has no owner, with the owner it gets and the rule the owner came \
             from. Nothing was recorded: check it, say what changes, and only then are the owners \
             recorded. Each wave's request then carries only its own items and the project's."
        }
        ("page.owners.none", Locale::PtBr) => "Todo item combinado já tem dono.",
        ("page.owners.none", Locale::EnUs) => "Every agreed item already has an owner.",
        ("page.owners.tally", Locale::PtBr) => {
            "{unowned} sem dono · {proposed} pela proposta · {given} pelo orquestrador · {left} ainda sem dono"
        }
        ("page.owners.tally", Locale::EnUs) => {
            "{unowned} without owner · {proposed} by the proposal · {given} by the orchestrator · {left} still \
             without owner"
        }
        ("page.owners.group.tasks", Locale::PtBr) => "Pelas tarefas que apontam o item",
        ("page.owners.group.tasks", Locale::EnUs) => "By the tasks that point to the item",
        ("page.owners.group.cited", Locale::PtBr) => "Pela onda citada no texto",
        ("page.owners.group.cited", Locale::EnUs) => "By the wave the text cites",
        ("page.owners.group.files", Locale::PtBr) => "Pelos arquivos em comum",
        ("page.owners.group.files", Locale::EnUs) => "By the shared files",
        ("page.owners.group.orchestrator", Locale::PtBr) => "Pelo orquestrador",
        ("page.owners.group.orchestrator", Locale::EnUs) => "By the orchestrator",
        ("page.owners.group.nothing", Locale::PtBr) => "Ainda sem dono",
        ("page.owners.group.nothing", Locale::EnUs) => "Still without owner",
        ("page.owners.owner", Locale::PtBr) => "Dono",
        ("page.owners.owner", Locale::EnUs) => "Owner",
        ("page.owners.from", Locale::PtBr) => "De onde veio",
        ("page.owners.from", Locale::EnUs) => "Where it came from",
        ("page.owners.project", Locale::PtBr) => "projeto",
        ("page.owners.project", Locale::EnUs) => "project",
        ("page.owners.wave", Locale::PtBr) => "onda {n}",
        ("page.owners.wave", Locale::EnUs) => "wave {n}",
        ("page.owners.waves", Locale::PtBr) => "ondas {waves}",
        ("page.owners.waves", Locale::EnUs) => "waves {waves}",
        ("page.owners.missing", Locale::PtBr) => "sem dono",
        ("page.owners.missing", Locale::EnUs) => "no owner",
        ("page.owners.from.tasks", Locale::PtBr) => "tarefas que nasceram do item ou citam o código dele: {tasks}",
        ("page.owners.from.tasks", Locale::EnUs) => "tasks born from the item or citing its code: {tasks}",
        ("page.owners.from.cited", Locale::PtBr) => "o texto do item cita a onda",
        ("page.owners.from.cited", Locale::EnUs) => "the item's text cites the wave",
        ("page.owners.from.files", Locale::PtBr) => "arquivos que o item cita e as tarefas da onda mexem: {files}",
        ("page.owners.from.files", Locale::EnUs) => "files the item cites and the wave's tasks touch: {files}",
        ("page.owners.from.orchestrator", Locale::PtBr) => "o orquestrador: {why}",
        ("page.owners.from.orchestrator", Locale::EnUs) => "the orchestrator: {why}",
        ("page.owners.from.nothing", Locale::PtBr) => {
            "nenhuma regra achou dono; o orquestrador classifica este item"
        }
        ("page.owners.from.nothing", Locale::EnUs) => {
            "no rule found an owner; the orchestrator classifies this item"
        }
        ("page.meta.phase", Locale::PtBr) => "fase",
        ("page.meta.phase", Locale::EnUs) => "phase",
        ("page.meta.branch", _) => "branch",
        ("page.meta.base", Locale::PtBr) => "sai de",
        ("page.meta.base", Locale::EnUs) => "cut from",
        ("page.empty", Locale::PtBr) => "Nada registrado ainda.",
        ("page.empty", Locale::EnUs) => "Nothing recorded yet.",
        ("page.replaced", Locale::PtBr) => "versão substituída",
        ("page.replaced", Locale::EnUs) => "replaced version",
        ("page.after_approval", Locale::PtBr) => "depois da aprovação",
        ("page.after_approval", Locale::EnUs) => "after the approval",
        ("page.wave.prompt", Locale::PtBr) => "O pedido da onda {n}",
        ("page.wave.prompt", Locale::EnUs) => "Wave {n}'s request",
        ("page.wave.prompt.summary", Locale::PtBr) => "{lines} linhas, como o agente as recebe",
        ("page.wave.prompt.summary", Locale::EnUs) => "{lines} lines, exactly as the agent gets them",
        ("page.wave.heading", Locale::PtBr) => "Onda {n}",
        ("page.wave.heading", Locale::EnUs) => "Wave {n}",
        ("page.conversation.cut", Locale::PtBr) => {
            "Os {count} registros mais antigos da conversa ficaram só no `spec.md`: com eles, a página \
             passaria de 16 MB, o tamanho que o claude.ai aceita."
        }
        ("page.conversation.cut", Locale::EnUs) => {
            "The {count} oldest conversation entries stayed only in `spec.md`: with them, the page \
             would pass 16 MB, the size claude.ai accepts."
        }
        ("page.withheld_found", Locale::PtBr) => {
            "{count} trechos com cara de segredo saíram da página como \"…\": {codes}. Expurgue cada \
             item com `mustard-rt run write purge` para tirar o trecho também do arquivo."
        }
        ("page.withheld_found", Locale::EnUs) => {
            "{count} excerpts that look like a secret left the page as \"…\": {codes}. Purge each \
             item with `mustard-rt run write purge` to take the excerpt out of the file too."
        }
        ("page.too_big", Locale::PtBr) => {
            "A página tem {bytes} bytes mesmo sem a conversa, e o claude.ai aceita até {max}: assim \
             ela não pode ser publicada."
        }
        ("page.too_big", Locale::EnUs) => {
            "The page has {bytes} bytes even without the conversation, and claude.ai takes up to {max}: \
             it cannot be published like this."
        }
        ("page.wave.sent", Locale::PtBr) => "Pedido enviado ({role}) · {lines} linhas, como o agente o recebeu",
        ("page.wave.sent", Locale::EnUs) => "Request sent ({role}) · {lines} lines, exactly as the agent got it",

        ("page.type.message", Locale::PtBr) => "mensagem",
        ("page.type.message", Locale::EnUs) => "message",
        ("page.type.response", Locale::PtBr) => "resposta",
        ("page.type.response", Locale::EnUs) => "reply",
        ("page.type.injection", Locale::PtBr) => "injeção",
        ("page.type.injection", Locale::EnUs) => "injection",
        ("page.type.hook", Locale::PtBr) => "gancho",
        ("page.type.hook", Locale::EnUs) => "hook",
        ("page.type.call", Locale::PtBr) => "chamada",
        ("page.type.call", Locale::EnUs) => "call",
        ("page.type.state", Locale::PtBr) => "estado",
        ("page.type.state", Locale::EnUs) => "state",
        ("page.type.publish", Locale::PtBr) => "publicação",
        ("page.type.publish", Locale::EnUs) => "publish",
        ("page.type.copy", Locale::PtBr) => "cópia",
        ("page.type.copy", Locale::EnUs) => "copy",
        ("page.type.work_type", Locale::PtBr) => "tipo de trabalho",
        ("page.type.work_type", Locale::EnUs) => "work type",
        ("page.type.point", Locale::PtBr) => "ponto",
        ("page.type.point", Locale::EnUs) => "point",
        ("page.type.rule", Locale::PtBr) => "regra",
        ("page.type.rule", Locale::EnUs) => "rule",
        ("page.type.limit", Locale::PtBr) => "limite",
        ("page.type.limit", Locale::EnUs) => "limit",
        ("page.type.contract", Locale::PtBr) => "contrato",
        ("page.type.contract", Locale::EnUs) => "contract",
        ("page.type.error", Locale::PtBr) => "erro",
        ("page.type.error", Locale::EnUs) => "error",
        ("page.type.edge_case", Locale::PtBr) => "caso de borda",
        ("page.type.edge_case", Locale::EnUs) => "edge case",
        ("page.type.out_of_scope", Locale::PtBr) => "fora do escopo",
        ("page.type.out_of_scope", Locale::EnUs) => "out of scope",
        ("page.type.decision", Locale::PtBr) => "decisão",
        ("page.type.decision", Locale::EnUs) => "decision",
        ("page.type.context", Locale::PtBr) => "contexto",
        ("page.type.context", Locale::EnUs) => "context",
        ("page.type.concern", Locale::PtBr) => "preocupação",
        ("page.type.concern", Locale::EnUs) => "concern",
        ("page.type.criterion", Locale::PtBr) => "critério",
        ("page.type.criterion", Locale::EnUs) => "criterion",
        ("page.type.criterion_run", Locale::PtBr) => "execução de critério",
        ("page.type.criterion_run", Locale::EnUs) => "criterion run",
        ("page.type.wave", Locale::PtBr) => "onda",
        ("page.type.wave", Locale::EnUs) => "wave",
        ("page.type.task", Locale::PtBr) => "tarefa",
        ("page.type.task", Locale::EnUs) => "task",
        ("page.type.skill", _) => "skill",
        ("page.type.send", Locale::PtBr) => "envio",
        ("page.type.send", Locale::EnUs) => "send",
        ("page.type.delivered", Locale::PtBr) => "entregou",
        ("page.type.delivered", Locale::EnUs) => "delivered",
        ("page.type.verdict", Locale::PtBr) => "veredito",
        ("page.type.verdict", Locale::EnUs) => "verdict",
        ("page.type.commit", _) => "commit",
        ("page.type.pr_summary", Locale::PtBr) => "resumo do pull request",
        ("page.type.pr_summary", Locale::EnUs) => "pull request summary",
        ("page.type.request", Locale::PtBr) => "pedido",
        ("page.type.request", Locale::EnUs) => "request",
        ("page.type.deferred", Locale::PtBr) => "pedido adiado",
        ("page.type.deferred", Locale::EnUs) => "deferred request",
        ("page.type.note", Locale::PtBr) => "anotação",
        ("page.type.note", Locale::EnUs) => "note",
        ("page.type.remove", Locale::PtBr) => "remoção",
        ("page.type.remove", Locale::EnUs) => "removal",
        ("page.type.purge", Locale::PtBr) => "expurgo",
        ("page.type.purge", Locale::EnUs) => "purge",

        ("page.group.work_type", Locale::PtBr) => "Tipo de trabalho",
        ("page.group.work_type", Locale::EnUs) => "Work type",
        ("page.group.point", Locale::PtBr) => "Pontos do levantamento",
        ("page.group.point", Locale::EnUs) => "Survey points",
        ("page.group.rule", Locale::PtBr) => "Regras",
        ("page.group.rule", Locale::EnUs) => "Rules",
        ("page.group.limit", Locale::PtBr) => "Limites",
        ("page.group.limit", Locale::EnUs) => "Limits",
        ("page.group.contract", Locale::PtBr) => "Contratos",
        ("page.group.contract", Locale::EnUs) => "Contracts",
        ("page.group.error", Locale::PtBr) => "Erros e mensagens",
        ("page.group.error", Locale::EnUs) => "Errors and messages",
        ("page.group.edge_case", Locale::PtBr) => "Casos de borda",
        ("page.group.edge_case", Locale::EnUs) => "Edge cases",
        ("page.group.out_of_scope", Locale::PtBr) => "Fora do escopo",
        ("page.group.out_of_scope", Locale::EnUs) => "Out of scope",
        ("page.group.decision", Locale::PtBr) => "Decisões",
        ("page.group.decision", Locale::EnUs) => "Decisions",
        ("page.group.context", Locale::PtBr) => "Contexto",
        ("page.group.context", Locale::EnUs) => "Context",
        ("page.group.concern", Locale::PtBr) => "Preocupações",
        ("page.group.concern", Locale::EnUs) => "Concerns",
        ("page.group.criterion_run", Locale::PtBr) => "Execuções",
        ("page.group.criterion_run", Locale::EnUs) => "Runs",
        ("page.group.skill", _) => "Skills",

        ("page.field.excerpt", Locale::PtBr) => "trecho",
        ("page.field.excerpt", Locale::EnUs) => "excerpt",
        ("page.field.reply_to", Locale::PtBr) => "Responde a",
        ("page.field.reply_to", Locale::EnUs) => "Replies to",
        ("page.field.hook", Locale::PtBr) => "Gancho",
        ("page.field.hook", Locale::EnUs) => "Hook",
        ("page.field.chars", Locale::PtBr) => "Caracteres",
        ("page.field.chars", Locale::EnUs) => "Characters",
        ("page.field.action", Locale::PtBr) => "Ação",
        ("page.field.action", Locale::EnUs) => "Action",
        ("page.field.tool", Locale::PtBr) => "Ferramenta",
        ("page.field.tool", Locale::EnUs) => "Tool",
        ("page.field.reason", Locale::PtBr) => "Motivo",
        ("page.field.reason", Locale::EnUs) => "Reason",
        ("page.field.command", Locale::PtBr) => "Comando",
        ("page.field.command", Locale::EnUs) => "Command",
        ("page.field.ms", Locale::PtBr) => "Tempo (ms)",
        ("page.field.ms", Locale::EnUs) => "Time (ms)",
        ("page.field.result", Locale::PtBr) => "Resultado",
        ("page.field.result", Locale::EnUs) => "Result",
        ("page.field.refusal", Locale::PtBr) => "Recusa",
        ("page.field.refusal", Locale::EnUs) => "Refusal",
        ("page.field.phase", Locale::PtBr) => "Fase",
        ("page.field.phase", Locale::EnUs) => "Phase",
        ("page.field.branch", _) => "Branch",
        ("page.field.base", _) => "Base",
        ("page.field.witness", Locale::PtBr) => "Pergunta e resposta",
        ("page.field.witness", Locale::EnUs) => "Question and answer",
        ("page.field.pr", _) => "Pull request",
        ("page.field.page", Locale::PtBr) => "Página",
        ("page.field.page", Locale::EnUs) => "Page",
        ("page.field.milestone", Locale::PtBr) => "Marco",
        ("page.field.milestone", Locale::EnUs) => "Milestone",
        ("page.field.ok", Locale::PtBr) => "Deu certo",
        ("page.field.ok", Locale::EnUs) => "Succeeded",
        ("page.field.url", Locale::PtBr) => "Endereço",
        ("page.field.url", Locale::EnUs) => "Address",
        ("page.field.template", Locale::PtBr) => "Template do Mustard",
        ("page.field.template", Locale::EnUs) => "Mustard template",
        ("page.field.last", Locale::PtBr) => "Último item copiado",
        ("page.field.last", Locale::EnUs) => "Last item copied",
        ("page.field.kinds", Locale::PtBr) => "Tipos",
        ("page.field.kinds", Locale::EnUs) => "Kinds",
        ("page.field.block", Locale::PtBr) => "Grupo de lacunas",
        ("page.field.block", Locale::EnUs) => "Gap group",
        ("page.field.gap", Locale::PtBr) => "Lacuna",
        ("page.field.gap", Locale::EnUs) => "Gap",
        ("page.field.from", Locale::PtBr) => "De onde veio",
        ("page.field.from", Locale::EnUs) => "Came from",
        ("page.field.status", Locale::PtBr) => "Situação",
        ("page.field.status", Locale::EnUs) => "Status",
        ("page.field.facts", Locale::PtBr) => "Fatos",
        ("page.field.facts", Locale::EnUs) => "Facts",
        ("page.field.closes", Locale::PtBr) => "Fecha",
        ("page.field.closes", Locale::EnUs) => "Closes",
        ("page.field.reminders", Locale::PtBr) => "Lembretes",
        ("page.field.reminders", Locale::EnUs) => "Reminders",
        ("page.field.example", Locale::PtBr) => "Exemplo",
        ("page.field.example", Locale::EnUs) => "Example",
        ("page.field.applies_to", Locale::PtBr) => "Vale para",
        ("page.field.applies_to", Locale::EnUs) => "Applies to",
        ("page.field.order", Locale::PtBr) => "Ordem de execução",
        ("page.field.order", Locale::EnUs) => "Execution order",
        ("page.field.no_code", Locale::PtBr) => "Não vira código",
        ("page.field.no_code", Locale::EnUs) => "Does not become code",
        ("page.field.value", Locale::PtBr) => "Valor",
        ("page.field.value", Locale::EnUs) => "Value",
        ("page.field.message", Locale::PtBr) => "Mensagem",
        ("page.field.message", Locale::EnUs) => "Message",
        ("page.field.expected", Locale::PtBr) => "O que acontece",
        ("page.field.expected", Locale::EnUs) => "What happens",
        ("page.field.why", Locale::PtBr) => "Por quê",
        ("page.field.why", Locale::EnUs) => "Why",
        ("page.field.when", Locale::PtBr) => "Quando",
        ("page.field.when", Locale::EnUs) => "When",
        ("page.field.then", Locale::PtBr) => "Então",
        ("page.field.then", Locale::EnUs) => "Then",
        ("page.field.proof", Locale::PtBr) => "Prova",
        ("page.field.proof", Locale::EnUs) => "Proof",
        ("page.field.contracts", Locale::PtBr) => "Contratos",
        ("page.field.contracts", Locale::EnUs) => "Contracts",
        ("page.field.criterion", Locale::PtBr) => "Critério",
        ("page.field.criterion", Locale::EnUs) => "Criterion",
        ("page.field.exit", Locale::PtBr) => "Código de saída",
        ("page.field.exit", Locale::EnUs) => "Exit code",
        ("page.field.output", Locale::PtBr) => "Saída",
        ("page.field.output", Locale::EnUs) => "Output",
        ("page.field.n", Locale::PtBr) => "Número",
        ("page.field.n", Locale::EnUs) => "Number",
        ("page.field.criteria", Locale::PtBr) => "Critérios",
        ("page.field.criteria", Locale::EnUs) => "Criteria",
        ("page.field.done_when", Locale::PtBr) => "Pronta quando",
        ("page.field.done_when", Locale::EnUs) => "Done when",
        ("page.field.depends_on", Locale::PtBr) => "Depende das ondas",
        ("page.field.depends_on", Locale::EnUs) => "Depends on waves",
        ("page.field.wave", Locale::PtBr) => "Onda",
        ("page.field.wave", Locale::EnUs) => "Wave",
        ("page.field.files", Locale::PtBr) => "Arquivos",
        ("page.field.files", Locale::EnUs) => "Files",
        ("page.field.skill", _) => "Skill",
        ("page.field.covers", Locale::PtBr) => "Cobre",
        ("page.field.covers", Locale::EnUs) => "Covers",
        ("page.field.points", Locale::PtBr) => "Nota",
        ("page.field.points", Locale::EnUs) => "Points",
        ("page.field.must_read", Locale::PtBr) => "Precisa ler",
        ("page.field.must_read", Locale::EnUs) => "Must read",
        ("page.field.name", Locale::PtBr) => "Nome",
        ("page.field.name", Locale::EnUs) => "Name",
        ("page.field.sha", Locale::PtBr) => "Identificador",
        ("page.field.sha", Locale::EnUs) => "Identifier",
        ("page.field.examples", Locale::PtBr) => "Exemplos",
        ("page.field.examples", Locale::EnUs) => "Examples",
        ("page.field.role", Locale::PtBr) => "Papel",
        ("page.field.role", Locale::EnUs) => "Role",
        ("page.field.lines", Locale::PtBr) => "Linhas",
        ("page.field.lines", Locale::EnUs) => "Lines",
        ("page.field.items", Locale::PtBr) => "Itens enviados",
        ("page.field.items", Locale::EnUs) => "Items sent",
        ("page.field.mustard", Locale::PtBr) => "Versão do Mustard",
        ("page.field.mustard", Locale::EnUs) => "Mustard version",
        ("page.field.lessons", Locale::PtBr) => "Lições",
        ("page.field.lessons", Locale::EnUs) => "Lessons",
        ("page.field.final", Locale::PtBr) => "Revisão final",
        ("page.field.final", Locale::EnUs) => "Final review",
        ("page.field.skills", _) => "Skills",
        ("page.field.copy", Locale::PtBr) => "Cópia separada",
        ("page.field.copy", Locale::EnUs) => "Separate copy",
        ("page.field.build_dir", Locale::PtBr) => "Pasta de compilação",
        ("page.field.build_dir", Locale::EnUs) => "Build folder",
        ("page.field.analysis", Locale::PtBr) => "Análise antes do envio",
        ("page.field.analysis", Locale::EnUs) => "Analysis before sending",
        ("page.field.title", Locale::PtBr) => "Título",
        ("page.field.title", Locale::EnUs) => "Title",
        ("page.field.waves", Locale::PtBr) => "Ondas",
        ("page.field.waves", Locale::EnUs) => "Waves",
        ("page.field.repo", Locale::PtBr) => "Repositório",
        ("page.field.repo", Locale::EnUs) => "Repository",
        ("page.field.effect", Locale::PtBr) => "Efeito",
        ("page.field.effect", Locale::EnUs) => "Effect",
        ("page.field.pending", Locale::PtBr) => "Pendência",
        ("page.field.pending", Locale::EnUs) => "Pending item",
        ("page.field.targets", Locale::PtBr) => "Itens",
        ("page.field.targets", Locale::EnUs) => "Items",
        ("page.field.filter", Locale::PtBr) => "Filtro",
        ("page.field.filter", Locale::EnUs) => "Filter",
        ("page.field.origin", Locale::PtBr) => "Origem",
        ("page.field.origin", Locale::EnUs) => "Origin",
        ("page.field.label", Locale::PtBr) => "Rótulo no rascunho",
        ("page.field.label", Locale::EnUs) => "Draft label",
        ("page.field.last_run", Locale::PtBr) => "Última execução",
        ("page.field.last_run", Locale::EnUs) => "Last run",
        ("page.field.closed_by", Locale::PtBr) => "Fechado por",
        ("page.field.closed_by", Locale::EnUs) => "Closed by",
        ("page.field.wave_state", Locale::PtBr) => "Estado da onda",
        ("page.field.wave_state", Locale::EnUs) => "Wave state",
        ("page.field.wave_commit", Locale::PtBr) => "Commit",
        ("page.field.wave_commit", Locale::EnUs) => "Commit",
        ("page.field.wave_receives", Locale::PtBr) => "Recebe",
        ("page.field.wave_receives", Locale::EnUs) => "Receives",
        ("page.field.wave_points", Locale::PtBr) => "Soma das notas",
        ("page.field.wave_points", Locale::EnUs) => "Points total",

        ("page.value.warn", Locale::PtBr) => "aviso",
        ("page.value.warn", Locale::EnUs) => "warning",
        ("page.value.block", Locale::PtBr) => "bloqueio",
        ("page.value.block", Locale::EnUs) => "block",
        ("page.value.ok", _) => "ok",
        ("page.value.refused", Locale::PtBr) => "recusada",
        ("page.value.refused", Locale::EnUs) => "refused",
        ("page.value.spec", _) => "spec",
        ("page.value.project", Locale::PtBr) => "projeto",
        ("page.value.project", Locale::EnUs) => "project",
        ("page.value.approval", Locale::PtBr) => "aprovação",
        ("page.value.approval", Locale::EnUs) => "approval",
        ("page.value.round", Locale::PtBr) => "rodada",
        ("page.value.round", Locale::EnUs) => "round",
        ("page.value.close", Locale::PtBr) => "fechamento",
        ("page.value.close", Locale::EnUs) => "close",
        ("page.value.feature", Locale::PtBr) => "funcionalidade",
        ("page.value.feature", Locale::EnUs) => "feature",
        ("page.value.fix", Locale::PtBr) => "correção",
        ("page.value.fix", Locale::EnUs) => "fix",
        ("page.value.refactor", Locale::PtBr) => "refatoração",
        ("page.value.refactor", Locale::EnUs) => "refactor",
        ("page.value.gap", Locale::PtBr) => "lacuna",
        ("page.value.gap", Locale::EnUs) => "gap",
        ("page.value.lesson", Locale::PtBr) => "lição",
        ("page.value.lesson", Locale::EnUs) => "lesson",
        ("page.value.prior_spec", Locale::PtBr) => "spec anterior",
        ("page.value.prior_spec", Locale::EnUs) => "prior spec",
        ("page.value.code_conflict", Locale::PtBr) => "conflito no código",
        ("page.value.code_conflict", Locale::EnUs) => "code conflict",
        ("page.value.outside_review", Locale::PtBr) => "revisor de fora",
        ("page.value.outside_review", Locale::EnUs) => "outside review",
        ("page.value.open", Locale::PtBr) => "pendente",
        ("page.value.open", Locale::EnUs) => "open",
        ("page.value.closed", Locale::PtBr) => "✓ fechado",
        ("page.value.closed", Locale::EnUs) => "✓ closed",
        ("page.value.not_applicable", Locale::PtBr) => "não se aplica",
        ("page.value.not_applicable", Locale::EnUs) => "not applicable",
        ("page.value.pass", Locale::PtBr) => "passou",
        ("page.value.pass", Locale::EnUs) => "passed",
        ("page.value.fail", Locale::PtBr) => "falhou",
        ("page.value.fail", Locale::EnUs) => "failed",
        ("page.value.create", Locale::PtBr) => "criação",
        ("page.value.create", Locale::EnUs) => "created",
        ("page.value.change", Locale::PtBr) => "mudança",
        ("page.value.change", Locale::EnUs) => "changed",
        ("page.value.drop", Locale::PtBr) => "remoção",
        ("page.value.drop", Locale::EnUs) => "dropped",
        ("page.value.wave", Locale::PtBr) => "agente de onda",
        ("page.value.wave", Locale::EnUs) => "wave agent",
        ("page.value.review", Locale::PtBr) => "revisor",
        ("page.value.review", Locale::EnUs) => "reviewer",
        ("page.value.skill", Locale::PtBr) => "autor de skill",
        ("page.value.skill", Locale::EnUs) => "skill author",
        ("page.value.approved", Locale::PtBr) => "aprovada",
        ("page.value.approved", Locale::EnUs) => "approved",
        ("page.value.rejected", Locale::PtBr) => "reprovada",
        ("page.value.rejected", Locale::EnUs) => "rejected",
        ("page.value.new_waves", Locale::PtBr) => "ondas novas",
        ("page.value.new_waves", Locale::EnUs) => "new waves",
        ("page.value.adjust_waves", Locale::PtBr) => "ajusta as ondas",
        ("page.value.adjust_waves", Locale::EnUs) => "adjusts the waves",
        ("page.value.secret", Locale::PtBr) => "segredo",
        ("page.value.secret", Locale::EnUs) => "secret",
        ("page.value.client_data", Locale::PtBr) => "dado de cliente",
        ("page.value.client_data", Locale::EnUs) => "client data",
        ("page.value.yes", Locale::PtBr) => "sim",
        ("page.value.yes", Locale::EnUs) => "yes",
        ("page.value.no", Locale::PtBr) => "não",
        ("page.value.no", Locale::EnUs) => "no",
        ("page.value.new", Locale::PtBr) => "novo",
        ("page.value.new", Locale::EnUs) => "new",
        ("page.value.tests_rule", Locale::PtBr) => "confere a regra",
        ("page.value.tests_rule", Locale::EnUs) => "tests the rule",
        ("page.value.not_tests_rule", Locale::PtBr) => "não confere a regra",
        ("page.value.not_tests_rule", Locale::EnUs) => "does not test the rule",
        ("page.value.repeated", Locale::PtBr) => "repetiu",
        ("page.value.repeated", Locale::EnUs) => "repeated",
        ("page.value.not_repeated", Locale::PtBr) => "não repetiu",
        ("page.value.not_repeated", Locale::EnUs) => "did not repeat",
        // O estado de cada onda, pela leitura da rodada; `.many` é a forma
        // da conta da visão das ondas ("3 entregues").
        ("page.value.wave_todo", Locale::PtBr) => "a fazer",
        ("page.value.wave_todo", Locale::EnUs) => "to do",
        ("page.value.wave_todo.many", Locale::PtBr) => "a fazer",
        ("page.value.wave_todo.many", Locale::EnUs) => "to do",
        ("page.value.wave_running", Locale::PtBr) => "em andamento",
        ("page.value.wave_running", Locale::EnUs) => "in progress",
        ("page.value.wave_running.many", Locale::PtBr) => "em andamento",
        ("page.value.wave_running.many", Locale::EnUs) => "in progress",
        ("page.value.wave_delivered", Locale::PtBr) => "entregue",
        ("page.value.wave_delivered", Locale::EnUs) => "delivered",
        ("page.value.wave_delivered.many", Locale::PtBr) => "entregues",
        ("page.value.wave_delivered.many", Locale::EnUs) => "delivered",
        ("page.value.wave_approved", Locale::PtBr) => "aprovada",
        ("page.value.wave_approved", Locale::EnUs) => "approved",
        ("page.value.wave_approved.many", Locale::PtBr) => "aprovadas",
        ("page.value.wave_approved.many", Locale::EnUs) => "approved",
        ("page.value.wave_rejected", Locale::PtBr) => "reprovada",
        ("page.value.wave_rejected", Locale::EnUs) => "rejected",
        ("page.value.wave_rejected.many", Locale::PtBr) => "reprovadas",
        ("page.value.wave_rejected.many", Locale::EnUs) => "rejected",

        ("page.phase.survey", Locale::PtBr) => "levantamento",
        ("page.phase.survey", Locale::EnUs) => "survey",
        ("page.phase.plan", Locale::PtBr) => "plano",
        ("page.phase.plan", Locale::EnUs) => "plan",
        ("page.phase.approved", Locale::PtBr) => "aprovada",
        ("page.phase.approved", Locale::EnUs) => "approved",
        ("page.phase.running", Locale::PtBr) => "em execução",
        ("page.phase.running", Locale::EnUs) => "running",
        ("page.phase.closed", Locale::PtBr) => "fechada",
        ("page.phase.closed", Locale::EnUs) => "closed",
        ("page.phase.pr_open", Locale::PtBr) => "pull request aberto",
        ("page.phase.pr_open", Locale::EnUs) => "pull request open",
        ("page.phase.delivered", Locale::PtBr) => "entregue",
        ("page.phase.delivered", Locale::EnUs) => "delivered",
        ("page.phase.discarded", Locale::PtBr) => "descartada",
        ("page.phase.discarded", Locale::EnUs) => "discarded",

        ("page.author.user", Locale::PtBr) => "usuário",
        ("page.author.user", Locale::EnUs) => "user",
        ("page.author.assistant", Locale::PtBr) => "assistente",
        ("page.author.assistant", Locale::EnUs) => "assistant",
        ("page.author.hook", Locale::PtBr) => "gancho",
        ("page.author.hook", Locale::EnUs) => "hook",
        ("page.author.binary", Locale::PtBr) => "binário",
        ("page.author.binary", Locale::EnUs) => "binary",
        ("page.author.wave", Locale::PtBr) => "agente de onda",
        ("page.author.wave", Locale::EnUs) => "wave agent",
        ("page.author.review", Locale::PtBr) => "revisor",
        ("page.author.review", Locale::EnUs) => "reviewer",
        ("page.author.skill", Locale::PtBr) => "autor de skill",
        ("page.author.skill", Locale::EnUs) => "skill author",

        ("page.metrics.col.measure", Locale::PtBr) => "Medida",
        ("page.metrics.col.measure", Locale::EnUs) => "Measure",
        ("page.metrics.col.value", Locale::PtBr) => "Valor",
        ("page.metrics.col.value", Locale::EnUs) => "Value",
        ("page.metrics.col.wave", Locale::PtBr) => "Onda",
        ("page.metrics.col.wave", Locale::EnUs) => "Wave",
        ("page.metrics.col.lines", Locale::PtBr) => "Linhas do pedido",
        ("page.metrics.col.lines", Locale::EnUs) => "Request lines",
        ("page.metrics.col.chars", Locale::PtBr) => "Caracteres do pedido",
        ("page.metrics.col.chars", Locale::EnUs) => "Request characters",
        ("page.metrics.col.items", Locale::PtBr) => "Itens lidos",
        ("page.metrics.col.items", Locale::EnUs) => "Items read",
        ("page.metrics.col.delivery", Locale::PtBr) => "Tempo até a entrega",
        ("page.metrics.col.delivery", Locale::EnUs) => "Time to delivery",
        ("page.metrics.col.rejected", Locale::PtBr) => "Reprovações",
        ("page.metrics.col.rejected", Locale::EnUs) => "Rejections",
        ("page.metrics.col.last", Locale::PtBr) => "Última revisão",
        ("page.metrics.col.last", Locale::EnUs) => "Last review",
        ("page.metrics.by_wave", Locale::PtBr) => "Medida por onda",
        ("page.metrics.by_wave", Locale::EnUs) => "Measure per wave",
        ("page.metrics.calls", Locale::PtBr) => "Passos do fluxo contra trabalho",
        ("page.metrics.calls", Locale::EnUs) => "Flow steps against work",
        ("page.metrics.calls.value", Locale::PtBr) => {
            "{count} chamadas, {refused} recusadas, para {done} ondas prontas"
        }
        ("page.metrics.calls.value", Locale::EnUs) => "{count} calls, {refused} refused, for {done} waves done",
        ("page.metrics.phases", Locale::PtBr) => "Tempo por fase",
        ("page.metrics.phases", Locale::EnUs) => "Time per phase",
        ("page.metrics.rework", Locale::PtBr) => "; voltaram da revisão as ondas {waves}",
        ("page.metrics.rework", Locale::EnUs) => "; waves sent back by the review: {waves}",
        ("page.metrics.reminders", Locale::PtBr) => "Lembretes que apareceram",
        ("page.metrics.reminders", Locale::EnUs) => "Reminders that showed up",
        ("page.metrics.reminders.value", Locale::PtBr) => "{count} mensagens antigas lembradas nos pontos",
        ("page.metrics.reminders.value", Locale::EnUs) => "{count} old messages recalled in the points",
        ("page.metrics.rtk", Locale::PtBr) => "Economia do rtk",
        ("page.metrics.rtk", Locale::EnUs) => "rtk savings",
        ("page.metrics.rtk.value", Locale::PtBr) => {
            "{commands} comandos, {saved} tokens a menos na saída ({pct}%), de {from} a {to}"
        }
        ("page.metrics.rtk.value", Locale::EnUs) => {
            "{commands} commands, {saved} fewer output tokens ({pct}%), from {from} to {to}"
        }
        ("page.metrics.hooks", Locale::PtBr) => "Bloqueios por gancho",
        ("page.metrics.hooks", Locale::EnUs) => "Blocks per hook",
        ("page.metrics.hooks.value", Locale::PtBr) => "{blocks} bloqueios, {warns} avisos",
        ("page.metrics.hooks.value", Locale::EnUs) => "{blocks} blocks, {warns} warnings",
        ("page.metrics.injected", Locale::PtBr) => "Texto colocado pelos ganchos",
        ("page.metrics.injected", Locale::EnUs) => "Text added by hooks",
        ("page.metrics.injected.value", Locale::PtBr) => "{chars} caracteres, cerca de {tokens} tokens",
        ("page.metrics.injected.value", Locale::EnUs) => "{chars} characters, about {tokens} tokens",
        ("page.metrics.sends", Locale::PtBr) => "Pedidos enviados aos agentes",
        ("page.metrics.sends", Locale::EnUs) => "Requests sent to agents",
        ("page.metrics.sends.value", Locale::PtBr) => "{count}, o maior com {lines} linhas",
        ("page.metrics.sends.value", Locale::EnUs) => "{count}, the largest with {lines} lines",
        ("page.metrics.verdicts", Locale::PtBr) => "Revisões",
        ("page.metrics.verdicts", Locale::EnUs) => "Reviews",
        ("page.metrics.verdicts.value", Locale::PtBr) => "{approved} aprovadas, {rejected} reprovadas",
        ("page.metrics.verdicts.value", Locale::EnUs) => "{approved} approved, {rejected} rejected",
        ("page.metrics.points", Locale::PtBr) => "Pontos do levantamento",
        ("page.metrics.points", Locale::EnUs) => "Survey points",
        ("page.metrics.points.value", Locale::PtBr) => "{open} pendentes, {closed} fechados",
        ("page.metrics.points.value", Locale::EnUs) => "{open} open, {closed} closed",

        // A página do projeto (`view::document::project`).
        ("project.kind", Locale::PtBr) => "projeto",
        ("project.kind", Locale::EnUs) => "project",
        ("project.specs", Locale::PtBr) => "Specs",
        ("project.specs", Locale::EnUs) => "Specs",
        ("project.stages", Locale::PtBr) => "Por fase: {stages}.",
        ("project.stages", Locale::EnUs) => "By phase: {stages}.",
        ("project.no_phase", Locale::PtBr) => "sem fase",
        ("project.no_phase", Locale::EnUs) => "with no phase",
        ("project.col.state", Locale::PtBr) => "Estado",
        ("project.col.state", Locale::EnUs) => "State",
        ("project.col.branch", _) => "Branch",
        ("project.col.created", Locale::PtBr) => "Aberta em",
        ("project.col.created", Locale::EnUs) => "Opened",
        ("project.col.updated", Locale::PtBr) => "Última mudança",
        ("project.col.updated", Locale::EnUs) => "Last change",
        ("project.stalled", Locale::PtBr) => "Specs paradas",
        ("project.stalled", Locale::EnUs) => "Idle specs",
        ("project.stalled.line", Locale::PtBr) => "{spec} ({phase}): parada desde {since}, há {days} d",
        ("project.stalled.line", Locale::EnUs) => "{spec} ({phase}): idle since {since}, {days} d ago",
        ("project.titles", Locale::PtBr) => "Regras e decisões de cada spec",
        ("project.titles", Locale::EnUs) => "Each spec's rules and decisions",
        ("project.titles.summary", Locale::PtBr) => "{spec} · {count} títulos",
        ("project.titles.summary", Locale::EnUs) => "{spec} · {count} titles",
        ("project.meta.specs", _) => "specs",
        ("project.meta.today", Locale::PtBr) => "em",
        ("project.meta.today", Locale::EnUs) => "on",
        ("project.footer", Locale::PtBr) => "Índice das specs: {path}",
        ("project.footer", Locale::EnUs) => "Spec index: {path}",

        // As recusas do comando `page`.
        ("page.missing_body", Locale::PtBr) => {
            "Diga o que gerar: --spec <nome> para a página de uma spec, ou --body \
             <arquivo.md> com --out <página.html> para uma página avulsa."
        }
        ("page.missing_body", Locale::EnUs) => {
            "Say what to build: --spec <name> for a spec's page, or --body <file.md> \
             with --out <page.html> for a standalone page."
        }
        ("page.unreadable_body", Locale::PtBr) => {
            "Não consegui ler {path}. Passe em --body um arquivo de texto UTF-8 com o \
             markdown da página."
        }
        ("page.unreadable_body", Locale::EnUs) => {
            "Could not read {path}. Pass --body a readable UTF-8 file holding the \
             page's markdown."
        }
        ("page.empty_title", Locale::PtBr) => {
            "A página não tem título. Passe --title ou comece o markdown com uma linha \
             \"# Título\"."
        }
        ("page.empty_title", Locale::EnUs) => {
            "The page has no title. Pass --title or start the markdown with a \
             \"# Title\" line."
        }
        ("page.write_failed", Locale::PtBr) => "Não consegui gravar {path}: {detail}.",
        ("page.write_failed", Locale::EnUs) => "Could not write {path}: {detail}.",
        ("page.owners.unreadable", Locale::PtBr) => {
            "Não consegui ler {path}. Passe em --owners um arquivo JSON com uma lista de linhas \
             `{\"code\": \"<código>\", \"waves\": [<ondas>], \"why\": \"<motivo>\"}`, ou com \
             `\"applies_to\": {\"files\": [\"**\"]}` no lugar de `waves` para o item do projeto. Nada foi \
             gravado."
        }
        ("page.owners.unreadable", Locale::EnUs) => {
            "Could not read {path}. Pass --owners a JSON file holding a list of lines \
             `{\"code\": \"<item code>\", \"waves\": [<waves>], \"why\": \"<reason>\"}`, or with \
             `\"applies_to\": {\"files\": [\"**\"]}` instead of `waves` for a project item. Nothing was \
             written."
        }
        ("page.owners.bad_line", Locale::PtBr) => {
            "A linha de {code} em {path} não serve: o código tem de ser de um item sem dono, o dono tem \
             de ser ondas do plano em `waves` ou o projeto em `\"applies_to\": {\"files\": [\"**\"]}`, e o \
             motivo vai em `why`. Nada foi gravado."
        }
        ("page.owners.bad_line", Locale::EnUs) => {
            "The line for {code} in {path} does not fit: the code must be an item without owner, the \
             owner must be planned waves in `waves` or the project in \
             `\"applies_to\": {\"files\": [\"**\"]}`, and the reason goes in `why`. Nothing was written."
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
            include_str!("page.rs"),
            super::PREFIXES,
            332,
            0x47a2_d70d_4276_d924,
        );
    }
}
