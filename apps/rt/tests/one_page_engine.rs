//! Um motor de página só.
//!
//! Fora de `apps/rt/src/report/`, nenhum arquivo de código do Mustard escreve
//! o começo de uma página HTML (`<!doctype html>`) ou uma folha de estilo
//! (`<style>`), nem converte markdown em HTML. A conversão aparece pelas
//! marcas que só ela produz: `<code>`, `<strong>` e `<em>`, que nascem das
//! crases e dos asteriscos. O código de teste fica de fora: ele confere
//! páginas, não as escreve.

use std::fs;
use std::path::{Path, PathBuf};

/// O que só o motor de página escreve.
const ENGINE_ONLY: &[&str] = &["<!doctype", "<style", "<code>", "<strong>", "<em>"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Os arquivos `.rs` de código sob `dir`, em ordem; pastas de teste, de
/// compilação e de dependências ficam de fora.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !matches!(name.to_str(), Some("target" | "node_modules" | ".git" | "tests" | "benches")) {
                rust_sources(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// O código de produção de um arquivo: cada item marcado com `#[cfg(test)]`
/// sai, do atributo até o `;` ou até o fim do bloco de chaves dele.
fn production_code(source: &str) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let item = &rest[at + "#[cfg(test)]".len()..];
        let semicolon = item.find(';');
        let brace = item.find('{');
        rest = match (brace, semicolon) {
            (Some(open), Some(end)) if end < open => &item[end + 1..],
            (Some(open), _) => {
                let mut depth = 0usize;
                let mut close = item.len();
                for (i, c) in item[open..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                close = open + i + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                &item[close..]
            }
            (None, Some(end)) => &item[end + 1..],
            (None, None) => "",
        };
    }
    out.push_str(rest);
    out
}

fn violations(path: &Path) -> Vec<&'static str> {
    let source = fs::read_to_string(path).unwrap_or_default();
    let code = production_code(&source).to_ascii_lowercase();
    ENGINE_ONLY.iter().copied().filter(|mark| code.contains(mark)).collect()
}

#[test]
fn only_the_page_engine_writes_html_pages_or_converts_markdown() {
    let root = repo_root();
    let mut files = Vec::new();
    for top in ["apps", "packages"] {
        rust_sources(&root.join(top), &mut files);
    }
    assert!(files.len() > 100, "the scan found only {} files", files.len());

    let engine = root.join("apps/rt/src/report");
    let mut found = Vec::new();
    for file in files.iter().filter(|f| !f.starts_with(&engine)) {
        for mark in violations(file) {
            found.push(format!("{} writes {mark}", file.strip_prefix(&root).unwrap_or(file).display()));
        }
    }
    assert!(found.is_empty(), "outside the page engine:\n{}", found.join("\n"));

    // A varredura enxerga o que procura: o próprio motor tem as marcas.
    let engine_marks: Vec<&str> = ["mod.rs", "markdown.rs", "render.rs"]
        .iter()
        .flat_map(|f| violations(&engine.join(f)))
        .collect();
    for mark in ["<!doctype", "<style", "<code>", "<strong>"] {
        assert!(engine_marks.contains(&mark), "the scan never sees {mark} even in the engine");
    }
}

#[test]
fn test_items_are_left_out_of_the_scan() {
    let source = "fn a() {}\n#[cfg(test)]\nuse x::y;\nfn b() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let _ = \"<style>\"; }\n}\n";
    let code = production_code(source);
    assert!(code.contains("fn a()") && code.contains("fn b()"), "{code}");
    assert!(!code.contains("<style>") && !code.contains("use x::y"), "{code}");
}
