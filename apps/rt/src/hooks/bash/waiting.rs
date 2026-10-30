//! `waiting` — a forma do laço que espera outro processo: `while` ou
//! `until` com `pgrep`, `pidof` ou `ps` entre a palavra e o `done`, que não
//! acaba sozinho. Quem lê é a conferência da onda parada
//! ([`crate::commands::flow::stuck`]), na linha de comando de um processo que
//! sobrou. Lê os comandos que [`super::lex::segments`] achou, nunca o texto
//! cru: `git commit -m "espera com pgrep"` só cita o laço.

use super::lex::Segment;

/// `true` when `script` — the text handed to a shell (`bash -c "…"`), or a
/// leftover process's own command line — is a loop waiting for another
/// process.
pub(crate) fn is_a_waiting_loop(script: &str) -> bool {
    waits_in_a_loop(&super::lex::segments(script))
}

/// `true` when a `while` or `until` loop, anywhere in the command, tests for
/// another process with `pgrep`, `pidof` or `ps` — in its condition or in its
/// body, since either one can wait forever on a search text another loop
/// also uses. A `for` loop, or a bare `pgrep` with no loop around it, is not
/// this danger.
fn waits_in_a_loop(segments: &[Segment]) -> bool {
    // process observed, unbounded condition, inside the body
    let mut loops: Vec<(bool, bool, bool)> = Vec::new();
    for seg in segments {
        if seg.leading.iter().any(|k| k == "done")
            && let Some((process, condition, _)) = loops.pop()
                && process && condition {
                    return true;
                }
        if seg.leading.iter().any(|k| k == "while" || k == "until") {
            // A numeric/file condition bounds an inspection loop. Only a
            // process condition or a constant true/false can wait for a process.
            let indefinite = matches!(seg.name(), "pgrep" | "pidof" | "ps" | "true" | "false" | ":");
            loops.push((false, indefinite, false));
        }
        if seg.leading.iter().any(|k| k == "do")
            && let Some((_, _, body)) = loops.last_mut() {
                *body = true;
            }
        if matches!(seg.name(), "pgrep" | "pidof" | "ps")
            && let Some((process, indefinite, body)) = loops.last_mut() {
                *process = true;
                // A compound condition may start with a file test and only
                // later query a process. It is still process-dependent.
                if !*body {
                    *indefinite = true;
                }
            }
    }
    loops.into_iter().any(|(process, indefinite, _)| process && indefinite)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os três exemplos que ficaram presos por cinco horas em 19/09: `while`
    /// e `until` que testam com `pgrep`, na condição ou depois de `&&`.
    #[test]
    fn a_loop_testing_for_a_process_is_a_waiting_loop() {
        for cmd in [
            r#"until ! pgrep -f "cargo test" >/dev/null; do sleep 3; done; echo "FINISHED""#,
            r#"while pgrep -f "cargo test" >/dev/null 2>&1; do sleep 2; done"#,
            r#"until [ -s /tmp/full_test_run.log ] && ! pgrep -f "cargo test --workspace" > /dev/null; do sleep 5; done"#,
            "while pidof cargo >/dev/null; do sleep 1; done",
            "until ps -p 123 >/dev/null; do sleep 1; done",
            "while true; do pgrep -f x >/dev/null && break; sleep 1; done",
        ] {
            assert!(is_a_waiting_loop(cmd), "{cmd}");
        }
    }

    /// O `pgrep` sem laço, o laço `for`, o laço com fim contado e o `pgrep`
    /// citado num texto não são o laço que espera.
    #[test]
    fn a_bare_pgrep_a_bounded_loop_and_quoted_text_are_not_a_waiting_loop() {
        for cmd in [
            "pgrep -f x",
            "for i in 1 2 3; do pgrep -f x; done",
            "i=0; while [ $i -lt 3 ]; do ps -p 123; i=$((i + 1)); done",
            r#"git commit -m "rodar pgrep até acabar""#,
            "grep -l pgrep scripts/*.sh",
        ] {
            assert!(!is_a_waiting_loop(cmd), "{cmd}");
        }
    }
}
