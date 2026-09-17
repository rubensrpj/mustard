//! As páginas: a da spec e o `.md` dela (blocos, tipos, campos, valores, fases,
//! autores e o painel de medição), a do projeto, o antigo resumo em HTML, a
//! publicação nos marcos e as recusas do comando `page`.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["page", "project", "doc"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Resumo da spec em HTML (`apps/rt/src/commands/spec/spec_doc.rs`) — o
        // documento que o usuário lê ANTES de aprovar. Texto de usuário, então
        // segue o `language.text` e mora aqui, não embutido no comando. `{wave}` e
        // `{term}` são preenchidos pelo chamador.
        ("doc.kind.approval", Locale::PtBr) => "spec para aprovar",
        ("doc.kind.approval", Locale::EnUs) => "spec awaiting approval",
        ("doc.kind.summary", Locale::PtBr) => "resumo da spec",
        ("doc.kind.summary", Locale::EnUs) => "spec summary",
        ("doc.meta.spec", _) => "spec",
        ("doc.meta.branch", _) => "branch",
        ("doc.meta.base", Locale::PtBr) => "sai de",
        ("doc.meta.base", Locale::EnUs) => "cut from",
        ("doc.stage.analyze", Locale::PtBr) => "em análise",
        ("doc.stage.analyze", Locale::EnUs) => "under analysis",
        ("doc.stage.awaiting", Locale::PtBr) => "aguardando aprovação",
        ("doc.stage.awaiting", Locale::EnUs) => "awaiting approval",
        ("doc.stage.approved", Locale::PtBr) => "aprovada, execução por começar",
        ("doc.stage.approved", Locale::EnUs) => "approved, execution not started",
        ("doc.stage.execute", Locale::PtBr) => "em execução",
        ("doc.stage.execute", Locale::EnUs) => "executing",
        ("doc.stage.review", Locale::PtBr) => "em revisão",
        ("doc.stage.review", Locale::EnUs) => "under review",
        ("doc.stage.verify", Locale::PtBr) => "em verificação",
        ("doc.stage.verify", Locale::EnUs) => "under verification",
        ("doc.stage.close", Locale::PtBr) => "fechando",
        ("doc.stage.close", Locale::EnUs) => "closing",
        ("doc.stage.completed", Locale::PtBr) => "fechada",
        ("doc.stage.completed", Locale::EnUs) => "closed",
        ("doc.summary.lead", Locale::PtBr) => "Resumo.",
        ("doc.summary.lead", Locale::EnUs) => "Summary.",
        ("doc.section.where", Locale::PtBr) => "Onde estamos",
        ("doc.section.where", Locale::EnUs) => "Where we are",
        ("doc.section.clarified", Locale::PtBr) => "O que foi esclarecido",
        ("doc.section.clarified", Locale::EnUs) => "What was clarified",
        ("doc.section.decisions", Locale::PtBr) => "Decisões",
        ("doc.section.decisions", Locale::EnUs) => "Decisions",
        ("doc.section.risks", Locale::PtBr) => "Riscos",
        ("doc.section.risks", Locale::EnUs) => "Risks",
        ("doc.section.flow", Locale::PtBr) => "Antes e depois",
        ("doc.section.flow", Locale::EnUs) => "Before and after",
        ("doc.section.spec", Locale::PtBr) => "A spec",
        ("doc.section.spec", Locale::EnUs) => "The spec",
        ("doc.section.criteria", Locale::PtBr) => "Critérios de aceite",
        ("doc.section.criteria", Locale::EnUs) => "Acceptance criteria",
        ("doc.section.waves", Locale::PtBr) => "Ondas e skills",
        ("doc.section.waves", Locale::EnUs) => "Waves and skills",
        ("doc.section.evidence", Locale::PtBr) => "Evidências",
        ("doc.section.evidence", Locale::EnUs) => "Evidence",
        ("doc.section.pending", Locale::PtBr) => "Pendências abertas",
        ("doc.section.pending", Locale::EnUs) => "Open pending items",
        ("doc.section.next", Locale::PtBr) => "Próximo passo",
        ("doc.section.next", Locale::EnUs) => "Next step",
        // Os sete passos do processo, na ordem em que acontecem.
        ("doc.step.analyze.name", Locale::PtBr) => "Análise",
        ("doc.step.analyze.name", Locale::EnUs) => "Analysis",
        ("doc.step.analyze.desc", Locale::PtBr) => "ler o código e a conversa.",
        ("doc.step.analyze.desc", Locale::EnUs) => "read the code and the conversation.",
        ("doc.step.plan.name", Locale::PtBr) => "Plano",
        ("doc.step.plan.name", Locale::EnUs) => "Plan",
        ("doc.step.plan.desc", Locale::PtBr) => {
            "spec e ondas escritas, e cada critério rodado para provar que hoje ele falha."
        }
        ("doc.step.plan.desc", Locale::EnUs) => {
            "spec and waves written, and each criterion run to prove it fails today."
        }
        ("doc.step.approval.name", Locale::PtBr) => "Aprovação",
        ("doc.step.approval.name", Locale::EnUs) => "Approval",
        ("doc.step.approval.desc", Locale::PtBr) => "você aprova com `/mustard:spec`.",
        ("doc.step.approval.desc", Locale::EnUs) => "you approve with `/mustard:spec`.",
        ("doc.step.execute.name", Locale::PtBr) => "Execução",
        ("doc.step.execute.name", Locale::EnUs) => "Execution",
        ("doc.step.execute.desc", Locale::PtBr) => {
            "um agente por onda escreve o código, uma onda depois da outra."
        }
        ("doc.step.execute.desc", Locale::EnUs) => {
            "one agent per wave writes the code, one wave after another."
        }
        ("doc.step.review.name", Locale::PtBr) => "Revisão",
        ("doc.step.review.name", Locale::EnUs) => "Review",
        ("doc.step.review.desc", Locale::PtBr) => "um agente revisor tenta achar falhas.",
        ("doc.step.review.desc", Locale::EnUs) => "a reviewer agent tries to find flaws.",
        ("doc.step.verify.name", Locale::PtBr) => "Verificação",
        ("doc.step.verify.name", Locale::EnUs) => "Verification",
        ("doc.step.verify.desc", Locale::PtBr) => "os critérios rodam de novo e agora precisam passar.",
        ("doc.step.verify.desc", Locale::EnUs) => "the criteria run again and must now pass.",
        ("doc.step.close.name", Locale::PtBr) => "Fechamento",
        ("doc.step.close.name", Locale::EnUs) => "Closing",
        ("doc.step.close.desc", Locale::PtBr) => "pull request e merge na base.",
        ("doc.step.close.desc", Locale::EnUs) => "pull request and merge into the base.",
        ("doc.step.here", Locale::PtBr) => "Estamos aqui.",
        ("doc.step.here", Locale::EnUs) => "We are here.",
        ("doc.col.question", Locale::PtBr) => "Pergunta",
        ("doc.col.question", Locale::EnUs) => "Question",
        ("doc.col.answer", Locale::PtBr) => "Resposta",
        ("doc.col.answer", Locale::EnUs) => "Answer",
        ("doc.clarified.definition", Locale::PtBr) => "O que quer dizer {term} aqui?",
        ("doc.clarified.definition", Locale::EnUs) => "What does {term} mean here?",
        ("doc.clarified.notes", Locale::PtBr) => "Nota:",
        ("doc.clarified.notes", Locale::EnUs) => "Note:",
        ("doc.col.severity", Locale::PtBr) => "Gravidade",
        ("doc.col.severity", Locale::EnUs) => "Severity",
        ("doc.col.risk", Locale::PtBr) => "Risco",
        ("doc.col.risk", Locale::EnUs) => "Risk",
        ("doc.col.mitigation", Locale::PtBr) => "O que atenua",
        ("doc.col.mitigation", Locale::EnUs) => "What mitigates it",
        ("doc.severity.alta", Locale::PtBr) => "Alta",
        ("doc.severity.alta", Locale::EnUs) => "High",
        ("doc.severity.media", Locale::PtBr) => "Média",
        ("doc.severity.media", Locale::EnUs) => "Medium",
        ("doc.severity.baixa", Locale::PtBr) => "Baixa",
        ("doc.severity.baixa", Locale::EnUs) => "Low",
        ("doc.criteria.lead", Locale::PtBr) => {
            "Cada critério é um comando. Antes de o código existir ele precisa falhar, e é \
             essa falha que prova que ele mede algo novo; depois da entrega, precisa passar."
        }
        ("doc.criteria.lead", Locale::EnUs) => {
            "Each criterion is a command. Before the code exists it must fail, and that \
             failure is what proves it measures something new; after delivery it must pass."
        }
        ("doc.col.id", _) => "Id",
        ("doc.col.criterion", Locale::PtBr) => "Quando… então…",
        ("doc.col.criterion", Locale::EnUs) => "When… then…",
        ("doc.col.wave", Locale::PtBr) => "Onda",
        ("doc.col.wave", Locale::EnUs) => "Wave",
        ("doc.waves.lead", Locale::PtBr) => {
            "As skills são os moldes que o agente de cada onda carrega antes de escrever os \
             arquivos que elas governam. A lista sai do cruzamento dos arquivos da onda com \
             as pastas de cada molde."
        }
        ("doc.waves.lead", Locale::EnUs) => {
            "Skills are the molds each wave's agent loads before writing the files they \
             govern. The list comes from crossing the wave's files with each mold's folders."
        }
        ("doc.wave.done", Locale::PtBr) => "concluída",
        ("doc.wave.done", Locale::EnUs) => "done",
        ("doc.wave.skills", _) => "Skills",
        ("doc.wave.covers", Locale::PtBr) => "cobre",
        ("doc.wave.covers", Locale::EnUs) => "covers",
        ("doc.wave.criteria", Locale::PtBr) => "Critérios",
        ("doc.wave.criteria", Locale::EnUs) => "Criteria",
        ("doc.wave.obligations", Locale::PtBr) => "Obrigação externa",
        ("doc.wave.obligations", Locale::EnUs) => "External obligation",
        ("doc.col.seen", Locale::PtBr) => "O que foi visto no código",
        ("doc.col.seen", Locale::EnUs) => "What was seen in the code",
        ("doc.col.where", Locale::PtBr) => "Onde",
        ("doc.col.where", Locale::EnUs) => "Where",
        ("doc.col.pending", Locale::PtBr) => "Pendência",
        ("doc.col.pending", Locale::EnUs) => "Pending item",
        ("doc.next.analyze", Locale::PtBr) => "A análise segue; a spec é escrita no passo seguinte.",
        ("doc.next.analyze", Locale::EnUs) => "Analysis continues; the spec is written in the next step.",
        ("doc.next.approve", Locale::PtBr) => {
            "Para aprovar, digite `/mustard:spec` neste branch. A onda 1 começa."
        }
        ("doc.next.approve", Locale::EnUs) => {
            "To approve, type `/mustard:spec` on this branch. Wave 1 starts."
        }
        ("doc.next.adjust", Locale::PtBr) => "Para ajustar, diga o que mudar.",
        ("doc.next.adjust", Locale::EnUs) => "To adjust, say what to change.",
        ("doc.next.approved", Locale::PtBr) => "A spec está aprovada. `/mustard:spec` começa a onda 1.",
        ("doc.next.approved", Locale::EnUs) => "The spec is approved. `/mustard:spec` starts wave 1.",
        ("doc.next.execute", Locale::PtBr) => "A execução segue na onda {wave}, com `/mustard:spec`.",
        ("doc.next.execute", Locale::EnUs) => "Execution continues with wave {wave}, via `/mustard:spec`.",
        ("doc.next.execute_done", Locale::PtBr) => "Todas as ondas terminaram; a revisão vem a seguir.",
        ("doc.next.execute_done", Locale::EnUs) => "Every wave is done; review comes next.",
        ("doc.next.review", Locale::PtBr) => {
            "A revisão e a verificação estão rodando; o resultado aparece nesta página."
        }
        ("doc.next.review", Locale::EnUs) => {
            "Review and verification are running; the result shows up on this page."
        }
        ("doc.next.close", Locale::PtBr) => "Falta fechar: pull request e merge na base.",
        ("doc.next.close", Locale::EnUs) => "What is left is closing: pull request and merge into the base.",
        ("doc.next.completed", Locale::PtBr) => "A unidade está fechada. Não há mais nada a fazer aqui.",
        ("doc.next.completed", Locale::EnUs) => "The unit is closed. Nothing else to do here.",
        ("doc.footer", Locale::PtBr) => {
            "Montado pelo Mustard a partir da spec, das ondas, do material da conversa, da \
             prova dos critérios e da lista de pendências."
        }
        ("doc.footer", Locale::EnUs) => {
            "Built by Mustard from the spec, the waves, the conversation material, the \
             criteria proof and the pending list."
        }
        // A publicação nos marcos, dita pelo plano, pela rodada e pelo
        // fechamento (`commands/spec_events/pages.rs`).
        ("page.publish", Locale::PtBr) => {
            "Publique a página da spec e a do projeto (`.claude/spec/project.html`) e grave as duas \
             publicações: `mustard-rt run write publish --spec <spec> --json '{\"page\":\"spec\",\"milestone\":\"{milestone}\",\"ok\":true,\"url\":\"…\"}'` \
             e `mustard-rt run write publish --spec <spec> --json '{\"page\":\"project\",\"milestone\":\"{milestone}\",\"ok\":true,\"url\":\"…\"}'`, \
             cada uma com `\"ok\":false` e `\"reason\"` quando falhar. Os endereços não vão para a \
             conversa: ficam na barra de status."
        }
        ("page.publish", Locale::EnUs) => {
            "Publish the spec page and the project page (`.claude/spec/project.html`) and record both \
             publications: `mustard-rt run write publish --spec <spec> --json '{\"page\":\"spec\",\"milestone\":\"{milestone}\",\"ok\":true,\"url\":\"…\"}'` \
             and `mustard-rt run write publish --spec <spec> --json '{\"page\":\"project\",\"milestone\":\"{milestone}\",\"ok\":true,\"url\":\"…\"}'`, \
             each with `\"ok\":false` and a `\"reason\"` when it fails. The addresses never go to \
             the conversation: they live in the status line."
        }
        ("page.purge_pending", Locale::PtBr) => {
            "Os itens {codes} ainda guardam no arquivo um trecho com cara de segredo, que saiu da \
             página como \"…\": expurgue cada um com `mustard-rt run write purge --spec <spec> --json \
             '{\"targets\":[\"<código>\"],\"reason\":\"secret\"}'`."
        }
        ("page.purge_pending", Locale::EnUs) => {
            "Items {codes} still keep in the file an excerpt that looks like a secret, which left the \
             page as \"…\": purge each one with `mustard-rt run write purge --spec <spec> --json \
             '{\"targets\":[\"<item>\"],\"reason\":\"secret\"}'`."
        }
        ("page.after_rebuild", Locale::PtBr) => {
            "Refaça a página com `mustard-rt run page --spec <spec>`, publique as duas páginas e \
             grave cada publicação com o marco `{milestone}`."
        }
        ("page.after_rebuild", Locale::EnUs) => {
            "Rebuild the page with `mustard-rt run page --spec <spec>`, publish both pages and \
             record each publication with the `{milestone}` milestone."
        }
        ("page.not_rebuilt", Locale::PtBr) => {
            "Não publique as páginas: a {page} não pôde ser refeita, e o motivo está em `warnings`."
        }
        ("page.not_rebuilt", Locale::EnUs) => {
            "Do not publish the pages: the {page} could not be rebuilt, and the reason is in `warnings`."
        }
        ("page.rebuild_failed", Locale::PtBr) => "Não consegui refazer a {page}: {detail}.",
        ("page.rebuild_failed", Locale::EnUs) => "Could not rebuild the {page}: {detail}.",
        ("page.name.spec", Locale::PtBr) => "página da spec",
        ("page.name.spec", Locale::EnUs) => "spec page",
        ("page.name.project", Locale::PtBr) => "página do projeto (`.claude/spec/project.html`)",
        ("page.name.project", Locale::EnUs) => "project page (`.claude/spec/project.html`)",

        // A página e o `.md` de uma spec (`view::document`): os títulos dos
        // blocos, os nomes dos tipos, os rótulos dos campos e dos valores.
        ("page.block.state", Locale::PtBr) => "Estado",
        ("page.block.state", Locale::EnUs) => "State",
        ("page.block.metrics", Locale::PtBr) => "Painel de medição",
        ("page.block.metrics", Locale::EnUs) => "Measurement panel",
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
        ("page.conversation.summary", Locale::PtBr) => "{count} registros",
        ("page.conversation.summary", Locale::EnUs) => "{count} entries",
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
        ("page.field.skills", _) => "Skills",
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
        ("page.field.wave_state", Locale::PtBr) => "Estado da onda",
        ("page.field.wave_state", Locale::EnUs) => "Wave state",
        ("page.field.wave_commit", Locale::PtBr) => "Commit",
        ("page.field.wave_commit", Locale::EnUs) => "Commit",
        ("page.field.wave_receives", Locale::PtBr) => "Recebe",
        ("page.field.wave_receives", Locale::EnUs) => "Receives",

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
        ("page.value.wave_todo", Locale::PtBr) => "a fazer",
        ("page.value.wave_todo", Locale::EnUs) => "to do",
        ("page.value.wave_running", Locale::PtBr) => "em execução",
        ("page.value.wave_running", Locale::EnUs) => "running",
        ("page.value.wave_done", Locale::PtBr) => "pronta",
        ("page.value.wave_done", Locale::EnUs) => "done",
        ("page.value.wave_reviewed", Locale::PtBr) => "revisada",
        ("page.value.wave_reviewed", Locale::EnUs) => "reviewed",

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
        ("page.metrics.col.rejected", Locale::PtBr) => "Reprovações",
        ("page.metrics.col.rejected", Locale::EnUs) => "Rejections",
        ("page.metrics.col.last", Locale::PtBr) => "Última revisão",
        ("page.metrics.col.last", Locale::EnUs) => "Last review",
        ("page.metrics.by_wave", Locale::PtBr) => "Tamanho do pedido e revisão, por onda",
        ("page.metrics.by_wave", Locale::EnUs) => "Request size and review, per wave",
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
        ("project.col.spec", Locale::PtBr) => "Spec",
        ("project.col.spec", Locale::EnUs) => "Spec",
        ("project.col.state", Locale::PtBr) => "Estado",
        ("project.col.state", Locale::EnUs) => "State",
        ("project.col.branch", _) => "Branch",
        ("project.col.goal", Locale::PtBr) => "Objetivo",
        ("project.col.goal", Locale::EnUs) => "Goal",
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
            include_str!("page.rs"),
            super::PREFIXES,
            333,
            0x3333_d997_5c8b_8fa8,
        );
    }

    /// O resumo da spec em HTML tira todo o texto do catálogo, nos dois idiomas,
    /// e o próximo passo da execução carrega o número da onda.
    #[test]
    fn i18n_translates_spec_doc_keys() {
        for key in [
            "doc.section.where",
            "doc.step.plan.name",
            "doc.next.approve",
            "doc.footer",
        ] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_ne!(translate(key, lang), "<missing-key>", "{key} missing for {lang}");
            }
            assert_ne!(
                translate(key, Locale::PtBr),
                translate(key, Locale::EnUs),
                "{key} must differ per locale (proof it is catalogue-driven)"
            );
        }
        for lang in [Locale::PtBr, Locale::EnUs] {
            assert!(translate("doc.next.execute", lang).contains("{wave}"));
            assert!(translate("doc.clarified.definition", lang).contains("{term}"));
        }
    }
}
