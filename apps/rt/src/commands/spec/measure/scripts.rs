//! Os scripts em volta do Mustard e as edições por script, lidos das
//! conversas do projeto desde a marca, conversa por conversa, com o papel de
//! cada uma: a sessão principal, que conduz; o agente de onda; ou outro
//! agente.
//!
//! Script em volta do Mustard é a chamada que passa a resposta do Mustard
//! por `python`, `jq` ou `grep`, ou que monta com `python` ou `jq` os dados
//! de `run write` ou `run answer`. Vale no mesmo comando, pelo encadeamento
//! de `|`, pela substituição (`$(…)`, `<(…)`) ou, na montagem, pela variável
//! que guarda a saída do script antes do comando do Mustard. Vale também pelo
//! arquivo: a resposta gravada (`> arquivo`) e lida depois pelo script, e o
//! arquivo que o script grava e o `--json @arquivo` lê depois. O arquivo se
//! casa pelo caminho nos dois lados, a partir da pasta de cada chamada; o
//! caminho que começa por variável (`$S/x.json`) se casa pelo texto.
//!
//! Edição por script é a que o agente de onda tem recusada
//! ([`edited_by_shell`]), contada aqui em toda conversa. Cada chamada conta
//! como foi pedida, também a que um gancho recusou.

use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use mustard_core::io::spend::{project_conversations, project_place};
use mustard_core::platform::i18n::{translate, Locale};
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::grouped;
use super::searches::{joined, moved, quoted, scripts, transcript, Call};
use crate::hooks::bash::lex::{segments, Segment};
use crate::hooks::bash::shell_edit::edited_by_shell;
use crate::hooks::write::wave_copy::wave_agent_kind;

/// Os programas do Mustard.
const RUNTIME: [&str; 4] = ["mustard-rt", "mustard-rt.exe", "mustard-boot", "mustard-boot.cmd"];

/// Os programas que, fora o `python`, leem a resposta do Mustard.
const READERS: [&str; 2] = ["jq", "grep"];

/// O programa que, fora o `python`, monta os dados de uma gravação.
const BUILDER: &str = "jq";

/// As gravações do Mustard que recebem dados: `run write` e `run answer`.
const WRITES: [&str; 2] = ["write", "answer"];

/// Os redirecionamentos que gravam a saída do comando num arquivo.
const SAVES: [&str; 6] = [">", ">>", ">|", "1>", "1>>", "&>"];

/// O papel da conversa.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Role {
    /// A sessão principal, que conduz.
    Main,
    /// O agente de onda.
    Wave,
    /// Outro agente.
    Other,
}

/// Cada papel, com a chave dele na resposta e a do catálogo.
const ROLES: [(Role, &str, &str); 3] = [
    (Role::Main, "main", "measure.role_main"),
    (Role::Wave, "wave", "measure.role_wave"),
    (Role::Other, "other", "measure.role_other"),
];

/// Uma conversa do projeto com chamada desde a marca: o arquivo dela dentro
/// da pasta das conversas, o papel, a descrição com que o agente foi lançado
/// e quantas chamadas têm script em volta do Mustard e edição por script.
#[derive(Serialize)]
pub(super) struct Counted {
    conversation: String,
    role: Role,
    description: Option<String>,
    around: usize,
    edits: usize,
}

/// As conversas do projeto de `root` na pasta de configuração `config` com
/// chamada desde `since`, cada uma com as suas contas.
pub(super) fn counted(config: &Path, root: &Path, since: DateTime<Utc>) -> Vec<Counted> {
    let folder = config.join("projects");
    let main = project_place(root).unwrap_or_else(|| root.to_path_buf());
    let after = |call: &Call| DateTime::parse_from_rfc3339(&call.at).is_ok_and(|when| when >= since);
    let conversations = project_conversations(config, root, SystemTime::from(since));
    conversations
        .iter()
        .filter_map(|path| {
            let calls: Vec<Call> = transcript(path)?.calls.into_iter().filter(after).collect();
            if calls.is_empty() {
                return None;
            }
            let (role, description) = role_of(path);
            let (around, edits) = scripted(&calls, &main);
            let conversation = path.strip_prefix(&folder).unwrap_or(path).to_string_lossy().replace('\\', "/");
            Some(Counted { conversation, role, description, around, edits })
        })
        .collect()
}

/// As contas de `found` por papel (conversas, scripts em volta do Mustard e
/// edições por script) e as conversas com alguma das duas contas.
pub(super) fn tally(found: &[Counted]) -> Value {
    let mut out = Map::new();
    for (role, key, _) in ROLES {
        let mine: Vec<&Counted> = found.iter().filter(|counted| counted.role == role).collect();
        let around: usize = mine.iter().map(|counted| counted.around).sum();
        let edits: usize = mine.iter().map(|counted| counted.edits).sum();
        out.insert(key.to_string(), json!({ "conversations": mine.len(), "around": around, "edits": edits }));
    }
    let marked: Vec<&Counted> = found.iter().filter(|counted| counted.around + counted.edits > 0).collect();
    out.insert("by_conversation".to_string(), json!(marked));
    Value::Object(out)
}

/// A tabela das contas de `found` por papel, em `lang`; sem script em volta
/// do Mustard nem edição por script, só a frase que diz isso.
pub(super) fn report(found: &[Counted], lang: Locale) -> String {
    if found.iter().all(|counted| counted.around + counted.edits == 0) {
        return translate("measure.no_scripts", lang).to_string();
    }
    let rows: [(&str, fn(&Counted) -> usize); 3] = [
        ("measure.conversations", |_| 1),
        ("measure.around", |counted| counted.around),
        ("measure.script_edits", |counted| counted.edits),
    ];
    let lines: String = rows
        .iter()
        .map(|(key, value)| {
            let cells: Vec<String> = ROLES
                .iter()
                .map(|&(role, _, _)| {
                    let sum: usize = found.iter().filter(|counted| counted.role == role).map(value).sum();
                    grouped(sum as u64, lang)
                })
                .collect();
            format!("\n| {} | {} |", translate(key, lang), cells.join(" | "))
        })
        .collect();
    let head: Vec<&str> = ROLES.iter().map(|&(_, _, key)| translate(key, lang)).collect();
    format!("| | {} |\n|---|---:|---:|---:|{lines}", head.join(" | "))
}

/// O papel da conversa `path`. A de agente mora em `subagents`, com o tipo e
/// a descrição no `.meta.json` ao lado dela.
fn role_of(path: &Path) -> (Role, Option<String>) {
    if !path.parent().and_then(Path::file_name).is_some_and(|name| name == "subagents") {
        return (Role::Main, None);
    }
    let read = std::fs::read_to_string(path.with_extension("meta.json")).ok();
    let meta: Value = read.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let role = if meta["agentType"].as_str().is_some_and(wave_agent_kind) { Role::Wave } else { Role::Other };
    (role, meta["description"].as_str().map(str::to_string))
}

/// Quantas das chamadas `calls`, na ordem da conversa, têm script em volta do
/// Mustard, e quantas mexem por script em arquivo do projeto do checkout
/// principal `main`.
fn scripted(calls: &[Call], main: &Path) -> (usize, usize) {
    let mut files = Files::default();
    let (mut around, mut edits) = (0, 0);
    for call in calls {
        let cwd = if call.cwd.is_empty() { main.to_string_lossy().into_owned() } else { call.cwd.clone() };
        match call.name.as_str() {
            "Bash" => {
                let command = call.input["command"].as_str().unwrap_or_default();
                let line = segments(command);
                around += usize::from(files.around(&line, command, &cwd));
                edits += usize::from(edited_by_shell(&line, main, Path::new(&cwd)));
            }
            "Grep" => {
                let path = call.input["path"].as_str().unwrap_or_default();
                around += usize::from(!path.is_empty() && files.answers.contains(&place(path, &cwd)));
            }
            _ => {}
        }
    }
    (around, edits)
}

/// Os arquivos que a conversa gravou até aqui, pelo caminho: as respostas do
/// Mustard e o que um script que monta dados gravou.
#[derive(Default)]
struct Files {
    answers: HashSet<String>,
    built: HashSet<String>,
}

impl Files {
    /// Guarda os arquivos que a linha `line`, o comando `command` rodado em
    /// `cwd`, grava, e diz se ela passa a resposta do Mustard por script ou
    /// monta por script os dados dele.
    fn around(&mut self, line: &[Segment], command: &str, cwd: &str) -> bool {
        let dirs = folders(line, cwd);
        for (segment, dir) in line.iter().zip(&dirs) {
            let saved = segment.redirects.iter().filter(|redirect| SAVES.contains(&redirect.op.as_str()));
            let saved: Vec<String> = saved.map(|redirect| place(&redirect.target.text, dir)).collect();
            if runs(segment) {
                self.answers.extend(saved);
            } else if builds(segment) {
                self.built.extend(saved);
                let written = embedded(segment, command).into_iter().map(|text| place(&text, dir));
                self.built.extend(written);
            }
        }
        (0..line.len()).any(|at| self.reads_answer(line, command, &dirs, at) || self.feeds_write(line, &dirs, at))
    }

    /// O comando `line[at]` é um script que lê a resposta do Mustard: pela
    /// substituição, pelo encadeamento ou por um arquivo de resposta, nomeado
    /// nele ou num comando que o encadeia até ele.
    fn reads_answer(&self, line: &[Segment], command: &str, dirs: &[String], at: usize) -> bool {
        if !reads(&line[at]) {
            return false;
        }
        let names = |index: usize| self.names_answer(&line[index], command, &dirs[index]);
        inside(line, at).iter().any(runs)
            || names(at)
            || (0..at).any(|index| fed(line, index).contains(&at) && (runs(&line[index]) || names(index)))
    }

    /// O comando `segment`, rodado em `dir`, nomeia um arquivo de resposta do
    /// Mustard: num argumento, na entrada (`<`) ou no texto do script.
    fn names_answer(&self, segment: &Segment, command: &str, dir: &str) -> bool {
        let args = segment.args.iter().map(|arg| arg.text.clone()).filter(|text| !text.starts_with('-'));
        let input = segment.redirects.iter().filter(|redirect| redirect.op == "<").map(|redirect| redirect.target.text.clone());
        let mut named = args.chain(input).chain(embedded(segment, command));
        named.any(|text| self.answers.contains(&place(&text, dir)))
    }

    /// O comando `line[at]` é uma gravação do Mustard com dados que um
    /// script montou: pela substituição, pelo encadeamento, pela variável
    /// que guardou a saída do script antes dela, ou pelo arquivo do
    /// `--json @arquivo` que um script gravou.
    fn feeds_write(&self, line: &[Segment], dirs: &[String], at: usize) -> bool {
        let segment = &line[at];
        let writes = runs(segment)
            && segment.args.first().is_some_and(|arg| arg.text == "run")
            && segment.args.get(1).is_some_and(|arg| WRITES.contains(&arg.text.as_str()));
        if !writes {
            return false;
        }
        let assigned = |index: usize| line[index].name().is_empty() && inside(line, index).iter().any(builds);
        inside(line, at).iter().any(builds)
            || (0..at).any(|index| (builds(&line[index]) && fed(line, index).contains(&at)) || assigned(index))
            || json_file(segment).is_some_and(|file| self.built.contains(&place(file, &dirs[at])))
    }
}

/// A pasta em que roda cada comando de `line`, a partir de `cwd`, seguindo
/// os `cd` da linha.
fn folders(line: &[Segment], cwd: &str) -> Vec<String> {
    let mut dir = joined(cwd, "");
    line.iter()
        .map(|segment| {
            if segment.name() == "cd" {
                dir = moved(segment, &dir).unwrap_or_else(|| dir.clone());
            }
            dir.clone()
        })
        .collect()
}

/// O caminho que `text` nomeia, a partir de `dir` quando é relativo; o que
/// começa por variável ou por `~` fica pelo texto.
fn place(text: &str, dir: &str) -> String {
    if text.starts_with(['$', '~']) { joined(text, "") } else { joined(text, dir) }
}

/// O comando roda o Mustard.
fn runs(segment: &Segment) -> bool {
    RUNTIME.contains(&segment.name())
}

/// O comando é `python`.
fn python(segment: &Segment) -> bool {
    let name = segment.name();
    matches!(name, "python" | "python3") || name.starts_with("python3.")
}

/// O comando é um script que lê a resposta do Mustard.
fn reads(segment: &Segment) -> bool {
    python(segment) || READERS.contains(&segment.name())
}

/// O comando é um script que monta dados.
fn builds(segment: &Segment) -> bool {
    python(segment) || segment.name() == BUILDER
}

/// Os textos entre aspas do programa que o `python` traz na própria linha.
fn embedded(segment: &Segment, command: &str) -> Vec<String> {
    if !python(segment) {
        return Vec::new();
    }
    let texts = scripts(segment, command);
    texts.iter().flat_map(|text| quoted(text)).map(str::to_string).collect()
}

/// Os comandos de dentro de `line[at]`: os que vêm logo depois dele, mais
/// fundos que ele.
fn inside(line: &[Segment], at: usize) -> &[Segment] {
    let depth = line[at].depth;
    let rest = &line[at + 1..];
    let end = rest.iter().position(|segment| segment.depth <= depth).unwrap_or(rest.len());
    &rest[..end]
}

/// As posições dos comandos que recebem a saída de `line[at]` pelo
/// encadeamento de `|`, no mesmo nível; os comandos de dentro de cada elo
/// não são elos.
fn fed(line: &[Segment], at: usize) -> Vec<usize> {
    let depth = line[at].depth;
    let mut piped = line[at].piped;
    let mut reached = Vec::new();
    for (index, segment) in line.iter().enumerate().skip(at + 1) {
        if segment.depth < depth || !piped {
            break;
        }
        if segment.depth > depth {
            continue;
        }
        reached.push(index);
        piped = segment.piped;
    }
    reached
}

/// O arquivo que a gravação `segment` lê pelo `--json @arquivo`.
fn json_file(segment: &Segment) -> Option<&str> {
    let words: Vec<&str> = segment.args.iter().map(|arg| arg.text.as_str()).collect();
    let value = words.windows(2).find_map(|pair| (pair[0] == "--json").then_some(pair[1]));
    value.or_else(|| words.iter().find_map(|word| word.strip_prefix("--json="))).and_then(|value| value.strip_prefix('@'))
}
