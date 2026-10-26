//! As fontes de código do Mustard, para o teste que confere uma regra em
//! todas elas: os arquivos `.rs` das pastas de código e, de cada um, só o
//! código de produção.

use std::fs;
use std::path::{Path, PathBuf};

/// Os arquivos `.rs` de código sob `dir`, em ordem; pastas de teste, de
/// compilação e de dependências ficam de fora.
pub fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
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
pub fn production_code(source: &str) -> String {
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
