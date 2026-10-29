//! A leitura da conversa do Claude Code que a medida dos scripts usa: as
//! chamadas de ferramenta, na ordem, e as pastas por onde o comando anda.

use std::collections::HashSet;
use std::io::BufRead;
use std::path::Path;

use serde_json::Value;

use crate::hooks::bash::lex::Segment;

/// Uma chamada de ferramenta da conversa, com o instante e a pasta da linha.
pub(super) struct Call {
    pub(super) name: String,
    pub(super) input: Value,
    pub(super) at: String,
    pub(super) cwd: String,
}

/// As chamadas de ferramenta de uma conversa, na ordem e cada uma uma vez só
/// (o arquivo pode trazer a mesma linha mais de uma vez).
pub(super) struct Transcript {
    pub(super) calls: Vec<Call>,
}

/// A conversa `path`, ou `None` quando o arquivo não abre.
pub(super) fn transcript(path: &Path) -> Option<Transcript> {
    let file = std::fs::File::open(path).ok()?;
    let mut read = Transcript { calls: Vec::new() };
    let mut seen: HashSet<String> = HashSet::new();
    for line in std::io::BufReader::new(file).split(b'\n').map_while(Result::ok) {
        let Ok(row) = serde_json::from_slice::<Value>(&line) else { continue };
        let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
        for part in row["message"]["content"].as_array().into_iter().flatten() {
            match part["type"].as_str() {
                Some("tool_use") if seen.insert(text(&part["id"])) => read.calls.push(Call {
                    name: text(&part["name"]),
                    input: part["input"].clone(),
                    at: text(&row["timestamp"]),
                    cwd: text(&row["cwd"]),
                }),
                _ => {}
            }
        }
    }
    Some(read)
}

/// A pasta para onde o comando `segment`, rodado em `dir`, muda, quando é um
/// `cd` para uma pasta nomeada.
pub(super) fn moved(segment: &Segment, dir: &str) -> Option<String> {
    let to = segment.args.first().filter(|to| segment.name() == "cd" && !to.text.starts_with('-'))?;
    Some(joined(&to.text, dir))
}

/// Os textos entre aspas, simples ou duplas, de `script`.
pub(super) fn quoted(script: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = script;
    while let Some(at) = rest.find(['\'', '"']) {
        let quote = if rest[at..].starts_with('"') { '"' } else { '\'' };
        let Some((inside, after)) = rest[at + 1..].split_once(quote) else { break };
        found.push(inside);
        rest = after;
    }
    found
}

/// `path`, a partir de `dir` quando é relativo, com a barra normal e sem os
/// trechos `.` e `..`.
pub(super) fn joined(path: &str, dir: &str) -> String {
    let path = path.replace('\\', "/");
    let absolute = path.starts_with('/') || path.get(1..3) == Some(":/");
    let full = if absolute || dir.is_empty() { path } else { format!("{}/{path}", dir.replace('\\', "/")) };
    let mut parts: Vec<&str> = Vec::new();
    for (at, part) in full.split('/').enumerate() {
        match part {
            "." => {}
            "" if at > 0 => {}
            ".." if parts.last().is_some_and(|last| !last.is_empty() && *last != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}
