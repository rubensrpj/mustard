//! `mustard-rt run read <bloco> [--spec <nome>]` — devolve só o bloco pedido
//! do arquivo de eventos da spec, sem os itens removidos ou substituídos e sem
//! o campo `search`.
//!
//! Sem `--spec`, lê a spec atual, pela mesma escada de todas as portas: a
//! variável `MUSTARD_ACTIVE_SPEC`, depois a branch do checkout, depois a spec
//! ligada à sessão. Sem nenhuma, recusa.
//!
//! A saída é um JSON com um evento por linha, na ordem do arquivo:
//!
//! ```text
//! {"ok":true,"spec":"teste","block":"wave-2","count":2,"events":[
//! {"v":1,"id":19,…,"type":"wave",…},
//! {"v":1,"id":20,…,"type":"task",…}
//! ]}
//! ```
//!
//! Uma linha do arquivo que não se entende entra em `warnings`, no idioma do
//! projeto, e o resto é lido.
//!
//! Com `--term`, a leitura devolve o que o termo acha, do mais forte para o
//! menos forte: um código de item devolve aquele item, e qualquer outro termo
//! passa pela busca por nota, que não exige que o item tenha todas as
//! palavras.
//!
//! O bloco `lessons` foge dessa regra: fica fora da spec, vive no banco do
//! projeto (`.claude/spec/lessons.ndjson`) e o `--term` é o número da lição
//! no banco, não uma busca. É assim que o pedido de uma onda leva a lição só
//! pelo número dela, sem copiar o texto.
//!
//! `dispatch-<n>` também foge: não é um bloco, é tudo o que o pedido da onda
//! `n` lista — os itens da spec, cada um com o código, e as lições do banco —,
//! pela mesma conta que monta o pedido, numa leitura só. O `--term` filtra
//! dentro disso.
//!
//! Quatro leituras respondem o que antes saía de script escrito na hora, e
//! nelas o `--term` não vale:
//!
//! - `item-<código>` traz a versão vigente do item, e `item-<número>`, aquela
//!   versão; as duas com `changed`, o que mudou da versão anterior. O item
//!   removido vem com `removed`, e a versão já substituída, com
//!   `replaced_by`. Uma mensagem sai só com o número e o tipo.
//! - `delivered-<n>` traz a entrega vigente da onda: a que a rodada assumiu
//!   ou, sem ela, a volta do agente. Do `agreed`, só o que não foi cumprido.
//! - `backlog` traz as tarefas ainda por entregar, uma por linha, e no fim o
//!   total de tarefas e de arquivos.
//! - `request-<n>` devolve, em texto puro, o pedido gravado no último envio da
//!   onda, igual byte a byte: é por ele que o agente lê o próprio pedido.
//!
//! O bloco `state` traz também o número da última mensagem do usuário, sem o
//! texto, que é o `origin` de quem grava a partir dela.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mustard_core::domain::lessons::kept;
use mustard_core::domain::mustard_id;
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::spec_events::{
    found_by, shown_line, Block, BlockQuery, EventRef, Hidden, ReadQuery, Refusal, SpecEvent, SpecLog,
};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::io::spec_events as store;
use mustard_core::io::wave_prompt::{lesson_bank, request_items};
use mustard_core::platform::i18n::Locale;
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// Options for `mustard-rt run read`.
pub struct ReadOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec pedida; sem ela, a spec atual.
    pub spec: Option<String>,
    pub block: String,
    pub term: Option<String>,
}

/// O núcleo testável de [`run`]: a saída pronta, ou a recusa. A sessão vem
/// do ambiente. Nunca entra em pânico.
pub(crate) fn read_at(opts: &ReadOpts) -> Result<String, Value> {
    read_for(opts, session_from_env().as_deref())
}

/// [`read_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn read_for(opts: &ReadOpts, session: Option<&str>) -> Result<String, Value> {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let block = opts.block.trim();
    if block == "lessons" {
        return read_lessons(&project.root, opts.spec.as_deref(), opts.term.as_deref(), lang);
    }
    let reading = ReadQuery::parse(block).ok_or_else(|| refuse(Refusal::UnknownBlock { found: block.to_string() }))?;
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root))
            .active(session)
            .ok_or_else(|| refuse(Refusal::NoCurrentSpec))?,
    };
    let path = store::spec_file(&project.root, &spec).map_err(refuse)?;
    let Some(log) = store::read(&path).map_err(refuse)? else {
        return Err(refuse(Refusal::NoSpecFile { spec }));
    };
    let codes = log.codes();
    let term = opts.term.as_deref().unwrap_or_default();
    // O que a leitura dá além dos eventos, depois da lista.
    let mut extra: Vec<(&str, Value)> = Vec::new();
    let events: Vec<String> = match reading {
        ReadQuery::Request(wave) => return Ok(request_text(&log, wave)),
        ReadQuery::Dispatch(wave) => dispatch_lines(&project.root, &log, wave, term, &codes, &project.languages),
        ReadQuery::Item(target) => item_lines(&log, &target, &codes),
        ReadQuery::Delivered(wave) => delivered_lines(&log, wave, &codes),
        ReadQuery::Backlog => {
            let (lines, total) = backlog_lines(&log, &codes);
            extra.push(("total", total));
            lines
        }
        ReadQuery::Block(query) => {
            if query == BlockQuery::Block(Block::State)
                && let Some(id) = last_user_message(&log)
            {
                extra.push(("last_user_message", json!(id)));
            }
            // O pedido de cada envio é o texto maior da spec: a lista das
            // ondas e o painel mostram o envio sem ele, e só a leitura de uma
            // onda o traz inteiro.
            let brief = matches!(query, BlockQuery::Block(Block::Waves | Block::Metrics));
            found_by(log.block(query), term, &codes, &project.languages).into_iter().map(|e| shown_with_code(e, &codes, brief)).collect()
        }
    };
    let warnings: Vec<String> = log.skipped.iter().map(|s| s.message(lang)).collect();
    Ok(render(&spec, block, &events, &extra, &warnings))
}

/// O pedido gravado no último envio da onda `wave`, igual byte a byte, em
/// texto puro. A onda sem envio não tem pedido: o texto vem vazio.
fn request_text(log: &SpecLog, wave: u64) -> String {
    let sent = log.last_by_wave("send").get(&wave).and_then(|id| log.get(*id));
    sent.and_then(|send| send.str_field("text")).unwrap_or_default().to_string()
}

/// O número da última mensagem do usuário que a leitura mostra, sem o texto:
/// é o `origin` de quem grava um item a partir do que ele acabou de dizer.
fn last_user_message(log: &SpecLog) -> Option<u64> {
    log.block(BlockQuery::Block(Block::Conversation))
        .into_iter()
        .filter(|event| event.event_type == "message" && event.str_field("author") == Some("user"))
        .map(|event| event.id)
        .max()
}

/// Um item só, como `item-<código>` ou `item-<número>` o pede. Pelo código,
/// a versão vigente — ou, com o item todo removido, a última versão dele;
/// pelo número, aquela versão. A versão vem com o código, com `removed`
/// quando uma remoção a tirou, com `replaced_by` quando outra já a
/// substitui, e com `changed` quando ela substitui uma anterior. Uma mensagem
/// é o texto do usuário, que esta leitura nunca mostra: sai só o número e o
/// tipo. Sem item, a lista vem vazia.
fn item_lines(log: &SpecLog, target: &EventRef, codes: &BTreeMap<u64, String>) -> Vec<String> {
    let hidden = log.hidden();
    let found = match target {
        EventRef::Id(id) => log.get(*id),
        EventRef::Code(code) => {
            let versions: Vec<&SpecEvent> = log.events.iter().filter(|e| codes.get(&e.id) == Some(code)).collect();
            versions.iter().rev().find(|e| !hidden.contains_key(&e.id)).or(versions.last()).copied()
        }
    };
    let Some(event) = found else {
        return Vec::new();
    };
    if event.event_type == "message" {
        let mut bare = Map::new();
        bare.insert("id".into(), json!(event.id));
        bare.insert("type".into(), json!("message"));
        return vec![shown_line(&bare)];
    }
    let mut fields = event.fields.clone();
    if let Some(code) = codes.get(&event.id) {
        fields.insert("code".into(), json!(code));
    }
    match hidden.get(&event.id) {
        Some(Hidden::Removed { .. }) => {
            fields.insert("removed".into(), json!(true));
        }
        Some(Hidden::Replaced { by }) => {
            fields.insert("replaced_by".into(), json!(by));
        }
        _ => {}
    }
    let previous = event.replaced().first().and_then(|old| log.get(*old));
    if let Some(changed) = previous.map(|old| changed_fields(&old.fields, &event.fields)).filter(|c| !c.is_empty()) {
        fields.insert("changed".into(), Value::Object(changed));
    }
    vec![shown_line(&fields)]
}

/// Os campos de uma linha que não são do item: o envelope, a busca e o
/// apontador da versão anterior.
const NOT_COMPARED: &[&str] = &["v", "id", "code", "at", "type", "search", "replaces"];

/// O que mudou de `old` para `new`, campo a campo. Uma lista diz o que entrou
/// (`added`) e o que saiu (`removed`) — os arquivos pelo caminho —; um
/// número, uma marca ou um campo que surgiu ou sumiu diz de quanto para
/// quanto (`from`, `to`); um texto ou um objeto só diz que mudou (`true`),
/// porque as duas versões se leem pelo número.
fn changed_fields(old: &Map<String, Value>, new: &Map<String, Value>) -> Map<String, Value> {
    let keys: BTreeSet<&str> =
        old.keys().chain(new.keys()).map(String::as_str).filter(|key| !NOT_COMPARED.contains(key)).collect();
    let mut changed = Map::new();
    for key in keys {
        let (before, after) = (old.get(key), new.get(key));
        if before == after {
            continue;
        }
        let scalar = |value: Option<&Value>| value.is_none_or(|v| v.is_number() || v.is_boolean() || v.is_null());
        let change = if before.is_some_and(Value::is_array) || after.is_some_and(Value::is_array) {
            list_change(before, after)
        } else if scalar(before) && scalar(after) {
            json!({ "from": before, "to": after })
        } else {
            Value::Bool(true)
        };
        changed.insert(key.to_string(), change);
    }
    changed
}

/// O que entrou e o que saiu de uma lista, na ordem de cada versão. Uma lista
/// que só mudou de ordem, ou de um detalhe de um arquivo que continua nela,
/// só diz que mudou.
fn list_change(before: Option<&Value>, after: Option<&Value>) -> Value {
    let items = |value: Option<&Value>| -> Vec<Value> {
        value.and_then(Value::as_array).map(|list| list.iter().map(list_key).collect()).unwrap_or_default()
    };
    let (old, new) = (items(before), items(after));
    let only_in = |these: &[Value], those: &[Value]| -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for item in these.iter().filter(|item| !those.contains(item)) {
            if !out.contains(item) {
                out.push(item.clone());
            }
        }
        out
    };
    let (added, removed) = (only_in(&new, &old), only_in(&old, &new));
    if added.is_empty() && removed.is_empty() {
        return Value::Bool(true);
    }
    json!({ "added": added, "removed": removed })
}

/// Como um elemento de lista se compara entre versões: o arquivo pelo
/// caminho, o resto pelo valor inteiro.
fn list_key(item: &Value) -> Value {
    item.get("path").filter(|path| path.is_string()).cloned().unwrap_or_else(|| item.clone())
}

/// A entrega vigente da onda `wave`: a que a rodada assumiu ou, sem ela, a
/// última volta do agente que a rodada ainda não assumiu — esta com
/// `returned`. Do `agreed`, fica só o item não cumprido. Sem entrega, a lista
/// vem vazia.
fn delivered_lines(log: &SpecLog, wave: u64, codes: &BTreeMap<u64, String>) -> Vec<String> {
    let assumed = log.last_by_wave("delivered").get(&wave).and_then(|id| log.get(*id));
    let delivery = assumed.or_else(|| {
        log.unassumed_returns().into_iter().rfind(|e| e.event_type == "delivered" && e.wave() == Some(wave))
    });
    let Some(delivery) = delivery else {
        return Vec::new();
    };
    let mut fields = delivery.fields.clone();
    if let Some(code) = codes.get(&delivery.id) {
        fields.insert("code".into(), json!(code));
    }
    if let Some(Value::Array(agreed)) = fields.get_mut("agreed") {
        agreed.retain(|answer| answer.get("met") != Some(&Value::Bool(true)));
    }
    vec![shown_line(&fields)]
}

/// As tarefas ainda por entregar, uma linha por tarefa na ordem do código:
/// o código, o número da versão vigente, a onda (ou `null`), quantos
/// arquivos ela declara e as tarefas ainda por entregar de que depende. O
/// total conta as tarefas e os arquivos distintos entre todas.
fn backlog_lines(log: &SpecLog, codes: &BTreeMap<u64, String>) -> (Vec<String>, Value) {
    let left = crate::commands::flow::round::tasks_left(log);
    let code_of = |id: u64| codes.get(&id).cloned().unwrap_or_else(|| id.to_string());
    let order = |id: u64| codes.get(&id).and_then(|code| mustard_id::parse(code)).map_or(u64::MAX, |(_, n)| n);
    let mut sorted: Vec<_> = left.iter().collect();
    sorted.sort_by_key(|task| (order(task.id), task.id));
    let lines = sorted
        .iter()
        .map(|task| {
            let mut line = Map::new();
            line.insert("id".into(), json!(task.id));
            line.insert("code".into(), json!(code_of(task.id)));
            line.insert("wave".into(), json!(log.get(task.id).and_then(SpecEvent::wave)));
            line.insert("files".into(), json!(task.files.len()));
            let waits: Vec<String> = task.depends_on.iter().map(|id| code_of(*id)).collect();
            line.insert("depends_on".into(), json!(waits));
            shown_line(&line)
        })
        .collect();
    let files: BTreeSet<&String> = left.iter().flat_map(|task| &task.files).collect();
    (lines, json!({ "tasks": left.len(), "files": files.len() }))
}

/// O que o pedido da onda `wave` lista, pela mesma conta que o monta
/// ([`request_items`], com a escolha gravada no envio): primeiro os itens da
/// spec, cada um com o código, depois as lições do banco do projeto `root`,
/// pelo número delas. Com `term`, um código de item acha só aquele item, e
/// qualquer outro termo passa pela busca por nota nos itens e nas lições,
/// nas línguas `languages`. A onda que o plano não tem não tem pedido: a
/// lista vem vazia.
fn dispatch_lines(
    root: &Path,
    log: &SpecLog,
    wave: u64,
    term: &str,
    codes: &BTreeMap<u64, String>,
    languages: &Languages,
) -> Vec<String> {
    if !log.planned_waves().contains(&wave) {
        return Vec::new();
    }
    let bank = lesson_bank(root);
    let found = request_items(log, bank.as_ref(), wave, None, languages);
    let mut lines: Vec<String> =
        found_by(found.items, term, codes, languages).into_iter().map(|e| shown_with_code(e, codes, false)).collect();
    // A lição não tem código de item, e o número dela no banco pode ser o de
    // um item da spec: ela passa só pela busca, nunca pelos códigos da spec.
    lines.extend(found_by(found.lessons, term, &BTreeMap::new(), languages).into_iter().map(|lesson| shown_line(&lesson.fields)));
    lines
}

/// O bloco `lessons`: fora da spec, no banco do projeto. `--term` é o número
/// da lição no banco — não uma busca, como no resto dos blocos —, porque é
/// assim que o pedido de uma onda a leva, sem copiar o texto dela. Sem
/// número, ou sem lição vigente com esse número, a lista vem vazia; sem
/// banco no disco, o mesmo.
fn read_lessons(root: &Path, spec: Option<&str>, term: Option<&str>, lang: Locale) -> Result<String, Value> {
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);
    let paths = ClaudePaths::for_project(root).map_err(|e| refuse(Refusal::Io { detail: e.to_string() }))?;
    let bank = mustard_core::io::lessons::read(&paths.lessons_path()).map_err(refuse)?.unwrap_or_default();
    let wanted = term.and_then(|t| t.trim().parse::<u64>().ok());
    let events: Vec<String> = kept(&bank)
        .into_iter()
        .filter(|lesson| wanted.is_some_and(|id| lesson.id == id))
        .map(|lesson| shown_line(&lesson.fields))
        .collect();
    Ok(render(spec.unwrap_or_default(), "lessons", &events, &[], &[]))
}

/// O checkout em que o comando roda, cuja branch diz qual é a spec atual.
pub(crate) fn checkout(start: &Path) -> PathBuf {
    let start = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
    mustard_core::io::workspace::workspace_root_or_self(&start)
}

/// A linha como a leitura mostra, com o código do item (`MSTD-<sigla>-<NNNN>`),
/// que é o jeito de citá-lo e o endereço dele na página: o gravado na linha
/// ou, numa linha sem código, o que a leitura dá a ela. Com `brief`, o envio
/// sai sem o pedido (`text`).
fn shown_with_code(event: &SpecEvent, codes: &BTreeMap<u64, String>, brief: bool) -> String {
    let mut fields = event.fields.clone();
    if let Some(code) = codes.get(&event.id) {
        fields.insert("code".into(), Value::String(code.clone()));
    }
    if brief && event.event_type == "send" {
        fields.remove("text");
    }
    shown_line(&fields)
}

/// A saída de uma leitura: o envelope, um evento por linha, depois os campos
/// de `extra` que a leitura dá além dos eventos, e os avisos por último.
fn render(spec: &str, block: &str, events: &[String], extra: &[(&str, Value)], warnings: &[String]) -> String {
    let mut out = format!(
        "{{\"ok\":true,\"spec\":{},\"block\":{},\"count\":{},\"events\":[",
        json!(spec),
        json!(block),
        events.len()
    );
    for (i, event) in events.iter().enumerate() {
        out.push_str(if i == 0 { "\n" } else { ",\n" });
        out.push_str(event);
    }
    if !events.is_empty() {
        out.push('\n');
    }
    out.push(']');
    for (key, value) in extra {
        out.push(',');
        out.push_str(&json!(key).to_string());
        out.push(':');
        out.push_str(&value.to_string());
    }
    if !warnings.is_empty() {
        out.push_str(",\"warnings\":");
        out.push_str(&json!(warnings).to_string());
    }
    out.push('}');
    out
}

/// Run `read` and print the block; exit 1 on a refusal. The recorded request
/// of a wave prints byte for byte, with no newline added.
pub fn run(opts: &ReadOpts) {
    match read_at(opts) {
        Ok(text) if matches!(ReadQuery::parse(&opts.block), Some(ReadQuery::Request(_))) => print!("{text}"),
        Ok(report) => println!("{report}"),
        Err(refusal) => {
            println!("{}", serde_json::to_string_pretty(&refusal).unwrap_or_else(|_| "{}".into()));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{seed_at, WriteOpts};
    use tempfile::tempdir;

    fn opts(root: &std::path::Path, block: &str, term: Option<&str>) -> ReadOpts {
        ReadOpts {
            root: root.to_path_buf(),
            spec: Some("teste".into()),
            block: block.into(),
            term: term.map(str::to_string),
        }
    }

    fn without_spec(root: &std::path::Path, block: &str) -> ReadOpts {
        ReadOpts { spec: None, ..opts(root, block, None) }
    }

    fn put(root: &std::path::Path, event_type: &str, fields: Value) -> u64 {
        // A spec já aberta: o arquivo de eventos existe antes da gravação,
        // como o comando que abre a spec o deixa.
        let path = store::spec_file(root, "teste").expect("spec file");
        if !path.exists() {
            std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
            std::fs::File::create(&path).expect("the event file");
        }
        let out = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("teste".into()),
            event_type: event_type.into(),
            json: fields.to_string(),
        });
        assert_eq!(out["ok"], json!(true), "{out}");
        out["id"].as_u64().unwrap()
    }

    fn events(report: &str) -> Vec<Value> {
        let parsed: Value = serde_json::from_str(report).expect("the report is JSON");
        parsed["events"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn reading_one_wave_brings_only_that_wave_and_never_the_search_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        let c1 = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}));
        let c2 = put(root, "criterion", json!({"when": "c", "then": "d", "proof": "q", "form": "ubiquitous", "origin": said}));
        put(root, "wave", json!({"n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": said}));
        put(root, "task", json!({"wave": 1, "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": said}));
        put(root, "wave", json!({"n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "origin": said}));
        put(root, "task", json!({"wave": 2, "text": "T2.", "files": [{"path": "b.rs"}], "depends_on": [], "origin": said}));

        let report = read_at(&opts(root, "wave-2", None)).unwrap();
        let got = events(&report);
        assert_eq!(got.len(), 2, "{report}");
        assert_eq!(got[0]["code"], json!("MSTD-WAVE-0002"), "each event carries its code");
        assert_eq!(got[1]["code"], json!("MSTD-TASK-0002"));
        for event in &got {
            let wave = event.get("n").or_else(|| event.get("wave")).and_then(Value::as_u64);
            assert_eq!(wave, Some(2), "{event}");
        }
        assert!(!report.contains("\"search\""), "{report}");
    }

    #[test]
    fn a_term_filters_the_conversation() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "apagando a pasta"}));
        put(root, "message", json!({"author": "user", "text": "outro assunto"}));
        let got = events(&read_at(&opts(root, "conversation", Some("apagar"))).unwrap());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["text"], json!("apagando a pasta"));
    }

    /// Com a língua do texto em espanhol, a busca na spec corta as palavras
    /// como espanhol: "correr" acha a mensagem que diz "corriendo", que o
    /// português e o inglês não ligariam.
    #[test]
    fn a_spanish_project_cuts_the_spec_search_as_spanish() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), r#"{"language": {"text": "es-ES"}}"#).unwrap();
        put(root, "message", json!({"author": "user", "text": "sigue corriendo la prueba"}));
        put(root, "message", json!({"author": "user", "text": "otro asunto"}));
        let got = events(&read_at(&opts(root, "conversation", Some("correr"))).unwrap());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["text"], json!("sigue corriendo la prueba"));
    }

    /// Uma frase inteira não exige que o item tenha todas as palavras: a
    /// leitura devolve o que a nota acha, do mais forte para o menos forte,
    /// onde a exigência de todas as palavras não devolvia nada.
    #[test]
    fn a_whole_phrase_brings_the_strongest_items_first_instead_of_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        let put_rule = |text: &str, key: &str| {
            put(root, "rule", json!({"text": text, "keys": [key], "example": "e", "origin": said}))
        };
        let weak = put_rule("A página mostra o commit da onda.", "página");
        let strong = put_rule("A rodada formata só os arquivos da rodada antes do commit.", "formatador");
        put_rule("O levantamento pergunta o tipo de trabalho.", "levantamento");

        let phrase = "a rodada formata os arquivos dela antes do commit da onda";
        let got = events(&read_at(&opts(root, "agreed", Some(phrase))).unwrap());
        assert!(!got.is_empty(), "the phrase finds something: {got:?}");
        let found: Vec<u64> = got.iter().filter_map(|e| e["id"].as_u64()).collect();
        assert_eq!(found.first(), Some(&strong), "the strongest comes first: {got:?}");
        assert!(found.contains(&weak), "a partial match still comes: {got:?}");
    }

    #[test]
    fn a_term_that_is_an_item_code_finds_that_item() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        put(root, "criterion", json!({"when": "a", "then": "b", "proof": "p", "keys": ["C-1"], "form": "ubiquitous", "origin": said}));
        put(root, "criterion", json!({"when": "c", "then": "d", "proof": "q", "keys": ["C-2"], "form": "ubiquitous", "origin": said}));
        let got = events(&read_at(&opts(root, "criteria", Some("MSTD-CRIT-0002"))).unwrap());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["code"], json!("MSTD-CRIT-0002"));
        assert_eq!(got[0]["when"], json!("c"));
    }

    /// Uma lição gravada no banco no disco (`.claude/spec/lessons.ndjson`).
    fn write_lesson(bank: &std::path::Path, text: &str, keys: &[&str]) -> u64 {
        let draft = json!({"class": "defect", "text": text, "keys": keys,
            "applies_to": {"files": ["**"]}, "found_in": {"source": "apps/rt/CLAUDE.md"}});
        let Value::Object(draft) = draft else { unreachable!() };
        mustard_core::io::lessons::write(bank, draft, None).expect("a lição entra no banco").id
    }

    /// O bloco `lessons` devolve a lição pelo número dela no banco, fora da
    /// spec: `--term` é o número, não uma busca — mesmo um termo que casa o
    /// texto de outra lição não a traz, porque não é pelo texto que a lição
    /// se acha aqui.
    #[test]
    fn the_lessons_block_returns_a_lesson_by_its_number_not_by_search() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let bank = mustard_core::ClaudePaths::for_project(root).unwrap().lessons_path();
        let id = write_lesson(&bank, "Nunca comitar sem rodar a suíte inteira.", &["suíte", "commit"]);
        write_lesson(&bank, "Outra lição qualquer, sem relação com a suíte.", &["outra"]);

        let got = events(&read_at(&opts(root, "lessons", Some(&id.to_string()))).unwrap());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["id"], json!(id));
        assert_eq!(got[0]["text"], json!("Nunca comitar sem rodar a suíte inteira."));

        // "suíte" acha as duas lições por texto; pelo número, não acha
        // nenhuma — a leitura de `lessons` nunca é busca por palavra.
        let by_word = events(&read_at(&opts(root, "lessons", Some("suíte"))).unwrap());
        assert!(by_word.is_empty(), "{by_word:?}");
    }

    #[test]
    fn an_unknown_block_and_a_spec_without_file_are_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let unknown = read_at(&opts(root, "everything", None)).unwrap_err();
        assert_eq!(unknown["reason"], json!("unknown-block"));
        assert!(unknown["hint"].as_str().unwrap().contains("everything"));
        let missing = read_at(&opts(root, "state", None)).unwrap_err();
        assert_eq!(missing["reason"], json!("no-spec-file"));
    }

    #[test]
    fn a_broken_line_shows_as_a_warning() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        let path = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str("{quebrada\n");
        std::fs::write(&path, raw).unwrap();
        let report = read_at(&opts(root, "conversation", None)).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["count"], json!(1));
        assert_eq!(parsed["warnings"].as_array().map(Vec::len), Some(1), "{report}");
    }

    #[test]
    fn read_without_spec_reads_the_spec_of_the_current_branch() {
        // Uma sobreposição herdada responde primeiro, por desenho: o teste
        // pula, em vez de depender do shell que roda a suíte.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        crate::shared::spec_state::stand_on_spec_branch(root, "teste");
        // Uma sessão ligada a outra spec não vence a branch.
        crate::shared::context::session::bind_session_spec(root.to_str().unwrap(), "s-leitura", "outra");

        let report = read_for(&without_spec(root, "conversation"), Some("s-leitura")).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["spec"], json!("teste"), "the resolved name is shown: {report}");
        assert_eq!(parsed["count"], json!(1), "{report}");
    }

    #[test]
    fn read_without_spec_reads_the_spec_bound_to_the_session() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        crate::shared::context::session::bind_session_spec(root.to_str().unwrap(), "s-leitura", "teste");

        let report = read_for(&without_spec(root, "conversation"), Some("s-leitura")).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["spec"], json!("teste"), "{report}");
    }

    #[test]
    fn read_without_spec_and_without_a_current_spec_is_refused_in_both_languages() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
        let pt = read_for(&without_spec(root, "state"), None).unwrap_err();
        assert_eq!(pt["ok"], json!(false));
        assert_eq!(pt["reason"], json!("no-current-spec"));
        assert!(pt["hint"].as_str().unwrap().starts_with("Nenhuma spec atual"), "{pt}");

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        let en = read_for(&without_spec(root, "state"), None).unwrap_err();
        assert_eq!(en["reason"], json!("no-current-spec"));
        assert!(en["hint"].as_str().unwrap().starts_with("No current spec"), "{en}");
    }

    /// Um evento gravado como o programa grava, sem a conferência de quem
    /// escreve de fora: o envio e a entrega, que só a rodada e o agente gravam.
    fn by_program(root: &std::path::Path, event_type: &str, fields: Value) -> u64 {
        let path = store::spec_file(root, "teste").expect("spec file");
        std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
        store::write(&path, event_type, fields.as_object().cloned().expect("an object"), &[]).expect(event_type).id
    }

    /// A única linha que a leitura devolve.
    fn only_line(root: &std::path::Path, block: &str) -> (String, Value) {
        let report = read_at(&opts(root, block, None)).unwrap();
        let got = events(&report);
        assert_eq!(got.len(), 1, "{report}");
        (report, got[0].clone())
    }

    /// O plano das leituras de item: duas ondas, a tarefa 1 na onda 1 com
    /// `a.rs` e `b.rs`, a tarefa 2 na onda 2, e a versão nova da tarefa 1,
    /// que passa para a onda 2, troca `b.rs` por `d.rs` e passa a depender da
    /// tarefa 2. Devolve a mensagem, a versão antiga, a tarefa 2 e a nova.
    fn plan_with_a_new_version(root: &std::path::Path) -> (u64, u64, u64, u64) {
        let said = put(root, "message", json!({"author": "user", "text": "o plano"}));
        for n in [1, 2] {
            let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}));
            put(root, "wave", json!({"n": n, "text": "Onda.", "criteria": [c], "done_when": "x", "origin": said}));
        }
        let old = put(root, "task", json!({"wave": 1, "text": "Somar.", "files": [{"path": "a.rs"}, {"path": "b.rs"}],
            "depends_on": [], "origin": said}));
        let other = put(root, "task", json!({"wave": 2, "text": "Mostrar.", "files": [{"path": "c.rs"}], "depends_on": [], "origin": said}));
        let new = put(root, "task", json!({"wave": 2, "text": "Somar e mostrar.", "files": [{"path": "a.rs"}, {"path": "d.rs"}],
            "depends_on": [other], "origin": said, "replaces": old}));
        (said, old, other, new)
    }

    /// Pelo código, a leitura traz a versão vigente do item, com o código e o
    /// que mudou da versão anterior: o arquivo que entrou e o que saiu, a onda
    /// de quanto para quanto, a dependência nova e o texto que mudou.
    #[test]
    fn an_item_read_by_code_brings_the_current_version_with_what_changed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, old, other, new) = plan_with_a_new_version(root);

        let (report, item) = only_line(root, "item-MSTD-TASK-0001");
        assert_eq!(item["id"], json!(new), "{report}");
        assert_eq!(item["code"], json!("MSTD-TASK-0001"), "{report}");
        assert_eq!(item["replaces"], json!(old), "{report}");
        let expected = json!({
            "depends_on": {"added": [other], "removed": []},
            "files": {"added": ["d.rs"], "removed": ["b.rs"]},
            "text": true,
            "wave": {"from": 1, "to": 2},
        });
        assert_eq!(item["changed"], expected, "{report}");
        assert!(!report.contains("\"search\""), "{report}");
        assert_eq!(report, read_at(&opts(root, "item-MSTD-TASK-0001", None)).unwrap(), "the reading is byte-stable");
    }

    /// Pelo número, a leitura traz aquela versão, mesmo já substituída, e diz
    /// qual versão a substitui; a versão nova, pelo número, sai igual à que o
    /// código traz.
    #[test]
    fn an_item_read_by_number_brings_that_version() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (_, old, _, new) = plan_with_a_new_version(root);

        let (report, item) = only_line(root, &format!("item-{old}"));
        assert_eq!(item["id"], json!(old), "{report}");
        assert_eq!(item["wave"], json!(1), "{report}");
        assert_eq!(item["files"], json!([{"path": "a.rs"}, {"path": "b.rs"}]), "{report}");
        assert_eq!(item["replaced_by"], json!(new), "{report}");
        assert_eq!(item.get("changed"), None, "the first version replaces nothing: {report}");
        assert_eq!(item.get("removed"), None, "{report}");

        let (_, current) = only_line(root, &format!("item-{new}"));
        let (_, by_code) = only_line(root, "item-MSTD-TASK-0001");
        assert_eq!(current, by_code);
    }

    /// O item removido ainda se lê pelo código, com a última versão e a marca
    /// de removido.
    #[test]
    fn a_removed_item_is_marked_removed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let (said, _, _, _) = plan_with_a_new_version(root);
        let gone = put(root, "task", json!({"wave": 1, "text": "Apagar.", "files": [{"path": "e.rs"}], "depends_on": [], "origin": said}));
        put(root, "remove", json!({"targets": [gone], "reason": "Saiu do plano."}));

        let (report, item) = only_line(root, "item-MSTD-TASK-0003");
        assert_eq!(item["id"], json!(gone), "{report}");
        assert_eq!(item["removed"], json!(true), "{report}");
        let (_, kept) = only_line(root, "item-MSTD-TASK-0002");
        assert_eq!(kept.get("removed"), None, "a task still in the plan is not marked");
    }

    /// Uma mensagem lida como item sai só com o número e o tipo: o texto do
    /// usuário nunca aparece por esta leitura.
    #[test]
    fn a_message_read_as_an_item_shows_no_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "a senha é segredo"}));

        let report = read_at(&opts(root, &format!("item-{said}"), None)).unwrap();
        assert_eq!(events(&report), vec![json!({"id": said, "type": "message"})], "{report}");
        assert!(!report.contains("segredo"), "{report}");
    }

    /// Com a volta do agente e a versão que a rodada assumiu, a leitura da
    /// entrega traz a assumida, com o que não foi cumprido do combinado, as
    /// sobras, o que ficou por fazer, os arquivos e o texto; sem a assumida,
    /// traz a volta do agente.
    #[test]
    fn the_delivery_reading_brings_the_assumed_delivery_over_the_agents_own() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        plan_with_a_new_version(root);
        let own = by_program(root, "delivered", json!({"author": "wave", "wave": 1, "text": "Do agente.", "files": ["a.rs"],
            "returned": true, "agreed": [{"item": "MSTD-RULE-0001", "met": false, "text": "falta"}]}));

        let (report, alone) = only_line(root, "delivered-1");
        assert_eq!(alone["id"], json!(own), "without the assumed one, the agent's own: {report}");
        assert_eq!(alone["returned"], json!(true), "{report}");

        let assumed = by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "Assumida.",
            "files": ["a.rs", "b.rs"], "replaces": [own],
            "agreed": [{"item": "MSTD-RULE-0001", "met": true}, {"item": "MSTD-RULE-0002", "met": false, "text": "falta o teste"}],
            "leftovers": [{"title": "Sobra", "detail": "no `x.rs`"}], "undone": ["MSTD-TASK-0002"]}));
        let (report, got) = only_line(root, "delivered-1");
        assert_eq!(got["id"], json!(assumed), "{report}");
        assert_eq!(got["text"], json!("Assumida."), "{report}");
        assert_eq!(got["agreed"], json!([{"item": "MSTD-RULE-0002", "met": false, "text": "falta o teste"}]), "{report}");
        assert_eq!(got["files"], json!(["a.rs", "b.rs"]), "{report}");
        assert_eq!(got["leftovers"], json!([{"title": "Sobra", "detail": "no `x.rs`"}]), "{report}");
        assert_eq!(got["undone"], json!(["MSTD-TASK-0002"]), "{report}");
        assert!(events(&read_at(&opts(root, "delivered-2", None)).unwrap()).is_empty(), "a wave with no delivery reads empty");
    }

    /// O backlog deixa de fora a tarefa da onda entregue e a tarefa removida;
    /// cada linha diz a onda, quantos arquivos a tarefa declara e de quais
    /// tarefas ainda por entregar ela depende, e o total conta as tarefas e os
    /// arquivos distintos.
    #[test]
    fn the_backlog_leaves_out_delivered_and_removed_tasks_and_counts_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o plano"}));
        let mut criteria = Vec::new();
        for n in [1_u64, 2, 3] {
            let c = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}));
            let after: Vec<u64> = (1..n).collect();
            put(root, "wave", json!({"n": n, "text": "Onda.", "criteria": [c], "done_when": "x", "depends_on": after, "origin": said}));
            criteria.push(c);
        }
        let task = |wave: Option<u64>, files: &[&str], depends_on: &[u64]| -> u64 {
            let files: Vec<Value> = files.iter().map(|path| json!({"path": path})).collect();
            let mut body = json!({"text": "Fazer.", "files": files, "depends_on": depends_on, "origin": said});
            match wave {
                Some(wave) => body["wave"] = json!(wave),
                None => body["covers"] = json!([criteria[0]]),
            }
            put(root, "task", body)
        };
        let delivered = task(Some(1), &["a.rs", "b.rs"], &[]);
        let second = task(Some(2), &["b.rs", "c.rs"], &[delivered]);
        let gone = task(Some(2), &["e.rs"], &[]);
        let third = task(Some(3), &["d.rs"], &[second]);
        let loose = task(None, &["c.rs"], &[]);
        put(root, "remove", json!({"targets": [gone], "reason": "Saiu do plano."}));
        by_program(root, "delivered", json!({"author": "binary", "wave": 1, "text": "Pronta.", "files": ["a.rs", "b.rs"]}));

        let report = read_at(&opts(root, "backlog", None)).unwrap();
        let expected = format!(
            "{{\"ok\":true,\"spec\":\"teste\",\"block\":\"backlog\",\"count\":3,\"events\":[\n\
             {{\"id\":{second},\"code\":\"MSTD-TASK-0002\",\"depends_on\":[],\"files\":2,\"wave\":2}},\n\
             {{\"id\":{third},\"code\":\"MSTD-TASK-0004\",\"depends_on\":[\"MSTD-TASK-0002\"],\"files\":1,\"wave\":3}},\n\
             {{\"id\":{loose},\"code\":\"MSTD-TASK-0005\",\"depends_on\":[],\"files\":1,\"wave\":null}}\n\
             ],\"total\":{{\"tasks\":3,\"files\":3}}}}"
        );
        assert_eq!(report, expected);
    }

    /// O estado traz o número da última mensagem do usuário, sem o texto
    /// dela; a mensagem de outro autor, depois, não conta.
    #[test]
    fn the_state_block_carries_the_last_user_message_number() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        by_program(root, "state", json!({"author": "binary", "phase": "plan", "branch": "feature/teste", "base": "dev"}));
        put(root, "message", json!({"author": "user", "text": "primeiro"}));
        let last = put(root, "message", json!({"author": "user", "text": "o último pedido"}));
        by_program(root, "message", json!({"author": "binary", "text": "aviso do programa"}));

        let report = read_at(&opts(root, "state", None)).unwrap();
        let parsed: Value = serde_json::from_str(&report).expect("the report is JSON");
        assert_eq!(parsed["last_user_message"], json!(last), "{report}");
        assert!(!report.contains("o último pedido"), "{report}");
    }
}
