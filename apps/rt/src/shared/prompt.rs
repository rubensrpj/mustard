//! `prompt` — a fala de uma pessoa, separada dos avisos do próprio programa.
//!
//! ## Por que existe
//!
//! O `UserPromptSubmit` dispara para tudo o que chega à sessão pelo canal do
//! usuário, e o Claude Code fala pelo mesmo canal. O fim de um comando em
//! segundo plano, a volta de um subagente e outros avisos chegam como "fala
//! do usuário", com a marca da máquina e, no caso do subagente, o relatório
//! inteiro.
//!
//! O único gancho desse evento, [`crate::hooks::session::prompt_entry`],
//! grava cada mensagem do usuário no bloco da conversa da spec. Sem este
//! filtro, cada aviso entraria ali como fala do usuário, e quem lê o bloco
//! tomaria o fim de um comando em segundo plano por um pedido.
//!
//! Um filtro só, com um dono só: o gancho e a abertura (`typed_messages`, que
//! lê a conversa guardada) passam a fala pelo mesmo [`person_text`], que tira
//! o aviso do programa e os blocos que o Claude Code põe na fala. Uma regra
//! dessas, copiada em cada leitor, diverge na primeira marca nova.
//!
//! ## Contrato
//!
//! Puro, total, nunca entra em pânico. As marcas são texto do protocolo do
//! Claude Code, e não frase para o usuário: ficam em inglês, qualquer que
//! seja o idioma do projeto, como os nomes de evento.

/// As marcas da fala que o PROGRAMA escreve, nunca uma pessoa.
///
/// Procuradas em qualquer ponto do texto, e não só no começo: o programa
/// pode pôr um texto dele antes da marca, e o aviso às vezes vem dentro de um
/// envelope maior.
const HARNESS_NOTICE_MARKERS: &[&str] =
    &["[SYSTEM NOTIFICATION - NOT USER INPUT]", "<task-notification>", "<agent-message from="];

/// `true` quando quem escreveu `prompt` foi o programa, e não uma pessoa.
///
/// Estreito de propósito: reconhece as marcas que o programa de fato manda e
/// nada mais. Um palpite ("parece texto de máquina") um dia engoliria um
/// pedido de verdade, e perder o que a pessoa pediu é o erro pior: gravar um
/// aviso a mais faz barulho, perder um pedido erra calado.
#[must_use]
fn is_harness_notice(prompt: &str) -> bool {
    HARNESS_NOTICE_MARKERS.iter().any(|m| prompt.contains(m))
}

/// Os blocos que o próprio Claude Code põe dentro da fala do usuário, pelo
/// começo do nome da marca: a saída de um comando local, o comando do modo
/// bash e a saída dele, e o lembrete do sistema.
const HARNESS_BLOCKS: &[&str] = &["local-command-", "bash-input", "bash-stdout", "bash-stderr", "system-reminder"];

/// O que a pessoa escreveu em `prompt`, o filtro único da fala, do gancho e
/// da abertura: `None` para o aviso do programa ([`is_harness_notice`]) ou
/// quando nada sobra; senão, o texto sem os blocos de [`HARNESS_BLOCKS`],
/// cada um tirado até a marca que o fecha. O bloco sem a marca de fechar
/// fica: perder o que a pessoa pediu é o erro pior.
#[must_use]
pub fn person_text(prompt: &str) -> Option<String> {
    if is_harness_notice(prompt) {
        return None;
    }
    let mut pieces = Vec::new();
    let mut rest = prompt;
    while let Some((start, end)) = harness_block(rest) {
        pieces.push(&rest[..start]);
        rest = &rest[end..];
    }
    pieces.push(rest);
    let text = pieces.into_iter().map(str::trim).filter(|piece| !piece.is_empty()).collect::<Vec<_>>().join("\n\n");
    (!text.is_empty()).then_some(text)
}

/// O primeiro bloco de [`HARNESS_BLOCKS`] fechado em `text`: onde a marca que
/// o abre começa e onde a que o fecha termina.
fn harness_block(text: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(at) = text[from..].find('<').map(|i| i + from) {
        let tag = &text[at + 1..];
        if HARNESS_BLOCKS.iter().any(|prefix| tag.starts_with(prefix)) {
            let name = &tag[..tag.find(|c: char| c == '>' || c.is_whitespace()).unwrap_or(tag.len())];
            let close = format!("</{name}>");
            if let Some(end) = text[at..].find(&close) {
                return Some((at, at + end + close.len()));
            }
        }
        from = at + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada marca é reconhecida, em qualquer ponto do texto.
    #[test]
    fn recognises_every_runtime_banner() {
        assert!(is_harness_notice("[SYSTEM NOTIFICATION - NOT USER INPUT]\nbody"));
        assert!(is_harness_notice("<task-notification>\n<status>completed</status>"));
        assert!(is_harness_notice("<agent-message from=\"onda-39\">relatório</agent-message>"));
        // Dentro de um envelope maior, segue aviso.
        assert!(is_harness_notice("prefix\n\n<task-notification>x</task-notification>\n"));
    }

    /// A fala de uma pessoa nunca é aviso, nem a que FALA de avisos: a
    /// procura é pela marca escrita, e não pela palavra.
    #[test]
    fn a_real_request_is_never_mistaken_for_one() {
        for real in [
            "muda o campo status para enum",
            "troca o gate para warn",
            "why did the task notification arrive twice?",
            "add a system notification banner to the UI",
            "",
        ] {
            assert!(!is_harness_notice(real), "misread as a notice: {real:?}");
        }
    }

    /// O bloco que abre e não fecha fica na fala, inteiro: sem a marca de
    /// fechar não se sabe onde ele acaba, e tirar dali até o fim perderia o
    /// pedido.
    #[test]
    fn a_block_without_its_closing_mark_stays_in_the_text() {
        let said = "<system-reminder>lembrete sem fim\narrume o login";
        assert_eq!(person_text(said).as_deref(), Some(said));
    }

    /// Dois blocos na mesma fala saem os dois, e fica só o que a pessoa
    /// escreveu.
    #[test]
    fn two_blocks_in_one_prompt_both_leave() {
        let said = "<system-reminder>lembrete</system-reminder>arrume o login\
                    <local-command-stdout>saída do comando</local-command-stdout>";
        assert_eq!(person_text(said).as_deref(), Some("arrume o login"));
    }

    /// O bloco no meio da fala sai, e o que vinha antes e depois dele fica,
    /// em dois parágrafos.
    #[test]
    fn a_block_in_the_middle_keeps_what_surrounds_it() {
        let said = "primeiro isto <bash-input>ls -la</bash-input> depois aquilo";
        assert_eq!(person_text(said).as_deref(), Some("primeiro isto\n\ndepois aquilo"));
    }

    /// A marca que abre o bloco com um atributo fecha pelo nome dela, sem o
    /// atributo.
    #[test]
    fn a_mark_with_an_attribute_closes_by_its_name() {
        let said = "<system-reminder origin=\"hook\">lembrete</system-reminder>arrume o login";
        assert_eq!(person_text(said).as_deref(), Some("arrume o login"));
    }
}
