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

/// O código de produção de um arquivo: cada item cujo atributo `cfg` só vale
/// em teste (`cfg(test)`, `cfg(all(test, ...))` e qualquer forma que exija
/// `test`) sai, do atributo até o `;` ou até o fim do bloco de chaves dele.
pub fn production_code(source: &str) -> String {
    const OPEN: &str = "#[cfg(";
    let mut out = String::new();
    let mut rest = source;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at + OPEN.len()..];
        match cfg_attribute(after) {
            Some((condition, item)) if requires_test(condition) => {
                out.push_str(&rest[..at]);
                rest = after_item(item);
            }
            _ => {
                out.push_str(&rest[..at + OPEN.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A condição do atributo `#[cfg(...)]` e o texto que vem depois dele, dado o
/// texto logo depois do `(` que abre a condição.
fn cfg_attribute(after_open: &str) -> Option<(&str, &str)> {
    let mut depth = 1usize;
    for (i, c) in after_open.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let item = after_open[i + 1..].trim_start().strip_prefix(']')?;
                    return Some((&after_open[..i], item));
                }
            }
            _ => {}
        }
    }
    None
}

/// `true` quando a condição `cfg` só vale em teste: `test`, um `all(...)` com
/// alguma parte que exige `test`, ou um `any(...)` em que todas as partes
/// exigem. `not(...)` e as demais condições valem fora de teste também.
fn requires_test(condition: &str) -> bool {
    let condition = condition.trim();
    if condition == "test" {
        return true;
    }
    let Some((name, args)) = condition.split_once('(') else {
        return false;
    };
    let Some(args) = args.trim_end().strip_suffix(')') else {
        return false;
    };
    let parts = top_level_parts(args);
    match name.trim() {
        "all" => parts.iter().any(|part| requires_test(part)),
        "any" => !parts.is_empty() && parts.iter().all(|part| requires_test(part)),
        _ => false,
    }
}

/// As partes de uma lista separadas por vírgula fora de parênteses, sem as
/// vazias (a vírgula do fim).
fn top_level_parts(list: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in list.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&list[start..]);
    parts.into_iter().filter(|part| !part.trim().is_empty()).collect()
}

/// O texto que sobra depois do item que começa em `item`, até o `;` ou até o
/// fim do bloco de chaves dele.
fn after_item(item: &str) -> &str {
    let semicolon = item.find(';');
    let brace = item.find('{');
    match (brace, semicolon) {
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
    }
}
