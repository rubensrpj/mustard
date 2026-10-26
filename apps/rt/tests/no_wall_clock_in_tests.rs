// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Nenhum teste do repositório decide pelo relógio da máquina.
//!
//! A suíte roda em paralelo, numa máquina carregada: o prazo que sobra numa
//! máquina folgada falta noutra, e o teste cai sem defeito nenhum. O teste que
//! mede tempo usa o relógio falso; o que espera outro processo ou outra linha
//! de execução espera o evento dela, com um limite de segurança largo que só
//! derruba o teste cujo evento nunca vem, sem afirmar nada sobre o tempo; o
//! resto afirma a ordem dos eventos; a pausa antes de afirmar que uma coisa não
//! aconteceu nunca derruba um teste. O relógio de verdade só entra no código de
//! teste pelos arquivos do relógio dos testes ([`CLOCK_FILES`]).
//!
//! Código de teste é o arquivo sob uma pasta `tests` (fora das `fixtures`, que
//! são dados), o item marcado `#[cfg(test)]` ou `#[cfg(all(test, …))]` e o
//! arquivo que uma declaração `mod x;` marcada assim traz. Nele, cada uma
//! destas leituras do relógio é acusada:
//!
//! - `.elapsed()`: o tempo que passou;
//! - `Instant::now()` que não é o argumento de uma chamada nem o valor de um
//!   campo: o começo de uma medida ou a comparação com um prazo. O argumento
//!   e o campo são o carimbo da hora de uma chamada ou o prazo que o código
//!   testado recebe de fora (`agora + espera`), e o teste não os mede;
//! - `thread::sleep(`: a espera pelo relógio;
//! - `recv_timeout(`, `wait_timeout(` e `park_timeout(`: a espera com prazo.
//!
//! A data do calendário (`SystemTime::now()`), que só nomeia uma pasta ou data
//! um arquivo, não mede o tempo que passa, e fica.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Os arquivos do relógio dos testes, do topo do repositório: o relógio
/// falso, a espera pelo evento de outro processo, a pausa antes de afirmar
/// que uma coisa não aconteceu e o cronômetro que só vai ao relato de uma
/// medição. São os únicos do código de teste que leem o relógio de verdade, e
/// nenhum deles decide que um teste cai pela lentidão da máquina: a espera só
/// derruba, no limite de segurança, o teste cujo evento nunca vem.
const CLOCK_FILES: &[&str] = &[
    "packages/core/tests/support/fake_clock.rs",
    "packages/core/tests/support/wait_for.rs",
    "packages/core/tests/support/pause.rs",
    "packages/core/tests/support/stopwatch.rs",
];

/// As pastas dos pacotes do repositório, do topo dele.
const PACKAGE_FOLDERS: &[&str] = &["apps", "packages"];

/// As pastas que não são código de nenhum pacote.
const SKIPPED_FOLDERS: &[&str] = &["target", "node_modules", ".git", "fixtures"];

/// As esperas com prazo, como o código as chama.
const BOUNDED_WAITS: &[&str] = &["recv_timeout(", "wait_timeout(", "park_timeout("];

/// Cada leitura do relógio acusada: onde está e o que ela faz.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    file: String,
    line: usize,
    what: &'static str,
}

/// `text` com os comentários e o conteúdo dos textos e dos caracteres
/// trocados por espaços, byte por byte: as posições e as quebras de linha
/// ficam onde estavam.
fn code_only(text: &str) -> Vec<u8> {
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

/// O atributo que abre em `at` no código `code`, inteiro, e onde ele acaba.
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

/// O atributo, sem espaços, só vale para os testes: `#[cfg(test)]` ou um
/// `#[cfg(all(…))]` com `test` entre as condições.
fn marks_test(attribute: &str) -> bool {
    attribute == "#[cfg(test)]"
        || attribute.strip_prefix("#[cfg(all(").and_then(|rest| rest.strip_suffix("))]")).is_some_and(|list| list.split(',').any(|part| part == "test"))
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

/// Os trechos de teste de um arquivo de código: cada um do atributo que
/// marca o item ao fim dele.
fn test_items(code: &[u8]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
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
            ranges.push((start, end));
            at = end;
        } else {
            at = cursor.max(start + 2);
        }
    }
    ranges
}

/// Os atributos colados antes do byte `at`, do mais perto ao mais longe,
/// pulando a visibilidade (`pub`, `pub(crate)`).
fn attributes_before(code: &[u8], at: usize) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut end = at;
    loop {
        let trimmed = code[..end].iter().rposition(|b| !b.is_ascii_whitespace()).map_or(0, |n| n + 1);
        let head = &code[..trimmed];
        if let Some(visible) = [&b"pub(crate)"[..], b"pub(super)", b"pub"].iter().find(|word| head.ends_with(word)) {
            end = trimmed - visible.len();
            continue;
        }
        if !head.ends_with(b"]") {
            return found;
        }
        let mut depth = 0usize;
        let Some(open) = (0..trimmed).rev().find(|&n| {
            match code[n] {
                b']' => depth += 1,
                b'[' => depth -= 1,
                _ => {}
            }
            depth == 0
        }) else {
            return found;
        };
        if open == 0 || code[open - 1] != b'#' {
            return found;
        }
        found.push((open - 1, trimmed));
        end = open - 1;
    }
}

/// Cada `mod x;` de um arquivo: o arquivo que ele traz e se ele só vale para
/// os testes. O arquivo vem do `#[path]` da fila de atributos, a partir da
/// pasta do arquivo, ou do nome.
fn declared_modules(file: &Path, original: &str, code: &[u8]) -> Vec<(PathBuf, bool)> {
    let mut found = Vec::new();
    for (at, _) in code.windows(4).enumerate().filter(|(_, window)| *window == b"mod ") {
        if at > 0 && (code[at - 1].is_ascii_alphanumeric() || code[at - 1] == b'_') {
            continue;
        }
        let rest = &code[at + 4..];
        let name_len = rest.iter().position(|b| !(b.is_ascii_alphanumeric() || *b == b'_')).unwrap_or(rest.len());
        let after = rest[name_len..].iter().find(|b| !b.is_ascii_whitespace());
        if name_len == 0 || after != Some(&b';') {
            continue;
        }
        let name = String::from_utf8_lossy(&rest[..name_len]).into_owned();
        let attributes = attributes_before(code, at);
        let test = attributes.iter().any(|(from, to)| marks_test(&String::from_utf8_lossy(&code[*from..*to]).split_whitespace().collect::<String>()));
        let path = attributes.iter().find_map(|(from, to)| {
            let text: String = String::from_utf8_lossy(&code[*from..*to]).split_whitespace().collect();
            text.starts_with("#[path=").then(|| original[*from..*to].split('"').nth(1).unwrap_or_default().to_string())
        });
        found.push((declared_module(file, &name, path.as_deref()), test));
    }
    found
}

/// O arquivo que `mod name;` de `file` traz: o do `#[path]`, a partir da
/// pasta do arquivo; sem ele, `name.rs` ou `name/mod.rs` na pasta do módulo.
fn declared_module(file: &Path, name: &str, path: Option<&str>) -> PathBuf {
    let folder = file.parent().unwrap_or(Path::new("."));
    if let Some(path) = path {
        return folder.join(path);
    }
    let stem = file.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    let base = if matches!(stem, "mod" | "lib" | "main") { folder.to_path_buf() } else { folder.join(stem) };
    let flat = base.join(format!("{name}.rs"));
    if flat.is_file() { flat } else { base.join(name).join("mod.rs") }
}

/// O número da linha (a partir de 1) do byte `at`.
fn line_of(code: &[u8], at: usize) -> usize {
    code[..at].iter().filter(|&&b| b == b'\n').count() + 1
}

/// O byte anterior a `at` que não é espaço, e o seguinte a `to`.
fn neighbours(code: &[u8], at: usize, to: usize) -> (Option<u8>, Option<u8>) {
    let before = code[..at].iter().rev().copied().find(|b| !b.is_ascii_whitespace());
    let after = code[to..].iter().copied().find(|b| !b.is_ascii_whitespace());
    (before, after)
}

/// As leituras do relógio acusadas em `code`, entre os bytes `from` e `to`,
/// com o nome `shown` do arquivo.
fn clock_reads(shown: &str, code: &[u8], from: usize, to: usize) -> Vec<Finding> {
    let mut found = Vec::new();
    let text = &code[from..to];
    let mut push = |offset: usize, what: &'static str| found.push(Finding { file: shown.to_string(), line: line_of(code, from + offset), what });
    for (offset, _) in text.windows(10).enumerate().filter(|(_, window)| *window == b".elapsed()") {
        push(offset, "mede o tempo que passou (`.elapsed()`)");
    }
    for (offset, _) in text.windows(14).enumerate().filter(|(_, window)| *window == b"Instant::now()") {
        let mut start = from + offset;
        while start > 0 && (code[start - 1].is_ascii_alphanumeric() || matches!(code[start - 1], b'_' | b':')) {
            start -= 1;
        }
        let (before, after) = neighbours(code, start, from + offset + 14);
        let argument = matches!(before, Some(b'(' | b',' | b':')) && matches!(after, Some(b',' | b')' | b'}' | b'+'));
        let measured = code[..start].ends_with(b"duration_since(");
        if !argument || measured {
            push(offset, "lê o relógio para medir ou dar prazo (`Instant::now()`)");
        }
    }
    for (offset, _) in text.windows(14).enumerate().filter(|(_, window)| *window == b"thread::sleep(") {
        push(offset, "espera pelo relógio (`thread::sleep`)");
    }
    for wait in BOUNDED_WAITS {
        for (offset, _) in text.windows(wait.len()).enumerate().filter(|(_, window)| *window == wait.as_bytes()) {
            push(offset, "espera com prazo do relógio");
        }
    }
    found
}

/// Os arquivos `.rs` de `dir`, as pastas de [`SKIPPED_FOLDERS`] de fora.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !SKIPPED_FOLDERS.iter().any(|skipped| name == *skipped) {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// O arquivo está sob uma pasta `tests`.
fn in_tests_folder(shown: &str) -> bool {
    shown.split('/').rev().skip(1).any(|part| part == "tests")
}

/// As leituras do relógio de todo o código de teste do repositório em `top`.
fn findings_under(top: &Path) -> Vec<Finding> {
    let mut files = Vec::new();
    for folder in PACKAGE_FOLDERS {
        rust_files(&top.join(folder), &mut files);
    }
    let shown_of = |file: &Path| -> String {
        let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        let top = std::fs::canonicalize(top).unwrap_or_else(|_| top.to_path_buf());
        canonical.strip_prefix(&top).unwrap_or(&canonical).to_string_lossy().replace('\\', "/")
    };
    let mut whole_tests: BTreeSet<String> = files.iter().map(|file| shown_of(file)).filter(|shown| in_tests_folder(shown)).collect();
    let mut sources = Vec::new();
    for file in &files {
        let original = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let code = code_only(&original);
        let ranges = test_items(&code);
        let modules: Vec<(String, bool)> = declared_modules(file, &original, &code).into_iter().map(|(module, test)| (shown_of(&module), test)).collect();
        whole_tests.extend(modules.iter().filter(|(_, test)| *test).map(|(module, _)| module.clone()));
        sources.push((shown_of(file), code, ranges, modules));
    }
    // Todo arquivo que um arquivo de teste declara também é de teste.
    loop {
        let before = whole_tests.len();
        for (shown, _, _, modules) in &sources {
            if whole_tests.contains(shown) {
                let declared: Vec<String> = modules.iter().map(|(module, _)| module.clone()).collect();
                whole_tests.extend(declared);
            }
        }
        if whole_tests.len() == before {
            break;
        }
    }
    let mut found = BTreeSet::new();
    for (shown, code, ranges, _) in &sources {
        if CLOCK_FILES.contains(&shown.as_str()) {
            continue;
        }
        if whole_tests.contains(shown) {
            found.extend(clock_reads(shown, code, 0, code.len()));
        } else {
            for (from, to) in ranges {
                found.extend(clock_reads(shown, code, *from, *to));
            }
        }
    }
    found.into_iter().collect()
}

/// As leituras do relógio de um arquivo só, `src/lib.rs` de um pacote em
/// `top`, com `text`.
fn findings_of(text: &str) -> Vec<(usize, &'static str)> {
    let top = tempfile::tempdir().unwrap();
    let src = top.path().join("packages/sample/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), text).unwrap();
    findings_under(top.path()).into_iter().map(|finding| (finding.line, finding.what)).collect()
}

/// A varredura acusa a medida, o prazo, a espera pelo relógio e a espera com
/// prazo do código de teste, e deixa o código do programa, os comentários, os
/// textos, o carimbo passado a uma chamada e a data do calendário.
#[test]
fn the_clock_scan_flags_only_the_measures_and_the_waits_of_the_test_code() {
    let text = "\
pub fn program() { let started = std::time::Instant::now(); let _ = started.elapsed(); }
#[cfg(test)]
mod tests {
    // started.elapsed() num comentário
    fn stamped() { record(\"thread::sleep(\", std::time::Instant::now()); call(Instant::now() + LIMIT); let _ = std::time::SystemTime::now(); }
    fn measured() { let started = Instant::now(); assert!(started.elapsed() < LIMIT); }
    fn bounded() { let deadline = Instant::now() + LIMIT; let _ = rx.recv_timeout(LIMIT); }
    fn paused() { std::thread::sleep(LIMIT); let _ = r#\"Instant::now() + x\"#; let _ = 'x'; }
}
";
    let found: Vec<usize> = findings_of(text).into_iter().map(|(line, _)| line).collect();
    assert_eq!(found, [6, 6, 7, 7, 8], "{:?}", findings_of(text));
}

/// O evento que nunca vem derruba o teste com a mensagem do evento esperado,
/// no limite da espera, em vez de deixar a suíte parada. O limite pequeno vem
/// de fora; o de toda espera dos testes é o de segurança, largo.
#[test]
#[should_panic(expected = "a soltura que ninguém escreve")]
fn an_event_that_never_comes_fails_the_test_instead_of_hanging() {
    crate::wait_for::wait_for_within(std::time::Duration::from_millis(50), "a soltura que ninguém escreve", || None::<()>);
}

/// A espera por um canal, a do limite de segurança, dá o que foi mandado e,
/// quando quem manda some, o fim do canal na hora, como a espera sem limite:
/// a linha de execução que segura uma trava até o teste soltá-la acaba com o
/// teste que caiu antes de soltar, sem ficar presa até o limite.
#[test]
fn the_channel_wait_gives_what_was_sent_and_then_the_end_of_the_channel() {
    let (sender, channel) = std::sync::mpsc::channel();
    sender.send("a soltura").unwrap();
    drop(sender);
    assert_eq!(crate::wait_for::wait_for_message("a soltura", &channel), Ok("a soltura"));
    assert_eq!(crate::wait_for::wait_for_message("o fim do canal", &channel), Err(std::sync::mpsc::RecvError));
}

/// Nenhum código de teste do repositório lê o relógio de verdade para medir,
/// dar prazo ou esperar: só o relógio dos testes o faz.
#[test]
fn no_test_asserts_on_wall_clock_time() {
    let top = manifest_dir::manifest_dir().join("../..");
    let found = findings_under(&top);
    let listed: Vec<String> = found.iter().map(|finding| format!("{}:{}: {}", finding.file, finding.line, finding.what)).collect();
    assert!(listed.is_empty(), "{} leituras do relógio no código de teste:\n{}", listed.len(), listed.join("\n"));
}
