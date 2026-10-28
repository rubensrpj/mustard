//! As fontes de código do Mustard e a leitura única do que, numa fonte Rust,
//! é código de teste: os arquivos `.rs` sob uma pasta ([`rust_files`]) e os
//! das pastas de código ([`rust_sources`]), o código sem os comentários e o
//! conteúdo dos textos ([`code_only`]), os trechos que só existem em teste
//! ([`test_spans`]) e, de cada arquivo, só o código de produção
//! ([`production_code`]).

use std::fs;
use std::path::{Path, PathBuf};

/// As pastas que não são código de produção: as de teste, a de compilação, a
/// de dependências e a do git.
const NOT_SOURCE_FOLDERS: &[&str] = &["target", "node_modules", ".git", "tests", "benches"];

/// Os arquivos `.rs` sob `dir`, em ordem; as pastas com um dos nomes de
/// `skipped` ficam de fora, em qualquer profundidade.
pub fn rust_files(dir: &Path, skipped: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            if !entry.file_name().to_str().is_some_and(|name| skipped.contains(&name)) {
                rust_files(&path, skipped, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Os arquivos `.rs` de código sob `dir`, em ordem; pastas de teste, de
/// compilação e de dependências ficam de fora.
pub fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    rust_files(dir, NOT_SOURCE_FOLDERS, out);
}

/// O código de produção de um arquivo: o texto sem os trechos que só existem
/// em teste ([`test_spans`]). Os comentários e os textos do resto ficam.
pub fn production_code(source: &str) -> String {
    let code = code_only(source);
    let mut out = String::new();
    let mut kept = 0;
    for (from, to) in test_spans(&code) {
        out.push_str(&source[kept..from]);
        kept = to;
    }
    out.push_str(&source[kept..]);
    out
}

/// `text` com os comentários e o conteúdo dos textos e dos caracteres
/// trocados por espaços, byte por byte: as posições e as quebras de linha
/// ficam onde estavam.
pub fn code_only(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for byte in &mut out[from..to.min(bytes.len())] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    let mut at = 0;
    while at < bytes.len() {
        let rest = &bytes[at..];
        if rest.starts_with(b"//") {
            let end = rest.iter().position(|&b| b == b'\n').map_or(bytes.len(), |n| at + n);
            blank(&mut out, at, end);
            at = end;
        } else if rest.starts_with(b"/*") {
            let mut depth = 0usize;
            let mut end = at;
            while end < bytes.len() {
                if bytes[end..].starts_with(b"/*") {
                    depth += 1;
                    end += 2;
                } else if bytes[end..].starts_with(b"*/") {
                    depth -= 1;
                    end += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    end += 1;
                }
            }
            blank(&mut out, at, end);
            at = end;
        } else if let Some(open) = raw_string_open(bytes, at) {
            let hashes = open.hashes;
            let mut end = open.body;
            loop {
                match bytes[end..].iter().position(|&b| b == b'"') {
                    Some(quote) if bytes[end + quote + 1..].iter().take(hashes).filter(|&&b| b == b'#').count() == hashes => {
                        end += quote + 1 + hashes;
                        break;
                    }
                    Some(quote) => end += quote + 1,
                    None => {
                        end = bytes.len();
                        break;
                    }
                }
            }
            blank(&mut out, open.body, end.saturating_sub(1 + hashes));
            at = end;
        } else if bytes[at] == b'"' {
            let mut end = at + 1;
            while end < bytes.len() && bytes[end] != b'"' {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            blank(&mut out, at + 1, end);
            at = end + 1;
        } else if bytes[at] == b'\'' {
            at = char_literal_end(bytes, at).map_or(at + 1, |end| {
                blank(&mut out, at + 1, end - 1);
                end
            });
        } else {
            at += 1;
        }
    }
    out
}

/// O começo de um texto cru (`r"…"`, `r#"…"#`, `br"…"`) em `at`.
struct RawOpen {
    body: usize,
    hashes: usize,
}

fn raw_string_open(bytes: &[u8], at: usize) -> Option<RawOpen> {
    let after_prefix = if bytes[at..].starts_with(b"br") {
        at + 2
    } else if bytes[at] == b'r' {
        at + 1
    } else {
        return None;
    };
    if at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_') {
        return None;
    }
    let hashes = bytes[after_prefix..].iter().take_while(|&&b| b == b'#').count();
    (bytes.get(after_prefix + hashes) == Some(&b'"')).then_some(RawOpen { body: after_prefix + hashes + 1, hashes })
}

/// O fim (depois da aspa) do caractere que abre em `at`, ou `None` quando a
/// aspa abre um tempo de vida (`'a`).
fn char_literal_end(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at + 1) == Some(&b'\\') {
        let close = bytes.get(at + 3..)?.iter().position(|&b| b == b'\'')?;
        return Some(at + 3 + close + 1);
    }
    let width = match bytes.get(at + 1)? {
        b if *b < 0x80 => 1,
        b if *b >= 0xF0 => 4,
        b if *b >= 0xE0 => 3,
        _ => 2,
    };
    (bytes.get(at + 1 + width) == Some(&b'\'')).then_some(at + 2 + width)
}

/// Os trechos que só existem em teste no código `code`, o de [`code_only`]:
/// cada item com um atributo `cfg` que só vale em teste, dos atributos
/// colados nele ao fim dele, em bytes e em ordem.
pub fn test_spans(code: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut at = 0;
    while let Some(found) = code[at..].windows(2).position(|pair| pair == b"#[") {
        let start = at + found;
        let mut cursor = start;
        let mut marked = false;
        while let Some((attribute, end)) = attribute_at(code, cursor) {
            marked |= marks_test(&attribute);
            cursor = end + code[end..].iter().take_while(|b| b.is_ascii_whitespace()).count();
        }
        if marked {
            let end = item_end(code, cursor);
            spans.push((start, end));
            at = end;
        } else {
            at = cursor.max(start + 2);
        }
    }
    spans
}

/// O atributo que abre em `at` no código `code`, inteiro e sem espaços, e
/// onde ele acaba.
fn attribute_at(code: &[u8], at: usize) -> Option<(String, usize)> {
    if !code[at..].starts_with(b"#[") {
        return None;
    }
    let mut depth = 0usize;
    for (offset, &byte) in code[at + 1..].iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    let end = at + 1 + offset + 1;
                    let text: String = String::from_utf8_lossy(&code[at..end]).split_whitespace().collect();
                    return Some((text, end));
                }
            }
            _ => {}
        }
    }
    None
}

/// O atributo, sem espaços, só vale em teste: um `#[cfg(...)]` cuja condição
/// exige `test` ([`requires_test`]).
fn marks_test(attribute: &str) -> bool {
    attribute.strip_prefix("#[cfg(").and_then(|rest| rest.strip_suffix(")]")).is_some_and(requires_test)
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

/// O fim do item que começa em `from`: o `;` dele fora de parênteses e
/// colchetes, ou a chave que fecha o bloco dele, o que vier primeiro.
fn item_end(code: &[u8], from: usize) -> usize {
    let (mut braces, mut nested) = (0usize, 0usize);
    for (offset, &byte) in code[from..].iter().enumerate() {
        match byte {
            b';' if braces == 0 && nested == 0 => return from + offset + 1,
            b'(' | b'[' => nested += 1,
            b')' | b']' => nested = nested.saturating_sub(1),
            b'{' => braces += 1,
            b'}' => {
                braces = braces.saturating_sub(1);
                if braces == 0 {
                    return from + offset + 1;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// O item que só existe em teste sai do código de produção inteiro, seja
/// qual for a forma do `cfg` que o marca (`test` sozinho, `all(test, ...)` ou
/// qualquer outra que exija `test`): dos atributos colados nele ao `;` dele,
/// passando pelo `;` de dentro de colchetes, ou ao fim do bloco de chaves.
#[test]
fn production_code_drops_every_item_that_only_exists_in_a_test_build() {
    let source = concat!(
        "use crate::kept;\n",
        "#[cfg(test)]\nmod plain {\n    use super::*;\n}\n",
        "#[cfg(all(test, target_os = \"linux\"))]\nmod with_target {\n    use super::*;\n}\n",
        "#[cfg(any(all(test, unix), all(test, windows)))]\nmod nested {\n    use super::*;\n}\n",
        "#[cfg( all( unix , test ) )]\nuse crate::only_in_tests;\n",
        "#[allow(dead_code)]\n#[cfg(test)]\nfn helper() {}\n",
        "#[cfg(test)]\nconst SIZES: [u8; 2] = [1, 2];\n",
        "fn kept() {}\n",
    );
    let code = production_code(source);
    let lines: Vec<&str> = code.lines().filter(|line| !line.trim().is_empty()).collect();
    assert_eq!(lines, ["use crate::kept;", "fn kept() {}"], "{code}");
}

/// O item que também compila fora de teste fica no código de produção, mesmo
/// quando o `cfg` nomeia `test`, e o `cfg` escrito num comentário ou num texto
/// não marca item nenhum.
#[test]
fn production_code_keeps_every_item_that_also_exists_outside_a_test_build() {
    let source = concat!(
        "#[cfg(not(test))]\nfn negated() {}\n",
        "#[cfg(any(test, unix))]\nfn either() {}\n",
        "#[cfg(all(unix, any(test, windows)))]\nfn mixed() {}\n",
        "#[cfg(all(unix, target_os = \"linux\"))]\nfn system() {}\n",
        "#[cfg(feature = \"test\")]\nfn feature() {}\n",
        "/// #[cfg(test)] marca o item que só existe em teste.\nfn documented() {}\n",
        "const NOTE: &str = \"#[cfg(test)]\";\nfn noted() {}\n",
    );
    assert_eq!(production_code(source), source);
}

/// O `//` de um endereço dentro de um texto não abre comentário: o código
/// depois do texto fica, e o comentário do fim da linha sai.
#[test]
fn code_only_keeps_the_code_after_an_address_inside_a_text() {
    let code = code_only("let page = \"https://example.com\"; use crate::kept; // crate::gone\n");
    let code = String::from_utf8_lossy(&code);
    assert!(code.contains("use crate::kept;"), "{code}");
    assert!(!code.contains("example") && !code.contains("crate::gone"), "{code}");
}
