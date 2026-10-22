//! `prompt` — telling a person's prompt apart from the runtime's own notices.
//!
//! ## Why this exists
//!
//! `UserPromptSubmit` fires for everything that reaches the session through the
//! user channel — and the runtime speaks through that same channel. A finished
//! background command, a completed subagent and other lifecycle notices all
//! arrive as a "user prompt", carrying a machine banner and, in the subagent
//! case, an entire report.
//!
//! The one hook on that trigger,
//! [`crate::hooks::session::prompt_entry`], writes each message the user
//! sends into the spec's conversation block. Without this predicate every
//! notice would land there as something the user said, and the reader of that
//! block would take a finished background command for a request.
//!
//! One filter, one owner: o gancho e a abertura (`typed_messages`, que lê a
//! conversa guardada) passam a fala pelo mesmo [`person_text`], que tira o
//! aviso do programa e os blocos que o Claude Code põe na fala. Uma regra
//! dessas, copiada em cada leitor, diverge na primeira marca nova.
//!
//! ## Contract
//!
//! Pure, total, never panics. The markers are runtime protocol
//! strings, not user-facing prose, so they stay English regardless of the
//! project's narrative locale — the same carve-out that keeps event names and
//! log strings English.

/// Prompt markers the RUNTIME authors, never a person.
///
/// Matched anywhere in the text rather than at its start: the runtime may
/// prepend its own framing before the banner, and a notice is sometimes wrapped
/// in a wider envelope.
const HARNESS_NOTICE_MARKERS: &[&str] =
    &["[SYSTEM NOTIFICATION - NOT USER INPUT]", "<task-notification>", "<agent-message from="];

/// `true` when `prompt` was authored by the runtime rather than by a person.
///
/// Deliberately narrow: it recognises the banners the runtime actually
/// emits and nothing else. A heuristic ("looks machine-generated") would
/// eventually swallow a real request, and losing what someone asked for is the
/// worse error — an observer that records one notice too many is noisy, one
/// that drops a request is silently wrong.
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

    /// Every banner is recognised, wherever it sits in the text.
    #[test]
    fn recognises_every_runtime_banner() {
        assert!(is_harness_notice("[SYSTEM NOTIFICATION - NOT USER INPUT]\nbody"));
        assert!(is_harness_notice("<task-notification>\n<status>completed</status>"));
        assert!(is_harness_notice("<agent-message from=\"onda-39\">relatório</agent-message>"));
        // Wrapped in a wider envelope — still a notice.
        assert!(is_harness_notice("prefix\n\n<task-notification>x</task-notification>\n"));
    }

    /// A person's prompt is never a notice, including one that TALKS about
    /// notifications. Substring matching is on the literal banner, not the word.
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
}
