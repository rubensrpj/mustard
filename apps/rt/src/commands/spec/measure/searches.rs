//! As buscas que o Mustard respondeu no projeto, lidas das conversas do
//! Claude Code desde a marca.
//!
//! A unidade é uma chamada `Grep`, ou `Bash` com `grep`, `rg` ou `git grep`,
//! que teve resposta do gancho. A cravada vai no lugar da busca: é a recusa
//! do gancho, no resultado da própria chamada. A parcial vai ao lado: é o
//! contexto que o gancho pôs antes da chamada, ligado a ela pelo código dela.
//! A resposta se reconhece pelo catálogo de textos, nos dois idiomas, e a
//! que diz que o mapa não achou ou não responde não entra. De cada busca
//! respondida ficam os arquivos que a resposta lista e as três chamadas
//! seguintes da mesma conversa, para dizer depois o que o Claude fez com ela.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use mustard_core::io::spend::project_conversations;
use mustard_core::platform::i18n::{translate, Locale};
use serde::Serialize;
use serde_json::{json, Value};

use crate::hooks::bash::lex::segments;

/// O trecho com que o Claude Code abre, na primeira linha do resultado, o
/// texto da recusa de um gancho.
const HOOK_REFUSAL: &str = "hook error: ";

/// O resultado que o Claude Code dá à chamada que terminou sem saída.
const NO_OUTPUT: [&str; 3] = ["(Bash completed with no output)", "No matches found", "No files found"];

/// Quantas chamadas seguintes de cada busca ficam guardadas.
const NEXT_CALLS: usize = 3;

/// Os idiomas em que a resposta pode ter saído.
const LOCALES: [Locale; 2] = [Locale::PtBr, Locale::EnUs];

/// A classe da resposta, pela chave do catálogo que abre o texto dela.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Class {
    /// Cravada: a resposta foi no lugar da busca.
    Pinned,
    /// Parcial, com o que faltou ao mapa.
    Partial,
    /// Parcial, sem certeza de que o lugar é este.
    PartialUnsure,
}

/// Cada classe, com a chave do catálogo do texto dela.
const CLASSES: [(Class, &str); 3] = [
    (Class::Pinned, "map.answer.pinned"),
    (Class::Partial, "map.answer.partial"),
    (Class::PartialUnsure, "map.answer.partial_unsure"),
];

/// Uma busca respondida: a chamada, a conversa (o arquivo dela dentro da
/// pasta das conversas), o instante, a classe e os arquivos da resposta, se
/// a busca deu erro ou nenhuma saída, a pasta em que rodou e as chamadas
/// seguintes, com o nome da ferramenta e a entrada.
#[derive(Serialize)]
pub(super) struct Answered {
    tool_use_id: String,
    conversation: String,
    at: String,
    class: Class,
    files: Vec<String>,
    failed: bool,
    cwd: String,
    next: Vec<Value>,
}

/// Uma chamada de ferramenta da conversa, com o instante e a pasta da linha.
struct Call {
    id: String,
    name: String,
    input: Value,
    at: String,
    cwd: String,
}

/// As buscas que o Mustard respondeu desde `since` nas conversas do projeto
/// de `root` na pasta de configuração `config`, na ordem das conversas e,
/// dentro de cada uma, na das chamadas.
pub(super) fn answered(config: &Path, root: &Path, since: DateTime<Utc>) -> Vec<Answered> {
    let folder = config.join("projects");
    let mut found = Vec::new();
    for path in project_conversations(config, root, SystemTime::from(since)) {
        let name = path.strip_prefix(&folder).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        read(&path, &name, since, &mut found);
    }
    found
}

/// A contagem de `found`: as respondidas, as de cada classe e as que deram
/// erro ou nenhuma saída.
pub(super) fn tally(found: &[Answered]) -> Value {
    let count = |class: Class| found.iter().filter(|search| search.class == class).count();
    json!({
        "answered": found.len(),
        "pinned": count(Class::Pinned),
        "partial": count(Class::Partial),
        "partial_unsure": count(Class::PartialUnsure),
        "failed": found.iter().filter(|search| search.failed).count(),
    })
}

/// Põe em `found` as buscas respondidas desde `since` da conversa `path`,
/// chamada `name`.
fn read(path: &Path, name: &str, since: DateTime<Utc>, found: &mut Vec<Answered>) {
    let Ok(file) = std::fs::File::open(path) else { return };
    let mut calls: Vec<Call> = Vec::new();
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    let mut notes: HashMap<String, Vec<String>> = HashMap::new();
    for line in std::io::BufReader::new(file).split(b'\n').map_while(Result::ok) {
        let Ok(row) = serde_json::from_slice::<Value>(&line) else { continue };
        let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
        for part in row["message"]["content"].as_array().into_iter().flatten() {
            match part["type"].as_str() {
                Some("tool_use") => calls.push(Call {
                    id: text(&part["id"]),
                    name: text(&part["name"]),
                    input: part["input"].clone(),
                    at: text(&row["timestamp"]),
                    cwd: text(&row["cwd"]),
                }),
                Some("tool_result") => {
                    let said = part["is_error"].as_bool().unwrap_or(false);
                    results.insert(text(&part["tool_use_id"]), (result_text(&part["content"]), said));
                }
                _ => {}
            }
        }
        let note = &row["attachment"];
        let before_call = note["hookName"].as_str().is_some_and(|hook| hook.starts_with("PreToolUse:"));
        if note["type"] == "hook_additional_context" && before_call {
            let said = note["content"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string);
            notes.entry(text(&note["toolUseID"])).or_default().extend(said);
        }
    }
    for (index, call) in calls.iter().enumerate() {
        let after = DateTime::parse_from_rfc3339(&call.at).is_ok_and(|when| when >= since);
        if !after || !is_search(call) {
            continue;
        }
        let (output, error) = results.get(&call.id).cloned().unwrap_or_default();
        let refusal = output.split_once(HOOK_REFUSAL).filter(|(head, _)| error && !head.contains('\n'));
        let instead = refusal.and_then(|(_, said)| class_of(said).map(|class| (class, said.to_string(), true)));
        let beside = || notes.get(&call.id)?.iter().find_map(|said| Some((class_of(said)?, said.clone(), false)));
        let Some((class, said, refused)) = instead.or_else(beside) else { continue };
        let shown = output.lines().map(str::trim).any(|line| !line.is_empty() && !NO_OUTPUT.contains(&line));
        let next = calls.iter().skip(index + 1).take(NEXT_CALLS);
        found.push(Answered {
            tool_use_id: call.id.clone(),
            conversation: name.to_string(),
            at: call.at.clone(),
            class,
            files: listed(&said),
            failed: !refused && (error || !shown),
            cwd: call.cwd.clone(),
            next: next.map(|call| json!({ "tool": call.name, "input": call.input })).collect(),
        });
    }
}

/// O texto do resultado de uma chamada: o texto direto, ou os pedaços de
/// texto juntos, um por linha.
fn result_text(content: &Value) -> String {
    match content {
        Value::Array(parts) => parts.iter().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.as_str().unwrap_or_default().to_string(),
    }
}

/// Se `call` é uma busca por texto: a ferramenta `Grep`, ou um comando de
/// terminal com `grep`, `egrep`, `fgrep`, `rg` ou `git grep`.
fn is_search(call: &Call) -> bool {
    let grep = |name: &str| matches!(name, "grep" | "egrep" | "fgrep" | "rg");
    match call.name.as_str() {
        "Grep" => true,
        "Bash" => segments(call.input["command"].as_str().unwrap_or_default()).iter().any(|segment| {
            grep(segment.name()) || (segment.name() == "git" && segment.args.iter().any(|word| word.text == "grep"))
        }),
        _ => false,
    }
}

/// A classe da resposta `said`, que abre com o texto de uma das chaves de
/// [`CLASSES`] num dos idiomas, ou `None` quando não é resposta do mapa.
fn class_of(said: &str) -> Option<Class> {
    let opens = |key: &str| LOCALES.iter().any(|&lang| opens_with(said, translate(key, lang)));
    CLASSES.iter().find(|(_, key)| opens(key)).map(|&(class, _)| class)
}

/// Se `said` abre com `template` até o fim da segunda frase dele, com cada
/// lacuna `{…}` valendo qualquer trecho da primeira linha.
fn opens_with(said: &str, template: &str) -> bool {
    let mut ends = template.char_indices().filter(|&(at, mark)| {
        mark == '.' && template[at + 1..].chars().next().is_none_or(char::is_whitespace)
    });
    let head = ends.nth(1).map_or(template, |(at, _)| &template[..=at]);
    let mut pieces = head.split('{').map(|part| part.split_once('}').map_or(part, |(_, fixed)| fixed));
    let line = said.lines().next().unwrap_or_default();
    let Some(mut rest) = line.strip_prefix(pieces.next().unwrap_or_default()) else { return false };
    pieces.all(|piece| match rest.find(piece) {
        Some(at) => {
            rest = &rest[at + piece.len()..];
            true
        }
        None => false,
    })
}

/// Os arquivos que a resposta `said` lista, na ordem e sem repetir: cada
/// linha de caminho depois da primeira, com ou sem o aviso de arquivo mudado
/// depois do mapa, e os da frase em que só o mapa aponta arquivos.
fn listed(said: &str) -> Vec<String> {
    let changed: Vec<String> = LOCALES.iter().map(|&lang| format!(" ({})", translate("map.answer.changed", lang))).collect();
    let only = LOCALES.map(|lang| translate("map.answer.map_only", lang).split("{files}").next().unwrap_or_default());
    let mut files: Vec<String> = Vec::new();
    for line in said.lines().skip(1) {
        let paths: Vec<&str> = match only.iter().find_map(|head| line.strip_prefix(head)) {
            Some(rest) => rest.split('`').skip(1).step_by(2).collect(),
            None => vec![changed.iter().find_map(|mark| line.strip_suffix(mark.as_str())).unwrap_or(line)],
        };
        for path in paths.into_iter().filter(|path| !path.is_empty() && !path.contains(char::is_whitespace)) {
            if !files.iter().any(|known| known == path) {
                files.push(path.to_string());
            }
        }
    }
    files
}
