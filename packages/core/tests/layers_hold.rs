//! `layers_hold` — as camadas da biblioteca de base cumprem o que cada uma
//! promete, lido no código de produção (o que vem antes do módulo de testes
//! de cada arquivo).
//!
//! - A camada do modelo (`domain/model`) é pura: não toca disco, processo,
//!   ambiente, rede nem escreve no terminal. Efeito colateral mora em `io` e
//!   `platform`.
//! - Arquivo se escreve pelo ajudante atômico de `io/fs` (arquivo temporário
//!   e troca de nome); nenhum outro trecho da biblioteca abre um arquivo para
//!   escrita por conta própria.
//! - O autômato de vários padrões nasce num lugar só, no vocabulário; quem
//!   precisa de um usa o dele.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::path::{Path, PathBuf};

/// A raiz do repositório na cópia que roda o teste.
fn repo_root() -> PathBuf {
    manifest_dir::manifest_dir().join("../..")
}

/// Todo `.rs` debaixo de `dir`, em ordem.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// O código de produção de um arquivo: tudo antes do primeiro módulo de
/// testes (o `#[cfg(test)]` na primeira coluna, seguido de comentários e
/// atributos até o `mod`), sem as linhas de comentário.
fn production_lines(text: &str) -> Vec<(usize, &str)> {
    let all: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    for (n, line) in all.iter().enumerate() {
        if *line == "#[cfg(test)]" {
            let next = all[n + 1..].iter().find(|l| !l.trim_start().starts_with("//") && !l.trim_start().starts_with("#["));
            if next.is_some_and(|l| l.starts_with("mod ") || l.starts_with("pub mod ")) {
                break;
            }
        }
        if !line.trim_start().starts_with("//") {
            kept.push((n + 1, *line));
        }
    }
    kept
}

/// Cada ocorrência de um dos `needles` no código de produção dos arquivos, já
/// escrita como `arquivo:linha: texto`.
fn hits(files: &[PathBuf], needles: &[&str]) -> Vec<String> {
    let root = repo_root();
    let mut found = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));
        for (n, line) in production_lines(&text) {
            if needles.iter().any(|needle| line.contains(needle)) {
                let shown = path.strip_prefix(&root).unwrap_or(path).display();
                found.push(format!("{shown}:{n}: {}", line.trim()));
            }
        }
    }
    found
}

fn sources_under(dirs: &[&str]) -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in dirs {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(files.len() >= 5, "the walk found almost nothing to read under {dirs:?}: {files:?}");
    files
}

/// A camada do modelo não toca o disco, o processo, o ambiente, a rede nem o
/// terminal.
#[test]
fn the_model_layer_touches_no_disk_process_environment_or_terminal() {
    let mut files = Vec::new();
    rust_files(&repo_root().join("packages/core/src/domain/model"), &mut files);
    assert!(files.len() >= 4, "the model layer has files to read: {files:?}");
    let found = hits(
        &files,
        &[
            "std::fs",
            "std::io::",
            "std::process",
            "std::env",
            "std::net",
            "println!",
            "eprintln!",
            "tracing::",
            "log::",
            "crate::io::",
        ],
    );
    assert!(found.is_empty(), "the model layer must stay pure, but reaches out here:\n{}", found.join("\n"));
}

/// Nenhum trecho fora de `io/fs` abre arquivo para escrita: quem grava chama
/// o ajudante atômico.
#[test]
fn files_are_written_only_through_the_atomic_helper() {
    let files: Vec<PathBuf> = sources_under(&["packages/core/src"])
        .into_iter()
        .filter(|p| !p.to_string_lossy().contains("packages/core/src/io/fs"))
        .collect();
    let found = hits(&files, &["fs::write(", "File::create(", "File::create_new(", "OpenOptions::new("]);
    assert!(
        found.is_empty(),
        "a file is written by hand here, outside the atomic helper of io/fs:\n{}",
        found.join("\n")
    );
}

/// O autômato de vários padrões é construído num arquivo só.
#[test]
fn the_multi_pattern_automaton_is_built_in_one_place() {
    let files: Vec<PathBuf> = sources_under(&["packages/core/src", "apps/cli/src", "apps/rt/src", "apps/scan/src"])
        .into_iter()
        .filter(|p| !p.ends_with("packages/core/src/domain/vocabulary/aho.rs"))
        .collect();
    let found = hits(&files, &["AhoCorasick::new(", "AhoCorasick::builder(", "AhoCorasickBuilder"]);
    assert!(found.is_empty(), "a second automaton is built here; reuse the vocabulary one:\n{}", found.join("\n"));
}
