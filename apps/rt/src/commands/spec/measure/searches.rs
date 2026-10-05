//! As buscas que o Mustard respondeu no projeto, lidas das conversas do
//! Claude Code desde a marca, e o que o Claude fez depois de cada uma.
//!
//! A unidade é uma chamada `Grep`, ou `Bash` com `grep`, `rg` ou `git grep`,
//! que teve resposta do gancho. A cravada vai no lugar da busca: é a recusa
//! do gancho, no resultado da própria chamada. A parcial vai ao lado: é o
//! contexto que o gancho pôs antes da chamada, ligado a ela pelo código dela.
//! A resposta se reconhece pelo catálogo de textos, nos dois idiomas, e a
//! que diz que o mapa não achou ou não responde não entra. De cada busca
//! respondida ficam os arquivos que a resposta lista e as três chamadas
//! seguintes da mesma conversa. A primeira delas que abre arquivo ou busca dá
//! o desfecho: aproveitou (abriu um arquivo da lista), abriu outro ou buscou
//! de novo; sem nenhuma, seguiu. A busca que falhou e a que seguiu ficam fora
//! da taxa, que só vale com [`MIN_SEARCHES`] buscas.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use mustard_core::domain::spend::day_of_stamp;
use mustard_core::io::spend::project_conversations;
use mustard_core::platform::i18n::{translate, Locale};
use serde::Serialize;
use serde_json::{json, Value};

use super::{day_list, fill, grouped};
use crate::hooks::bash::lex::{segments, Segment};
use crate::hooks::bash::reading::{text_search, READERS};

/// O trecho com que o Claude Code abre, na primeira linha do resultado, o
/// texto da recusa de um gancho.
const HOOK_REFUSAL: &str = "hook error: ";

/// O resultado que o Claude Code dá à chamada de terminal que terminou sem
/// saída.
const NO_OUTPUT: &str = "(Bash completed with no output)";

/// Os trechos com que o Claude Code diz, no começo do resultado, que a
/// chamada foi barrada pela permissão ou interrompida.
const STOPPED: [&str; 3] = ["Permission to use", "The user doesn't want to proceed", "Request interrupted"];

/// Até quantos caracteres do começo do resultado vale o aviso de chamada
/// barrada.
const STOPPED_HEAD: usize = 400;

/// O aviso do zsh que recusa um curinga sem aspas que não casou com nada,
/// seguido da palavra recusada.
const NO_MATCH: &str = "no matches found: ";

/// Os programas que, ao fim de um cano, juntam as linhas da busca numa saída
/// sem o nome do arquivo.
const AGGREGATORS: [&str; 9] = ["wc", "sort", "uniq", "awk", "tr", "paste", "jq", "python3", "xargs"];

/// Os programas que rodam um script escrito na própria linha.
const SCRIPTERS: [&str; 5] = ["python", "python3", "perl", "node", "ruby"];

/// Quantas buscas na taxa o número pede para valer.
const MIN_SEARCHES: usize = 200;

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

/// O que o Claude fez depois da resposta, pelas chamadas seguintes.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    /// Abriu um arquivo que a resposta listou.
    Used,
    /// Abriu um arquivo fora da lista, ou qualquer um quando ela não lista.
    OpenedOther,
    /// Buscou de novo antes de abrir arquivo.
    SearchedAgain,
    /// Nenhuma das chamadas seguintes abriu arquivo nem buscou.
    MovedOn,
}

/// O que uma chamada faz: abre arquivos (relativos à pasta da busca), busca,
/// ou nenhum dos dois.
enum Action {
    Open(Vec<String>),
    Search,
    Neutral,
}

/// Uma busca respondida: a chamada, a conversa (o arquivo dela dentro da
/// pasta das conversas), o instante, a classe e os arquivos da resposta, se
/// a busca falhou, a pasta da conversa, a raiz contra a qual os caminhos se
/// comparam, as chamadas seguintes, com o nome da ferramenta e a entrada, o
/// desfecho e, quando aproveitou, a melhor posição na lista dos arquivos
/// abertos, contada de um.
#[derive(Serialize)]
pub(super) struct Answered {
    tool_use_id: String,
    conversation: String,
    at: String,
    class: Class,
    files: Vec<String>,
    failed: bool,
    cwd: String,
    root: String,
    next: Vec<Value>,
    outcome: Outcome,
    position: Option<usize>,
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

/// A contagem de `found`: as respondidas, as de cada classe, as que
/// falharam e, das outras, as da taxa, as de cada desfecho e as que seguiram.
pub(super) fn tally(found: &[Answered]) -> Value {
    let count = |class: Class| found.iter().filter(|search| search.class == class).count();
    let ended = |outcome: Outcome| found.iter().filter(|search| !search.failed && search.outcome == outcome).count();
    let rate = [Outcome::Used, Outcome::OpenedOther, Outcome::SearchedAgain].map(ended);
    json!({
        "answered": found.len(),
        "pinned": count(Class::Pinned),
        "partial": count(Class::Partial),
        "partial_unsure": count(Class::PartialUnsure),
        "failed": found.iter().filter(|search| search.failed).count(),
        "in_rate": rate.iter().sum::<usize>(),
        "used": rate[0],
        "opened_other": rate[1],
        "searched_again": rate[2],
        "moved_on": ended(Outcome::MovedOn),
    })
}

/// A tabela de cada 100 buscas na taxa de `found`, em `lang`, com a linha
/// mais fraca (a maior das duas perdas) em negrito, e a frase: quantas
/// buscas, de que dia a que dia, e se o número vale ou quanto falta. Sem
/// busca na taxa, só a frase.
pub(super) fn report(found: &[Answered], lang: Locale) -> String {
    let rated: Vec<&Answered> =
        found.iter().filter(|search| !search.failed && search.outcome != Outcome::MovedOn).collect();
    let min = ("{min}", MIN_SEARCHES.to_string());
    let verdict = match MIN_SEARCHES.saturating_sub(rated.len()) {
        0 => fill("measure.searches_hold", lang, &[min]),
        missing => fill("measure.searches_missing", lang, &[("{missing}", missing.to_string()), min]),
    };
    let days: Vec<String> = rated.iter().filter_map(|search| day_of_stamp(&search.at)).collect();
    let (Some(first), Some(last)) = (days.iter().min(), days.iter().max()) else {
        return format!("{} {verdict}", translate("measure.no_searches", lang));
    };
    let count = |outcome: Outcome| rated.iter().filter(|search| search.outcome == outcome).count();
    let weakest = if count(Outcome::SearchedAgain) > count(Outcome::OpenedOther) {
        Outcome::SearchedAgain
    } else {
        Outcome::OpenedOther
    };
    let rows = [
        (Outcome::Used, "measure.used"),
        (Outcome::OpenedOther, "measure.opened_other"),
        (Outcome::SearchedAgain, "measure.searched_again"),
    ];
    let lines: String = rows
        .iter()
        .map(|&(outcome, key)| {
            let (label, share) = (translate(key, lang), (count(outcome) * 100 + rated.len() / 2) / rated.len());
            if outcome == weakest { format!("\n| **{label}** | **{share}** |") } else { format!("\n| {label} | {share} |") }
        })
        .collect();
    let head = [translate("measure.search_head", lang), translate("measure.per_hundred", lang)];
    let span = [
        ("{count}", grouped(rated.len() as u64, lang)),
        ("{first}", day_list(std::slice::from_ref(first), lang)),
        ("{last}", day_list(std::slice::from_ref(last), lang)),
    ];
    format!("| {} | {} |\n|---|---:|{lines}\n\n{} {verdict}", head[0], head[1], fill("measure.searches", lang, &span))
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
        let files = listed(&said);
        let next: Vec<&Call> = calls.iter().skip(index + 1).take(NEXT_CALLS).collect();
        let root = root_of(call);
        let (outcome, position) = outcome(&files, &root, &next);
        found.push(Answered {
            tool_use_id: call.id.clone(),
            conversation: name.to_string(),
            at: call.at.clone(),
            class,
            files,
            // A recusa de outro gancho também tira a busca da taxa.
            failed: !refused && (refusal.is_some() || failed(call, &output)),
            cwd: call.cwd.clone(),
            root,
            next: next.iter().map(|call| json!({ "tool": call.name, "input": call.input })).collect(),
            outcome,
            position,
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
    match call.name.as_str() {
        "Grep" => true,
        "Bash" => segments(call.input["command"].as_str().unwrap_or_default()).iter().any(greps),
        _ => false,
    }
}

/// Se o comando `segment` é `grep`, `egrep`, `fgrep`, `rg` ou `git grep`.
fn greps(segment: &Segment) -> bool {
    matches!(segment.name(), "grep" | "egrep" | "fgrep" | "rg")
        || (segment.name() == "git" && segment.args.iter().any(|word| word.text == "grep"))
}

/// Se a busca `call`, que o gancho não recusou, falhou com o resultado
/// `output`: a chamada foi barrada pela permissão ou interrompida; o zsh
/// recusou o curinga sem aspas de uma palavra da busca (o resto da linha
/// roda); ou nenhuma linha da saída é de arquivo, mesmo com texto de outros
/// comandos. A busca que passa a saída por um cano a um programa que junta
/// as linhas ([`AGGREGATORS`]) ou não mostra o nome do arquivo vale com
/// qualquer texto. Na linha com várias buscas, a respondida
/// é a última busca em pasta, porque a nota do gancho que fica é a dela.
fn failed(call: &Call, output: &str) -> bool {
    let line = if call.name == "Bash" { segments(call.input["command"].as_str().unwrap_or_default()) } else { Vec::new() };
    let searches: Vec<usize> = match line.iter().rposition(|segment| text_search(segment).is_some()) {
        Some(at) => vec![at],
        None => (0..line.len()).filter(|&at| greps(&line[at])).collect(),
    };
    let ours = |word: &str| searches.iter().flat_map(|&at| &line[at].args).any(|arg| arg.text == word || arg.raw == word);
    let head: String = output.chars().take(STOPPED_HEAD).collect();
    if STOPPED.iter().any(|said| head.contains(said))
        || output.split(NO_MATCH).skip(1).filter_map(|rest| rest.split_whitespace().next()).any(ours)
    {
        return true;
    }
    if output.lines().any(file_line) {
        return false;
    }
    let rest: Vec<&str> = output.lines().filter(|line| !line.contains(NO_MATCH)).collect();
    let joins = |at: usize| {
        let mut fed = line[at + 1..].iter().scan(line[at].piped, |piped, next| std::mem::replace(piped, next.piped).then(|| next.name()));
        fed.any(|name| AGGREGATORS.contains(&name))
    };
    let bare = searches.iter().any(|&at| joins(at) || hides_names(&line[at]));
    !bare || matches!(rest.join("\n").trim(), "" | NO_OUTPUT)
}

/// Se a busca `search` mostra as linhas sem o nome do arquivo: `-h` ou
/// `--no-filename` no `grep`, `-I` ou `--no-filename` no `rg`.
fn hides_names(search: &Segment) -> bool {
    let rg = search.name() == "rg";
    search.args.iter().map(|word| word.text.as_str()).any(|arg| {
        arg == "--no-filename" || if rg { arg == "-I" } else { !arg.starts_with("--") && arg.starts_with('-') && arg.contains('h') }
    })
}

/// Se a linha `line` da saída abre com um caminho de arquivo, como o `grep`
/// e o `rg` mostram (`caminho:linha:`, `caminho:` ou só o caminho). O nome
/// sem pasta só vale seguido de `:`, e a contagem zero (`caminho:0`) não vale.
fn file_line(line: &str) -> bool {
    let (path, after) = line.split_at(line.find(|c: char| c == ':' || c.is_whitespace()).unwrap_or(line.len()));
    path_like(path) && has_extension(path) && after != ":0" && (path.contains('/') || after.starts_with(':'))
}

/// Se `text` só tem caracteres de caminho.
fn path_like(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || "_@.+-[]/\\~".contains(c))
}

/// Se o último trecho de `path` tem nome e uma extensão de uma a oito letras
/// ou algarismos.
fn has_extension(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        stem.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == ']')
            && (1..=8).contains(&extension.len())
            && extension.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

/// Se `path`, a partir da pasta `dir`, é um arquivo: o que existe no disco
/// como arquivo ou, fora do disco, o caminho com extensão. Pasta nunca é.
fn file_like(path: &str, dir: &str) -> bool {
    let full = (!dir.is_empty()).then(|| Path::new(dir).join(path));
    path_like(path)
        && match full {
            Some(full) if full.is_dir() => false,
            Some(full) if full.is_file() => true,
            _ => has_extension(path),
        }
}

/// A raiz da pasta em que a busca `call` rodou de verdade: a pasta da
/// conversa, seguida pelos `cd` da linha antes da primeira busca, e dali para
/// cima a primeira pasta com `.git`; sem ela, a própria pasta. A lista do mapa
/// é relativa a essa raiz.
fn root_of(call: &Call) -> String {
    let mut dir = joined(&call.cwd, "");
    if call.name == "Bash" {
        let line = segments(call.input["command"].as_str().unwrap_or_default());
        for segment in line.iter().take_while(|segment| !greps(segment)) {
            dir = moved(segment, &dir).unwrap_or(dir);
        }
    }
    let up = Path::new(&dir).ancestors().find(|up| !dir.is_empty() && up.join(".git").exists());
    up.map_or_else(|| dir.clone(), |up| joined(&up.to_string_lossy(), ""))
}

/// A pasta para onde o comando `segment`, rodado em `dir`, muda, quando é um
/// `cd` para uma pasta nomeada.
fn moved(segment: &Segment, dir: &str) -> Option<String> {
    let to = segment.args.first().filter(|to| segment.name() == "cd" && !to.text.starts_with('-'))?;
    Some(joined(&to.text, dir))
}

/// O desfecho da busca que listou `listed`, relativos à raiz `root`, pelas
/// chamadas `next`: a primeira que abre arquivo ou busca decide; aproveitou
/// leva a melhor posição na lista dos arquivos abertos, contada de um.
fn outcome(listed: &[String], root: &str, next: &[&Call]) -> (Outcome, Option<usize>) {
    let listed: Vec<String> = listed.iter().map(|path| joined(path, "")).collect();
    for call in next {
        match action(call, root) {
            Action::Search => return (Outcome::SearchedAgain, None),
            Action::Open(files) => {
                let best = files.iter().filter_map(|file| listed.iter().position(|known| known == file)).min();
                return best.map_or((Outcome::OpenedOther, None), |at| (Outcome::Used, Some(at + 1)));
            }
            Action::Neutral => {}
        }
    }
    (Outcome::MovedOn, None)
}

/// O que a chamada `call` faz, com os arquivos abertos relativos a `base`, a
/// raiz da busca. Ler e editar abrem; `Grep`, `Glob` e o agente `Explore`
/// buscam, menos o `Grep` num arquivo só, que o abre.
fn action(call: &Call, base: &str) -> Action {
    let field = |key: &str| call.input[key].as_str().unwrap_or_default();
    let open = |path: &str| {
        if path.is_empty() { Action::Neutral } else { Action::Open(vec![relative(path, &call.cwd, base)]) }
    };
    match call.name.as_str() {
        "Read" | "Edit" => open(field("file_path")),
        "NotebookEdit" => open(call.input["notebook_path"].as_str().unwrap_or(field("file_path"))),
        "Grep" if file_like(field("path"), &call.cwd) => open(field("path")),
        "Grep" | "Glob" => Action::Search,
        "Task" | "Agent" if field("subagent_type") == "Explore" => Action::Search,
        "Bash" => terminal(field("command"), &call.cwd, base),
        _ => Action::Neutral,
    }
}

/// O que a linha de terminal `command`, rodada em `cwd`, faz: o primeiro
/// comando que abre ou busca decide o tipo, e todo arquivo aberto antes da
/// primeira busca conta. Segue os `cd` da linha.
fn terminal(command: &str, cwd: &str, base: &str) -> Action {
    let mut dir = cwd.to_string();
    let mut opened = Vec::new();
    for segment in segments(command) {
        if segment.name() == "cd" {
            dir = moved(&segment, &dir).unwrap_or(dir);
            continue;
        }
        match opens(&segment, command, &dir) {
            Some(files) => opened.extend(files.iter().map(|file| relative(file, &dir, base))),
            None if opened.is_empty() => return Action::Search,
            None => break,
        }
    }
    if opened.is_empty() { Action::Neutral } else { Action::Open(opened) }
}

/// Os arquivos que o comando `segment` da linha `command`, rodado em `dir`,
/// abre (nenhum quando não abre nem busca); `None` quando busca. Abrem: o
/// programa que mostra arquivo com arquivo nomeado, a busca de texto só em
/// arquivos, o `sed -i` e o `perl -i`, o resumo e o trecho do mapa, e o
/// script escrito na linha, por texto entre aspas que seja um caminho.
/// Buscam: a busca de texto em pasta, `find`, `fd` e as outras perguntas do
/// mapa.
fn opens(segment: &Segment, command: &str, dir: &str) -> Option<Vec<String>> {
    let name = segment.name();
    let words: Vec<&str> = segment.args.iter().map(|word| word.text.as_str()).collect();
    let files = |texts: &[&str]| -> Vec<String> {
        texts.iter().filter(|text| !text.starts_with('-') && file_like(text, dir)).map(|text| (*text).to_string()).collect()
    };
    if let Some(search) = text_search(segment) {
        let named = !search.paths.is_empty() && search.paths.iter().all(|path| file_like(path, dir));
        return named.then_some(search.paths);
    }
    match (name, words.as_slice()) {
        ("find" | "fd", _) => None,
        ("mustard-rt", ["run", "map", question, rest @ ..]) => {
            let after = rest.iter().position(|word| *word == "--file").and_then(|at| rest.get(at + 1)).copied();
            let file = after.or_else(|| rest.iter().find_map(|word| word.strip_prefix("--file=")));
            match (*question, file) {
                ("summary" | "slice", Some(file)) => Some(vec![file.to_string()]),
                _ => None,
            }
        }
        _ if READERS.contains(&name) || (name == "perl" && words.iter().any(|word| word.starts_with("-i"))) => {
            Some(files(&words))
        }
        _ if SCRIPTERS.contains(&name) => {
            let texts = scripts(segment, command);
            Some(files(&texts.iter().flat_map(|text| quoted(text)).collect::<Vec<_>>()))
        }
        _ => Some(Vec::new()),
    }
}

/// Os scripts que o comando `segment` da linha `command` traz escritos: o
/// texto depois de `-c` ou `-e`, e o corpo do heredoc, lido do texto cru da
/// linha.
fn scripts(segment: &Segment, command: &str) -> Vec<String> {
    let pairs = segment.args.windows(2).filter(|pair| matches!(pair[0].text.as_str(), "-c" | "-e"));
    let mut texts: Vec<String> = pairs.map(|pair| pair[1].text.clone()).collect();
    let heredoc = |op: &str| matches!(op.trim_start_matches(|c: char| c.is_ascii_digit()), "<<" | "<<-");
    for redirect in segment.redirects.iter().filter(|redirect| heredoc(&redirect.op)) {
        let delimiter = redirect.target.text.as_str();
        let body = command.lines().skip_while(|line| !line.contains("<<")).skip(1);
        texts.push(body.take_while(|line| line.trim() != delimiter).collect::<Vec<_>>().join("\n"));
    }
    texts
}

/// Os textos entre aspas, simples ou duplas, de `script`.
fn quoted(script: &str) -> Vec<&str> {
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
fn joined(path: &str, dir: &str) -> String {
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

/// `path`, aberto em `dir`, relativo a `base`, a raiz da busca, quando cai
/// dentro dela.
fn relative(path: &str, dir: &str, base: &str) -> String {
    let (full, base) = (joined(path, dir), joined(base, ""));
    match full.strip_prefix(&format!("{base}/")) {
        Some(inside) if !base.is_empty() => inside.to_string(),
        _ => full,
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
