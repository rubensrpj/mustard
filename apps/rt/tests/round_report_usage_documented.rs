//! A ajuda de `run round --report` é o único lugar que o orquestrador lê
//! antes de montar o relatório da rodada seguinte, sem abrir código nenhum.
//! Ela precisa dizer que existe a linha `USAGE`, que só o orquestrador
//! escreve com o consumo que a plataforma lhe entrega — nunca um número
//! digitado pelo agente —, ao lado de `PAUSED` e `ANALYSIS`; a entrega e o
//! veredito não vêm no relatório, porque cada agente grava a própria volta
//! com `run write delivered` ou `run write verdict`. Sem essa frase, o
//! marcador funciona mas ninguém descobre que ele existe.

use std::process::Command;

/// `mustard-rt run round --help`, como o orquestrador o veria.
fn round_help() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "round", "--help"])
        .output()
        .expect("run mustard-rt");
    assert!(out.status.success(), "run round --help failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn round_help_documents_the_usage_line() {
    let help = round_help();
    assert!(help.contains("USAGE"), "the help of `run round --report` never mentions the USAGE line: {help}");
    assert!(
        help.contains("nunca digitado pelo agente") || help.contains("never a number the agent typed"),
        "the help never says the agent's own number does not count as usage: {help}"
    );
}
