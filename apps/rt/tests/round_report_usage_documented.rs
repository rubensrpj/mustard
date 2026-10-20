//! A ajuda de `run round --report` é o único lugar que o orquestrador lê
//! antes de montar o relatório da rodada seguinte, sem abrir código nenhum.
//! Ela precisa dizer que existe a linha `USAGE`, que o orquestrador escreve
//! quando o agente de onda termina, e que o consumo vem dos arquivos de
//! conversa da plataforma — nunca de um número digitado pelo agente —, ao
//! lado de `PAUSED`; a entrega e o
//! veredito não vêm no relatório, porque cada agente grava a própria volta
//! com `run write delivered` ou `run write verdict`. Sem essa frase, o
//! marcador funciona mas ninguém descobre que ele existe.

#[path = "support/binary.rs"]
mod binary;

/// `mustard-rt run round --help`, como o orquestrador o veria.
fn round_help() -> String {
    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
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

/// A ajuda do `--report` não ensina mais a linha `ANALYSIS`: a onda sai sem a
/// escolha de quem conduz, e a linha sozinha é recusada.
#[test]
fn round_help_no_longer_teaches_the_analysis_line() {
    let help = round_help();
    assert!(!help.contains("ANALYSIS"), "the help still teaches the ANALYSIS line: {help}");
    assert!(help.contains("PAUSED"), "{help}");
}

/// A ajuda do `--report` ensina a linha com que quem conduz a obra reprova a
/// volta de uma onda, com a onda e o motivo.
#[test]
fn round_help_documents_the_rejected_line() {
    let help = round_help();
    assert!(help.contains(r#"<REJECTED>{"wave":1,"reason":"…"}</REJECTED>"#), "{help}");
}

#[test]
fn round_help_explains_that_usage_does_not_approve_code() {
    let help = round_help();
    for field in ["review-wave", "REVIEWED", "note", "`USAGE`", "sozinho não aprova"] {
        assert!(help.contains(field), "missing {field}: {help}");
    }
}

/// O aceite da revisão vem sem a impressão: o binário sabe qual volta quem
/// conduz viu. A onda que gravou a entrega não pede linha de uso.
#[test]
fn round_help_accepts_the_review_without_fingerprint_or_usage_line() {
    let help = round_help();
    assert!(help.contains(r#"<REVIEWED>{"wave":1,"note":"…"}</REVIEWED>"#), "{help}");
    assert!(help.contains("sem linha de uso"), "{help}");
}

/// A ajuda do `--copy` do `run write` diz o que ele faz hoje: nada prepara
/// cópia, e a resposta só aponta a publicação explícita.
#[test]
fn write_help_says_copy_only_points_to_explicit_publishing() {
    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt")).args(["run", "write", "--help"]).output().expect("run mustard-rt");
    assert!(out.status.success(), "run write --help failed: {}", String::from_utf8_lossy(&out.stderr));
    let help = String::from_utf8_lossy(&out.stdout).to_string();
    let copy = help.split("--copy").nth(1).unwrap_or_default().split("--root").next().unwrap_or_default();
    assert!(copy.contains("/mustard:pages"), "{copy}");
    assert!(!copy.contains("prepare the copy") && !copy.contains("answer the order to copy"), "{copy}");
}
