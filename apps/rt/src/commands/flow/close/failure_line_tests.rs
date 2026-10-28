//! A linha de cada falha de prova na recusa do fechamento, também nas que não
//! vêm de uma saída vermelha: a prova que não rodou teste, a que cita um teste
//! que não existe, a que já passava no código de antes da obra e a prova de um
//! pedido. A linha diz o motivo e o item, e o comando dela lê a execução
//! gravada, com a saída inteira, que a recusa não repete.

use std::path::Path;

use mustard_core::io::spec_events as store;
use mustard_core::platform::git;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::tests::{close, git_at, read_by, ready_with_files, ready_with_waves, wave_file};

/// O que lê o comando da linha da falha `reason` do item `code` na recusa
/// `refused`. A linha tem de existir e dizer como ler a execução gravada.
fn read_of_the_line(root: &Path, refused: &Value, reason: &str, code: &str) -> String {
    let lines = refused["failures"].as_array().cloned().unwrap_or_default();
    let line = lines.iter().find(|line| line["reason"] == json!(reason) && line["criterion"] == json!(code));
    let line = line.unwrap_or_else(|| panic!("a falha {reason} de {code} tem a linha dela: {refused}"));
    let read = line["read"].as_str().unwrap_or_else(|| panic!("a linha diz como ler a execução: {line}"));
    read_by(root, read)
}

/// Os códigos dos itens do tipo `kind` da obra `x`, na ordem em que nasceram.
fn codes_of(root: &Path, kind: &str) -> Vec<String> {
    let log = store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap();
    let codes = log.codes();
    log.visible().into_iter().filter(|e| e.event_type == kind).map(|e| codes[&e.id].clone()).collect()
}

/// A base da obra passa a ser a branch `dev`, no commit em que o repositório
/// nasceu: o fechamento roda cada prova também no código de antes da obra.
fn based_on_the_first_commit(root: &Path) {
    let first = git::run(root, &["rev-list", "--max-parents=0", "HEAD"]).result().unwrap();
    git_at(root, &["branch", "dev", first.trim()]);
}

/// A prova verde que diz zero teste ganha a linha dela na recusa, e o comando
/// da linha lê o que o executor escreveu.
#[test]
fn a_proof_that_ran_no_test_gives_its_line_with_the_read_of_its_run() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ready_with_waves(root, "x", &["echo Tests: 3 total", "echo Tests: 0 total"], 1);

    let refused = close(root, "x");
    let read = read_of_the_line(root, &refused, "criterion-ran-no-test", &codes_of(root, "criterion")[1]);
    assert!(read.contains("Tests: 0 total"), "o comando da linha lê a execução da prova: {read}");
}

/// A prova verde que cita um teste que não existe no projeto ganha a linha
/// dela na recusa, e o comando da linha lê a execução reprovada dela; a do
/// critério que cobre a onda, ao lado, passou.
#[test]
fn a_proof_citing_a_missing_test_gives_its_line_with_the_read_of_its_run() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "#[test]\nfn sum_of_two() {}\n").unwrap();
    ready_with_waves(root, "x", &["echo sum_of_two sum_of_four"], 1);

    let refused = close(root, "x");
    let read = read_of_the_line(root, &refused, "criterion-missing-test", &codes_of(root, "criterion")[0]);
    assert!(read.contains(r#""type":"criterion_run""#) && read.contains(r#""result":"fail""#), "o comando da linha lê a execução da prova: {read}");
}

/// A prova que já passava no código de antes da obra ganha a linha dela na
/// recusa, e o comando da linha lê a execução de antes, e não a do commit da
/// obra.
#[test]
fn a_proof_green_before_the_change_gives_its_line_with_the_read_of_the_run_before() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    ready_with_waves(root, "x", &["echo Tests: 2 total"], 1);
    based_on_the_first_commit(root);

    let refused = close(root, "x");
    let code = &codes_of(root, "criterion")[0];
    let read = read_of_the_line(root, &refused, "criterion-passed-before", code);
    assert!(read.contains(&format!("criterion-before:{code}")), "o comando da linha lê a execução de antes: {read}");
}

/// A prova de um pedido, nascido de um achado, ganha a linha dela na recusa
/// pelo código do pedido, e o comando da linha lê a execução dela.
#[test]
fn a_request_proof_gives_its_line_with_the_read_of_its_run() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let finding = "- A soma de `src/w1.rs` ignora o estouro. Prova: `echo Tests: 2 total`";
    ready_with_files(root, "x", &[], &[wave_file(1)], &[], &[finding]);
    based_on_the_first_commit(root);

    let refused = close(root, "x");
    let code = &codes_of(root, "request")[0];
    let read = read_of_the_line(root, &refused, "criterion-passed-before", code);
    assert!(read.contains(&format!("criterion-before:{code}")), "o comando da linha lê a execução da prova do pedido: {read}");
}
