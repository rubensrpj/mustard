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

/// O que a reescrita faz com uma etapa.
pub(super) enum Step {
    /// A etapa vira este texto.
    Put(String),
    /// A etapa sai, com o encadeamento que a liga à etapa anterior: é o que o
    /// corte da resposta do Mustard pede, porque ali a etapa inteira é o
    /// problema, e não um pedaço dela.
    Out,
}

/// `command` com cada etapa trocada ou tirada pelo que `change` der para ela.
///
/// Tirar uma etapa leva o `|` que a liga à anterior: o texto entre as duas
/// faixas é o encadeamento, e deixá-lo no lugar daria um comando que não roda.
/// A primeira etapa não tem anterior, e tirá-la leva só a faixa dela.
///
/// `None` no fim quando nenhuma etapa mudou.
pub(super) fn rewritten_steps(command: &str, change: impl Fn(&str) -> Option<Step>) -> Option<String> {
    let ranges = lex::command_ranges(command);
    let mut changes: Vec<(Range<usize>, String)> = Vec::new();
    for (at, range) in ranges.iter().enumerate() {
        let source = command.get(range.clone())?;
        let Some(step) = change(source.trim()) else { continue };
        let start = range.start + (source.len() - source.trim_start().len());
        let end = range.end - (source.len() - source.trim_end().len());
        match step {
            Step::Put(text) => changes.push((start..end, text)),
            Step::Out => {
                let from = at.checked_sub(1).map_or(start, |before| ranges[before].end);
                changes.push((from..range.end, String::new()));
            }
        }
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
