// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O código do programa não instala nada na máquina de quem o roda.
//!
//! O programa compilado da branch do Mustard nasce numa pasta de compilação, e
//! a sessão passa a ele a chamada: ninguém copia nada para a pasta do plugin,
//! e nenhum `cargo install` troca o programa instalado. Esta lista confere o
//! código de `apps/` e `packages/` fora dos testes:
//!
//! - nenhuma linha roda `cargo install`, salvo a do instalador que oferece o
//!   `ripgrep`: é ferramenta de terceiro, que o usuário aceita ao rodar o
//!   `mustard init`, e nada tem a ver com o programa do Mustard;
//! - o registro de plugins e o programa do plugin só são lidos: nenhuma linha
//!   que os cita grava, copia, move ou apaga.
//!
//! O que o código de dentro de um `#[cfg(test)]` faz não conta: um teste monta
//! o plugin falso que quiser.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::path::{Path, PathBuf};

/// A pasta de código de cada pacote do repositório, a partir de `apps/rt`.
const SOURCE_FOLDERS: &[&str] = &["../rt/src", "../scan/src", "../cli/src", "../../packages/core/src"];

/// O instalador que oferece o `ripgrep`: o único lugar que roda `cargo install`,
/// de uma ferramenta de terceiro, e só quando o usuário o aceita.
const THIRD_PARTY_INSTALLER: &str = "apps/cli/src/commands/init/tools.rs";

/// Os nomes que levam ao plugin instalado: o arquivo do registro, os leitores
/// dele e o programa de dentro do plugin.
const PLUGIN_NAMES: &[&str] =
    &["INSTALLED_PLUGINS", "installed_plugin", "newer_installed_rt", "rt_binary", "CLAUDE_PLUGIN_ROOT", "plugins/cache"];

/// O que grava, copia, move, apaga ou muda a permissão de um arquivo.
const WRITES: &[&str] = &[
    "fs::write",
    "fs::copy",
    "fs::rename",
    "fs::remove",
    "fs::create_dir",
    "fs::set_permissions",
    "File::create",
    "OpenOptions",
    "hard_link",
    "symlink",
];

/// Os arquivos `.rs` sob `folder`, em ordem.
fn rust_files(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())).flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// As linhas de `text` que o programa roda — numeradas a partir de 1 —, sem
/// os comentários e sem os itens marcados `#[cfg(test)]`: um item de teste
/// termina na linha que fecha o bloco dele, na mesma coluna da marca, ou na
/// própria linha dele quando ela acaba em `;`.
fn production_lines(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim() == "#[cfg(test)]" {
            let indent = line.len() - line.trim_start().len();
            let mut item = at + 1;
            while lines.get(item).is_some_and(|next| next.trim_start().starts_with("#[")) {
                item += 1;
            }
            at = if lines.get(item).is_some_and(|first| first.trim_end().ends_with(';')) {
                item + 1
            } else {
                let closing = format!("{}}}", " ".repeat(indent));
                let end = (item..lines.len()).find(|&n| lines[n].trim_end().starts_with(&closing) && lines[n].len() <= closing.len() + 1);
                end.map_or(lines.len(), |end| end + 1)
            };
            continue;
        }
        if !line.trim_start().starts_with("//") {
            kept.push((at + 1, line));
        }
        at += 1;
    }
    kept
}

/// As linhas de código do programa, com o arquivo e o número, de cada pasta de
/// código do repositório.
fn program_lines() -> Vec<(String, usize, String)> {
    let apps_rt = manifest_dir::manifest_dir();
    let root = apps_rt.join("../..");
    let mut all = Vec::new();
    for folder in SOURCE_FOLDERS {
        for file in rust_files(&apps_rt.join(folder)) {
            let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            let shown = std::fs::canonicalize(&file).unwrap().strip_prefix(std::fs::canonicalize(&root).unwrap()).unwrap().to_path_buf();
            let shown = shown.to_string_lossy().replace('\\', "/");
            all.extend(production_lines(&text).into_iter().map(|(n, line)| (shown.clone(), n, line.to_string())));
        }
    }
    all
}

/// A linha roda `cargo install`: a frase inteira, ou o `install` como
/// argumento de um comando que a linha chama de `cargo`.
fn runs_cargo_install(line: &str) -> bool {
    line.contains("cargo install") || (line.contains("\"cargo\"") && line.contains("\"install\""))
}

/// A linha cita o plugin instalado e grava, copia, move ou apaga.
fn writes_to_the_plugin(line: &str) -> bool {
    PLUGIN_NAMES.iter().any(|name| line.contains(name)) && WRITES.iter().any(|write| line.contains(write))
}

/// A leitura do código separa o que o programa roda do que o teste monta: o
/// `cargo install` de produção aparece, o de dentro de um `#[cfg(test)]` ou de
/// um comentário não.
#[test]
fn the_scan_reads_the_program_and_skips_the_tests_and_the_comments() {
    let text = "\
fn install() { run(\"cargo\", [\"install\", \"x\"]); }
// cargo install em comentário
#[cfg(test)]
mod tests {
    fn fake() { run(\"cargo install y\"); }
}
#[cfg(test)]
use fake::cargo_install;
fn plugin() { std::fs::write(INSTALLED_PLUGINS, \"\"); }
";
    let lines = production_lines(text);
    let cargo: Vec<usize> = lines.iter().filter(|(_, line)| runs_cargo_install(line)).map(|(n, _)| *n).collect();
    assert_eq!(cargo, [1], "só o `cargo install` do programa, e não o do teste nem o do comentário");
    let plugin: Vec<usize> = lines.iter().filter(|(_, line)| writes_to_the_plugin(line)).map(|(n, _)| *n).collect();
    assert_eq!(plugin, [9]);
    assert!(!lines.iter().any(|(n, _)| (3..=8).contains(n)), "o item de teste inteiro sai: {lines:?}");
}

/// Nenhuma linha do programa roda `cargo install`, fora a do instalador que
/// oferece o `ripgrep`.
#[test]
fn no_program_code_outside_the_tests_runs_cargo_install() {
    let found: Vec<String> = program_lines()
        .into_iter()
        .filter(|(file, _, line)| file != THIRD_PARTY_INSTALLER && runs_cargo_install(line))
        .map(|(file, n, line)| format!("{file}:{n}: {}", line.trim()))
        .collect();
    assert!(found.is_empty(), "o programa instala o próprio programa: {found:#?}");
}

/// O registro de plugins e o programa do plugin só são lidos pelo código do
/// programa: nenhuma linha que os cita grava, copia, move ou apaga.
#[test]
fn the_plugin_registry_and_the_plugin_program_are_only_read() {
    let found: Vec<String> = program_lines()
        .into_iter()
        .filter(|(_, _, line)| writes_to_the_plugin(line))
        .map(|(file, n, line)| format!("{file}:{n}: {}", line.trim()))
        .collect();
    assert!(found.is_empty(), "o programa escreve na pasta do plugin: {found:#?}");
}
