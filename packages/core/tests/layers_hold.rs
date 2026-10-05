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
//! - Na busca do mapa, o índice e a leitura das notas em dia moram em módulos
//!   de baixo, o sentido não volta à busca nem à ordem, e quem grava a nota
//!   fica em cima de todos: nenhuma importação fecha ciclo entre eles.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::path::{Path, PathBuf};

/// A raiz do repositório na cópia que roda o teste.
fn repo_root() -> PathBuf {
    manifest_dir::manifest_dir().join("../..")
}

/// Se `path` está na pasta do ajudante atômico de escrita. O sistema que usa a
/// barra invertida entrega `packages/core/src\io\fs\lock.rs`, e a pasta, escrita
/// com `/`, só casa depois de todo separador virar a barra normal.
fn in_the_atomic_helper_folder(path: &Path) -> bool {
    path.to_string_lossy().replace('\\', "/").contains("packages/core/src/io/fs")
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

/// Se a linha abre o módulo `tests`, com ou sem `pub` e restrição de alcance.
fn opens_tests_module(line: &str) -> bool {
    let rest = match line.strip_prefix("pub(") {
        Some(after) => after.split_once(") ").map_or("", |(_, rest)| rest),
        None => line.strip_prefix("pub ").unwrap_or(line),
    };
    rest.strip_prefix("mod tests").is_some_and(|after| after.starts_with([' ', '{', ';']))
}

/// O código de produção de um arquivo: tudo antes do primeiro módulo de
/// testes (o `#[cfg(test)]` na primeira coluna, seguido de comentários e
/// atributos até o `mod tests`, com ou sem `pub` e restrição de alcance),
/// sem as linhas de comentário. Um módulo de apoio dos testes que venha antes
/// dele não encerra a leitura: o código de produção depois dele continua lido.
fn production_lines(text: &str) -> Vec<(usize, &str)> {
    let all: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    for (n, line) in all.iter().enumerate() {
        if *line == "#[cfg(test)]" {
            let next = all[n + 1..].iter().find(|l| !l.trim_start().starts_with("//") && !l.trim_start().starts_with("#["));
            if next.is_some_and(|l| opens_tests_module(l)) {
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
    assert!(files.len() >= 2, "the model layer has files to read: {files:?}");
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
        .filter(|p| !in_the_atomic_helper_folder(p))
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

/// Se a linha cita algum dos módulos `names` como palavra inteira (`map_notes`
/// não casa com `map_notes_fresh`).
fn names_a_module(line: &str, names: &[&str]) -> bool {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|word| names.contains(&word))
}

/// Cada linha de produção de `path` que cita algum dos módulos `names`,
/// escrita como `arquivo:linha: texto`.
fn lines_naming(path: &Path, names: &[&str]) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));
    let shown = path.strip_prefix(repo_root()).unwrap_or(path).display().to_string();
    production_lines(&text)
        .into_iter()
        .filter(|(_, line)| names_a_module(line, names))
        .map(|(n, line)| format!("{shown}:{n}: {}", line.trim()))
        .collect()
}

fn io_lines_naming(file: &str, names: &[&str]) -> Vec<String> {
    lines_naming(&repo_root().join("packages/core/src/io").join(file), names)
}

/// O índice e a leitura das notas em dia ficam embaixo: só conhecem o banco;
/// o sentido não importa a busca nem a ordem nem a escrita da nota; a busca
/// não importa a escrita da nota; e ninguém na biblioteca importa a escrita
/// da nota, que refaz o índice e os vetores e por isso fica em cima de todos.
#[test]
fn the_index_the_fresh_notes_and_the_sense_sit_below_the_search_and_the_note_writer() {
    const ABOVE_THE_INDEX: &[&str] = &[
        "map_search",
        "map_order",
        "map_sense",
        "map_meaning",
        "map_triage",
        "map_glossary",
        "map_notes",
        "project_map",
    ];
    let mut found: Vec<String> = Vec::new();
    found.extend(io_lines_naming("map_index.rs", ABOVE_THE_INDEX));
    found.extend(io_lines_naming("map_notes_fresh.rs", ABOVE_THE_INDEX));
    found.extend(io_lines_naming("map_notes_fresh.rs", &["map_index"]));
    found.extend(io_lines_naming("map_meaning.rs", &["map_search", "map_order", "map_sense", "map_notes"]));
    found.extend(io_lines_naming("map_sense.rs", &["map_search", "map_order", "map_triage", "map_notes"]));
    found.extend(io_lines_naming("map_search.rs", &["map_notes"]));
    for path in sources_under(&["packages/core/src"]) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name == "map_notes.rs" || path.ends_with("packages/core/src/io/mod.rs") {
            continue;
        }
        found.extend(lines_naming(&path, &["map_notes"]));
    }
    assert!(found.is_empty(), "an import closes a loop among the search layers:\n{}", found.join("\n"));
}

/// As duas funções da busca que só os testes chamavam não voltam: os testes
/// chamam as que a busca usa, com o que ela recebe de fato.
#[test]
fn the_search_has_no_function_only_its_tests_call() {
    let path = repo_root().join("packages/core/src/io/map_search.rs");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));
    for gone in ["fn ranked_files(", "fn sources("] {
        assert!(!text.contains(gone), "{gone} came back in map_search.rs; call the `_near` one with `Near::none()`");
    }
}

/// A pasta do ajudante atômico é reconhecida com qualquer separador: o mesmo
/// arquivo, escrito com a barra invertida do Windows ou com a barra normal,
/// cai dentro dela, e o arquivo de outra pasta fica de fora.
#[test]
fn the_atomic_helper_folder_is_recognized_with_either_separator() {
    for inside in ["packages/core/src\\io\\fs\\lock.rs", "packages/core/src/io/fs/lock.rs"] {
        assert!(in_the_atomic_helper_folder(Path::new(inside)), "{inside} is inside the helper folder");
    }
    for outside in ["packages/core/src\\io\\map_db.rs", "packages/core/src/io/map_db.rs"] {
        assert!(!in_the_atomic_helper_folder(Path::new(outside)), "{outside} is outside the helper folder");
    }
}
