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

use mustard_core::platform::i18n::{Locale, translate};
use serde_json::json;

/// O arquivo local do projeto: o git o ignora, e o `mustard.json` o lista em
/// `localFiles`, que só vale para as cópias das ondas.
const LOCAL_FILE: &str = "local.env";

/// Um segundo arquivo local, de que a suíte não depende.
const OTHER_LOCAL_FILE: &str = "outro.env";

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

/// Quem só tem o arquivo local na máquina não vê, na saída da suíte, por que
/// ela caiu: a recusa diz que o comando rodou sem os arquivos que o
/// `mustard.json` lista em `localFiles`, e nomeia cada um, inclusive o que a
/// suíte não lê.
#[test]
fn the_suite_refusal_names_each_local_file_the_clean_copy_ran_without() {
    let local = [LOCAL_FILE, OTHER_LOCAL_FILE];
    let commands = json!({"testCommand": format!("test -f {LOCAL_FILE}"), "localFiles": local});
    let project = one_wave_spec::Project::new("sem-locais", &commands, &local);
    project.approved();
    project.run(&["round", "--dispatch", "--spec", "sem-locais"]);
    project.deliver_the_first_wave();
    for file in local {
        std::fs::write(project.root.join(file), "SECRET=1\n").unwrap();
    }

    let refused = project.refused(&["close", "--spec", "sem-locais"]);

    assert_eq!(refused["reason"], json!("suite-failed"), "{refused}");
    let note = translate("close.ran_without_local_files", Locale::PtBr).replace("{files}", "`local.env`, `outro.env`");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains(&note), "the refusal does not name the local files: {hint}");
}
