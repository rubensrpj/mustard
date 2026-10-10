//! Textos consumidos por relatórios, busca de eventos, statusline e gasto.
//! Tipos e fases são montados dinamicamente; textos exclusivos das páginas
//! automáticas retiradas não integram mais este catálogo.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["page"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        ("page.purge_pending", Locale::PtBr) => {
            "Os itens {codes} guardam no arquivo um trecho com cara de segredo. Expurgue-os antes de compartilhar \
             registros do projeto. Expurgue cada um com \
             `mustard-rt run write purge --spec {spec} --json '{\"targets\":[\"<código>\"],\"reason\":\"secret\"}'`."
        }
        ("page.purge_pending", Locale::EnUs) => {
            "Items {codes} keep in the file an excerpt that looks like a secret. Purge them before sharing \
             project records. Purge each one with \
             `mustard-rt run write purge --spec {spec} --json '{\"targets\":[\"<item>\"],\"reason\":\"secret\"}'`."
        }
        ("page.spend.refusal.not_an_address", Locale::PtBr) => {
            "O texto `{found}` não é o endereço de uma página publicada: ele começa com `https://`. Passe o \
             endereço que a publicação devolveu."
        }
        ("page.spend.refusal.not_an_address", Locale::EnUs) => {
            "The text `{found}` is not the address of a published page: it starts with `https://`. Pass the \
             address the publication returned."
        }
        ("page.spend.refusal.no_machine_folder", Locale::PtBr) => {
            "Não achei a pasta pessoal da máquina para guardar o gasto. Defina `MUSTARD_SPEND_DIR` com a pasta \
             onde guardar."
        }
        ("page.spend.refusal.no_machine_folder", Locale::EnUs) => {
            "Could not find the machine's home folder to keep the spend. Set `MUSTARD_SPEND_DIR` to the folder \
             where it goes."
        }
        ("page.spend.refusal.unreadable_ledger", Locale::PtBr) => {
            "O arquivo do gasto {path} não se lê: {detail}. Apague o arquivo para contar tudo de novo pelas \
             conversas. O endereço da página se perde, e `mustard-rt run spend --republish` publica outra."
        }
        ("page.spend.refusal.unreadable_ledger", Locale::EnUs) => {
            "The spend file {path} cannot be read: {detail}. Delete the file to count everything again from \
             the conversations. The page address is lost, and `mustard-rt run spend --republish` publishes \
             another one."
        }
        ("page.spend.refusal.io", Locale::PtBr) => "Não consegui ler ou gravar o gasto: {detail}.",
        ("page.spend.refusal.io", Locale::EnUs) => "Could not read or write the spend: {detail}.",
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
        ("page.not_found", Locale::PtBr) => "Nada encontrado. Tente outra palavra ou o código do item, como `DEC-0142`.",
        ("page.not_found", Locale::EnUs) => "Nothing found. Try another word or an item's code, like `DEC-0142`.",
        ("page.of", Locale::PtBr) => "{n} de {total}",
        ("page.of", Locale::EnUs) => "{n} of {total}",
        ("page.count.one", _) => "{n} item",
        ("page.count.many", Locale::PtBr) => "{n} itens",
        ("page.count.many", Locale::EnUs) => "{n} items",
        ("page.loading", Locale::PtBr) => "Lendo o banco de dados da página…",
        ("page.loading", Locale::EnUs) => "Reading the page's database…",
        ("page.no_data", Locale::PtBr) => {
            "Ainda não há dados: o Mustard ainda não copiou nada para o banco de dados desta página."
        }
        ("page.no_data", Locale::EnUs) => "No data yet: Mustard has not copied anything to this page's database.",
        ("page.read_failed", Locale::PtBr) => "Não deu para ler o banco de dados da página. Recarregue a página.",
        ("page.read_failed", Locale::EnUs) => "Could not read the page's database. Reload the page.",
        ("page.block.agreed", Locale::PtBr) => "Requisitos acordados",
        ("page.block.agreed", Locale::EnUs) => "Agreed requirements",
        ("page.findings.heading", Locale::PtBr) => "O que o plano achou",
        ("page.findings.heading", Locale::EnUs) => "What the plan found",
        ("page.wave.prompt", Locale::PtBr) => "O pedido da onda {n}",
        ("page.wave.prompt", Locale::EnUs) => "Wave {n}'s request",
        ("page.wave.prompt.summary", Locale::PtBr) => "{lines} linhas, como o agente as recebe",
        ("page.wave.prompt.summary", Locale::EnUs) => "{lines} lines, exactly as the agent gets them",
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
        ("page.type.tracking", Locale::PtBr) => "rastreabilidade",
        ("page.type.tracking", Locale::EnUs) => "traceability",
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
        ("page.field.proof", Locale::PtBr) => "Verificação",
        ("page.field.proof", Locale::EnUs) => "Verification",
        ("page.field.final", Locale::PtBr) => "Aceitação",
        ("page.field.final", Locale::EnUs) => "Acceptance",
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
        ("page.missing_body", Locale::PtBr) => {
            "Passe --body <arquivo.md> e --out <página.html> para gerar uma página avulsa."
        }
        ("page.missing_body", Locale::EnUs) => {
            "Pass --body <file.md> and --out <page.html> to build a standalone page."
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
    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("page.rs"),
            super::PREFIXES,
            70,
            0xf80f_a570_4181_817f,
        );
    }

}
