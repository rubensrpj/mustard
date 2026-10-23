// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A suíte do fechamento roda num checkout limpo do commit da obra, sem o que
//! o git ignora: o teste que lê um arquivo local (`.env`) cai no fechamento,
//! como cairia na esteira do servidor, e não só depois de aberto o pull
//! request.
//!
//! Prova de ponta a ponta, pelo binário de verdade, num repositório
//! temporário: a spec anda pelos comandos e ganchos do uso real até o
//! fechamento.

#![cfg(unix)]

use crate::one_wave_spec;

use serde_json::json;

/// O arquivo local do projeto: o git o ignora, e o `mustard.json` o lista em
/// `localFiles`, que só vale para as cópias das ondas.
const LOCAL_FILE: &str = "local.env";

/// A suíte do projeto só passa onde o arquivo local existe. Na pasta do
/// projeto ele existe, e antes a suíte do fechamento passava ali mesmo; na
/// cópia limpa ele não chega, e o fechamento recusa pela suíte.
#[test]
fn the_closing_suite_fails_when_the_test_reads_a_file_git_ignores() {
    let commands = json!({"testCommand": format!("test -f {LOCAL_FILE}"), "localFiles": [LOCAL_FILE]});
    let project = one_wave_spec::Project::new("limpa", &commands, &[LOCAL_FILE]);
    project.approved();
    project.run(&["round", "--dispatch", "--spec", "limpa"]);
    project.deliver_the_first_wave();
    std::fs::write(project.root.join(LOCAL_FILE), "SECRET=1\n").unwrap();

    let refused = project.refused(&["close", "--spec", "limpa"]);

    assert_eq!(refused["reason"], json!("suite-failed"), "{refused}");
}
