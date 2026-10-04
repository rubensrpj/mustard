//! Cada pacote tem um programa só de testes de integração (`tests/it.rs`), com
//! a descoberta automática desligada: o arquivo de `tests/` que a lista do
//! programa não declara nunca roda, e nada avisaria. Este teste lê a pasta de
//! cada pacote e reprova o arquivo que ficou fora da lista.

use std::path::PathBuf;

/// Os arquivos de `tests/` do pacote em `package` (raiz do repositório mais o
/// caminho) que a lista de `tests/it.rs` não declara.
fn files_left_out(package: &str) -> Vec<String> {
    let tests: PathBuf = crate::manifest_dir::manifest_dir().join("../..").join(package).join("tests");
    let program = std::fs::read_to_string(tests.join("it.rs"))
        .unwrap_or_else(|e| panic!("{} não tem o programa de testes: {e}", tests.display()));
    let declared: Vec<&str> =
        program.lines().filter_map(|line| line.strip_prefix("mod ")?.strip_suffix(';')).collect();
    let mut left_out: Vec<String> = std::fs::read_dir(&tests)
        .unwrap_or_else(|e| panic!("{} não abriu: {e}", tests.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .filter_map(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .filter(|stem| stem != "it" && !declared.contains(&stem.as_str()))
        .collect();
    left_out.sort();
    left_out.into_iter().map(|stem| format!("{package}/tests/{stem}.rs")).collect()
}

#[test]
fn every_integration_test_file_is_declared_in_the_program_of_its_package() {
    let left_out: Vec<String> = ["apps/cli", "apps/rt", "apps/scan", "packages/core"]
        .into_iter()
        .flat_map(files_left_out)
        .collect();
    assert!(
        left_out.is_empty(),
        "arquivo de teste fora da lista do `tests/it.rs` do pacote dele, que nunca roda; \
         declare `mod <nome>;` na lista:\n{}",
        left_out.join("\n")
    );
}
