//! A reescrita de um comando composto, etapa por etapa.
//!
//! Uma recusa que só diz o que acrescentar custa uma volta ao agente, e num
//! comando com várias etapas ele ainda precisa remontar as que não têm relação
//! com o motivo. Por isso as recusas que podem devolvem o comando inteiro
//! pronto, mexendo só na etapa problemática.
//!
//! Duas recusas fazem isso: a da leitura do arquivo de configuração
//! ([`super::config_read`]) e a da busca que varre esse arquivo
//! ([`super::reading`]). As duas percorrem as mesmas faixas de texto e aplicam
//! a troca da mesma forma, e é essa travessia que mora aqui. Antes cada uma
//! tinha a sua, copiada da outra (21/10/2026).
//!
//! NADA DA LINHA DE QUEM CHAMOU É EXECUTADO: a reescrita é texto sobre texto.

use std::ops::Range;

use super::lex;

/// `command` com cada etapa trocada pelo que `change` der para ela.
///
/// `change` recebe o texto da etapa, sem os espaços das pontas, e devolve o
/// que pôr no lugar dela, ou `None` para deixá-la como está. `None` no fim
/// quando nenhuma etapa mudou.
///
/// As trocas são aplicadas de trás para frente, para as faixas das etapas de
/// antes não saírem do lugar.
pub(super) fn rewritten(command: &str, change: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut changes: Vec<(Range<usize>, String)> = Vec::new();
    for range in lex::command_ranges(command) {
        let source = command.get(range.clone())?;
        let trimmed = source.trim();
        let Some(replacement) = change(trimmed) else { continue };
        let start = range.start + (source.len() - source.trim_start().len());
        let end = range.end - (source.len() - source.trim_end().len());
        changes.push((start..end, replacement));
    }
    if changes.is_empty() {
        return None;
    }
    let mut out = command.to_string();
    for (range, replacement) in changes.into_iter().rev() {
        out.replace_range(range, &replacement);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A etapa que a troca não quer fica como foi escrita, e as faixas das
    /// outras não saem do lugar.
    #[test]
    fn only_the_chosen_steps_change_and_the_others_stay_as_written() {
        let command = "grep -rn jev . && git log --oneline -3 ; echo fim";
        let out = rewritten(command, |step| {
            step.starts_with("grep ").then(|| format!("{step} --exclude=mustard.json"))
        });
        assert_eq!(
            out.as_deref(),
            Some("grep -rn jev . --exclude=mustard.json && git log --oneline -3 ; echo fim"),
        );
    }

    /// Duas etapas trocadas de uma vez, cada uma na faixa dela.
    #[test]
    fn two_steps_change_at_once() {
        let out = rewritten("grep a . && grep b .", |step| {
            step.starts_with("grep ").then(|| format!("{step} -q"))
        });
        assert_eq!(out.as_deref(), Some("grep a . -q && grep b . -q"));
    }

    /// Sem troca nenhuma, não há comando novo.
    #[test]
    fn no_change_means_no_rewrite() {
        assert_eq!(rewritten("ls && pwd", |_| None), None);
    }
}
